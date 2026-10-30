#!/usr/bin/env python3
"""Real signed repositories and bundle origins, entirely in temporary directories."""
import base64
import configparser
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import flatpak_repository as repository


@unittest.skipUnless(all(shutil.which(tool) for tool in ['flatpak', 'ostree', 'gpg']), 'Flatpak, OSTree and GPG required')
class RepositoryTests(unittest.TestCase):
    def test_signed_bundle_configures_origin_and_wrong_signing_keys_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home = root / 'gnupg'
            home.mkdir(mode=0o700)
            def run(*args, **kwargs):
                return subprocess.run(list(map(str, args)), check=True, capture_output=True, timeout=60, **kwargs)
            run('gpg', '--homedir', home, '--batch', '--pinentry-mode', 'loopback', '--passphrase', '', '--quick-generate-key', 'Temporary Flatpak Test', 'ed25519', 'sign', '0')
            key = root / 'public.asc'
            key.write_bytes(run('gpg', '--homedir', home, '--armor', '--export').stdout)
            secret = run('gpg', '--homedir', home, '--export-secret-keys').stdout
            config = {'appId': 'dev.noryn.client', 'repositoryUrl': 'https://example.invalid/flatpak/', 'runtimeRepository': 'https://dl.flathub.org/repo/flathub.flatpakrepo'}
            site, tree = root / 'site', root / 'build'
            site.mkdir()
            (tree / 'files/bin').mkdir(parents=True)
            (tree / 'export').mkdir()
            (tree / 'files/bin/noryn').write_text('#!/bin/sh\nexit 0\n')
            (tree / 'files/bin/noryn').chmod(0o755)
            arch = run('flatpak', '--default-arch').stdout.decode().strip()
            (tree / 'metadata').write_text(f'[Application]\nname=dev.noryn.client\nruntime=org.gnome.Platform/{arch}/51\nsdk=org.gnome.Platform/{arch}/51\ncommand=noryn\n')
            repo = site / 'flatpak'
            env = {'FLATPAK_GPG_PRIVATE_KEY': base64.b64encode(secret).decode()}
            try:
                with patch.object(repository, 'KEY', key), patch.dict(os.environ, env), patch.object(repository, 'read_state', return_value=None):
                    repository.seed(repo, config, '1.2.3', arch)
                    with repository.signing() as options:
                        run('flatpak', 'build-export', *options, repo, tree, 'stable')
                        repository.finish(site, repo, config, '1.2.3', options)
                    bundle = root / 'fixture.flatpak'
                    run('flatpak', 'build-bundle', f'--gpg-keys={key}', f'--repo-url={config["repositoryUrl"]}', repo, bundle, config['appId'], 'stable')
                # Use a completely separate installation and do not deploy or start the app.
                isolated = dict(os.environ, XDG_DATA_HOME=str(root / 'data'), XDG_CONFIG_HOME=str(root / 'config'), XDG_CACHE_HOME=str(root / 'cache'))
                run('flatpak', '--user', 'install', '--noninteractive', '--assumeyes', '--no-deps', '--no-deploy', bundle, env=isolated)
                remotes = run('flatpak', '--user', 'remotes', '--columns=url', env=isolated).stdout.decode()
                self.assertIn(config['repositoryUrl'], remotes)
                installed_config = configparser.ConfigParser()
                installed_config.read(root / 'data/flatpak/repo/config')
                origins = [section for section in installed_config.sections() if section.startswith('remote ')]
                self.assertTrue(origins)
                self.assertTrue(all(installed_config.getboolean(section, 'gpg-verify', fallback=True) for section in origins))
                parser = configparser.ConfigParser()
                parser.read(site / 'Noryn.flatpakref')
                ref = parser['Flatpak Ref']
                self.assertEqual(ref['Url'], config['repositoryUrl'])
                self.assertEqual(base64.b64decode(ref['GPGKey']), key.read_bytes())
                self.assertEqual(ref['RuntimeRepo'], config['runtimeRepository'])
                self.assertEqual(json.loads((repo / 'state.json').read_text()), {'version': '1.2.3'})
                for path in site.rglob('*'):
                    if path.is_file():
                        self.assertNotIn(secret, path.read_bytes(), 'Private key must never enter the published repository')
                # Test the real OSTree signature enforcement. Only the fixture transport
                # is replaced with file://; production repository_url rejects it.
                with patch.object(repository, 'KEY', key), patch.object(repository, 'read_state', return_value={'version': '1.2.3'}), patch.object(repository, 'repository_url', return_value=repo.as_uri() + '/'):
                    repository.seed(root / 'mirror', config, '1.2.4', arch)
                original = run('ostree', f'--repo={repo}', 'rev-parse', f'app/dev.noryn.client/{arch}/stable').stdout
                mirrored = run('ostree', f'--repo={root / "mirror"}', 'rev-parse', f'app/dev.noryn.client/{arch}/stable').stdout
                self.assertEqual(original, mirrored)
                (tree / 'files/bin/noryn').write_text('unsigned different payload\n')
                run('flatpak', 'build-export', root / 'unsigned', tree, 'stable')
                with patch.object(repository, 'KEY', key), patch.object(repository, 'read_state', return_value={'version': '1.2.3'}), patch.object(repository, 'repository_url', return_value=(root / 'unsigned').as_uri() + '/'):
                    with self.assertRaises(subprocess.CalledProcessError):
                        repository.seed(root / 'reject-unsigned', config, '1.2.4', arch)
                # A public key alone must not pass the build's signing check.
                with patch.object(repository, 'KEY', key), patch.dict(os.environ, {'FLATPAK_GPG_PRIVATE_KEY': base64.b64encode(key.read_bytes()).decode()}):
                    with self.assertRaises((SystemExit, subprocess.CalledProcessError)):
                        with repository.signing():
                            self.fail('Public material is not a signing key')
            finally:
                run('gpgconf', '--homedir', home, '--kill', 'gpg-agent')

    def test_https_and_old_releases_are_rejected_and_only_404_means_first_publish(self):
        config = {'repositoryUrl': 'https://example.invalid/flatpak/'}
        for url in ['http://example.invalid/', 'https://user:password@example.invalid/', 'https://example.invalid/?x=1', 'https://example.invalid/#test', 'file:///tmp/repo', 'https://example.invalid/repo']:
            with self.subTest(url=url), self.assertRaises(SystemExit):
                repository.repository_url({'repositoryUrl': url})
        with patch.object(repository, 'read_state', return_value={'version': '2.0.0'}), patch.object(repository, 'run'):
            for version in ['1.9.9', '2.0.0']:
                with self.assertRaises(SystemExit):
                    repository.seed(Path('/fixture-only'), config, version, 'x86_64')
        for code in [404, 403, 500]:
            error = urllib.error.HTTPError('https://example.invalid/', code, 'fixture', {}, None)
            with patch('urllib.request.OpenerDirector.open', side_effect=error):
                if code == 404:
                    self.assertIsNone(repository.read_state(config['repositoryUrl']))
                else:
                    with self.assertRaises(urllib.error.HTTPError):
                        repository.read_state(config['repositoryUrl'])


if __name__ == '__main__':
    unittest.main()
