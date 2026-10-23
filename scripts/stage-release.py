#!/usr/bin/env python3
"""Collect native Tauri outputs with unambiguous portable/installer asset names."""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
version = json.loads((root / 'package.json').read_text())['version']
tag = os.environ.get('RELEASE_TAG', '')
if tag and tag != f'v{version}':
    raise SystemExit(f'Tag {tag} does not match package version v{version}')
arch = {'amd64': 'x64', 'x86_64': 'x64', 'arm64': 'arm64', 'aarch64': 'arm64'}.get(platform.machine().lower(), platform.machine().lower())
build = root / 'target/release'
output = root / 'release-assets'
output.mkdir(exist_ok=True)

def single(pattern):
    matches = list((build / 'bundle').glob(pattern))
    if len(matches) != 1:
        raise SystemExit(f'Expected one {pattern}, found {len(matches)}')
    return matches[0]

def copy(source, suffix):
    destination = output / f'Noryn-{version}-{suffix}'
    shutil.copy2(source, destination)
    return destination

if platform.system() == 'Windows':
    copy(build / 'noryn.exe', f'windows-{arch}-portable.exe')
    copy(single('nsis/*.exe'), f'windows-{arch}-setup.exe')
elif platform.system() == 'Darwin':
    app = single('macos/*.app')
    archive = output / f'Noryn-{version}-macos-{arch}-portable.app.zip'
    subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(archive)], check=True)
    copy(single('dmg/*.dmg'), f'macos-{arch}.dmg')
else:
    copy(single('appimage/*.AppImage'), f'linux-{arch}-portable.AppImage')
    copy(single('deb/*.deb'), f'linux-{arch}.deb')
for asset in sorted(output.iterdir()):
    if asset.is_file() and not asset.name.endswith('.sha256'):
        with asset.open('rb') as source:
            checksum = hashlib.file_digest(source, 'sha256').hexdigest()
        asset.with_name(asset.name + '.sha256').write_text(f'{checksum}  {asset.name}\n')
