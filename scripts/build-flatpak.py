#!/usr/bin/env python3
"""Wrap the already-built native Linux application in a single-file Flatpak bundle.

Only the Platform runtime is needed: there is no second Rust/Node compilation or SDK.
The runtime must be installed beforehand; the release workflow uses Flathub.
"""
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
from xml.sax.saxutils import escape
from release_version import validate_release

ROOT = Path(__file__).resolve().parents[1]


def build():
    if platform.system() != 'Linux':
        raise SystemExit('Flatpak packaging requires Linux')
    if os.environ.get('RELEASE_TAG'):
        validate_release(ROOT, os.environ['RELEASE_TAG'])
    config = json.loads((ROOT / 'packaging/flatpak/config.json').read_text())
    app_id = config['appId']
    assert app_id == 'dev.noryn.client'
    version = json.loads((ROOT / 'package.json').read_text())['version']
    work = ROOT / 'target/flatpak'
    tree, repo = work / 'build', work / 'repo'
    output = ROOT / 'target/release/bundle/flatpak/Noryn.flatpak'
    binary = ROOT / 'target/release/noryn'
    if not binary.is_file():
        raise SystemExit('Build the Linux desktop bundles before packaging Flatpak')
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    # Platform serves as the SDK for this binary-only assembly. No compiler is used.
    subprocess.run(['flatpak', 'build-init', str(tree), app_id, config['runtime'], config['runtime'], config['runtimeVersion']], check=True)
    app = tree / 'files'
    (app / 'bin').mkdir(parents=True)
    shutil.copy2(binary, app / 'bin/noryn')
    (app / 'bin/noryn').chmod(0o755)
    desktop = app / 'share/applications' / f'{app_id}.desktop'
    desktop.parent.mkdir(parents=True)
    shutil.copy2(ROOT / 'packaging/flatpak' / desktop.name, desktop)
    icon = app / 'share/icons/hicolor/256x256/apps' / f'{app_id}.png'
    icon.parent.mkdir(parents=True)
    shutil.copy2(ROOT / 'src-tauri/icons/128x128@2x.png', icon)
    metainfo = app / 'share/metainfo' / f'{app_id}.metainfo.xml'
    metainfo.parent.mkdir(parents=True)
    metainfo.write_text(f'''<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>{app_id}</id>
  <metadata_license>CC0-1.0</metadata_license>
  <name>Noryn</name>
  <summary>Remote workspace for SSH, SFTP, FTP and FTPS</summary>
  <description><p>Manage remote connections, transfer files and organize encrypted personal and team vaults.</p></description>
  <launchable type="desktop-id">{app_id}.desktop</launchable>
  <url type="homepage">https://github.com/Lucas-Gruber/noryn</url>
  <releases><release version="{escape(version)}"/></releases>
</component>
''', encoding='utf-8')
    # Fail the release if the native binary cannot resolve all libraries in its runtime.
    subprocess.run(['flatpak', 'build', '--runtime', '--readonly', str(tree), 'sh', '-ec',
                    'report=$(ldd /app/bin/noryn 2>&1); if printf "%s\\n" "$report" | grep -E "not found|version .*not found"; then exit 1; fi; printf "All native libraries resolved in Flatpak runtime\\n"'], check=True)
    subprocess.run(['flatpak', 'build-finish', '--no-inherit-permissions', '--command=noryn', *config['finishArgs'], str(tree)], check=True)
    subprocess.run(['flatpak', 'build-export', str(repo), str(tree), 'stable'], check=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.unlink(missing_ok=True)
    subprocess.run(['flatpak', 'build-bundle', f'--runtime-repo={config["runtimeRepository"]}', str(repo), str(output), app_id, 'stable'], check=True)
    print(f'Flatpak bundle: {output.relative_to(ROOT)}')


if __name__ == '__main__':
    build()
