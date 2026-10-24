#!/usr/bin/env python3
"""Validate signed updater metadata and assemble one complete GitHub release feed."""
import base64
import json
import os
from pathlib import Path
from urllib.parse import quote, urlsplit
from release_version import version_from_tag


def validate_signature(signature, version, public_key):
    try:
        lines = base64.b64decode(signature, validate=True).decode('utf-8').splitlines()
        key_lines = base64.b64decode(public_key, validate=True).decode('utf-8').splitlines()
        signed = base64.b64decode(lines[1], validate=True)
        public = base64.b64decode(key_lines[1], validate=True)
        if len(signed) != 74 or len(public) != 42 or signed[2:10] != public[2:10]:
            raise ValueError('signing key does not match the app public key')
        if not lines[2].startswith('trusted comment: ') or f'version:{version}' not in lines[2].removeprefix('trusted comment: ').split('\t'):
            raise ValueError('signature is not bound to the release version')
        if len(base64.b64decode(lines[3], validate=True)) != 64:
            raise ValueError('invalid global signature')
    except (ValueError, IndexError, UnicodeDecodeError) as error:
        raise SystemExit(f'Invalid updater signature metadata: {error}')


def asset_url(repository, tag, name):
    return f'https://github.com/{repository}/releases/download/{quote(tag, safe="")}/{quote(name)}'


def assemble(directory, repository, tag, public_key):
    version = version_from_tag(tag)
    platforms = {}
    systems = set()
    for fragment in sorted(directory.glob('Noryn-*-update.json')):
        data = json.loads(fragment.read_text())
        if data['version'] != version:
            raise SystemExit('Updater fragment has a different version')
        for target, entry in data['platforms'].items():
            system, arch = target.split('-', 1)
            if system not in {'windows', 'linux', 'darwin'} or arch not in {'x86_64', 'aarch64'} or target in platforms:
                raise SystemExit('Unexpected or duplicate updater target')
            path = urlsplit(entry['url'])
            candidates = [p for p in directory.iterdir() if p.is_file() and asset_url(repository, tag, p.name) == entry['url']]
            if path.scheme != 'https' or path.netloc != 'github.com' or len(candidates) != 1:
                raise SystemExit('Updater file missing or URL points outside this release')
            payload = candidates[0]
            extension = {'windows': '-setup.exe', 'linux': '.AppImage', 'darwin': '-update.app.tar.gz'}[system]
            if not payload.name.endswith(extension):
                raise SystemExit('Unexpected updater package type')
            signature = payload.with_name(payload.name + '.sig').read_text().strip()
            if entry['signature'] != signature:
                raise SystemExit('Updater signature differs from its sidecar')
            validate_signature(signature, version, public_key)
            platforms[target] = entry
            systems.add(system)
    if systems != {'windows', 'linux', 'darwin'}:
        raise SystemExit('Updater metadata must contain complete Windows, Linux and macOS builds')
    manifest = {'version': version, 'notes': f'Noryn {version}\nVerbesserungen und Änderungen in diesem Release.', 'platforms': platforms}
    (directory / 'latest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8', newline='\n')
    return manifest


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    config = json.loads((root / 'src-tauri/tauri.conf.json').read_text())
    assemble(root / 'artifacts', os.environ['GITHUB_REPOSITORY'], os.environ['RELEASE_TAG'], config['plugins']['updater']['pubkey'])
    print('Complete signed updater feed: artifacts/latest.json')
