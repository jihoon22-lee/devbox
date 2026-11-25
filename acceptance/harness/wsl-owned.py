#!/usr/bin/env python3
"""Owned accept-instance proof, bounded RPC, seal/reseal and exact cleanup (ported from the reviewed S6 helper).

S7 changes (changes §10-§11), flagged for review:
- The RPC allowlist covers every call of the S7 cases, each bound to recorded owned identities. The
  replies that later calls must match (trust preview, discard check, changes, cleanup candidates) and
  the identities a create returns are written to the manifest on disk by rpc_owned() before it returns,
  so authorize() always judges against the on-disk record, as the S6 subprocess did.
- agents.changes is read-only and added: the product's merge `expect` comes from it
  (app/src/features/agents/detail/useReviewAction.ts); agents.review returns no oids.
- The Hello version is the running phase's version (m['phaseVersion']); rc.1 needs an exact match.
- reseal() accepts only setup-managed unit files that changed; cleanup ends owned product objects first.
- N1 (review 2 §3.3; Ruling 88): the owned recovery (RECOVERY, from main only) goes on when lstat proves the fixture
  root absent, skipping only the root's own steps. The run, its RPC and its teardown still need the root.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import stat
import struct
import subprocess
import time

from common import dump, redact_text, require_lock, rpc_deadline, run_checked, trust_context, update_manifest
from fixture import (HOME, MARKER, TOKEN, UNIT, accept_paths, accept_runtime_units, check_root, fixture_root, parent_candidates, systemd_env,
                     verify_staged)
from installed_binary import installed_binary
import agents_live

READ_ONLY = {'system.info', 'projects.list', 'runtime.get', 'runtime.runs', 'system.doctor', 'agents.list', 'agents.get', 'agents.usage',
             'agents.discard_check', 'agents.changes', 'agents.cleanup_candidates', 'logs.search', 'search.notes'}
OWNED_MUTATIONS = {'projects.add', 'projects.trust_preview', 'projects.trust_approve', 'runtime.start', 'runtime.stop', 'agents.create',
                   'agents.send', 'agents.resume', 'agents.review', 'agents.merge', 'agents.discard', 'agents.cleanup', 'terminal.create',
                   'terminal.close'}
PROFILES = {'claude', 'codex', 'opencode', 'pi', 's7-shell'}
# The operations the harness previews (fix round 1 item 5); launch, test and pr are never sent.
TRUST_OPERATIONS = {'create', 'run', 'cleanup', 'discard', 'merge'}
# M9: written to disk before a claude/codex agents.create, so a lost reply still counts as the observation.
PENDING = 'pending'
FIXED_RUN_DEFS = ('s7sv1', 's7w1a', 's7w1b', 's7w1e', 's7w1h', 's7w1u', 's7w1x')
CL_PROJECTS = ('P-CL', 'P-SUB')
TITLE = re.compile('s7-[a-z0-9-]{1,40}')
RUN_ID = re.compile('[0-9a-f]{32}')
SETUP_MARKER = b'# Managed by devbox setup\n'
# setup.rs render_units names and the instance paths.conf drop-ins (setup.rs:129-228).
SETUP_UNITS = {'devbox-accept@.socket', 'devbox-accept@.service', 'devbox-accept-tmux@.service'}
SETUP_DROPINS = {'devbox-accept@accept.service.d', 'devbox-accept@accept.socket.d', 'devbox-accept-tmux@accept.service.d'}
INSTANCE_UNITS = ('devbox-accept@accept.service', 'devbox-accept@accept.socket', 'devbox-accept-tmux@accept.service')
TMUX_UNIT = 'devbox-accept-tmux@accept.service'
# N1 (Ruling 88): README §5's owned recovery, the only actions that accept a fixture root that lstat proves absent.
RECOVERY = ('seal', 'reseal', 'cleanup')


def command(*args):
    return run_checked(args, timeout=20, text=True, env=systemd_env()).stdout.strip()


def _exact(params, keys, what):
    if not isinstance(params, dict) or set(params) != set(keys):
        raise ValueError(what + ': exactly ' + ','.join(sorted(keys)))


def _int(value, low, high):
    return type(value) is int and low <= value <= high


def project_id(m, name):
    return (m.get('projectIdByName') or {}).get(name)


def cl_project_ids(m):
    return [i for i in (project_id(m, name) for name in CL_PROJECTS) if i]


def discard_expect(check):
    """The app's mapping (useReviewAction.ts): fingerprint becomes discardFingerprint, left out when null."""
    expect = {'headOid': check['headOid'], 'unmergedCommits': check['unmergedCommits']}
    if check.get('fingerprint') is not None:
        expect['discardFingerprint'] = check['fingerprint']
    return expect


def authorize(m, method, params):
    """Every call names only resources this run created or recorded (changes §10); anything else refuses."""
    params = {} if params is None else params
    agents = m.get('agentIds', [])
    runs = m.get('runIds', [])
    projects = m.get('projectIds', [])
    if method in ('system.info', 'projects.list', 'system.doctor'):
        _exact(params, (), method)
    elif method == 'runtime.get':
        _exact(params, ('runId',), method)
        if params['runId'] not in runs:
            raise ValueError('runtime.get limited to owned runs')
    elif method == 'runtime.runs':
        limit = params.get('limit', 100)
        if set(params) - {'projectId', 'limit'} or params.get('projectId') not in projects or not _int(limit, 1, 100):
            raise ValueError('runtime.runs limited to an owned project')
    elif method == 'agents.list':
        if params != {} and (set(params) != {'includeClosedHours'} or not _int(params['includeClosedHours'], 0, 168)):
            raise ValueError('agents.list takes {} or {includeClosedHours: 0..168}')
    elif method in ('agents.get', 'agents.usage', 'agents.discard_check', 'agents.resume', 'agents.changes', 'agents.review'):
        _exact(params, ('id',), method)
        allowed = m.get('clAgentIds', []) if method in ('agents.changes', 'agents.review') else agents
        if params['id'] not in allowed:
            raise ValueError(method + ' limited to recorded owned agents')
    elif method == 'agents.cleanup_candidates':
        _exact(params, ('projectId',), method)
        if params['projectId'] not in projects:
            raise ValueError('cleanup candidates limited to owned projects')
    elif method == 'logs.search':
        _exact(params, ('sources', 'query', 'regex', 'limit'), method)
        sources = params['sources']
        if (not isinstance(sources, list) or not 1 <= len(sources) <= 4
                or any(not isinstance(s, dict) or set(s) != {'kind', 'runId'} or s['kind'] != 'run' or s['runId'] not in runs for s in sources)
                or not isinstance(params['query'], str) or len(params['query']) > 200 or params['regex'] is not False or not _int(params['limit'], 1, 100)):
            raise ValueError('logs.search limited to owned runs, a plain query and a bounded limit')
    elif method == 'search.notes':
        _exact(params, ('text', 'limit'), method)
        if params['text'] != 's7-nt1-' + m['nonce'] or not _int(params['limit'], 1, 50):
            raise ValueError('search.notes limited to the owned NT1 text')
    elif method == 'projects.add':
        _exact(params, ('path',), method)
        if params['path'] not in (m.get('projectPaths') or {}).values():
            raise ValueError('projects.add limited to the owned fixture projects')
    elif method == 'projects.trust_preview':
        # Fix round 1 item 5: the app's five keys (common.trust_context), naming only owned, recorded objects.
        _exact(params, ('id', 'operation', 'agentId', 'worktrees', 'worktree'), method)
        operation = params['operation']
        if params['id'] not in projects or operation not in TRUST_OPERATIONS:
            raise ValueError('Trust preview limited to owned projects and create/run/cleanup/discard/merge')
        try:
            same = trust_context(params['id'], operation, params['agentId'], params['worktrees'], params['worktree']) == params
        except (TypeError, ValueError):
            same = False
        task = (m.get('agents') or {}).get(params['agentId']) or {} if isinstance(params['agentId'], str) else {}
        listed = ((m.get('lastCandidates') or {}).get(params['id']) or {}).get('worktrees') or []
        if (not same or (operation == 'create' and params['worktree'] is not True)
                or (operation in ('discard', 'merge') and (params['agentId'] not in agents or task.get('projectId') != params['id']))
                or (operation == 'merge' and params['agentId'] not in m.get('clAgentIds', []))
                or (operation == 'cleanup' and (params['id'] not in cl_project_ids(m) or not set(params['worktrees']) <= set(listed)))):
            raise ValueError('Trust preview context must be the app\'s params for an owned, recorded object')
    elif method == 'projects.trust_approve':
        _exact(params, ('id', 'expectedCanonicalPath', 'expectedDigest', 'expectedScopeVersion'), method)
        last = (m.get('lastPreview') or {}).get(params['id'])
        if params['id'] not in projects or not last or {k: params[k] for k in last} != last:
            raise ValueError('Trust approval must equal the last preview of that owned project')
    elif method == 'runtime.start':
        _exact(params, ('projectId', 'id'), method)
        defs = [d for d in (m.get('a4ServiceId'), *FIXED_RUN_DEFS) if d]
        if params['projectId'] not in projects or params['id'] not in defs:
            raise ValueError('runtime.start limited to owned run definitions')
    elif method == 'runtime.stop':
        _exact(params, ('runId',), method)
        if params['runId'] not in runs:
            raise ValueError('runtime.stop limited to owned runs')
    elif method == 'agents.create':
        _exact(params, ('projectId', 'title', 'profile', 'worktree', 'prompt', 'branch'), method)
        nonce = m['nonce']
        if (params['projectId'] not in projects or not isinstance(params['title'], str) or not TITLE.fullmatch(params['title'])
                or params['profile'] not in PROFILES or params['worktree'] is not True
                or params['prompt'] not in agents_live.prompt_texts(nonce).values()
                or not isinstance(params['branch'], str) or not re.fullmatch('s7-' + re.escape(nonce) + '-[0-9]{1,3}', params['branch'])):
            raise ValueError('agents.create limited to an owned project, s7 title/branch, a listed profile and a fixed prompt')
        tool = params['profile']
        if tool in ('claude', 'codex'):
            answer = {'claude': 'Q1', 'codex': 'Q2'}[tool]
            observed = (m.get('trustObservation') or {}).get(tool)
            if (m.get('answers') or {}).get(answer) is not True and (observed or params['projectId'] != project_id(m, 'P-TOOLS')):
                raise ValueError(tool + ' tasks need ' + answer + ' beyond the one trust-observation task in P-TOOLS')
    elif method == 'agents.send':
        _exact(params, ('id', 'text'), method)
        if params['id'] not in agents or params['text'] not in agents_live.send_texts(m['nonce']):
            raise ValueError('agents.send limited to recorded agents and the fixed texts')
    elif method == 'agents.merge':
        _exact(params, ('id', 'strategy', 'message', 'expect'), method)
        last = (m.get('lastChanges') or {}).get(params['id'])
        if (params['id'] not in m.get('clAgentIds', []) or params['strategy'] not in ('squash', 'merge') or params['message'] is not None
                or not last or params['expect'] != last):
            raise ValueError('agents.merge limited to CL agents with the last agents.changes head/base oids')
    elif method == 'agents.discard':
        _exact(params, ('id', 'expect'), method)
        last = (m.get('lastDiscardCheck') or {}).get(params['id'])
        if params['id'] not in agents or not last or params['expect'] != discard_expect(last):
            raise ValueError('agents.discard expect must equal the last discard_check of that agent')
    elif method == 'agents.cleanup':
        _exact(params, ('projectId', 'worktrees', 'branches'), method)
        last = (m.get('lastCandidates') or {}).get(params['projectId'])
        if (params['projectId'] not in cl_project_ids(m) or not last or not isinstance(params['worktrees'], list) or not isinstance(params['branches'], list)
                or not all(isinstance(v, str) for v in params['worktrees'] + params['branches'])):
            raise ValueError('agents.cleanup limited to CL projects and the last candidates read')
        # A folder listed for its submodules may be selected (CL3's refusal path, cleanup.rs:282-304); its
        # branch only together with it, as the product allows.
        held = set(params['worktrees']) & set(last.get('submodules', []))
        held_branches = {row.get('branch') for i, row in (m.get('agents') or {}).items()
                         if i in m.get('clAgentIds', []) and row.get('worktreePath') in held}
        if (not set(params['worktrees']) <= set(last['worktrees']) | set(last.get('submodules', []))
                or not set(params['branches']) <= set(last['branches']) | held_branches):
            raise ValueError('agents.cleanup limited to CL projects and the last candidates read')
    elif method == 'terminal.create':
        _exact(params, ('projectId', 'cwd', 'title'), method)
        if params != {'projectId': project_id(m, 'P-SUB'), 'cwd': (m.get('projectPaths') or {}).get('P-SUB'), 'title': 's7-cl3'} or not params['projectId']:
            raise ValueError('terminal.create limited to the owned P-SUB terminal')
    elif method == 'terminal.close':
        _exact(params, ('id',), method)
        if params['id'] not in m.get('terminalIds', []):
            raise ValueError('terminal.close limited to recorded terminals')
    else:
        raise ValueError('RPC method not allowed: ' + method)


def hello(m):
    """The Hello frame for the running phase: `m['phaseVersion']`, set by the driver (rc.1 needs an exact match)."""
    version = m.get('phaseVersion')
    if not isinstance(version, str) or not version:
        raise ValueError('Phase version unset; the driver sets it before any RPC')
    return {'type': 'hello', 'version': version, 'protocol': 1, 'client': 'cli', 'instance': 'accept'}


def rpc(m, method, params=None):
    params = {} if params is None else params
    authorize(m, method, params)
    timeout = rpc_deadline(method)
    endpoint = Path(f'/run/user/{os.getuid()}/devbox-accept.sock')
    if endpoint.is_symlink() or not stat.S_ISSOCK(endpoint.lstat().st_mode) or endpoint.lstat().st_uid != os.getuid():
        raise ValueError('Exact owned accept socket required')
    frame = hello(m)
    version = frame['version']
    with socket.socket(socket.AF_UNIX) as sock:
        deadline = time.monotonic() + timeout
        sock.settimeout(timeout)
        sock.connect(str(endpoint))

        def send(value):
            body = json.dumps(value).encode()
            sock.sendall(struct.pack('<IBI', len(body) + 5, 0, 0) + body)

        def exact(n):
            out = b''
            while len(out) < n:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError('RPC whole-request deadline')
                sock.settimeout(remaining)
                part = sock.recv(n - len(out))
                if not part:
                    raise RuntimeError('RPC closed')
                out += part
            return out

        def receive():
            length = struct.unpack('<I', exact(4))[0]
            if not 5 <= length <= 1024 * 1024:
                raise ValueError('Frame size')
            body = exact(length)
            if body[0] != 0:
                raise ValueError('Unexpected diagnostic frame')
            return json.loads(body[5:])

        send(frame)
        welcome = receive()
        if (welcome.get('type'), welcome.get('protocol'), welcome.get('version')) != ('welcome', 1, version):
            raise ValueError('No matching accept Welcome')
        send({'type': 'request', 'id': 1, 'method': method, 'params': params})
        for _ in range(64):
            value = receive()
            if value.get('type') == 'response' and value.get('id') == 1:
                if 'error' in value:
                    raise RpcError(value['error'])
                return value['result']
        raise ValueError('Too many unrelated frames')


class RpcError(RuntimeError):
    """A daemon error reply; `code` is the product's error code (e.g. agents.awaiting_input)."""

    def __init__(self, error):
        self.error = error if isinstance(error, dict) else {'message': str(error)}
        self.code = self.error.get('code')
        super().__init__(json.dumps(self.error, ensure_ascii=False)[:800])


def _append(m, key, value):
    rows = m.setdefault(key, [])
    if value not in rows:
        rows.append(value)


def record_reply(m, method, params, result):
    """The identities and replies later calls are judged against (changes §10). Pure: mutates m only."""
    if method == 'projects.add':
        names = {path: name for name, path in (m.get('projectPaths') or {}).items()}
        if not isinstance(result, dict) or not isinstance(result.get('id'), str) or params['path'] not in names:
            raise ValueError('projects.add reply without an id')
        m.setdefault('projectIdByName', {})[names[params['path']]] = result['id']
        _append(m, 'projectIds', result['id'])
    elif method == 'projects.trust_preview':
        m.setdefault('lastPreview', {})[params['id']] = {'expectedCanonicalPath': result['canonicalPath'], 'expectedDigest': result.get('digest'),
                                                          'expectedScopeVersion': result['scopeVersion']}
    elif method == 'agents.discard_check':
        m.setdefault('lastDiscardCheck', {})[params['id']] = {k: result.get(k) for k in ('headOid', 'unmergedCommits', 'fingerprint', 'hasSubmodules', 'dirtyFiles')}
    elif method == 'agents.changes':
        m.setdefault('lastChanges', {})[params['id']] = {'headOid': result['headOid'], 'baseOid': result['baseOid']}
    elif method == 'agents.cleanup_candidates':
        m.setdefault('lastCandidates', {})[params['projectId']] = {key: list(result.get(key) or []) for key in ('worktrees', 'branches', 'offBase', 'submodules')}
    elif method == 'runtime.runs':
        # runtime.runs is limited to an owned project: its runs are adopted before any runtime.get (S6 A4 rule).
        for item in (result or {}).get('items') or []:
            run_id = item.get('id') if isinstance(item, dict) else None
            if (isinstance(run_id, str) and item.get('projectId') == params['projectId'] and RUN_ID.fullmatch(run_id)
                    and item.get('unit') == 'devbox-accept-run-' + run_id + '.service'):
                _append(m, 'runIds', run_id)
                _append(m, 'runUnits', item['unit'])
    elif method == 'terminal.create':
        if not isinstance(result, dict) or not isinstance(result.get('id'), str):
            raise ValueError('terminal.create reply without an id')
        _append(m, 'terminalIds', result['id'])
    elif method == 'runtime.start':
        run_id, unit = result.get('id'), result.get('unit')
        if result.get('projectId') in m.get('projectIds', []) and isinstance(run_id, str) and RUN_ID.fullmatch(run_id) and unit == 'devbox-accept-run-' + run_id + '.service':
            _append(m, 'runIds', run_id)
            _append(m, 'runUnits', unit)
    if method in ('agents.create', 'agents.get', 'agents.send', 'agents.resume', 'agents.review', 'agents.discard', 'agents.merge') and isinstance(result, dict):
        agent_id = result.get('id')
        if method == 'agents.create':
            if not isinstance(agent_id, str) or result.get('projectId') != params['projectId']:
                raise ValueError('agents.create reply without the owned project')
            _append(m, 'agentIds', agent_id)
            if params['projectId'] in cl_project_ids(m):
                _append(m, 'clAgentIds', agent_id)
            if params['profile'] in ('claude', 'codex'):
                observed = m.setdefault('trustObservation', {})
                if observed.get(params['profile']) in (None, PENDING):
                    observed[params['profile']] = agent_id
        if agent_id in m.get('agentIds', []):
            row = m.setdefault('agents', {}).setdefault(agent_id, {'terminalIds': [], 'sessionIds': []})
            for key in ('projectId', 'profile', 'worktreePath', 'branch', 'state'):
                if result.get(key) is not None:
                    row[key] = result[key]
            if result.get('terminalId'):
                _append(row, 'terminalIds', result['terminalId'])
            if result.get('sessionId'):
                _append(row, 'sessionIds', result['sessionId'])
            if result.get('worktreePath'):
                _append(m, 'agentWorktrees', result['worktreePath'])
    if method == 'agents.list' and isinstance(result, dict):
        for item in result.get('items') or []:
            if isinstance(item, dict) and item.get('id') in m.get('agentIds', []) and item.get('terminalId'):
                _append(m['agents'].setdefault(item['id'], {'terminalIds': [], 'sessionIds': []}), 'terminalIds', item['terminalId'])


def rpc_owned(path, method, params=None, call=rpc):
    """Authorize against the on-disk manifest, call, then record the reply on disk before returning.

    M9: a claude or codex agents.create first writes the pending trust observation to disk (after authorize, so it
    cannot refuse the first create), so a reply lost after the product made the task still blocks a second one."""
    params = {} if params is None else params
    _, m = load_owned(path)
    authorize(m, method, params)
    tool = params.get('profile') if method == 'agents.create' else None
    if tool in ('claude', 'codex') and not (m.get('trustObservation') or {}).get(tool):
        update_manifest(path, lambda disk: disk.setdefault('trustObservation', {}).setdefault(tool, PENDING))
    result = call(m, method, params)
    update_manifest(path, lambda disk: record_reply(disk, method, params, result))
    return result


def unit_state(name):
    result = subprocess.run(['systemctl', '--user', 'show', name, '-p', 'LoadState', '-p', 'ActiveState', '-p', 'SubState', '-p', 'MainPID', '-p', 'InvocationID', '-p', 'FragmentPath'],
                            capture_output=True, text=True, timeout=20, env=systemd_env())
    fields = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
    if result.returncode not in (0, 1) or (result.returncode and fields.get('LoadState') != 'not-found') or 'ActiveState' not in fields:
        raise RuntimeError('Cannot inspect exact unit: %s (systemctl exit %d: %s)' % (name, result.returncode, redact_text(result.stderr, 400)))
    return fields


def fingerprint():
    """Read-only prod channel identity: immutable files by hash, units by state, data roots by existence."""
    paths = [HOME / '.local/share/devbox/bin/devbox', HOME / '.config/devbox/config.toml', HOME / '.config/devbox/config.local.toml',
             HOME / '.config/devbox/prod/env', *[UNIT / p for p in ['devbox@.service', 'devbox@.socket', 'devbox-tmux@.service']]]
    files = []
    for path in paths:
        row = {'path': str(path), 'exists': path.exists() or path.is_symlink()}
        if path.is_symlink():
            row['symlink'] = os.readlink(path)
        elif path.is_file():
            st = path.stat()
            row.update(sha256=hashlib.sha256(path.read_bytes()).hexdigest(), mtimeNs=st.st_mtime_ns, size=st.st_size)
        files.append(row)
    roots = [{'path': str(p), 'exists': p.exists(), 'isDir': p.is_dir() and not p.is_symlink()}
             for p in [HOME / '.local/share/devbox/prod', HOME / '.local/state/devbox/prod', HOME / '.config/devbox']]
    units = {name: {k: v for k, v in unit_state(name).items() if k in ('LoadState', 'ActiveState', 'SubState', 'MainPID', 'InvocationID')}
             for name in ['devbox@prod.service', 'devbox@prod.socket', 'devbox-tmux@prod.service']}
    return {'files': files, 'roots': roots, 'units': units}


def load_owned(path):
    """The manifest, only when it names this run's own fixture root (M12: fixture.fixture_root(token), an lstat-proven
    0700 directory of this user) and that root holds the owner marker."""
    path, m, _ = owned_manifest(path)
    return path, m


def owned_manifest(path, absent_ok=False):
    """(path, m, root): load_owned's checks, where `root` is check_root's 'present', or 'absent' only with absent_ok.

    N1 (review 2 §3.3; Ruling 88): only main passes absent_ok, for RECOVERY. A root that lstat proves absent is then
    accepted. A root that exists is checked in full: a real 0700 directory of this user with the matching marker. The
    location checks run either way. The run, its RPC and its teardown keep load_owned."""
    path = Path(path)
    m = json.loads(path.read_text())
    token = m.get('token')
    root = Path(m['fixtureRoot'])
    if (m.get('channel') != 'accept' or not isinstance(token, str) or not TOKEN.fullmatch(token) or root != fixture_root(token)
            or path.name != 'ownership-' + token + '.json'):
        raise ValueError('Owned fixture location mismatch')
    return path, m, check_root(root, token, m['sourceSha'], absent_ok)


def instance_proof(m):
    """The accept daemon runs the exact pinned Linux artifact of the running phase for instance 'accept'."""
    expected = Path(f'/run/user/{os.getuid()}/devbox-accept.sock')
    if m['socketPath'] != str(expected) or expected.is_symlink() or not expected.is_socket() or expected.stat().st_uid != os.getuid():
        raise ValueError('Exact owned accept socket absent')
    staged = verify_staged(m)
    phase = m.get('phase')
    artifact = (m.get('linuxArtifacts') or {}).get(phase)
    if phase not in ('candidate', 'baseline') or not artifact or artifact['source_sha'] != m.get('phaseSha') or artifact['version'] != m.get('phaseVersion'):
        raise ValueError('Linux source/version mismatch for the running phase')
    state = unit_state('devbox-accept@accept.service')
    pid = int(state.get('MainPID', '0'))
    if pid <= 1 or state.get('ActiveState') != 'active':
        raise ValueError('Owned daemon is not active')
    with installed_binary(Path(m['acceptBinary']), artifact['sha256'], home=HOME) as (fd, target):
        pinned = os.fstat(fd)
        running = os.stat(f'/proc/{pid}/exe')
        if (pinned.st_dev, pinned.st_ino) != (running.st_dev, running.st_ino):
            raise ValueError('Running daemon does not use the exact pinned Linux artifact')
    kept = None
    baseline = (m.get('linuxArtifacts') or {}).get('baseline')
    if phase == 'candidate' and baseline:
        old = Path(m['acceptBinary']).parent / ('devbox-' + baseline['sha256'])
        if old.is_symlink() or not old.is_file():
            raise ValueError('The rc.1 binary must stay installed after the upgrade (PROGRESS.md:206)')
        kept = str(old)
    info = rpc(m, 'system.info')
    if info.get('instance') != 'accept' or info.get('dataDir') != str(HOME / '.local/share/devbox/accept'):
        raise ValueError('Daemon instance/data root mismatch')
    return {'status': 'PASS', 'phase': phase, 'sourceSha': m['phaseSha'], 'version': m['phaseVersion'], 'socket': str(expected),
            'verifiedTarget': target, 'binarySha256': artifact['sha256'], 'baselineBinaryKept': kept,
            'runningPid': pid, 'invocationId': state.get('InvocationID'), 'runningBinaryPinned': True, 'instance': 'accept', 'staged': staged}


def seal_row(item, wanted):
    """S6 seal rule for one absent-before path that now exists (None when absent)."""
    if item.exists() and item.is_dir() and not item.is_symlink():
        tag = item / MARKER
        if tag.is_symlink():
            raise ValueError('Owned directory marker became symlink')
        if tag.exists() and json.loads(tag.read_text()) != wanted:
            raise ValueError('Foreign marker')
        if not tag.exists():
            tag.write_text(json.dumps(wanted))
        return {'path': str(item), 'kind': 'directory'}
    if item.is_symlink():
        if item.parent.name not in ('sockets.target.wants', 'default.target.wants') or not str(item.resolve()).startswith(str(UNIT) + '/devbox-accept'):
            raise ValueError('Unexpected accept symlink; retain')
        return {'path': str(item), 'kind': 'symlink', 'target': os.readlink(item)}
    if item.exists() and item.is_file():
        if item.parent == UNIT and not item.read_bytes().startswith(SETUP_MARKER):
            raise ValueError('Unmanaged accept unit file')
        return {'path': str(item), 'kind': 'file', 'sha256': hashlib.sha256(item.read_bytes()).hexdigest()}
    if item.exists() and item.is_socket():
        return {'path': str(item), 'kind': 'socket'}
    return None


def seal(path, m):
    """Mark every accept root the installer/setup created (all proved absent before this run)."""
    wanted = {'token': m['token'], 'sourceSha': m['sourceSha']}
    created = [row for row in (seal_row(Path(raw), wanted) for raw in m['absentBefore']) if row]
    m['sealedPaths'] = created
    dump(path, m)
    return {'status': 'PASS', 'sealedPaths': created}


def setup_managed(item):
    item = Path(item)
    return (item.parent == UNIT and item.name in SETUP_UNITS) or (
        item.name == 'paths.conf' and item.parent.name in SETUP_DROPINS and item.parent.parent == UNIT)


def reseal(path, m):
    """Upgrade flows (changes §11): a sealed file may change only into a setup-managed unit file.

    Directories need their marker re-checked; links and sockets must be unchanged. A path absent at the
    first seal and present now was still proved absent before this run, so it is sealed by the same rule
    as seal(). Anything else raises and every resource is retained.
    """
    wanted = {'token': m['token'], 'sourceSha': m['sourceSha']}
    sealed = {row['path']: row for row in m.get('sealedPaths', [])}
    rows, changes = [], []
    for raw in m['absentBefore']:
        item = Path(raw)
        row = sealed.get(raw)
        if row is None:
            new = seal_row(item, wanted)
            if new:
                rows.append(new)
                changes.append({'path': raw, 'kind': new['kind'], 'oldSha256': None, 'newSha256': new.get('sha256'), 'new': True})
            continue
        if row['kind'] == 'directory':
            tag = item / MARKER
            if item.is_symlink() or not item.is_dir() or tag.is_symlink() or not tag.is_file() or json.loads(tag.read_text()) != wanted:
                raise ValueError('Sealed directory or its marker changed; retain: ' + raw)
        elif row['kind'] == 'file':
            if item.is_symlink() or not item.is_file():
                raise ValueError('Sealed file replaced; retain: ' + raw)
            data = item.read_bytes()
            digest = hashlib.sha256(data).hexdigest()
            if digest != row['sha256']:
                if not data.startswith(SETUP_MARKER) or not setup_managed(item):
                    raise ValueError('Changed sealed file is not a setup-managed unit; retain: ' + raw)
                changes.append({'path': raw, 'oldSha256': row['sha256'], 'newSha256': digest})
                row = {**row, 'sha256': digest}
        elif row['kind'] == 'symlink':
            if not item.is_symlink() or os.readlink(item) != row['target']:
                raise ValueError('Sealed link changed; retain: ' + raw)
        elif row['kind'] == 'socket':
            if (item.exists() or item.is_symlink()) and (item.is_symlink() or not item.is_socket()):
                raise ValueError('Sealed socket replaced; retain: ' + raw)
        rows.append(row)
    m['sealedPaths'] = rows
    m.setdefault('resealed', []).extend(changes)
    dump(path, m)
    return {'status': 'PASS', 'resealed': changes, 'sealedPaths': rows}


def proc_start(pid):
    return (Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()[19]


def stop_mcp(rows, kill=os.kill, start=proc_start, sleep=time.sleep):
    """SIGTERM, then SIGKILL after 2 s, only for a recorded MCP pid whose start time still matches."""
    done = []
    for row in rows:
        try:
            if start(row['pid']) != row['start']:
                done.append({'pid': row['pid'], 'state': 'gone-or-reused'})
                continue
        except (FileNotFoundError, ProcessLookupError, IndexError):
            done.append({'pid': row['pid'], 'state': 'gone'})
            continue
        kill(row['pid'], signal.SIGTERM)
        sleep(2)
        try:
            if start(row['pid']) == row['start']:
                kill(row['pid'], signal.SIGKILL)
                done.append({'pid': row['pid'], 'state': 'killed'})
                continue
        except (FileNotFoundError, ProcessLookupError, IndexError):
            pass
        done.append({'pid': row['pid'], 'state': 'terminated'})
    return done


def end_product_objects(path, m, call=None):
    """Cleanup steps 2-4 (changes §11): owned tasks through the product, then runs, then terminals."""
    call = call or (lambda method, params: rpc_owned(path, method, params))
    steps = []
    for agent_id in list(m.get('agentIds', [])):
        try:
            task = call('agents.get', {'id': agent_id})
            if task.get('state') == 'closed':
                steps.append({'action': 'agent', 'id': agent_id, 'status': 'PASS', 'state': 'closed'})
                continue
            check = call('agents.discard_check', {'id': agent_id})
            call('agents.discard', {'id': agent_id, 'expect': discard_expect(check)})
            steps.append({'action': 'agent', 'id': agent_id, 'status': 'PASS', 'state': 'discarded'})
        except Exception as error:
            steps.append({'action': 'agent', 'id': agent_id, 'status': 'FAIL', 'error': str(error)[:300]})
    for run_id in list(m.get('runIds', [])):
        try:
            steps.append({'action': 'runtime.stop', 'runId': run_id, 'status': 'PASS', 'state': call('runtime.stop', {'runId': run_id}).get('state')})
        except Exception as error:
            # An exited run refuses a stop; the unit logic below proves what stays loaded.
            steps.append({'action': 'runtime.stop', 'runId': run_id, 'status': 'PASS', 'note': str(error)[:300]})
    for terminal_id in list(m.get('terminalIds', [])):
        try:
            call('terminal.close', {'id': terminal_id})
            steps.append({'action': 'terminal.close', 'id': terminal_id, 'status': 'PASS'})
        except Exception as error:
            steps.append({'action': 'terminal.close', 'id': terminal_id, 'status': 'PASS', 'note': str(error)[:300]})
    return steps


def owned_worktrees(m, listing=None):
    """Recorded agent worktrees per project; anything else is preserved (changes §11)."""
    listing = listing or (lambda project: [line[9:] for line in command('git', '-C', str(project), 'worktree', 'list', '--porcelain').splitlines()
                                             if line.startswith('worktree ')])
    root = Path(m['fixtureRoot'])
    recorded = set(m.get('agentWorktrees', []))
    remove = []
    for name, raw in (m.get('projectPaths') or {}).items():
        project = Path(raw)
        if not (project / '.git').exists():
            continue
        for tree in listing(project):
            tree = Path(tree)
            if tree == project:
                continue
            if str(tree) not in recorded or tree.parent != root / (name + '-agents') or '..' in tree.parts:
                raise ValueError('Unexpected worktree; preserve: ' + str(tree))
            remove.append((project, tree))
    return remove


def remove_absent_parents(m):
    """M6: a parent the product may create, proven absent before the run, is removed only when it is empty now."""
    rows = []
    for raw in sorted(m.get('absentParentsBefore') or [], key=lambda r: (-len(Path(r).parts), r)):
        item = Path(raw)
        if not item.exists() and not item.is_symlink():
            rows.append({'path': raw, 'state': 'absent'})
        elif item.is_symlink() or not item.is_dir() or any(item.iterdir()):
            rows.append({'path': raw, 'state': 'retained', 'why': 'not an empty directory'})
        else:
            item.rmdir()
            rows.append({'path': raw, 'state': 'removed'})
    return rows


def cleanup(path, m, call=None, absent_ok=False):
    # M5: the load_owned checks (location, name, mode, marker) run here, on the driver path too, before anything
    # is stopped or removed; the on-disk manifest is the one used.
    # N1 (review 2 §3.3; Ruling 88): only main's RECOVERY passes absent_ok. A fixture root that lstat proves absent skips
    # only the root's own steps: the product objects of its projects (the owned RPC needs the root), its worktrees, the
    # created files under it and its removal. Everything else goes by the manifest and seal rows, as before, and the
    # result says "fixtureRoot": "absent".
    path, owned, root_state = owned_manifest(path, absent_ok)
    absent = root_state == 'absent'
    if any(owned.get(key) != m.get(key) for key in ('token', 'fixtureRoot', 'sourceSha')):
        raise ValueError('Cleanup was given a manifest that differs from the owned one on disk')
    m = owned
    if set(m['absentBefore']) != {str(p) for p in accept_paths()}:
        raise ValueError('Cleanup allowlist mismatch')
    if not set(m.get('absentParentsBefore') or []) <= {str(p) for p in parent_candidates()}:
        raise ValueError('Recorded parent directory is not one the product creates; retain everything')
    mcp = stop_mcp(m.get('mcpProcesses', []))
    if absent:
        # Their processes still end below: the recorded run and agent units, the tmux unit and the daemon are stopped.
        product = [{'action': 'product', 'status': 'NOT_RUN', 'reason': 'fixture root absent'}]
    elif Path(m['socketPath']).is_socket():
        product = end_product_objects(path, m, call)
    else:
        product = [{'action': 'product', 'status': 'NOT_RUN', 'reason': 'socket absent'}]
    dump(path.parent / 'cleanup-product-steps.json', {'mcp': mcp, 'product': product})
    journal = subprocess.run(['journalctl', '--user', '-u', 'devbox-accept@accept.service', '-n', '300', '--no-pager', '-o', 'short-monotonic'],
                             capture_output=True, text=True, timeout=20, env=systemd_env())
    dump(path.parent / 'owned-daemon-journal.json', {'exit': journal.returncode, 'stdout': journal.stdout[-200000:],
                                                     'stderr': redact_text(journal.stderr, 4000)})
    units = set(accept_runtime_units())
    if any(not u.endswith(('.service', '.scope')) for u in units):
        raise ValueError('Unknown owned runtime unit')
    allowed = set(INSTANCE_UNITS)
    known = {*('devbox-accept-run-' + i + '.service' for i in m.get('runIds', [])), *('devbox-accept-agent-' + i + '.scope' for i in m.get('agentIds', []))}
    if not units <= known:
        raise ValueError('Unrecorded accept jobs; preserve until ownership reviewed')
    sealed = {row['path']: row for row in m.get('sealedPaths', [])}
    for unit in sorted(units | allowed):
        state = unit_state(unit)
        if state['LoadState'] == 'not-found':
            continue
        if unit in allowed:
            check_fragment(state, sealed, unit)
        run_checked(['systemctl', '--user', 'stop', unit], timeout=20, env=systemd_env())
    for unit in sorted(units | allowed):
        if unit_state(unit)['ActiveState'] not in ('inactive', 'failed'):
            raise ValueError('Owned unit still active')
    # A failed transient run/agent unit stays loaded until reset-failed (the product's runtime.stop does the
    # same). Only units recorded in this manifest (units <= known above) are reset; nothing else is touched.
    for unit in sorted(units):
        if unit_state(unit)['ActiveState'] == 'failed':
            run_checked(['systemctl', '--user', 'reset-failed', unit], timeout=20, env=systemd_env())
    root = Path(m['fixtureRoot'])
    for project, tree in ([] if absent else owned_worktrees(m)):
        run_checked(['git', '-C', str(project), 'worktree', 'remove', '--force', str(tree)], timeout=30,
                    env={'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_NOSYSTEM': '1', 'PATH': '/usr/bin:/bin'})
    for row in reversed(m.get('createdFiles', [])):
        item = Path(row['path'])
        if item.is_relative_to(root):
            continue
        if item.exists() and item.is_file() and not item.is_symlink() and hashlib.sha256(item.read_bytes()).hexdigest() == row['sha256']:
            item.unlink()
    for raw in reversed(m['absentBefore']):
        item = Path(raw)
        if not item.exists() and not item.is_symlink():
            continue
        row = sealed.get(raw)
        if row is None:
            if item.is_dir() and not item.is_symlink() and not any(item.iterdir()):
                item.rmdir()
                continue
            raise ValueError('Unsealed accept resource retained: ' + raw)
        if row['kind'] == 'directory':
            if item.is_symlink():
                raise ValueError('Owned directory replaced')
            if json.loads((item / MARKER).read_text()) != {'token': m['token'], 'sourceSha': m['sourceSha']}:
                raise ValueError('Marker changed')
            shutil.rmtree(item)
        elif row['kind'] == 'symlink':
            if not item.is_symlink() or os.readlink(item) != row['target']:
                raise ValueError('Owned link changed')
            item.unlink()
        elif row['kind'] == 'file':
            if item.is_symlink() or hashlib.sha256(item.read_bytes()).hexdigest() != row['sha256']:
                raise ValueError('Owned file changed')
            item.unlink()
        elif row['kind'] == 'socket':
            if not item.is_socket() or item.is_symlink():
                raise ValueError('Owned socket changed')
            item.unlink()
    run_checked(['systemctl', '--user', 'daemon-reload'], timeout=20, env=systemd_env())
    parents = remove_absent_parents(m)
    if absent:
        if owned_manifest(path, True)[2] != 'absent':  # N1: a root that appeared since the first check is retained
            raise ValueError('Owned fixture root appeared during cleanup; retain: ' + str(root))
    else:
        load_owned(path)  # M5: checked again right before the root goes
        shutil.rmtree(root)
    remaining = [str(p) for p in accept_paths() if p.exists() or p.is_symlink()]
    deadline = time.monotonic() + 10
    loaded = accept_runtime_units()
    while loaded and time.monotonic() < deadline:
        time.sleep(0.5)
        loaded = accept_runtime_units()
    if remaining or loaded:
        raise ValueError('Accept resources remain after cleanup: ' + repr(remaining + loaded))
    failed = [s for s in product if s.get('status') == 'FAIL']
    note = {'fixtureRoot': 'absent'} if absent else {}
    dump(path.parent / 'fixture-cleanup.json', {'status': 'PASS', 'units': sorted(units | allowed), 'root': str(root), **note, 'parents': parents,
                                                'productFailures': failed})
    return {'status': 'PASS', 'productFailures': failed, **note}


def check_fragment(state, sealed, unit):
    template = Path(state.get('FragmentPath', ''))
    row = sealed.get(str(template))
    if not row or row.get('kind') != 'file' or template.is_symlink() or hashlib.sha256(template.read_bytes()).hexdigest() != row['sha256']:
        raise ValueError('Unit file ownership changed; will not stop it: ' + unit)


def restart_tmux(m, case, state=unit_state, run=subprocess.run):
    """K1 추가(tmux 업그레이드) only, with Q3: restart the owned tmux unit after the fragment hash check."""
    if case != 'K1X-TMUXUP' or m.get('flow') != 'wsl-fresh' or (m.get('answers') or {}).get('Q3') is not True:
        raise ValueError('The owned tmux unit restarts only in K1X-TMUXUP with Q3 yes')
    current = state(TMUX_UNIT)
    check_fragment(current, {row['path']: row for row in m.get('sealedPaths', [])}, TMUX_UNIT)
    run_checked(['systemctl', '--user', 'restart', TMUX_UNIT], timeout=20, run=run, env=systemd_env())
    return {'status': 'PASS', 'unit': TMUX_UNIT, 'before': {k: current.get(k) for k in ('ActiveState', 'MainPID', 'InvocationID')}}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=['fingerprint', 'instance', 'seal', 'reseal', 'cleanup', 'rpc', 'restart-tmux'])
    parser.add_argument('--ownership')
    parser.add_argument('--method')
    parser.add_argument('--params', default='{}')
    parser.add_argument('--case')
    args = parser.parse_args()
    if args.action == 'fingerprint':
        print(json.dumps(fingerprint()))
        return
    require_lock()
    # N1 (Ruling 88): only RECOVERY accepts a fixture root that lstat proves absent; cleanup reports it itself.
    path, m, root = owned_manifest(args.ownership, args.action in RECOVERY)
    note = {'fixtureRoot': 'absent'} if root == 'absent' else {}
    if args.action == 'rpc':
        print(json.dumps(rpc_owned(path, args.method, json.loads(args.params))))
        return
    actions = {'instance': lambda: instance_proof(m), 'seal': lambda: {**seal(path, m), **note}, 'reseal': lambda: {**reseal(path, m), **note},
               'cleanup': lambda: cleanup(path, m, absent_ok=True), 'restart-tmux': lambda: restart_tmux(m, args.case)}
    print(json.dumps(actions[args.action]()))


if __name__ == '__main__':
    main()
