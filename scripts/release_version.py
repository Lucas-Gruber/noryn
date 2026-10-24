#!/usr/bin/env python3
"""Validate a release tag and every local package version before building."""
import json
import os
from pathlib import Path
import re
import tomllib

VERSION = re.compile(r'(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?')


def version_from_tag(tag):
    version = tag.removeprefix('v')
    if not VERSION.fullmatch(version):
        raise SystemExit(f'Invalid release tag {tag!r}; expected X.Y.Z or vX.Y.Z (optionally -rc.1)')
    if '-' in version:
        for part in version.split('-', 1)[1].split('.'):
            if part.isdigit() and len(part) > 1 and part.startswith('0'):
                raise SystemExit(f'Invalid numeric prerelease identifier in {tag!r}')
    return version


def validate_release(root, tag):
    version = version_from_tag(tag)
    versions = {
        'package.json': json.loads((root / 'package.json').read_text())['version'],
        'src-tauri/tauri.conf.json': json.loads((root / 'src-tauri/tauri.conf.json').read_text())['version'],
    }
    workspace = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']
    packages = {}
    for member in workspace['members']:
        name = f'{member}/Cargo.toml'
        package = tomllib.loads((root / name).read_text())['package']
        versions[name] = package['version']
        packages[package['name']] = package['version']
    locked = tomllib.loads((root / 'Cargo.lock').read_text())['package']
    for name in packages:
        entries = [p for p in locked if p['name'] == name and 'source' not in p]
        versions[f'Cargo.lock ({name})'] = entries[0]['version'] if len(entries) == 1 else '(missing or ambiguous)'
    mismatches = [f'{name}: {actual}' for name, actual in versions.items() if actual != version]
    if mismatches:
        raise SystemExit(f'Release tag {tag} requires version {version} in all manifests. Mismatches: ' + '; '.join(mismatches) + '. Commit matching versions before creating a new release tag.')
    return version


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    tag = os.environ.get('RELEASE_TAG', '')
    version = validate_release(root, tag)
    if output := os.environ.get('GITHUB_OUTPUT'):
        with Path(output).open('a', encoding='utf-8', newline='\n') as destination:
            destination.write(f'version={version}\n')
    print(f'Release {tag}: all package versions match {version}')
