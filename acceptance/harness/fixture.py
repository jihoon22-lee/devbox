"""S7 accept-only fixture. Importing/planning never creates host resources.

Ported from the reviewed S6 fixture: same accept identity (crates/protocol/src/channel.rs), same
owned-marker/absent-before rules and resource drop-ins. S7 changes (changes §9): two fixture modes
(`windows` with exactly one owned config.local.toml Windows root, `wsl` with none), MCP write on,
cleanup_after_close on, one `s7-shell` profile, and the owned fixture projects.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import uuid

from common import BASE, dump, evidence_root, git, redact_text, require_lock, process_start, run_checked

HOME = Path.home()
UNIT = HOME / '.config/systemd/user'
MARKER = '.s7-accept-owner.json'
# M12: the owned fixture projects live outside the user's repository (claude reads parent folders for CLAUDE.md and
# folder trust): /tmp/devbox-s7-fixture-<token>/, mode 0700, with the owner marker. Evidence stays under s7/runs/.
FIXTURE_PARENT = Path('/tmp')
FIXTURE_PREFIX = 'devbox-s7-fixture-'
TOKEN = re.compile('owned-[0-9a-f]{12}')
BINDING_FILE = BASE / 'source-binding.json'
# Source files whose contract this fixture depends on; hashed at bind time and re-checked before install.
BOUND_FILES = ['crates/protocol/src/channel.rs', 'crates/cli/src/setup.rs', 'crates/core/src/paths.rs', 'crates/core/src/config.rs',
               'app/src-tauri/src/core/settings.rs', 'app/src-tauri/src/core/self_check.rs', 'app/src-tauri/src/platform/paint_trace.rs',
               'crates/protocol/src/windows/native.ps1', 'crates/protocol/src/windows/probe.ps1', 'crates/protocol/src/windows/bootstrap.ps1',
               'crates/projects/src/run_config.rs', 'crates/tools/src/support/paths.rs', 'app/src/rpc/gen/rpc.ts',
               # The install and exec scripts and setup arguments (wsl_daemon.py), and the argv and timeouts.
               'app/src-tauri/src/core/deploy.rs', 'app/src-tauri/src/platform/deploy.rs']
MODES = ('windows', 'wsl')
WINDOWS_ROOT = re.compile(r'C:\\dev\\devbox-s7-accept-[a-z0-9-]+')
GIT_ENV = {'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_NOSYSTEM': '1', 'PATH': '/usr/bin:/bin'}
IDENTITY = (('user.name', 's7-accept'), ('user.email', 's7-accept@invalid'), ('commit.gpgSign', 'false'), ('core.hooksPath', '/dev/null'))
README = 's7 accept fixture project\n'
# Fixed project texts (matrix rows CL1/CL2, SV1, W1). P-CL's failing variant and P-WIN's edited service
# are the only edits a case may commit.
P_CL_TOML = '[agent]\nteardown = ["sh -c \'echo s7 > s7-teardown.txt\'"]\n'
P_CL_FAILING_TOML = '[agent]\nteardown = ["sh -c \'echo s7 > s7-teardown.txt; exit 3\'"]\n'
P_SV_TOML = '[[service]]\nid = "s7sv1"\ncommand = "sh -c \'echo s7-sv1-up; exec sleep 600\'"\n'
W1_TASKS = (('task', 's7w1a', 'cmd /c start "" /b powershell -NoProfile -Command "Start-Sleep 20"'),
            ('task', 's7w1b', 'cmd /c exit 0'),
            ('task', 's7w1h', 'cmd /c start "" /b powershell -NoProfile -Command "Start-Sleep 300"'),
            ('task', 's7w1u', 'powershell -NoProfile -Command "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Write-Output \'한글 s7\'"'),
            ('task', 's7w1x', 'cmd /c exit 7'),
            ('service', 's7w1e', 'powershell -NoProfile -Command "Start-Sleep 600"'))
W1_EDITED_SERVICE = 'powershell -NoProfile -Command "Start-Sleep 601"'


def p_win_toml(service_command=W1_TASKS[-1][2]):
    rows = [(kind, run_id, service_command if run_id == 's7w1e' else command) for kind, run_id, command in W1_TASKS]
    return ''.join("[[%s]]\nid = '%s'\ntarget = 'windows'\ncommand = '''%s'''\n" % row for row in rows)


def project_names(mode, upgrade):
    names = ['P-TOOLS', 'P-CL', 'P-SUB', 'sub-src', 'P-SV']
    if mode == 'windows':
        names += ['P-WIN', 'P-A4']
    if upgrade:
        names.append('P-U1')
    return names


PROJECT_TOML = {'P-CL': P_CL_TOML, 'P-SV': P_SV_TOML}


def project_files(name):
    """The fixed files of one project's initial commit. P-A4's toml carries a run-time port (a4_windows_service)."""
    files = {'README.md': README}
    if name == 'P-WIN':
        files['.devbox/devbox.toml'] = p_win_toml()
    elif name in PROJECT_TOML:
        files['.devbox/devbox.toml'] = PROJECT_TOML[name]
    return files


def systemd_env():
    return {**os.environ, 'XDG_RUNTIME_DIR': f'/run/user/{os.getuid()}', 'DBUS_SESSION_BUS_ADDRESS': f'unix:path=/run/user/{os.getuid()}/bus'}


UNBOUND = {'schema': 2, 'candidate': None, 'baseline': None}


def source_config():
    if not BINDING_FILE.exists():
        return dict(UNBOUND)
    return json.loads(BINDING_FILE.read_text())


def _side(worktree, sha):
    if not re.fullmatch('[0-9a-f]{40}', sha):
        raise ValueError('Exact SHA required')
    files = {name: hashlib.sha256(git(worktree, 'show', sha + ':' + name)).hexdigest() for name in BOUND_FILES}
    version = json.loads(git(worktree, 'show', sha + ':app/package.json'))['version']
    return {'sha': sha, 'version': version, 'files': files}


def bind_source(worktree, candidate_sha, baseline_sha=None):
    """Compute (not trust) the binding; baseline files are hashed at the baseline SHA with git show."""
    return {'schema': 2, 'candidate': _side(worktree, candidate_sha),
            'baseline': None if baseline_sha is None else _side(worktree, baseline_sha)}


def verify_source(worktree, candidate_sha, baseline_sha=None):
    config = source_config()
    if not config.get('candidate') or config['candidate'].get('sha') != candidate_sha:
        raise ValueError('S7 source binding is UNBOUND or differs; bind and review the exact candidate before install')
    if (config.get('baseline') or {}).get('sha') != baseline_sha:
        raise ValueError('S7 source binding baseline differs from this flow')
    if bind_source(worktree, candidate_sha, baseline_sha) != config:
        raise ValueError('Reviewed source contract changed for this SHA')
    for name, digest in config['candidate']['files'].items():
        if hashlib.sha256((Path(worktree) / name).read_bytes()).hexdigest() != digest:
            raise ValueError('Worktree file differs from bound source: ' + name)
    return config


def accept_paths():
    """Every WSL path the accept channel may create (setup.rs + core/paths.rs for instance 'accept')."""
    uid = os.getuid()
    return [HOME / '.config/devbox-accept', HOME / '.local/share/devbox-accept',
            HOME / '.local/share/devbox/accept', HOME / '.local/state/devbox/accept',
            Path(f'/run/user/{uid}/devbox-accept.sock'),
            *[UNIT / x for x in ['devbox-accept@.service', 'devbox-accept@.socket', 'devbox-accept-tmux@.service',
                                 'devbox-accept@accept.service.d', 'devbox-accept@accept.socket.d', 'devbox-accept-tmux@accept.service.d',
                                 'devbox-accept-agent-.scope.d', 'devbox-accept-run-.service.d']],
            Path(f'/tmp/tmux-{uid}/devbox-accept'),
            UNIT / 'sockets.target.wants/devbox-accept@accept.socket',
            UNIT / 'default.target.wants/devbox-accept@accept.service',
            UNIT / 'default.target.wants/devbox-accept-tmux@accept.service']


def accept_runtime_units():
    """Loaded accept run/agent transient units (read-only list; a failed one stays loaded until reset)."""
    result = subprocess.run(['systemctl', '--user', 'list-units', '--all', '--output=json', 'devbox-accept-*'],
                            capture_output=True, text=True, env=systemd_env(), timeout=20)
    if result.returncode:
        raise RuntimeError('Cannot list accept units: ' + redact_text(result.stderr, 400))
    return sorted(r['unit'] for r in json.loads(result.stdout or '[]') if r['unit'].startswith(('devbox-accept-run-', 'devbox-accept-agent-')))


def parent_candidates():
    """M6: the parents the product creates when they are absent: every ancestor of an accept path strictly below HOME
    (e.g. ~/.local/share/devbox, ~/.local/state/devbox, the *.target.wants folders), and /tmp/tmux-<uid>. Deepest first."""
    found = {Path(f'/tmp/tmux-{os.getuid()}')}
    for path in accept_paths():
        for parent in path.parents:
            if parent == HOME or not parent.is_relative_to(HOME):
                break
            found.add(parent)
    return sorted(found, key=lambda p: (-len(p.parts), str(p)))


def absent_parents():
    """The candidates proven absent now (recorded before the first mutation; cleanup removes them only when empty)."""
    return [str(p) for p in parent_candidates() if not p.exists() and not p.is_symlink()]


def fixture_root(token):
    return FIXTURE_PARENT / (FIXTURE_PREFIX + token)


def check_root(root, token=None, sha=None, absent_ok=False):
    """The owned root by lstat (a real directory of this user, mode 0700) and, with a token, its owner marker.

    N1 (review 2 §3.3; Ruling 88): absent_ok is the owned recovery's alone (wsl-owned.py RECOVERY: seal, reseal and
    cleanup). With it, a root that lstat proves absent (ENOENT: a WSL restart emptied the tmpfs /tmp) gives 'absent'
    instead of raising. A root that exists is checked in full either way and gives 'present'."""
    try:
        st = Path(root).lstat()
    except FileNotFoundError:
        if not absent_ok:
            raise
        return 'absent'
    if stat.S_ISLNK(st.st_mode) or not stat.S_ISDIR(st.st_mode) or st.st_uid != os.getuid() or st.st_mode & 0o777 != 0o700:
        raise ValueError('Owned fixture root must be a 0700 directory of this user: ' + str(root))
    if token is not None:
        marker = Path(root) / MARKER
        if marker.is_symlink() or json.loads(marker.read_text()) != {'token': token, 'sourceSha': sha}:
            raise ValueError('Owned fixture marker mismatch')
    return 'present'


def create_root(root, token, sha):
    """M12: proven absent, then an exclusive mkdir 0700 directly under FIXTURE_PARENT, lstat-checked, then the marker."""
    root = Path(root)
    parent = Path(FIXTURE_PARENT)
    if root != fixture_root(token) or parent.is_symlink() or parent.resolve() != parent:
        raise ValueError('Owned fixture root must be ' + str(fixture_root(token)) + ' under a real ' + str(parent))
    if root.exists() or root.is_symlink():
        raise RuntimeError('Owned fixture root already exists; preserved: ' + str(root))
    root.mkdir(mode=0o700)  # exclusive: anything that appeared since the check makes this fail
    os.chmod(root, 0o700)
    check_root(root)
    (root / MARKER).write_text(json.dumps({'token': token, 'sourceSha': sha}))


def inspect_accept_absent():
    existing = [str(p) for p in accept_paths() if p.exists() or p.is_symlink()]
    if existing:
        raise RuntimeError('Existing accept resources are preserved; ownership decision required: ' + repr(existing))
    jobs = accept_runtime_units()
    if jobs:
        raise RuntimeError('Existing accept runtime units are preserved; ownership decision required: ' + repr(jobs))
    states = {}
    for unit in ['devbox-accept@accept.service', 'devbox-accept@accept.socket', 'devbox-accept-tmux@accept.service']:
        result = subprocess.run(['systemctl', '--user', 'show', unit, '-p', 'LoadState', '-p', 'ActiveState'], capture_output=True, text=True, env=systemd_env(), timeout=10)
        if result.returncode not in (0, 1) or 'LoadState=not-found' not in result.stdout or 'ActiveState=inactive' not in result.stdout:
            raise RuntimeError('Existing accept unit is preserved: %s (systemctl exit %d: %s)' % (unit, result.returncode, redact_text(result.stderr, 400)))
        states[unit] = result.stdout
    return {'absentPaths': [str(p) for p in accept_paths()], 'units': states, 'runtimeUnits': jobs}


def runs_dir(root):
    return evidence_root(root, create=False) / 'runs'


def delegate_windows(evidence, source_sha, root):
    """Keep this fd open for the complete driver lifetime (same lock-proof shape as S4/S6)."""
    owner = require_lock()
    evidence = Path(evidence).resolve()
    if not evidence.is_relative_to(runs_dir(root)):
        raise ValueError('Delegation must belong to this run evidence')
    path = evidence / ('windows-delegation-' + uuid.uuid4().hex + '.json')
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_EXCL, 0o600)
    proof = {'sourceSha': source_sha, 'driverPid': os.getpid(), 'driverStart': process_start(os.getpid()),
             'ownerPid': owner, 'ownerStart': process_start(owner), 'fd': fd}
    os.write(fd, json.dumps(proof).encode())
    os.fsync(fd)
    return fd, path


PROFILE = {'id': 's7-shell', 'label': 's7 shell', 'command': 'bash', 'hooks': 'none'}


def fixture_plan(sha, token, mode, windows_root=None, home=HOME, upgrade=False):
    if not re.fullmatch('[0-9a-f]{40}', sha) or not TOKEN.fullmatch(token):
        raise ValueError('Exact source SHA and owned nonce required')
    if mode not in MODES:
        raise ValueError('Fixture mode is windows or wsl')
    if mode == 'windows' and (not isinstance(windows_root, str) or not WINDOWS_ROOT.fullmatch(windows_root)):
        raise ValueError('Owned Windows copy root required')
    if mode == 'wsl' and windows_root is not None:
        raise ValueError('The WSL fixture has no Windows root')
    root = fixture_root(token)
    vault = root / ('custom-vault-' + token[6:])
    config = ('[notes]\nvault = ' + json.dumps(str(vault)) + '\n[pc]\nname = "s7-accept-fixture"\n[mcp]\nwrite = true\n'
              '[projects]\nroots = ' + json.dumps([str(root)]) + '\n[agents]\ncleanup_after_close = true\n'
              '[[agents.profiles]]\nid = "s7-shell"\nlabel = "s7 shell"\ncommand = "bash"\nhooks = "none"\n')
    local = None if mode == 'wsl' else '[windows]\nroot = ' + json.dumps(windows_root) + '\n'
    names = project_names(mode, upgrade)
    return {'schema': 2, 'sourceSha': sha, 'channel': 'accept', 'mode': mode, 'upgrade': bool(upgrade), 'token': token, 'nonce': token[6:],
            'fixtureRoot': str(root), 'vaultPath': str(vault), 'markerPath': 's7-vault-' + token + '.md', 'markerText': token + '\n' + sha + '\n',
            'projectPaths': {name: str(root / name) for name in names},
            'configPath': str(Path(home) / '.config/devbox-accept/config.toml'), 'configText': config,
            'configLocalPath': str(Path(home) / '.config/devbox-accept/config.local.toml'), 'configLocalText': local,
            'windowsRoot': windows_root,
            'socketPath': f'/run/user/{os.getuid()}/devbox-accept.sock', 'acceptBinary': str(Path(home) / '.local/share/devbox-accept/bin/devbox'),
            'paletteShortcut': 'Ctrl+Alt+Shift+F10', 'terminalShortcut': 'Ctrl+Alt+Shift+F11',
            'runIds': [], 'agentIds': [], 'projectIds': [], 'terminalIds': [], 'agents': {}, 'agentWorktrees': [], 'clAgentIds': [],
            'mcpProcesses': [], 'createdFiles': [], 'sealedPaths': [], 'windowsProcesses': []}


def validate_plan(m):
    import tomllib
    root = Path(m['fixtureRoot'])
    token = m['token']
    if not TOKEN.fullmatch(token) or '..' in root.parts or root != fixture_root(token) or m.get('mode') not in MODES:
        raise ValueError('Fixture must be the private owned directory ' + str(fixture_root(token)))
    if Path(m['vaultPath']).parent != root or m['markerPath'] != 's7-vault-' + token + '.md' or m['markerText'] != token + '\n' + m['sourceSha'] + '\n':
        raise ValueError('Exact owned notes vault/marker required')
    config = tomllib.loads(m['configText'])
    if (config.get('notes') != {'vault': m['vaultPath']} or 'windows' in config or config.get('mcp', {}).get('write') is not True
            or config.get('agents', {}).get('cleanup_after_close') is not True or config.get('agents', {}).get('profiles') != [PROFILE]
            or config.get('projects') != {'roots': [str(root)]} or config.get('pc') != {'name': 's7-accept-fixture'}
            or set(config) != {'notes', 'pc', 'mcp', 'projects', 'agents'} or set(config['agents']) != {'cleanup_after_close', 'profiles'}):
        raise ValueError('Invalid scoped S7 fixture TOML')
    if m['mode'] == 'windows':
        if not isinstance(m.get('configLocalText'), str) or not WINDOWS_ROOT.fullmatch(m.get('windowsRoot') or '') or \
                tomllib.loads(m['configLocalText']) != {'windows': {'root': m['windowsRoot']}}:
            raise ValueError('config.local.toml may carry only the owned Windows root')
    elif m.get('configLocalText') is not None or m.get('windowsRoot') is not None:
        raise ValueError('The WSL fixture has no config.local.toml and no Windows root')
    expected = {name: str(root / name) for name in project_names(m['mode'], m.get('upgrade'))}
    if m.get('projectPaths') != expected:
        raise ValueError('Fixture projects must be the fixed names directly under the fixture root')


def verify_staged(m):
    validate_plan(m)
    config = Path(m['configPath'])
    local = Path(m['configLocalPath'])
    vault = Path(m['vaultPath'])
    marker = vault / m['markerPath']
    owned = [(config, 0o600), (vault, 0o700), (marker, 0o600)] + ([(local, 0o600)] if m['mode'] == 'windows' else [])
    for path, mode in owned:
        st = path.lstat()
        if path.is_symlink() or st.st_uid != os.getuid() or st.st_mode & 0o777 != mode:
            raise ValueError('Fixture must remain private and owned: ' + str(path))
    if m['mode'] == 'wsl' and (local.exists() or local.is_symlink()):
        raise ValueError('The WSL fixture must not have config.local.toml')
    if config.read_text() != m['configText'] or marker.read_text() != m['markerText'] or (m['mode'] == 'windows' and local.read_text() != m['configLocalText']):
        raise ValueError('Preinstalled configuration/marker changed')
    return {'status': 'PASS', 'vaultPath': str(vault), 'mode': m['mode'], 'configLocal': str(local) if m['mode'] == 'windows' else None,
            'sourceSha': m['sourceSha']}


def make_project(path, files, run=subprocess.run):
    """git init + repo-local identity + one initial commit of the fixed files (changes §9)."""
    path = Path(path)
    path.mkdir(mode=0o700)
    for name, text in files.items():
        target = path / name
        target.parent.mkdir(mode=0o700, exist_ok=True)
        with target.open('x') as stream:
            stream.write(text)
    steps = [['git', 'init', '-q', '-b', 'main', str(path)]]
    steps += [['git', '-C', str(path), 'config', key, value] for key, value in IDENTITY]
    steps += [['git', '-C', str(path), 'add', '-A'],
              ['git', '-C', str(path), '-c', 'commit.gpgSign=false', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 's7: owned fixture']]
    for argv in steps:
        if run is subprocess.run:
            run_checked(argv, timeout=15, env=GIT_ENV)  # a failure keeps its redacted stderr (fix round 1 item 6)
        else:
            run(argv, check=True, capture_output=True, timeout=15, env=GIT_ENV)


def stage(sha, evidence, mode, proof, root=None, upgrade=False, extra=None):
    """Windows mode needs native_preflight acceptAbsent; WSL mode needs wsl_daemon.drvfs_absent()."""
    require_lock()
    evidence = Path(evidence).resolve()
    if not evidence.is_dir() or not evidence.is_relative_to(runs_dir(root)):
        raise ValueError('Unique owned run evidence required')
    absent = inspect_accept_absent()
    if mode == 'windows':
        if proof.get('channel') != 'accept' or proof.get('acceptAbsent') is not True or proof.get('sourceSha') != sha:
            raise ValueError('Native accept absence/source proof required')
        windows_root = proof['windowsRoot']
    elif mode == 'wsl':
        if proof.get('drvfsAbsent') is not True:
            raise ValueError('drvfs accept absence proof required')
        windows_root = None
    else:
        raise ValueError('Fixture mode is windows or wsl')
    token = 'owned-' + uuid.uuid4().hex[:12]
    m = fixture_plan(sha, token, mode, windows_root, upgrade=upgrade)
    m.update(**(extra or {}))
    root = Path(m['fixtureRoot'])
    if root.exists() or root.is_symlink():
        raise RuntimeError('Owned fixture root already exists; preserved: ' + str(root))
    # M6 and M12: proven absent before the first mutation and recorded with the manifest.
    m.update(nativePreflight=proof if mode == 'windows' else None, drvfsPreflight=proof if mode == 'wsl' else None,
             wslPreflight=absent, absentBefore=[str(p) for p in accept_paths()], absentParentsBefore=absent_parents(),
             fixtureRootAbsentBefore=True, status='ownership-recorded-before-mutation')
    manifest = evidence / ('ownership-' + token + '.json')
    with manifest.open('x') as stream:
        json.dump(m, stream, indent=2)
    manifest.chmod(0o600)
    create_root(root, token, sha)

    def write_owned(path, text):
        data = text.encode()
        m['createdFiles'].append({'path': str(path), 'sha256': hashlib.sha256(data).hexdigest()})
        dump(manifest, m)
        with path.open('xb') as stream:
            stream.write(data)
        path.chmod(0o600)

    def owned_dir(directory):
        m['sealedPaths'].append({'path': str(directory), 'kind': 'directory'})
        dump(manifest, m)
        directory.mkdir(mode=0o700)
        (directory / MARKER).write_text(json.dumps({'token': token, 'sourceSha': sha}))

    validate_plan(m)
    vault = Path(m['vaultPath'])
    vault.mkdir(mode=0o700)
    write_owned(vault / m['markerPath'], m['markerText'])
    config = Path(m['configPath'])
    owned_dir(config.parent)
    write_owned(config, m['configText'])
    if mode == 'windows':
        write_owned(Path(m['configLocalPath']), m['configLocalText'])
    # The owned P1 추가 target: never approved, removed by cleanup if present (empty file).
    m['createdFiles'].append({'path': '/tmp/devbox-s7-p1x-' + m['nonce'], 'sha256': hashlib.sha256(b'').hexdigest(), 'optional': True})
    dump(manifest, m)
    for name, path in m['projectPaths'].items():
        make_project(path, project_files(name))
    cpus = ' '.join(str(x) for x in sorted(os.sched_getaffinity(0))[:4])
    for unit in ['devbox-accept@accept.service', 'devbox-accept-tmux@accept.service', 'devbox-accept-run-.service', 'devbox-accept-agent-.scope']:
        item = UNIT / (unit + '.d') / '90-s7-owned-resources.conf'
        owned_dir(item.parent)
        section = 'Scope' if unit.endswith('.scope') else 'Service'
        text = '[' + section + ']\nCPUQuota=400%\nMemoryHigh=6G\nMemoryMax=8G\nMemorySwapMax=1G\n'
        if section == 'Service':
            text += 'Nice=10\nCPUAffinity=' + cpus + '\nEnvironment=GIT_CONFIG_GLOBAL=/dev/null\nEnvironment=GIT_CONFIG_NOSYSTEM=1\n'
        write_owned(item, text)
    verify_staged(m)
    m['status'] = 'staged-before-install'
    dump(manifest, m)
    return manifest
