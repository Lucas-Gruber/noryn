#!/usr/bin/env python3
"""Offline release tests: package discovery, streamed uploads and safe retry behavior."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import runpy
import shutil
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import urllib.parse
from release_version import validate_release, version_from_tag
from update_manifest import assemble, asset_url, validate_signature

ROOT = Path(__file__).resolve().parents[1]

@contextlib.contextmanager
def working_directory(path):
    previous = Path.cwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(previous)

def manifest_fixture(root, version='1.2.3'):
    (root / 'package.json').write_text(json.dumps({'version': version}))
    (root / 'src-tauri').mkdir(exist_ok=True)
    (root / 'src-tauri/tauri.conf.json').write_text(json.dumps({'version': version}))
    members = {'src-tauri': 'noryn', 'crates/core': 'noryn-core', 'server': 'noryn-sync'}
    (root / 'Cargo.toml').write_text('[workspace]\nmembers = ' + json.dumps(list(members)) + '\n')
    locked = []
    for folder, name in members.items():
        (root / folder).mkdir(parents=True, exist_ok=True)
        package = f'name = "{name}"\nversion = "{version}"\n'
        (root / folder / 'Cargo.toml').write_text('[package]\n' + package)
        locked.append('[[package]]\n' + package)
    (root / 'Cargo.lock').write_text('\n'.join(locked))


class ReleaseTests(unittest.TestCase):
    def test_native_asset_layouts_and_checksums(self):
        for system, machine, files, expected in [
            ('Linux', 'x86_64', ['appimage/Noryn.AppImage', 'deb/Noryn.deb', 'flatpak/Noryn.flatpak'], ['linux-x64.AppImage', 'linux-x64.deb', 'linux-x64.flatpak']),
            ('Windows', 'AMD64', ['nsis/Noryn-setup.exe'], ['windows-x64-portable.exe', 'windows-x64-setup.exe']),
            ('Darwin', 'arm64', ['macos/Noryn.app/Contents/MacOS/noryn', 'dmg/Noryn.dmg'], ['macos-arm64-portable.app.zip', 'macos-arm64.dmg']),
        ]:
            with self.subTest(system=system), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / 'scripts').mkdir()
                script = root / 'scripts/stage-release.py'
                shutil.copy(ROOT / 'scripts/stage-release.py', script)
                manifest_fixture(root)
                for name in files + ['../noryn.exe']:
                    target = root / 'target/release/bundle' / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(b'native output')
                def ditto(args, **kwargs):
                    self.assertEqual(args[0], 'ditto')
                    Path(args[-1]).write_bytes(b'app zip')
                output_file = root / 'github-output'
                with patch('platform.system', return_value=system), patch('platform.machine', return_value=machine), patch('subprocess.run', side_effect=ditto), patch.dict(os.environ, {'RELEASE_TAG': '1.2.3' if system == 'Windows' else 'v1.2.3', 'GITHUB_OUTPUT': str(output_file)}):
                    runpy.run_path(str(script))
                outputs = dict(line.split('=', 1) for line in output_file.read_text().splitlines())
                kinds = ['portable', 'installer'] + (['flatpak'] if system == 'Linux' else [])
                self.assertEqual(set(outputs), set(kinds) | {kind + '_sha256' for kind in kinds})
                expected_names = {f'Noryn-1.2.3-{suffix}' for suffix in expected}
                self.assertEqual({p.name for p in (root / 'release-assets').iterdir()}, expected_names | {name + '.sha256' for name in expected_names})
                for kind, suffix in zip(kinds, expected):
                    relative = f'release-assets/Noryn-1.2.3-{suffix}'
                    self.assertEqual(outputs[kind], relative)
                    self.assertEqual(outputs[kind + '_sha256'], relative + '.sha256')
                for suffix in expected:
                    asset = root / 'release-assets' / f'Noryn-1.2.3-{suffix}'
                    self.assertTrue(asset.is_file())
                    self.assertEqual(asset.with_name(asset.name + '.sha256').read_text(), f'{hashlib.sha256(asset.read_bytes()).hexdigest()}  {asset.name}\n')
                with patch.dict(os.environ, {'RELEASE_TAG': 'v1.2.4'}):
                    with self.assertRaises(SystemExit):
                        runpy.run_path(str(script))

    def test_release_requires_flatpak_when_requested(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest_fixture(root)
            (root / 'scripts').mkdir()
            script = root / 'scripts/stage-release.py'
            shutil.copy(ROOT / 'scripts/stage-release.py', script)
            for name in ['appimage/Noryn.AppImage', 'deb/Noryn.deb']:
                path = root / 'target/release/bundle' / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'package')
            with patch('platform.system', return_value='Linux'), patch.dict(os.environ, {'NORYN_REQUIRE_FLATPAK': '1', 'RELEASE_TAG': 'v1.2.3'}):
                with self.assertRaisesRegex(SystemExit, 'Expected one flatpak'):
                    runpy.run_path(str(script))

    def test_signed_update_staging_and_complete_feed(self):
        fixture = ROOT / 'src-tauri/tests/fixtures/updater'
        signature = (fixture / 'payload.txt.sig').read_text().strip()
        public = (fixture / 'public.pub').read_text().strip()
        with tempfile.TemporaryDirectory() as temporary:
            download = Path(temporary) / 'artifacts'
            download.mkdir()
            for system, machine, files, payload in [
                ('Linux', 'x86_64', ['appimage/Noryn.AppImage', 'deb/Noryn.deb'], 'appimage/Noryn.AppImage'),
                ('Windows', 'AMD64', ['nsis/Noryn-setup.exe', '../noryn.exe'], 'nsis/Noryn-setup.exe'),
                ('Darwin', 'arm64', ['macos/Noryn.app/Contents/MacOS/noryn', 'macos/Noryn.app.tar.gz', 'dmg/Noryn.dmg'], 'macos/Noryn.app.tar.gz'),
            ]:
                with self.subTest(system=system), tempfile.TemporaryDirectory() as directory:
                    root = Path(directory)
                    manifest_fixture(root, '99.0.0')
                    config = root / 'src-tauri/tauri.conf.json'
                    config.write_text(json.dumps({'version': '99.0.0', 'plugins': {'updater': {'pubkey': public}}}))
                    (root / 'scripts').mkdir()
                    script = root / 'scripts/stage-release.py'
                    shutil.copy(ROOT / 'scripts/stage-release.py', script)
                    for name in files:
                        path = root / 'target/release/bundle' / name
                        path.parent.mkdir(parents=True, exist_ok=True)
                        path.write_bytes((fixture / 'payload.txt').read_bytes())
                    source = root / 'target/release/bundle' / payload
                    source.with_name(source.name + '.sig').write_text(signature)
                    def ditto(args, **kwargs):
                        Path(args[-1]).write_bytes(b'portable app archive')
                    env = {'RELEASE_TAG': '99.0.0', 'NORYN_REQUIRE_UPDATER': '1', 'GITHUB_REPOSITORY': 'owner/repo', 'GITHUB_OUTPUT': str(root / 'outputs')}
                    with patch('platform.system', return_value=system), patch('platform.machine', return_value=machine), patch('subprocess.run', side_effect=ditto), patch.dict(os.environ, env):
                        runpy.run_path(str(script))
                    outputs = dict(line.split('=', 1) for line in (root / 'outputs').read_text().splitlines())
                    self.assertIn('updater_signature', outputs)
                    self.assertIn('updater_fragment', outputs)
                    if system == 'Darwin':
                        self.assertTrue(outputs['updater_package'].endswith('.app.tar.gz'))
                    for file in (root / 'release-assets').iterdir():
                        shutil.copy(file, download / file.name)
            manifest = assemble(download, 'owner/repo', '99.0.0', public)
            self.assertEqual(set(manifest['platforms']), {'windows-x86_64', 'linux-x86_64', 'darwin-aarch64'})
            for entry in manifest['platforms'].values():
                self.assertIn('/releases/download/99.0.0/', entry['url'])
                self.assertEqual(entry['signature'], signature)
            windows = next(download.glob('*windows-*-update.json'))
            original = windows.read_text()
            broken = json.loads(original)
            broken['version'] = '98.0.0'
            windows.write_text(json.dumps(broken))
            with self.assertRaisesRegex(SystemExit, 'different version'):
                assemble(download, 'owner/repo', '99.0.0', public)
            windows.write_text(original)
            with self.assertRaisesRegex(SystemExit, 'outside this release'):
                assemble(download, 'other/repo', '99.0.0', public)
            windows.unlink()
            with self.assertRaisesRegex(SystemExit, 'complete Windows'):
                assemble(download, 'owner/repo', '99.0.0', public)
            with self.assertRaisesRegex(SystemExit, 'bound to the release version'):
                validate_signature(signature, '99.0.1', public)

    def run_publisher(self, existing=None, changed=False, fail_upload=False, asset_names=None, tag='v1.2.3', published_release=False):
        calls = []
        published = []
        binary = b'example installer'
        class Opener:
            def open(inner, req, timeout):
                self.assertEqual(req.get_header('Authorization'), 'Bearer fake-test-token')
                path = req.full_url
                data = req.data.read() if hasattr(req.data, 'read') else req.data
                calls.append((req.get_method(), path, data, req.headers))
                if '/releases/assets/' in path:
                    return io.BytesIO(b'changed' if changed else binary)
                if '/releases/tags/' in path:
                    if existing is None:
                        raise urllib.error.HTTPError(path, 404, 'Not Found', {}, io.BytesIO())
                    response = {'id': 7, 'draft': not published_release, 'html_url': 'https://github.invalid/release/7', 'upload_url': 'https://uploads.github.com/repos/owner/repo/releases/7/assets{?name,label}'}
                elif req.get_method() == 'POST' and path.endswith('/releases'):
                    response = json.loads(data)
                    self.assertTrue(response['draft'])
                    self.assertEqual(response['tag_name'], tag)
                    self.assertIn('ghcr.io/owner/repo:1.2.3`', response['body'])
                    response.update(id=7, html_url='https://github.invalid/release/7', upload_url='https://uploads.github.com/repos/owner/repo/releases/7/assets{?name,label}')
                elif req.get_method() == 'GET' and '/assets?per_page=' in path:
                    response = [] if existing is None else [{'name': 'Noryn-portable.exe', 'id': 99}]
                elif req.get_method() == 'POST' and '/assets?' in path:
                    if fail_upload:
                        raise urllib.error.HTTPError(path, 500, 'Upload failed', {}, io.BytesIO())
                    self.assertIn(binary, data)
                    self.assertEqual(int(req.get_header('Content-length')), len(data))
                    response = {'id': 99}
                elif req.get_method() == 'PATCH':
                    published.append(json.loads(data))
                    response = {}
                else:
                    raise AssertionError((req.get_method(), path))
                return io.BytesIO(json.dumps(response).encode())
        with tempfile.TemporaryDirectory() as temporary, working_directory(temporary):
            Path('artifacts').mkdir()
            for name in asset_names or ['Noryn-portable.exe']:
                Path('artifacts', name).write_bytes(binary)
            environment = {'GITHUB_API_URL': 'https://api.github.invalid', 'GITHUB_REPOSITORY': 'owner/repo', 'RELEASE_TAG': tag, 'GITHUB_TOKEN': 'fake-test-token'}
            with patch.dict(os.environ, environment), patch('urllib.request.build_opener', return_value=Opener()), contextlib.redirect_stdout(io.StringIO()):
                if changed or fail_upload:
                    with self.assertRaises((SystemExit, urllib.error.HTTPError)) as caught:
                        runpy.run_path(str(ROOT / 'scripts/publish-release.py'))
                    if isinstance(caught.exception, urllib.error.HTTPError):
                        caught.exception.close()
                else:
                    runpy.run_path(str(ROOT / 'scripts/publish-release.py'))
        return calls, published

    def test_streamed_upload_publishes_draft_only_after_success(self):
        calls, published = self.run_publisher()
        self.assertEqual(published, [{'draft': False}])
        self.assertTrue(any(method == 'POST' and '/assets?' in path for method, path, _, _ in calls))

    def test_each_native_file_and_checksum_is_a_separate_release_asset(self):
        names = ['Noryn-windows-portable.exe', 'Noryn-windows-setup.exe', 'Noryn-linux.AppImage', 'Noryn-linux.deb', 'Noryn-macos-portable.app.zip', 'Noryn-macos.dmg', 'latest.json']
        names += [name + '.sha256' for name in names if name != 'latest.json']
        calls, published = self.run_publisher(asset_names=names)
        uploads = [urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)['name'][0] for method, url, _, _ in calls if method == 'POST' and '/assets?' in url]
        self.assertCountEqual(uploads, names)
        self.assertEqual(uploads[-1], 'latest.json')
        self.assertEqual(published, [{'draft': False}])

    def test_unprefixed_tag_creates_a_release_and_uses_the_full_image_version(self):
        calls, published = self.run_publisher(tag='1.2.3')
        self.assertEqual(published, [{'draft': False}])
        self.assertTrue(any('/releases/tags/1.2.3' in url for _, url, _, _ in calls))

    def test_existing_published_release_gets_missing_assets_without_republishing(self):
        calls, published = self.run_publisher(existing=True, tag='1.2.3', published_release=True, asset_names=['Noryn-linux.AppImage'])
        self.assertTrue(any(method == 'POST' and '/assets?' in url for method, url, _, _ in calls))
        self.assertEqual(published, [])

    def test_release_version_accepts_both_tag_styles_and_rejects_mismatches(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            manifest_fixture(root)
            for tag in ['v1.2.3', '1.2.3']:
                self.assertEqual(validate_release(root, tag), '1.2.3')
            for file in ['package.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'crates/core/Cargo.toml', 'server/Cargo.toml', 'Cargo.lock']:
                with self.subTest(file=file):
                    path = root / file
                    original = path.read_text()
                    path.write_text(original.replace('1.2.3', '1.2.4'))
                    with self.assertRaisesRegex(SystemExit, 'Mismatches:'):
                        validate_release(root, '1.2.3')
                    path.write_text(original)
            with self.assertRaisesRegex(SystemExit, 'requires version 0.0.2'):
                validate_release(root, '0.0.2')
            manifest_fixture(root, '1.2.3-rc.1')
            self.assertEqual(validate_release(root, 'v1.2.3-rc.1'), '1.2.3-rc.1')

    def test_non_version_tags_are_rejected_before_publishing(self):
        for tag in ['', 'main', 'release/1.2.3', 'vv1.2.3', '1.2', '01.2.3', '1.2.3-rc.01', '1.2.3\n', '1.2.3;echo hi']:
            with self.subTest(tag=tag), self.assertRaises(SystemExit):
                version_from_tag(tag)

    def test_failed_upload_leaves_draft_unpublished(self):
        _, published = self.run_publisher(fail_upload=True)
        self.assertEqual(published, [])

    def test_identical_retry_authenticates_private_download_and_skips_upload(self):
        calls, published = self.run_publisher(existing=True)
        self.assertEqual(published, [{'draft': False}])
        self.assertFalse(any(method == 'POST' for method, _, _, _ in calls))

    def test_changed_retry_never_replaces_existing_asset(self):
        calls, published = self.run_publisher(existing=True, changed=True)
        self.assertEqual(published, [])
        self.assertFalse(any(method in ['DELETE', 'PATCH', 'POST'] for method, _, _, _ in calls))

if __name__ == '__main__':
    unittest.main()
