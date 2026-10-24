#!/usr/bin/env python3
"""Update all Noryn package versions together, without resolving dependencies or using Git."""
import argparse
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import tomllib
from release_version import validate_release, version_from_tag


def version_key(version):
    core, _, prerelease = version.partition('-')
    identifiers = tuple((0, int(part)) if part.isdigit() else (1, part) for part in prerelease.split('.')) if prerelease else ()
    return (*map(int, core.split('.')), not prerelease, identifiers)


def next_version(current, requested):
    if requested in {'patch', 'minor', 'major'}:
        parts = list(map(int, current.split('-', 1)[0].split('.')))
        position = {'major': 0, 'minor': 1, 'patch': 2}[requested]
        parts[position] += 1
        parts[position + 1:] = [0] * (2 - position)
        target = '.'.join(map(str, parts))
    else:
        target = version_from_tag(requested)
    if version_key(target) < version_key(current):
        raise SystemExit(f'Version {target} liegt unter {current}; ein Release-Downgrade ist nicht erlaubt.')
    return target


def json_version(text, target):
    """Replace only the top-level JSON value, retaining formatting and unrelated content."""
    data = json.loads(text)
    if not isinstance(data, dict) or not isinstance(data.get('version'), str):
        raise SystemExit('Versionsfeld im JSON-Manifest fehlt')
    decoder = json.JSONDecoder()
    position = text.index('{') + 1
    spans = []
    while True:
        position = re.compile(r'\s*').match(text, position).end()
        if text[position] == '}':
            break
        name, position = decoder.raw_decode(text, position)
        position = re.compile(r'\s*:\s*').match(text, position).end()
        start = position
        _, position = decoder.raw_decode(text, position)
        if name == 'version':
            spans.append((start, position))
        position = re.compile(r'\s*').match(text, position).end()
        if text[position] == ',':
            position += 1
        else:
            break
    if len(spans) != 1:
        raise SystemExit('JSON-Manifest enthält kein eindeutiges Versionsfeld')
    start, end = spans[0]
    return text[:start] + json.dumps(target) + text[end:]


def toml_version(text, target, table):
    header = re.compile(r'(?m)^[ \t]*\[' + re.escape(table) + r'\][ \t]*(?:#[^\r\n]*)?\r?$')
    headers = list(header.finditer(text))
    if len(headers) != 1:
        raise SystemExit(f'TOML-Tabelle [{table}] fehlt oder ist nicht eindeutig')
    start = headers[0].end()
    following = re.search(r'(?m)^[ \t]*\[', text[start:])
    end = start + following.start() if following else len(text)
    field = re.compile(r'''(?m)^([ \t]*version[ \t]*=[ \t]*)("(?:\\.|[^"\\\r\n])*"|'[^'\r\n]*')''')
    section, count = field.subn(lambda m: m[1] + json.dumps(target), text[start:end])
    if count != 1:
        raise SystemExit(f'TOML-Versionsfeld in [{table}] fehlt oder ist nicht eindeutig')
    updated = text[:start] + section + text[end:]
    if tomllib.loads(updated)[table]['version'] != target:
        raise SystemExit('TOML-Versionsänderung konnte nicht geprüft werden')
    return updated


def lock_versions(text, packages, current, target):
    headers = list(re.finditer(r'(?m)^\[\[package\]\][ \t]*\r?$', text))
    updated = text[:headers[0].start()] if headers else text
    changed = set()
    for index, header in enumerate(headers):
        end = headers[index + 1].start() if index + 1 < len(headers) else len(text)
        block = text[header.start():end]
        package = tomllib.loads(block)['package'][0]
        if package['name'] in packages and 'source' not in package:
            # A single [[package]] block is otherwise identical to a normal TOML table.
            normal = block.replace('[[package]]', '[package]', 1)
            block = toml_version(normal, target, 'package').replace('[package]', '[[package]]', 1)
            changed.add(package['name'])
        # Cargo disambiguates local dependencies with "name version" when necessary.
        # Registry/git-qualified dependencies must keep their original versions.
        for name in packages:
            block = block.replace(json.dumps(f'{name} {current}'), json.dumps(f'{name} {target}'))
        updated += block
    if changed != packages:
        raise SystemExit('Lokale Pakete fehlen in Cargo.lock')
    expected = tomllib.loads(text)
    references = {f'{name} {current}': f'{name} {target}' for name in packages}
    for package in expected['package']:
        if package['name'] in packages and 'source' not in package:
            package['version'] = target
        if 'dependencies' in package:
            package['dependencies'] = [references.get(dependency, dependency) for dependency in package['dependencies']]
    if tomllib.loads(updated) != expected:
        raise SystemExit('Cargo.lock enthält unerwartete Änderungen; nichts wurde geschrieben')
    return updated


def write_all(changes):
    """Stage every file first and restore prior bytes on ordinary write failures."""
    staged, backups, written = {}, {}, []
    try:
        for path, (before, after) in changes.items():
            for content, destinations in [(after, staged), (before, backups)]:
                with tempfile.NamedTemporaryFile(dir=path.parent, prefix=f'.{path.name}.version-', delete=False) as handle:
                    destinations[path] = Path(handle.name)
                    handle.write(content)
                    handle.flush()
                    os.fsync(handle.fileno())
                os.chmod(destinations[path], stat.S_IMODE(path.stat().st_mode))
        for path, (before, _) in changes.items():
            if path.read_bytes() != before:
                raise SystemExit(f'{path.name} wurde zwischenzeitlich geändert; bitte erneut ausführen.')
        try:
            for path, (before, _) in changes.items():
                if path.read_bytes() != before:
                    raise OSError(f'{path.name} wurde zwischenzeitlich geändert')
                os.replace(staged[path], path)
                written.append(path)
        except (OSError, KeyboardInterrupt):
            for path in reversed(written):
                # Preserve any concurrent user edit instead of replacing it during rollback.
                if path.read_bytes() == changes[path][1]:
                    os.replace(backups[path], path)
            raise
    finally:
        for path in [*staged.values(), *backups.values()]:
            path.unlink(missing_ok=True)


def bump_version(root, requested, dry_run=False):
    root = root.resolve()
    current = json.loads((root / 'package.json').read_bytes())['version']
    validate_release(root, current)
    target = next_version(current, requested)
    if target == current:
        return current, target, []
    workspace = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']
    manifests = [root / member / 'Cargo.toml' for member in workspace['members']]
    packages = {tomllib.loads(path.read_text())['package']['name'] for path in manifests}
    paths = [root / 'package.json', root / 'src-tauri/tauri.conf.json', *manifests, root / 'Cargo.lock']
    changes = {}
    for path in paths:
        if path.is_symlink() or not path.resolve().is_relative_to(root):
            raise SystemExit(f'Manifest muss eine reguläre Datei im Projekt sein: {path}')
        before = path.read_bytes()
        text = before.decode('utf-8')
        if path.suffix == '.json':
            after = json_version(text, target)
            assert json.loads(after)['version'] == target
        elif path.name == 'Cargo.lock':
            after = lock_versions(text, packages, current, target)
            local = {p['name']: p['version'] for p in tomllib.loads(after)['package'] if p['name'] in packages and 'source' not in p}
            assert local == dict.fromkeys(packages, target)
        else:
            after = toml_version(text, target, 'package')
        changes[path] = (before, after.encode('utf-8'))
    if not dry_run:
        write_all(changes)
    return current, target, [path.relative_to(root).as_posix() for path in paths]


def main():
    parser = argparse.ArgumentParser(description='Noryn-Version in allen Manifesten und Cargo.lock setzen (Python 3.11+).')
    parser.add_argument('version', help='patch, minor, major oder eine Version wie 0.2.0 / v0.2.0-rc.1')
    parser.add_argument('--dry-run', action='store_true', help='Änderungen nur anzeigen')
    args = parser.parse_args()
    try:
        current, target, paths = bump_version(Path(__file__).resolve().parents[1], args.version, args.dry_run)
    except (OSError, ValueError) as error:
        raise SystemExit(f'Versionsänderung fehlgeschlagen: {error}') from error
    if not paths:
        print(f'Version {current} ist bereits überall gesetzt.')
        return
    print(f'{"Vorschau" if args.dry_run else "Aktualisiert"}: {current} → {target}')
    for path in paths:
        print(f'  {path}')
    if not args.dry_run:
        print('Nur lokale Dateien geändert. Noch kein Commit, Tag oder Push erstellt.')
    print(f'Nächster Release-Tag nach Prüfung und Commit: v{target}')


if __name__ == '__main__':
    main()
