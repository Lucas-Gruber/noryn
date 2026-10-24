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
    portable = copy(build / 'noryn.exe', f'windows-{arch}-portable.exe')
    installer = copy(single('nsis/*.exe'), f'windows-{arch}-setup.exe')
elif platform.system() == 'Darwin':
    app = single('macos/*.app')
    portable = output / f'Noryn-{version}-macos-{arch}-portable.app.zip'
    subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(portable)], check=True)
    installer = copy(single('dmg/*.dmg'), f'macos-{arch}.dmg')
else:
    portable = copy(single('appimage/*.AppImage'), f'linux-{arch}-portable.AppImage')
    installer = copy(single('deb/*.deb'), f'linux-{arch}.deb')
assets = {'portable': portable, 'installer': installer}
for name, asset in list(assets.items()):
    with asset.open('rb') as source:
        checksum = hashlib.file_digest(source, 'sha256').hexdigest()
    sidecar = asset.with_name(asset.name + '.sha256')
    sidecar.write_text(f'{checksum}  {asset.name}\n', encoding='utf-8', newline='\n')
    assets[name + '_sha256'] = sidecar

# Every raw artifact action requires exactly one file, never a platform-wide archive.
if github_output := os.environ.get('GITHUB_OUTPUT'):
    with Path(github_output).open('a', encoding='utf-8', newline='\n') as destination:
        for name, asset in assets.items():
            destination.write(f'{name}={asset.relative_to(root).as_posix()}\n')
