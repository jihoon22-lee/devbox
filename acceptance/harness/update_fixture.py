"""1.0.1 (D5): the accept update fixture, as the product reads it (pure file work; nothing starts a process).

An accept build reads DEVBOX_UPDATE_FIXTURE once at start (app/src-tauri/src/platform/update.rs:26-48) and opens the
folder with `Fixture::open(path, source)` (app/src-tauri/src/core/update/fixture.rs:35-95), where `source` is the
40-hex SOURCE_SHA the build was compiled with (app/src-tauri/build.rs:19-30). The folder holds exactly:
- owner.json (at most 2048 bytes, no other keys): schema 1, channel "accept", sourceSha equal to that build's
  SOURCE_SHA, a stable currentVersion, and the sha256 of metadata.json and of installer.exe;
- metadata.json (at most 1 MiB): a GitHub release, `select`ed against currentVersion (release.rs:46-100);
- installer.exe: the size and sha256 the release's one asset names.
Nothing in these checks names the installer's source SHA or its version. check_fixture() mirrors them, so a fixture
this harness builds is refused here, before a run, rather than silently by the app ("Invalid accept update fixture").
"""
import hashlib
import json
from pathlib import Path
import re
import shutil

from evidence import REPO as REPOSITORY, UPDATE_SHAPES as SHAPES

STABLE = re.compile(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)')
SHA = re.compile('[0-9a-f]{40}')
DIGEST = re.compile('[0-9a-f]{64}')
MAX_INSTALLER = 256 * 1024 * 1024  # release.rs:4
MAX_METADATA = 1024 * 1024  # release.rs:5
OWNER_LIMIT = 2048  # fixture.rs:44
OWNER_KEYS = {'schema', 'channel', 'sourceSha', 'currentVersion', 'metadataSha256', 'installerSha256'}
FIXTURE_FILES = ('owner.json', 'metadata.json', 'installer.exe')


def core(version):
    """The numeric x.y.z of a bound version (a pre-release suffix is cut), as a tuple."""
    match = STABLE.match(str(version))
    if not match:
        raise ValueError('Not a version: ' + repr(version))
    return tuple(int(part) for part in match.groups())


def check_shape(shape, baseline_version, candidate_version):
    """Refuses a version pair the named shape cannot run. `same-version` is what the accept runner accepts today."""
    if shape not in SHAPES:
        raise ValueError('Unknown update shape')
    if shape == 'same-version' and baseline_version != candidate_version:
        raise ValueError('update shape same-version needs equal bound versions (%s, %s): the accept runner restarts the app only when the installed '
                         'devbox-version.txt equals its own version (app/src-tauri/src/platform/update_runner/execute.rs:221-227, 315-324)'
                         % (baseline_version, candidate_version))
    if shape == 'next-version' and (not STABLE.fullmatch(str(candidate_version)) or core(candidate_version) <= core(baseline_version)):
        raise ValueError('update shape next-version needs a stable candidate version above the baseline')
    return shape


def fixture_versions(baseline_version, candidate_version):
    """(owner.json currentVersion, release version). The release is the candidate's x.y.z. The service compares it with
    currentVersion, not with the running version (service.rs:424), so with equal versions the marker claims 0.0.0."""
    target, running = core(candidate_version), core(baseline_version)
    current = running if running < target else (0, 0, 0)
    if target <= current:
        raise ValueError('The candidate version cannot be offered as an update')
    return '.'.join(map(str, current)), '.'.join(map(str, target))


def metadata_bytes(version, size, digest, asset_id=1):
    """The release JSON release.rs:46-100 accepts: one uploaded asset named devbox-<version>-setup.exe with its exact URLs."""
    name = 'devbox-%s-setup.exe' % version
    return json.dumps({'tag_name': 'v' + version, 'draft': False, 'prerelease': False, 'assets': [{
        'id': asset_id, 'name': name, 'state': 'uploaded', 'size': size, 'digest': 'sha256:' + digest,
        'url': 'https://api.github.com/repos/%s/releases/assets/%d' % (REPOSITORY, asset_id),
        'browser_download_url': 'https://github.com/%s/releases/download/v%s/%s' % (REPOSITORY, version, name)}]}, indent=1).encode()


def select(metadata, current):
    """release.rs `select`, mirrored: the candidate {version, id, size, digest}, or None when the release is not newer."""
    if len(metadata) > MAX_METADATA:
        raise ValueError('metadata.json is too large')
    release = json.loads(metadata)
    tag = release['tag_name']
    if not isinstance(tag, str) or not tag.startswith('v') or not STABLE.fullmatch(tag[1:]):
        raise ValueError('tag_name is not v<stable version>')
    if release['draft'] is not False or release['prerelease'] is not False:
        raise ValueError('draft or prerelease')
    if not STABLE.fullmatch(str(current)):
        raise ValueError('currentVersion is not a stable version')
    version = tag[1:]
    if core(version) <= core(current):
        return None
    if not isinstance(release['assets'], list) or len(release['assets']) != 1:
        raise ValueError('exactly one asset is required')
    asset = release['assets'][0]
    name = 'devbox-%s-setup.exe' % version
    if (type(asset['id']) is not int or asset['id'] <= 0 or asset['name'] != name or asset['state'] != 'uploaded'
            or asset['url'] != 'https://api.github.com/repos/%s/releases/assets/%d' % (REPOSITORY, asset['id'])
            or asset['browser_download_url'] != 'https://github.com/%s/releases/download/v%s/%s' % (REPOSITORY, version, name)):
        raise ValueError('asset name, state or URLs differ from the release shape')
    if type(asset['size']) is not int or not 0 < asset['size'] <= MAX_INSTALLER:
        raise ValueError('asset size out of range')
    digest = asset.get('digest')
    if not isinstance(digest, str) or not digest.startswith('sha256:') or not DIGEST.fullmatch(digest[7:]):
        raise ValueError('asset digest is not sha256:<64 hex>')
    return {'version': version, 'id': asset['id'], 'size': asset['size'], 'digest': digest[7:]}


def file_sha256(path):
    digest, size = hashlib.sha256(), 0
    with open(path, 'rb') as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
            size += len(chunk)
    return digest.hexdigest(), size


def check_fixture(directory, expected_source):
    """fixture.rs `Fixture::open(path, source)`, mirrored (fixture.rs:35-95): what a build of `expected_source` accepts."""
    directory = Path(directory)
    if not directory.is_absolute() or not SHA.fullmatch(str(expected_source)):
        raise ValueError('absolute folder and a 40-hex source SHA required')
    if directory.is_symlink() or not directory.is_dir() or sorted(p.name for p in directory.iterdir()) != sorted(FIXTURE_FILES):
        raise ValueError('the fixture folder must hold exactly owner.json, metadata.json and installer.exe')
    if any((directory / name).is_symlink() or not (directory / name).is_file() for name in FIXTURE_FILES):
        raise ValueError('fixture entries must be regular files')
    owner_bytes = (directory / 'owner.json').read_bytes()
    if len(owner_bytes) > OWNER_LIMIT:
        raise ValueError('owner.json is over %d bytes' % OWNER_LIMIT)
    owner = json.loads(owner_bytes)
    if not isinstance(owner, dict) or set(owner) != OWNER_KEYS:
        raise ValueError('owner.json keys must be exactly ' + ', '.join(sorted(OWNER_KEYS)))
    if (owner['schema'] != 1 or type(owner['schema']) is not int or owner['channel'] != 'accept' or owner['sourceSha'] != expected_source
            or not STABLE.fullmatch(str(owner['currentVersion']))):
        raise ValueError('owner.json schema, channel, sourceSha or currentVersion mismatch')
    metadata = (directory / 'metadata.json').read_bytes()
    if not DIGEST.fullmatch(str(owner['metadataSha256'])) or hashlib.sha256(metadata).hexdigest() != owner['metadataSha256']:
        raise ValueError('metadata.json sha256 differs from owner.json')
    candidate = select(metadata, owner['currentVersion'])
    if candidate is None:
        raise ValueError('the release is not newer than currentVersion')
    if not DIGEST.fullmatch(str(owner['installerSha256'])) or candidate['digest'] != owner['installerSha256']:
        raise ValueError('the asset digest differs from owner.json installerSha256')
    digest, size = file_sha256(directory / 'installer.exe')
    if size != candidate['size'] or digest != candidate['digest']:
        raise ValueError('installer.exe size or sha256 differs from the release asset')
    return {**candidate, 'currentVersion': owner['currentVersion'], 'sourceSha': owner['sourceSha']}


def build_fixture(directory, source_sha, installer, installer_sha256, baseline_version, candidate_version):
    """Write the fixture folder for a baseline build of `source_sha`: installer.exe is a byte copy of the candidate's
    accept installer (checked against the receipt's sha256). Returns its record; check_fixture() has passed on it."""
    directory, installer = Path(directory), Path(installer)
    if not SHA.fullmatch(str(source_sha)) or not DIGEST.fullmatch(str(installer_sha256)):
        raise ValueError('exact source SHA and installer sha256 required')
    current, target = fixture_versions(baseline_version, candidate_version)
    directory.mkdir(mode=0o700)
    shutil.copyfile(installer, directory / 'installer.exe')
    digest, size = file_sha256(directory / 'installer.exe')
    if digest != installer_sha256:
        raise ValueError('The copied installer differs from the receipt sha256')
    metadata = metadata_bytes(target, size, digest)
    owner = json.dumps({'schema': 1, 'channel': 'accept', 'sourceSha': source_sha, 'currentVersion': current,
                        'metadataSha256': hashlib.sha256(metadata).hexdigest(), 'installerSha256': digest}, indent=1).encode()
    for name, data in (('metadata.json', metadata), ('owner.json', owner)):
        with open(directory / name, 'xb') as stream:
            stream.write(data)
    candidate = check_fixture(directory.resolve(), source_sha)
    return {'directory': str(directory), 'sourceSha': source_sha, 'currentVersion': current, 'targetVersion': target, 'installerBytes': size,
            'installerSha256': digest, 'metadataSha256': hashlib.sha256(metadata).hexdigest(), 'ownerSha256': hashlib.sha256(owner).hexdigest(),
            'accepted': candidate}


def debug_port(token):
    """The owned debug port of one run: 39000-39899, below the Windows dynamic range. The launch refuses an occupied port."""
    return 39000 + int(hashlib.sha256(str(token).encode()).hexdigest()[:8], 16) % 900
