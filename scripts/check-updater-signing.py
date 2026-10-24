#!/usr/bin/env python3
"""Probe CI signing without printing key material, before expensive native builds."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
from update_manifest import validate_signature

root = Path(__file__).resolve().parents[1]
if not os.environ.get('TAURI_SIGNING_PRIVATE_KEY'):
    raise SystemExit('Missing TAURI_SIGNING_PRIVATE_KEY in Actions repository secrets')
config = json.loads((root / 'src-tauri/tauri.conf.json').read_text())
with tempfile.TemporaryDirectory(prefix='noryn-signing-check-') as directory:
    probe = Path(directory) / 'probe'
    probe.write_bytes(b'Noryn updater signing configuration probe\n')
    result = subprocess.run(['node', str(root / 'node_modules/@tauri-apps/cli/tauri.js'), 'signer', 'sign', '--app-version', config['version'], str(probe)], capture_output=True, cwd=root, timeout=60)
    if result.returncode:
        raise SystemExit('Updater signing failed. Check TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD; no secret material was logged.')
    validate_signature(probe.with_name(probe.name + '.sig').read_text().strip(), config['version'], config['plugins']['updater']['pubkey'])
print('Updater signing key, password, public key and signed version match.')
