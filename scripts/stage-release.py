#!/usr/bin/env python3
"""Collect native Tauri outputs with unambiguous portable/installer asset names."""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
from release_version import validate_release
from update_manifest import asset_url, validate_signature

root = Path(__file__).resolve().parents[1]
version = json.loads((root / 'package.json').read_text())['version']
tag = os.environ.get('RELEASE_TAG', '')
if tag:
    validate_release(root, tag)
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
    update_source = single('nsis/*.exe')
    portable = copy(build / 'noryn.exe', f'windows-{arch}-portable.exe')
    installer = copy(update_source, f'windows-{arch}-setup.exe')
elif platform.system() == 'Darwin':
    app = single('macos/*.app')
    portable = output / f'Noryn-{version}-macos-{arch}-portable.app.zip'
    subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(portable)], check=True)
    update_source = None
    installer = copy(single('dmg/*.dmg'), f'macos-{arch}.dmg')
else:
    update_source = single('appimage/*.AppImage')
    portable = copy(update_source, f'linux-{arch}.AppImage')
    installer = copy(single('deb/*.deb'), f'linux-{arch}.deb')
assets = {'portable': portable, 'installer': installer}
if platform.system() == 'Linux':
    flatpaks = list((build / 'bundle/flatpak').glob('*.flatpak'))
    if flatpaks or os.environ.get('NORYN_REQUIRE_FLATPAK') == '1':
        assets['flatpak'] = copy(single('flatpak/*.flatpak'), f'linux-{arch}.flatpak')
    reference = root / 'target/flatpak/site/Noryn.flatpakref'
    if reference.is_file():
        assets['flatpakref'] = copy(reference, f'linux-{arch}.flatpakref')
    elif os.environ.get('NORYN_REQUIRE_FLATPAK') == '1' and '-' not in version:
        raise SystemExit('Stable Flatpak releases require the signed .flatpakref')
if os.environ.get('NORYN_REQUIRE_UPDATER') == '1':
    if not tag:
        raise SystemExit('Signed updater artifacts require a release tag')
    config = json.loads((root / 'src-tauri/tauri.conf.json').read_text())
    if platform.system() == 'Darwin':
        update_source = single('macos/*.app.tar.gz')
        payload = copy(update_source, f'macos-{arch}-update.app.tar.gz')
        assets['updater_package'] = payload
    else:
        payload = installer if platform.system() == 'Windows' else portable
    signature = copy(update_source.with_name(update_source.name + '.sig'), payload.name.removeprefix(f'Noryn-{version}-') + '.sig')
    signature_text = signature.read_text().strip()
    validate_signature(signature_text, version, config['plugins']['updater']['pubkey'])
    os_name = {'Windows': 'windows', 'Darwin': 'darwin', 'Linux': 'linux'}[platform.system()]
    target_arch = {'x64': 'x86_64', 'arm64': 'aarch64'}[arch]
    fragment = output / f'Noryn-{version}-{os_name}-{arch}-update.json'
    fragment.write_text(json.dumps({'version': version, 'platforms': {f'{os_name}-{target_arch}': {'url': asset_url(os.environ['GITHUB_REPOSITORY'], tag, payload.name), 'signature': signature_text}}}, indent=2) + '\n', encoding='utf-8', newline='\n')
    assets['updater_signature'] = signature
    assets['updater_fragment'] = fragment
for name, asset in list(assets.items()):
    if name in {'updater_signature', 'updater_fragment'}:
        continue
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
