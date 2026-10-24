#!/usr/bin/env python3
"""Publish GitHub release assets without replacing already published bytes."""
import hashlib
import json
import os
from pathlib import Path
import urllib.error
import urllib.parse
import urllib.request
from release_version import version_from_tag

server = os.environ.get('GITHUB_API_URL', 'https://api.github.com').rstrip('/')
repository = os.environ['GITHUB_REPOSITORY']
tag = os.environ['RELEASE_TAG']
token = os.environ['GITHUB_TOKEN']
version = version_from_tag(tag)
if not server.startswith('https://') or not token:
    raise SystemExit('HTTPS GitHub API, version tag and token are required')
api = f'{server}/repos/{repository}'
# Upload the feed last, so an existing published release never advertises incomplete packages.
assets = sorted((p for p in Path('artifacts').rglob('*') if p.is_file() and not p.name.endswith('-update.json')), key=lambda p: (p.name == 'latest.json', p.name))
if not assets or len({p.name for p in assets}) != len(assets):
    raise SystemExit('Release artifacts are missing or have duplicate names')

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

def request(method, url, payload=None, content_type=None):
    headers = {'Authorization': f'Bearer {token}', 'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28'}
    if isinstance(payload, dict):
        payload = json.dumps(payload).encode()
        content_type = 'application/json'
    if content_type:
        headers['Content-Type'] = content_type
    if payload is not None and hasattr(payload, 'fileno'):
        headers['Content-Length'] = str(os.fstat(payload.fileno()).st_size)
    req = urllib.request.Request(url, data=payload, headers=headers, method=method)
    with urllib.request.build_opener(NoRedirect).open(req, timeout=300) as response:
        return json.load(response)

try:
    release = request('GET', api + '/releases/tags/' + urllib.parse.quote(tag, safe=''))
except urllib.error.HTTPError as error:
    if error.code != 404:
        raise
    error.close()
    release = request('POST', api + '/releases', {'tag_name': tag, 'name': f'Noryn {tag}', 'body': 'Portable applications and installers for Windows, macOS and Linux, with SHA-256 checksums.\n\nServer image: `ghcr.io/' + repository.lower() + ':' + version + '`', 'draft': True, 'prerelease': '-' in tag})
endpoint = f'{api}/releases/{release["id"]}'
existing = {asset['name']: asset for asset in request('GET', endpoint + '/assets?per_page=100')}
upload = release['upload_url'].split('{', 1)[0]
parsed = urllib.parse.urlsplit(upload)
if parsed.scheme != 'https' or parsed.netloc not in {'uploads.github.com', urllib.parse.urlsplit(server).netloc}:
    raise SystemExit('Unexpected release upload destination')

class DownloadRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlsplit(newurl).scheme != 'https':
            raise SystemExit('Release download redirected outside HTTPS')
        result = super().redirect_request(req, fp, code, msg, headers, newurl)
        if result and urllib.parse.urlsplit(newurl).netloc != urllib.parse.urlsplit(server).netloc:
            result.remove_header('Authorization')
        return result

def remote_digest(asset):
    # The authenticated API URL works for private repositories too.
    url = f'{api}/releases/assets/{asset["id"]}'
    req = urllib.request.Request(url, headers={'Authorization': f'Bearer {token}', 'Accept': 'application/octet-stream'})
    with urllib.request.build_opener(DownloadRedirect).open(req, timeout=300) as response:
        digest = hashlib.sha256()
        while chunk := response.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()

for asset in assets:
    if asset.name in existing:
        with asset.open('rb') as source:
            expected = hashlib.file_digest(source, 'sha256').hexdigest()
        if remote_digest(existing[asset.name]) != expected:
            raise SystemExit(f'Existing release asset differs: {asset.name}; use a new version tag')
        print(f'Already uploaded: {asset.name}')
        continue
    with asset.open('rb') as source:
        request('POST', upload + '?name=' + urllib.parse.quote(asset.name), source, 'application/octet-stream')
    print(f'Uploaded: {asset.name}')
if release['draft']:
    request('PATCH', endpoint, {'draft': False})
print(f'Release ready: {release["html_url"]}')
