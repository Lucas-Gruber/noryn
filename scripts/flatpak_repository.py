#!/usr/bin/env python3
"""Signed Flatpak repository for GitHub Pages; no private key enters a build tree."""
import base64
import contextlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from bump_version import version_key
from release_version import version_from_tag

ROOT = Path(__file__).resolve().parents[1]
KEY = ROOT / 'packaging/flatpak/repository.asc'


def run(*args, **kwargs):
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def repository_url(config):
    url = config['repositoryUrl']
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != 'https' or not parts.hostname or parts.username or parts.password or parts.query or parts.fragment or not url.endswith('/'):
        raise SystemExit('Flatpak repositoryUrl must be a public HTTPS directory URL')
    return url


def fingerprint(home, key):
    data = run('gpg', '--homedir', home, '--batch', '--with-colons', '--show-keys', key, capture_output=True).stdout.decode()
    keys = re.findall(r'^pub:', data, re.M)
    fingerprints = re.findall(r'^fpr:::::::::([A-F0-9]+):', data, re.M)
    if len(keys) != 1 or not fingerprints:
        raise SystemExit('Exactly one Flatpak signing identity is required')
    return fingerprints[0]


@contextlib.contextmanager
def signing():
    with tempfile.TemporaryDirectory(prefix='noryn-flatpak-signing-') as temporary:
        home = Path(temporary)
        home.chmod(0o700)
        try:
            expected = fingerprint(home, KEY)
            encoded = os.environ.get('FLATPAK_GPG_PRIVATE_KEY')
            key_file = os.environ.get('FLATPAK_GPG_PRIVATE_KEY_FILE')
            if encoded:
                private = base64.b64decode(encoded, validate=True)
            elif key_file and not os.environ.get('GITHUB_ACTIONS'):
                private = Path(key_file).read_bytes()
            else:
                raise SystemExit('Configure FLATPAK_GPG_PRIVATE_KEY (base64) in Actions secrets')
            run('gpg', '--homedir', home, '--batch', '--import', input=private, capture_output=True)
            secret = run('gpg', '--homedir', home, '--batch', '--with-colons', '--list-secret-keys', expected, capture_output=True).stdout
            if b'sec:' not in secret:
                raise SystemExit('Flatpak private key does not match the committed public key')
            yield [f'--gpg-homedir={home}', f'--gpg-sign={expected}']
        finally:
            run('gpgconf', '--homedir', home, '--kill', 'gpg-agent', capture_output=True)


def read_state(url):
    class HttpsRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, headers, newurl):
            if urllib.parse.urlsplit(newurl).scheme != 'https':
                raise SystemExit('Flatpak state redirected outside HTTPS')
            return super().redirect_request(req, fp, code, msg, headers, newurl)
    try:
        with urllib.request.build_opener(HttpsRedirect).open(url + 'state.json', timeout=30) as response:
            if urllib.parse.urlsplit(response.url).scheme != 'https':
                raise SystemExit('Flatpak state redirected outside HTTPS')
            data = response.read(16_385)
            if len(data) > 16_384:
                raise SystemExit('Flatpak state is too large')
            return json.loads(data)
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        error.close()
        return None


def seed(repo, config, version, arch):
    """Retain the preceding signed commit, so in-flight clients can finish pulling it."""
    url = repository_url(config)
    state = read_state(url)
    run('ostree', f'--repo={repo}', 'init', '--mode=archive-z2')
    if state is None:
        return
    previous = version_from_tag(state['version'])
    if version_key(version) <= version_key(previous):
        raise SystemExit(f'Pages already serves {previous}; use a newer release version')
    run('ostree', f'--repo={repo}', 'remote', 'add', f'--gpg-import={KEY}', 'previous', url)
    run('ostree', f'--repo={repo}', 'pull', '--mirror', '--depth=0', 'previous')
    run('ostree', f'--repo={repo}', 'remote', 'delete', 'previous')


def descriptors(site, config):
    url = repository_url(config)
    key = base64.b64encode(KEY.read_bytes()).decode()
    (site / 'Noryn.flatpakrepo').write_text(f'[Flatpak Repo]\nTitle=Noryn\nUrl={url}\nHomepage=https://github.com/Lucas-Gruber/noryn\nDefaultBranch=stable\nGPGKey={key}\n')
    (site / 'Noryn.flatpakref').write_text(f'[Flatpak Ref]\nName={config["appId"]}\nBranch=stable\nTitle=Noryn\nIsRuntime=false\nUrl={url}\nGPGKey={key}\nRuntimeRepo={config["runtimeRepository"]}\nSuggestRemoteName=noryn\n')
    (site / '.nojekyll').touch()
    (site / 'index.html').write_text('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Noryn Flatpak</title><body><h1>Noryn.</h1><p><a href="Noryn.flatpakref">Install Noryn with Flatpak updates</a></p><p>Your software manager will ask to trust the signed Noryn repository.</p><p><a href="https://github.com/Lucas-Gruber/noryn/releases">All releases and other platforms</a></p></body></html>')


def finish(site, repo, config, version, options):
    run('flatpak', 'build-update-repo', *options, '--generate-static-deltas', '--prune', '--prune-depth=1', '--title=Noryn', '--default-branch=stable', repo)
    # Pages allows at most 1 GiB; fail before producing an undeployable artifact.
    size = sum(p.stat().st_size for p in site.rglob('*') if p.is_file())
    if size > 900 * 1024 * 1024:
        raise SystemExit('Flatpak Pages site exceeds the 900 MiB safety budget; use dedicated hosting')
    (repo / 'state.json').write_text(json.dumps({'version': version}) + '\n')
    descriptors(site, config)


def init_key():
    """One dedicated CI signing key, with only its public half committed."""
    private = Path.home() / '.config/dev.noryn.client/release-signing/flatpak-repository.key'
    if KEY.exists() or private.exists():
        raise SystemExit('A Flatpak signing key already exists; never replace an established identity')
    private.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='noryn-flatpak-key-') as home:
        run('gpg', '--homedir', home, '--batch', '--pinentry-mode', 'loopback', '--passphrase', '', '--quick-generate-key', 'Noryn Flatpak Release Signing', 'ed25519', 'sign', '0', capture_output=True)
        public = run('gpg', '--homedir', home, '--batch', '--armor', '--export', capture_output=True).stdout
        secret = run('gpg', '--homedir', home, '--batch', '--export-secret-keys', capture_output=True).stdout
        with os.fdopen(os.open(private, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), 'wb') as output:
            output.write(secret)
        KEY.write_bytes(public)
        run('gpgconf', '--homedir', home, '--kill', 'gpg-agent', capture_output=True)
    print(f'Public key: {KEY}\nPrivate key (do not commit): {private}')


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['init-key', 'check-signing'])
    action = parser.parse_args().action
    if action == 'init-key':
        init_key()
    else:
        with signing() as options, tempfile.TemporaryDirectory() as temporary:
            home, identity = (option.split('=', 1)[1] for option in options)
            probe = Path(temporary) / 'probe'
            probe.write_text('Noryn Flatpak signing configuration check\n')
            run('gpg', '--homedir', home, '--batch', '--pinentry-mode', 'loopback', '--passphrase', '', '--local-user', identity, '--detach-sign', probe, capture_output=True)
            run('gpg', '--homedir', home, '--batch', '--verify', str(probe) + '.sig', probe, capture_output=True)
        print('Flatpak signing configuration verified')
