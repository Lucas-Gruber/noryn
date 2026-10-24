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

ROOT = Path(__file__).resolve().parents[1]

@contextlib.contextmanager
def working_directory(path):
    previous = Path.cwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(previous)

class ReleaseTests(unittest.TestCase):
    def test_native_asset_layouts_and_checksums(self):
        for system, machine, files, expected in [
            ('Linux', 'x86_64', ['appimage/Noryn.AppImage', 'deb/Noryn.deb'], ['linux-x64-portable.AppImage', 'linux-x64.deb']),
            ('Windows', 'AMD64', ['nsis/Noryn-setup.exe'], ['windows-x64-portable.exe', 'windows-x64-setup.exe']),
            ('Darwin', 'arm64', ['macos/Noryn.app/Contents/MacOS/noryn', 'dmg/Noryn.dmg'], ['macos-arm64-portable.app.zip', 'macos-arm64.dmg']),
        ]:
            with self.subTest(system=system), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / 'scripts').mkdir()
                script = root / 'scripts/stage-release.py'
                shutil.copy(ROOT / 'scripts/stage-release.py', script)
                (root / 'package.json').write_text('{"version":"1.2.3"}')
                for name in files + ['../noryn.exe']:
                    target = root / 'target/release/bundle' / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(b'native output')
                def ditto(args, **kwargs):
                    self.assertEqual(args[0], 'ditto')
                    Path(args[-1]).write_bytes(b'app zip')
                output_file = root / 'github-output'
                with patch('platform.system', return_value=system), patch('platform.machine', return_value=machine), patch('subprocess.run', side_effect=ditto), patch.dict(os.environ, {'RELEASE_TAG': 'v1.2.3', 'GITHUB_OUTPUT': str(output_file)}):
                    runpy.run_path(str(script))
                outputs = dict(line.split('=', 1) for line in output_file.read_text().splitlines())
                self.assertEqual(set(outputs), {'portable', 'installer', 'portable_sha256', 'installer_sha256'})
                expected_names = {f'Noryn-1.2.3-{suffix}' for suffix in expected}
                self.assertEqual({p.name for p in (root / 'release-assets').iterdir()}, expected_names | {name + '.sha256' for name in expected_names})
                for kind, suffix in zip(['portable', 'installer'], expected):
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

    def run_publisher(self, existing=None, changed=False, fail_upload=False, asset_names=None):
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
                    response = {'id': 7, 'draft': True, 'html_url': 'https://github.invalid/release/7', 'upload_url': 'https://uploads.github.com/repos/owner/repo/releases/7/assets{?name,label}'}
                elif req.get_method() == 'POST' and path.endswith('/releases'):
                    response = json.loads(data)
                    self.assertTrue(response['draft'])
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
            environment = {'GITHUB_API_URL': 'https://api.github.invalid', 'GITHUB_REPOSITORY': 'owner/repo', 'RELEASE_TAG': 'v1.2.3', 'GITHUB_TOKEN': 'fake-test-token'}
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
        names = ['Noryn-windows-portable.exe', 'Noryn-windows-setup.exe', 'Noryn-linux.AppImage', 'Noryn-linux.deb', 'Noryn-macos-portable.app.zip', 'Noryn-macos.dmg']
        names += [name + '.sha256' for name in names]
        calls, published = self.run_publisher(asset_names=names)
        uploads = [urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)['name'][0] for method, url, _, _ in calls if method == 'POST' and '/assets?' in url]
        self.assertCountEqual(uploads, names)
        self.assertEqual(published, [{'draft': False}])

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
