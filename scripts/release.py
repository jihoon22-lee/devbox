"""Validate an annotated production tag; optionally publish its verified installer."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib
from dogfood import REPO, command, gh_json, verify

def require(condition, message):
    if not condition:
        raise ValueError(message)

def preflight():
    ref = os.environ['GITHUB_REF']
    if os.environ['GITHUB_EVENT_NAME'] != 'push' or not ref.startswith('refs/tags/v'):
        return None
    tag = ref.removeprefix('refs/tags/')
    require(re.fullmatch(r'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', tag), 'Invalid stable release tag')
    sha = os.environ['SOURCE_SHA']
    require(re.fullmatch('[0-9a-f]{40}', sha) and os.environ['GITHUB_REPOSITORY'] == REPO, 'Invalid source/repository')
    versions = [json.loads(Path(p).read_text())['version'] for p in ['app/package.json', 'app/src-tauri/tauri.conf.json']]
    versions += [tomllib.loads(Path('Cargo.toml').read_text())['workspace']['package']['version'], tomllib.loads(Path('app/src-tauri/Cargo.toml').read_text())['package']['version']]
    require(all(version == tag[1:] for version in versions), 'Tag/manifest mismatch')
    annotation = subprocess.check_output(('git', 'cat-file', 'tag', ref)).decode('utf-8')
    require(command('git', 'rev-parse', 'HEAD') == sha == command('git', 'rev-parse', ref + '^{commit}'), 'Tag/source mismatch')
    remote = command('git', 'ls-remote', '--exit-code', 'origin', ref, ref + '^{}')
    require(set(remote.splitlines()) == {command('git', 'rev-parse', ref) + '\t' + ref, sha + '\t' + ref + '^{}'}, 'Remote annotated tag mismatch')
    command('git', 'fetch', '--no-tags', 'origin', '+refs/heads/main:refs/remotes/origin/main')
    try:  # Only commits that reached main (and its required PR CI) may be published.
        command('git', 'merge-base', '--is-ancestor', sha, 'refs/remotes/origin/main')
    except subprocess.CalledProcessError:
        raise ValueError('Tag commit is not on origin/main') from None
    notes = re.split(r'^-----BEGIN [A-Z ]*SIGNATURE-----$', annotation.partition('\n\n')[2], maxsplit=1, flags=re.M)[0]
    require(notes.strip(), 'Annotated tag message is empty')
    return tag, sha, notes, remote

def main():
    identity = preflight()
    if identity is None or len(sys.argv) == 1:
        return
    require(len(sys.argv) == 2 and os.environ.get('GH_TOKEN'), 'Expected installer directory and job token')
    tag, sha, notes, _remote = identity
    directory = Path(sys.argv[1])
    receipt = verify(directory, sha, os.environ['GITHUB_RUN_ID'])
    require(receipt['version'] == tag[1:] and len(receipt['installers']) == 1, 'Expected singleton versioned prod installer')
    name = 'devbox-' + tag[1:] + '-setup.exe'
    content = (directory / next(iter(receipt['installers']))).read_bytes()
    digest = 'sha256:' + hashlib.sha256(content).hexdigest()
    require(digest[7:] == next(iter(receipt['installers'].values())).lower(), 'Installer changed after verification')
    expected = dict(tag_name=tag, target_commitish=sha, name=tag, body=notes, prerelease=False)
    base = f'repos/{REPO}/releases'
    with tempfile.TemporaryDirectory() as temporary:
        staging = Path(temporary)
        (staging / name).write_bytes(content)
        def api(endpoint, method='GET', payload=None):
            arguments = ['api', endpoint, '--method', method]
            if payload is not None:
                (staging / 'request.json').write_text(json.dumps(payload), encoding='utf-8')
                arguments += ['--input', str(staging / 'request.json')]
            return gh_json(*arguments)
        def listing(endpoint):
            pages = gh_json('api', endpoint, '--method', 'GET', '--paginate', '--slurp', '-f', 'per_page=100')
            require(pages and all(isinstance(page, list) for page in pages), 'Invalid paginated listing')
            return [item for page in pages for item in page]
        def metadata(release):
            require(all(release.get(key) == value for key, value in expected.items()), 'Conflicting release metadata')
            require(type(release['id']) is int and release['id'] > 0 and type(release['draft']) is bool, 'Invalid release identity')
        def asset(value):
            require((value.get('name'), value.get('state'), value.get('size'), value.get('digest')) == (name, 'uploaded', len(content), digest), 'Conflicting or incomplete release asset')
            require(type(value['id']) is int and value['id'] > 0, 'Invalid asset ID')
        matches = [release for release in listing(base) if release['tag_name'] == tag]
        require(len(matches) <= 1, 'Ambiguous release tag')
        release = matches[0] if matches else api(base, 'POST', dict(expected, draft=True))
        metadata(release)
        endpoint = base + '/' + str(release['id'])
        assets = listing(endpoint + '/assets')
        if not assets and release['draft']:
            uploaded = gh_json('api', f'https://uploads.github.com/{endpoint}/assets?name={name}', '--method', 'POST', '--input', str(staging / name), '-H', 'Content-Type: application/octet-stream')
            asset(uploaded)
            assets = listing(endpoint + '/assets')
            require(len(assets) == 1 and assets[0]['id'] == uploaded['id'], 'Uploaded asset identity mismatch')
        require(len(assets) == 1, 'Expected exactly one public asset')
        asset(assets[0])
        fresh = api(base + '/assets/' + str(assets[0]['id']))
        asset(fresh)
        require(fresh['id'] == assets[0]['id'], 'Fresh asset identity mismatch')
        require(preflight() == identity, 'Tag changed before publication')
        current = api(endpoint)
        metadata(current)
        require(current['id'] == release['id'] and current['draft'] == release['draft'], 'Release changed before publication')
        if release['draft']:
            current = api(endpoint, 'PATCH', dict(draft=False))
            metadata(current)
            require(current['id'] == release['id'] and current['draft'] is False, 'Publication not confirmed')

if __name__ == '__main__':
    main()
