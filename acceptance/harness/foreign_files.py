"""1.0.1 (D5, gate G2): owned marker files in the owned accept install folder, across the app's own update and across the uninstall.

The user's real install folder also holds files of another product, so the product's installer and uninstaller must
leave every file they did not install. This module plants a fixed set of owned files (three files and one folder with
one file) in this run's owned install folder, reads them back, compares them byte for byte, and removes exactly those
names again. Their names start with `s7-foreign-<run token>-`, which no product file has.

Two sets, never mixed: `update` is planted before the app's update and read after it (case UP2); `uninstall` is planted
before the plain uninstall and read by native-lifecycle.ps1 between the uninstaller's exit and the owned tree removal
(case A7). Everything here is pure file work on paths the caller owns; nothing starts a process.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import stat

TOKEN = re.compile('owned-[0-9a-f]{12}')
MARKER = '.s7-accept-owner.json'
# Set name -> the letter in its file names. The names match native-lifecycle.ps1's -Foreign pattern.
SETS = {'update': 'u', 'uninstall': 'r'}
INNER = 'inner.dat'


def prefix(token):
    if not isinstance(token, str) or not TOKEN.fullmatch(token):
        raise ValueError('Owned run token required')
    return 's7-foreign-' + token + '-'


def directory_name(token, kind):
    return prefix(token) + SETS[kind] + 'd'


def plan(token, kind):
    """The fixed files of one set, as (relative name with `/`, bytes): a text file, a 4096-byte file, an empty file with
    no extension, and one file inside the set's folder."""
    if kind not in SETS:
        raise ValueError('Unknown foreign file set')
    head, letter = prefix(token), SETS[kind]
    seed = hashlib.sha256(('s7 foreign %s %s' % (token, kind)).encode()).digest()
    return [(head + letter + '1.txt', ('s7 foreign file, set %s, run %s\n' % (kind, token)).encode()),
            (head + letter + '2.bin', seed * 128),
            (head + letter + '3', b''),
            (directory_name(token, kind) + '/' + INNER, seed[::-1] * 8 + b'\x00')]


def check_install(install, token, source_sha):
    """The owned install folder: a real directory that holds this run's owner marker (written by the install action)."""
    install = Path(install)
    if install.is_symlink() or not install.is_dir():
        raise ValueError('Owned install folder must be a real directory')
    marker = json.loads((install / MARKER).read_text(encoding='utf-8-sig'))
    if not isinstance(marker, dict) or marker.get('token') != token or marker.get('sourceSha') != source_sha:
        raise ValueError('Install folder owner mismatch')
    return install


def manifest(token, kind):
    """What the planted set must be: names, lengths and sha256, without touching the disk."""
    return {'schema': 1, 'token': token, 'set': kind, 'directory': directory_name(token, kind),
            'files': [{'relative': name, 'length': len(data), 'sha256': hashlib.sha256(data).hexdigest()} for name, data in plan(token, kind)]}


def plant(install, token, kind, source_sha):
    """Create the set. Refuses when any of its names already exists; a failed write leaves what was written for remove()."""
    install = check_install(install, token, source_sha)
    rows = plan(token, kind)
    folder = install / directory_name(token, kind)
    for name, _ in rows:
        target = install / name
        if target.exists() or target.is_symlink():
            raise ValueError('Foreign file name already exists: ' + name)
    if folder.exists() or folder.is_symlink():
        raise ValueError('Foreign folder name already exists: ' + folder.name)
    folder.mkdir()
    for name, data in rows:
        with open(install / name, 'xb') as stream:
            stream.write(data)
    return manifest(token, kind)


def _checked(value):
    """A manifest is only ever the fixed plan of its token and set; anything else is refused before a path is built."""
    if not isinstance(value, dict) or value != manifest(value.get('token'), value.get('set')):
        raise ValueError('Foreign file manifest is not the fixed plan of its run')
    return value


def observe(install, planted):
    """One row per planted name: exists, and for a regular file its length and sha256. Links are reported, never read."""
    install, rows = Path(install), []
    for entry in _checked(planted)['files']:
        path = install / entry['relative']
        row = {'relative': entry['relative'], 'exists': False}
        try:
            info = os.lstat(path)
        except FileNotFoundError:
            rows.append(row)
            continue
        row['exists'] = True
        if stat.S_ISREG(info.st_mode):
            row.update(length=info.st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        else:
            row['notRegular'] = True
        rows.append(row)
    return rows


def compare(planted, rows):
    """PASS only when every planted file was read back as a regular file with the planted length and sha256."""
    planted = _checked(planted)
    seen = {}
    for row in rows if isinstance(rows, list) else []:
        if isinstance(row, dict):
            seen[str(row.get('relative'))] = row
    missing, changed = [], []
    for entry in planted['files']:
        row = seen.get(entry['relative'])
        if not row or row.get('exists') is not True:
            missing.append(entry['relative'])
        elif (row.get('notRegular') or row.get('reparsePoint') or row.get('length') != entry['length']
              or str(row.get('sha256') or '').lower() != entry['sha256']):
            changed.append(entry['relative'])
    ok = isinstance(rows, list) and not missing and not changed and len(seen) == len(planted['files'])
    return {'status': 'PASS' if ok else 'FAIL', 'set': planted['set'], 'files': len(planted['files']), 'missing': missing, 'changed': changed,
            'observed': len(seen)}


def remove(install, planted):
    """Owned cleanup: exactly the planted names, then the set's folder when it is empty. Anything else is left and reported."""
    install = Path(install)
    planted = _checked(planted)
    removed, left = [], []
    for entry in planted['files']:
        path = install / entry['relative']
        try:
            info = os.lstat(path)
        except FileNotFoundError:
            continue
        if stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode):
            path.unlink()
            removed.append(entry['relative'])
        else:
            left.append(entry['relative'])
    folder = install / planted['directory']
    if folder.is_symlink():
        left.append(planted['directory'])
    elif folder.is_dir():
        extra = sorted(p.name for p in folder.iterdir())
        if extra:
            left.append(planted['directory'] + ' (holds ' + ', '.join(extra[:5]) + ')')
        else:
            folder.rmdir()
            removed.append(planted['directory'])
    return {'status': 'FAIL' if left else 'PASS', 'removed': removed, 'left': left}


def decision(comparison, cleanup=None):
    """(status, reason) of UP2 or A7: the comparison decides; a marker the owned cleanup could not remove is a FAIL too."""
    if not isinstance(comparison, dict) or comparison.get('status') != 'PASS':
        detail = comparison or {}
        return 'FAIL', 'Foreign files did not survive: missing %s, changed %s' % (detail.get('missing'), detail.get('changed'))
    if cleanup is not None and cleanup.get('status') != 'PASS':
        return 'FAIL', 'The owned cleanup left foreign file names: %s' % cleanup.get('left')
    return 'PASS', None
