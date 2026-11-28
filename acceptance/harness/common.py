"""S7 acceptance shared helpers. Importing this module never touches Windows or WSL state."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time

BASE = Path(__file__).resolve().parent
PS = '/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe'
SHA_RE = re.compile('[0-9a-f]{40}')
# Product budgets: crates/cli/src/windows_observation.rs and app/src-tauri/src/platform/windows_probe.rs.
BUDGET_SECONDS = {'observation': 3.0, 'listeners': 5.0, 'paths': 5.0}
PROBE_FILES = ('crates/protocol/src/windows/native.ps1', 'crates/protocol/src/windows/probe.ps1')
# Owned RPC whole-request deadlines. A service runtime.start awaits readiness until ready_timeout_sec
# (crates/runtime/src/execution.rs wait_ready), so its deadline must exceed the fixture's readiness timeout.
RPC_DEADLINE_SECONDS = 15
A4_READY_TIMEOUT_SEC = 180
START_RPC_DEADLINE_SECONDS = 210


def rpc_deadline(method):
    """Fixed per-method deadline (never caller-chosen): only runtime.start may exceed the 15 s default."""
    return START_RPC_DEADLINE_SECONDS if method == 'runtime.start' else RPC_DEADLINE_SECONDS


def verification_root(base=BASE):
    """The devbox-verification directory of this Git common dir (never a relative parents[n] guess)."""
    for parent in Path(base).resolve().parents:
        if parent.name == 'devbox-verification' and (parent.parent / 'objects').is_dir() and (parent.parent / 'HEAD').is_file():
            return parent
    raise RuntimeError('Harness must live under <git-common-dir>/devbox-verification')


def evidence_root(arg=None, create=True, base=BASE):
    """The S7 evidence root (changes §4): <verification_root>/s7 or a directory under it.

    No `..`, no symlink anywhere on the path, owned by this user with mode 0700. Created (one level,
    0700) only when create is true; the driver checks with create=False before the lock.
    """
    allowed = verification_root(base) / 's7'
    raw = Path(arg) if arg else allowed
    if '..' in raw.parts or not raw.is_absolute():
        raise ValueError('Evidence root must be an absolute path without ..')
    path = raw.resolve()
    if str(path) != os.path.abspath(raw) or raw.is_symlink():
        raise ValueError('Evidence root must not be or contain a symlink')
    if path != allowed and not path.is_relative_to(allowed):
        raise ValueError('Evidence root must equal or lie under ' + str(allowed))
    if not path.exists():
        if not create:
            return path
        if path != allowed and (path.parent.is_symlink() or not path.parent.is_dir()):
            raise ValueError('Evidence root parent must already exist')
        path.mkdir(mode=0o700)
    st = path.lstat()
    if not path.is_dir() or path.is_symlink() or st.st_uid != os.getuid() or st.st_mode & 0o777 != 0o700:
        raise ValueError('Evidence root must be a directory owned by this user with mode 0700: ' + str(path))
    return path


def interop_probe(timeout=20, ps=PS, run=subprocess.run):
    """Changes §1.5: one trivial PowerShell start (ps_command's argv). Unavailable on any sign of broken interop.

    `exit 0` prints no JSON, so the exit code and stderr are read directly instead of ps_command's JSON parse.
    """
    from evidence import Unavailable
    try:
        result = run([ps, '-NoProfile', '-NonInteractive', '-Command', 'exit 0'], capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        raise Unavailable('Interop probe timed out after %d s' % timeout)
    except OSError as error:
        raise Unavailable('Interop probe could not start: ' + type(error).__name__)
    stderr = (result.stderr or b'').decode('utf-8', 'replace')
    if result.returncode != 0 or 'UtilAcceptVsock' in stderr:
        raise Unavailable('Interop probe failed (exit %s, UtilAcceptVsock lines %d)' % (result.returncode, stderr.count('UtilAcceptVsock')))
    return {'status': 'PASS', 'exit': 0}


def dump(path, value):
    path = Path(path)
    tmp = path.with_name(path.name + '.tmp')
    tmp.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n')
    tmp.replace(path)


# --- Redaction (changes §8.1.6), shared since fix round 1 (M7, item 6) -----------------------------------
REDACT_LINE = re.compile(r'(?i)(api[_-]?key|token|secret|password|bearer|sk-[a-z0-9]{8})')
EMAIL = re.compile(r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}')
# Fix round 3 (R3-M1): in a line the whole-line rule leaves, key-shaped tokens, and the value after key, token, secret
# or password with `=` or `:`, become MASK. Git OIDs, sha256 hex, the a/t + 32-hex ids and the fixture paths have none
# of these shapes, so they stay as they are; so does the phrase "API key" that round 2's stop evidence needs.
MASK = '[masked]'
MASK_TOKEN = re.compile(r'(?<![A-Za-z0-9])(?:sk-[A-Za-z0-9_-]{16,}|(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}'
                        r'|xox[abprs]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}'
                        r'|[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})(?![A-Za-z0-9])')
MASK_VALUE = re.compile(r'(?i)((?:key|token|secret|password)\s*[=:]\s*)\S+')
STDERR_LIMIT = 2000


def mask_line(line):
    """Key-shaped tokens, and the value after key/token/secret/password with `=` or `:`, replaced in place by MASK."""
    return MASK_VALUE.sub(lambda match: match.group(1) + MASK, MASK_TOKEN.sub(MASK, line))


def redact(lines, tail=60):
    """A line that names a secret becomes [redacted]. In any other line, key-shaped tokens become [masked] (mask_line)
    and e-mail addresses [email]. Only the last `tail` lines are kept (None keeps every line, for texts that are
    bounded by size instead)."""
    out = []
    for line in (lines if tail is None else lines[-tail:]):
        out.append('[redacted]' if REDACT_LINE.search(line) else EMAIL.sub('[email]', mask_line(line)))
    return out


def redact_text(text, limit=STDERR_LIMIT):
    """Bounded, redacted text: the last `limit` characters, each line through redact()."""
    if text is None:
        return None
    if isinstance(text, bytes):
        text = text.decode('utf-8', 'replace')
    return '\n'.join(redact(str(text)[-limit:].splitlines(), tail=None))


def _expand(value):
    if isinstance(value, dict):
        return {key: _expand(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_expand(item) for item in value]
    if isinstance(value, str) and value[:1] in ('{', '['):
        try:
            return _expand(json.loads(value))
        except ValueError:
            return value
    return value


def redact_json(value, limit=20000):
    """M7: a JSON value or JSON text, pretty-printed one value per line (JSON held in strings expanded too), each line
    through redact(), bounded to `limit` characters. A compact pair is one line, which REDACT_LINE would blank whole."""
    if value is None:
        return None
    if isinstance(value, (bytes, str)):
        text = value.decode('utf-8', 'replace') if isinstance(value, bytes) else value
        try:
            value = json.loads(text)
        except ValueError:
            return redact(text[-limit:].splitlines(), tail=None)
    out, size = [], 0
    for line in redact(json.dumps(_expand(value), ensure_ascii=False, indent=1).splitlines(), tail=None):
        size += len(line) + 1
        if size > limit:
            out.append('(cut at the %d-character evidence bound)' % limit)
            break
        out.append(line)
    return out


class CommandFailed(RuntimeError):
    """A harness-run subprocess that failed or timed out (fix round 1 item 6). Only bounded, redacted text is kept:
    `stderr` is the last STDERR_LIMIT characters through redact(); the argv is bounded per word."""

    def __init__(self, argv, returncode, stderr, timed_out=False):
        self.argv = [str(word)[:200] for word in list(argv)[:16]]
        self.returncode, self.timed_out = returncode, timed_out
        self.stderr = redact_text(stderr) or ''
        last = next((line for line in reversed(self.stderr.splitlines()) if line.strip()), '')
        what = 'timed out' if timed_out else 'exit %s' % returncode
        super().__init__(('Command failed (%s): %s: %s' % (what, ' '.join(self.argv[:8]), last))[:600])

    @classmethod
    def from_timeout(cls, error):
        return cls(error.cmd if isinstance(error.cmd, (list, tuple)) else [error.cmd], None, error.stderr, timed_out=True)


def run_checked(argv, timeout, run=None, **kwargs):
    """subprocess.run with captured output; a non-zero exit or a timeout raises CommandFailed with redacted stderr."""
    argv = [str(word) for word in argv]
    try:
        result = (run or subprocess.run)(argv, capture_output=True, timeout=timeout, **kwargs)
    except subprocess.TimeoutExpired as error:
        raise CommandFailed.from_timeout(error) from None
    if result.returncode:
        raise CommandFailed(argv, result.returncode, result.stderr)
    return result


def error_evidence(error):
    """What a case keeps of an error that ended it: type, bounded message, exit code and redacted stderr."""
    return {'type': type(error).__name__, 'message': redact_text(str(error), 600), 'exit': getattr(error, 'returncode', None),
            'timedOut': bool(getattr(error, 'timed_out', False)) or isinstance(error, (TimeoutError, subprocess.TimeoutExpired)),
            'stderr': redact_text(getattr(error, 'stderr', None))}


# --- Project trust as the app asks for it (fix round 1 item 5) --------------------------------------------
TRUST_TASK_OPERATIONS = ('test', 'discard', 'merge', 'pr')
TRUST_ERRORS = ('agents.project_trust_required', 'projects.trust_required', 'projects.trust_preview_stale')  # useProjectTrust.tsx:11-17


def trust_context(project_id, operation, agent_id=None, worktrees=None, worktree=None):
    """projects.trust_preview params exactly as the app sends them (TrustPreviewParams, crates/projects/src/model.rs:194-204):
    create {worktree: <the create's value>} (NewTaskDialog.tsx:85-91); run and launch with nulls (useRunActions.tsx:35,
    ProjectSwitcher.tsx:113); test, discard, merge and pr with the task id (useReviewAction.ts:65-75, AgentActions.tsx:100-106);
    cleanup with the selected candidate worktrees, sent only when there is one (AgentCleanup.tsx:58-72)."""
    worktrees = list(worktrees or [])
    if operation == 'create':
        ok = type(worktree) is bool and agent_id is None and not worktrees
    elif operation in ('run', 'launch'):
        ok = agent_id is None and not worktrees and worktree is None
    elif operation in TRUST_TASK_OPERATIONS:
        ok = isinstance(agent_id, str) and bool(agent_id) and not worktrees and worktree is None
    elif operation == 'cleanup':
        ok = bool(worktrees) and all(isinstance(w, str) for w in worktrees) and agent_id is None and worktree is None
    else:
        ok = False
    if not ok:
        raise ValueError('Not a trust context the app sends: %s' % operation)
    return {'id': project_id, 'operation': operation, 'agentId': agent_id, 'worktrees': worktrees, 'worktree': worktree}


def ensure_trust(rpc, context, check=None):
    """The app's trust.ensure() (useProjectTrust.tsx:66-118): preview with the context; go on when no approval is needed
    or it is already given; refuse when one is needed without a config or digest; else approve with exactly the
    previewed values. Returns the evidence row; a refusal raises before the action. `check(preview)` runs before any
    approval and may refuse it (A4 checks the owned service definition there)."""
    preview = rpc('projects.trust_preview', context)
    row = {'operation': context['operation'], 'agentId': context['agentId'], 'worktrees': list(context['worktrees']), 'worktree': context['worktree'],
           'requiresApproval': preview.get('requiresApproval'), 'approved': preview.get('approved'), 'digest': preview.get('digest'),
           'runDefinitions': [d.get('id') for d in preview.get('runDefinitions') or [] if isinstance(d, dict)], 'approvedNow': False}
    if preview.get('projectId') not in (None, context['id']):
        raise RuntimeError('The trust preview answered for another project')
    if check is not None:
        check(preview)
    if not preview.get('requiresApproval') or preview.get('approved'):
        return row
    if preview.get('config') is None or not preview.get('digest'):
        raise RuntimeError('The trust preview needs an approval but has no config or digest; the app refuses the action')
    rpc('projects.trust_approve', {'id': context['id'], 'expectedCanonicalPath': preview['canonicalPath'], 'expectedDigest': preview['digest'],
                                   'expectedScopeVersion': preview['scopeVersion']})
    row['approvedNow'] = True
    return row


def with_trust(rpc, context, call, rows=None):
    """The app's withTrust for runs (useRunActions.tsx:30-39): try; on a project trust error, ensure() with the run
    context, then exactly one retry."""
    try:
        return call()
    except Exception as error:
        if getattr(error, 'code', None) not in TRUST_ERRORS:
            raise
        row = dict(ensure_trust(rpc, context), after=error.code)
        if rows is not None:
            rows.append(row)
        return call()


# One writer at a time for the ownership manifest inside this process (the RPC reply recorder runs on
# the start thread too). Every change is load-modify-write, so no writer persists a stale copy.
MANIFEST_LOCK = threading.RLock()


def update_manifest(path, change):
    with MANIFEST_LOCK:
        value = json.loads(Path(path).read_text())
        change(value)
        dump(path, value)
        return value


def process_start(pid):
    return (Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()[19]


def _parent(pid):
    return int((Path('/proc') / str(pid) / 'stat').read_text().rsplit(')', 1)[1].split()[1])


def lock_owner_for(subject_pid, lock=None):
    """Return the verify-resources.py supervisor that holds windows-acceptance for subject_pid."""
    lock = Path(lock) if lock else verification_root() / 'windows-acceptance.lock'
    st = lock.stat()
    key = f'{os.major(st.st_dev):02x}:{os.minor(st.st_dev):02x}:{st.st_ino}'
    for line in Path('/proc/locks').read_text().splitlines():
        if ' FLOCK ' not in line or ' WRITE ' not in line or key not in line.split():
            continue
        pid = int(line.split()[4])
        try:
            args = (Path('/proc') / str(pid) / 'cmdline').read_bytes().split(b'\0')
        except FileNotFoundError:
            continue
        if b'--lock' not in args or b'windows-acceptance' not in args or not any(b'verify-resources.py' in a for a in args):
            continue
        current = subject_pid
        while current > 1:
            if current == pid:
                return pid
            current = _parent(current)
        # Under an enforced cgroup the command runs in a systemd scope, not as a descendant.
        cgroup = (Path('/proc') / str(subject_pid) / 'cgroup').read_text()
        children = (Path('/proc') / str(pid) / 'task' / str(pid) / 'children').read_text().split()
        for child in children:
            command = (Path('/proc') / child / 'cmdline').read_bytes().split(b'\0')
            if b'--scope' in command and b'--unit' in command:
                unit = command[command.index(b'--unit') + 1].decode()
                if unit.startswith('devbox-verify-') and '/' + unit + '.scope' in cgroup:
                    return pid
    raise RuntimeError('Run inside the owning windows-acceptance supervisor; a contended foreign lock is insufficient')


def require_lock():
    return lock_owner_for(os.getpid())


def cgroup_evidence():
    cgroup = Path('/proc/self/cgroup').read_text().strip()
    return {'cgroup': cgroup, 'scoped': '/devbox-verify-' in cgroup and cgroup.rstrip().endswith('.scope')}


def git(worktree, *args):
    return run_checked(['git', '-C', str(worktree), *args], timeout=30).stdout


def probe_script(worktree, sha):
    """Exact bytes of POWERSHELL_OBSERVATION_PROBE = concat!(native.ps1, "\\n", probe.ps1) at sha."""
    if not SHA_RE.fullmatch(sha):
        raise ValueError('Exact 40-hex source SHA required')
    if git(worktree, 'rev-parse', sha + '^{commit}').decode().strip() != sha:
        raise ValueError('Source SHA is not a commit in this worktree')
    native, probe = (git(worktree, 'show', sha + ':' + name) for name in PROBE_FILES)
    script = native + b'\n' + probe
    text = script.decode('ascii')  # Non-ASCII would change interop argv encoding; refuse it.
    return text, {
        'sourceSha': sha,
        'files': {name: hashlib.sha256(data).hexdigest() for name, data in zip(PROBE_FILES, (native, probe))},
        'scriptSha256': hashlib.sha256(script).hexdigest(),
        'scriptBytes': len(script),
    }


def harness_hashes(*names):
    return {name: hashlib.sha256((BASE / name).read_bytes()).hexdigest() for name in ('common.py', *names)}


def native_class_source(worktree, sha):
    return git(worktree, 'show', sha + ':' + PROBE_FILES[0]).decode('ascii')


def assigned_receipt(nonce, pid, creation, sid, session, run='s7-owned-run'):
    return {'schema': 1, 'runId': run, 'launchId': 's7-owned-launch', 'attemptId': 's7-owned-attempt',
            'nonce': nonce, 'jobName': 'Local\\Devbox.Observe.' + nonce, 'pid': int(pid),
            'creationTime': str(creation), 'sid': sid, 'sessionId': int(session)}


def valid_reply(kind, parsed):
    """Mirror WindowsProbeResult::valid_for plus serde shape for the two probes used here."""
    if not isinstance(parsed, dict) or parsed.get('type') != kind or set(parsed) != {'type', 'state'}:
        return False
    state = parsed['state']
    if kind == 'observation':
        if not isinstance(state, dict) or state.get('state') not in {'active', 'empty', 'notFound', 'unavailable', 'identityMismatch'}:
            return False
        if state['state'] == 'active':
            return set(state) == {'state', 'count'} and isinstance(state['count'], int) and state['count'] > 0
        return set(state) == {'state'}
    return state in {'owned', 'notOwned', 'empty', 'notFound', 'identityMismatch', 'unavailable'}


def run_probe(script, request, harness_timeout=30.0, ps=PS):
    """Invoke the probe exactly like query_with_timeout (same argv, stdin JSON, 4096-byte stdout bound).

    The harness deadline is longer than the product budget so overruns are measured, not truncated;
    productState applies the product budget (counted from spawn, as tokio::time::timeout does).
    """
    kind = request['type']
    body = json.dumps(request, separators=(',', ':')).encode()
    started = time.monotonic()
    child = subprocess.Popen([ps, '-NoProfile', '-NonInteractive', '-Command', script],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    spawned = time.monotonic()
    output = b''
    timed_out = False
    try:
        child.stdin.write(body)
        child.stdin.close()
        holder = {}
        reader = threading.Thread(target=lambda: holder.update(data=child.stdout.read(4097)), daemon=True)
        reader.start()
        reader.join(max(0.0, harness_timeout - (time.monotonic() - started)))
        if reader.is_alive():
            timed_out = True
        else:
            output = holder.get('data', b'')
            child.wait(timeout=max(0.1, harness_timeout - (time.monotonic() - started)))
    except subprocess.TimeoutExpired:
        timed_out = True
    finally:
        if child.poll() is None:
            child.kill()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
        child.stdout.close()
    ended = time.monotonic()
    raw = None
    parsed = None
    if not timed_out and child.returncode == 0 and len(output) <= 4096:
        try:
            parsed = json.loads(output)
            if valid_reply(kind, parsed):
                raw = parsed['state'] if kind == 'listeners' else parsed['state']['state']
        except ValueError:
            parsed = None
    after_spawn = ended - spawned
    within = raw is not None and after_spawn <= BUDGET_SECONDS[kind]
    return {'kind': kind, 'exit': child.returncode, 'timedOut': timed_out, 'stdout': output[:4096].decode('utf-8', 'replace'),
            'reply': parsed, 'rawState': raw, 'productState': raw if within else 'unavailable',
            'budgetSeconds': BUDGET_SECONDS[kind], 'withinBudget': within,
            'wallSeconds': round(ended - started, 4), 'afterSpawnSeconds': round(after_spawn, 4),
            'monotonicStart': round(started, 4)}


class LineProcess:
    """An owned interop PowerShell fixture with bounded line reads (never an unbounded ReadToEnd)."""

    def __init__(self, script, ps=PS, limit=200):
        self.child = subprocess.Popen([ps, '-NoProfile', '-NonInteractive', '-Command', script],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.limit = limit
        self.lines = []
        self.stderr = []
        self._cv = threading.Condition()
        threading.Thread(target=self._pump, args=(self.child.stdout, self.lines), daemon=True).start()
        threading.Thread(target=self._pump, args=(self.child.stderr, self.stderr), daemon=True).start()

    def _pump(self, stream, sink):
        for raw in iter(stream.readline, b''):
            with self._cv:
                if len(sink) < (self.limit if sink is self.lines else 200):
                    sink.append(raw.decode('utf-8', 'replace').rstrip('\r\n')[:4000])
                self._cv.notify_all()
        with self._cv:
            self._cv.notify_all()

    def read_json(self, index, timeout):
        deadline = time.monotonic() + timeout
        with self._cv:
            while len(self.lines) <= index:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or (self.child.poll() is not None and len(self.lines) <= index):
                    raise TimeoutError('Owned fixture line %d not received; stderr=%r' % (index, redact(self.stderr[-5:], tail=None)))
                self._cv.wait(min(remaining, 0.2))
            return json.loads(self.lines[index])

    def send(self, line):
        self.child.stdin.write((line + '\n').encode())
        self.child.stdin.flush()

    def close(self, timeout=20):
        try:
            if self.child.stdin and not self.child.stdin.closed:
                self.child.stdin.close()
        except OSError:
            pass
        try:
            return self.child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self.child.kill()
            try:
                self.child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
            return None


def ps_command(script, timeout=60, ps=PS):
    """One bounded read-only/owned PowerShell command returning parsed JSON stdout."""
    result = run_checked([ps, '-NoProfile', '-NonInteractive', '-Command', script], timeout=timeout)
    return json.loads(result.stdout.decode('utf-8-sig', 'replace').strip())


# Read-only identity check for an exact owned PID + creation FILETIME. Never opens with more than
# PROCESS_QUERY_LIMITED_INFORMATION, never signals anything.
IDENTITY_TYPE = r'''
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class S7Identity {
  [StructLayout(LayoutKind.Sequential)] struct FileTime { public uint Low, High; }
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr process, out FileTime c, out FileTime e, out FileTime k, out FileTime u);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  public static string State(uint pid, string creation) {
    IntPtr p=OpenProcess(0x1000,false,pid);
    if(p==IntPtr.Zero) { int e=Marshal.GetLastWin32Error(); return e==87 ? "absent" : "unavailable:"+e; }
    try {
      FileTime c,x,k,u;
      if(!GetProcessTimes(p,out c,out x,out k,out u)) return "unavailable:times";
      string created=(((ulong)c.High<<32)|c.Low).ToString(System.Globalization.CultureInfo.InvariantCulture);
      if(created!=creation) return "reused";
      uint code; if(!GetExitCodeProcess(p,out code)) return "unavailable:exit";
      return code==259 ? "alive" : "exited:"+code;
    } finally { CloseHandle(p); }
  }
  public static string Creation(uint pid) {
    IntPtr p=OpenProcess(0x1000,false,pid); if(p==IntPtr.Zero) return null;
    try { FileTime c,x,k,u; if(!GetProcessTimes(p,out c,out x,out k,out u)) return null;
      return (((ulong)c.High<<32)|c.Low).ToString(System.Globalization.CultureInfo.InvariantCulture); } finally { CloseHandle(p); }
  }
}
'@
'''


def identity_gone(state):
    """The exact owned PID/creation pair is known to have exited or no longer exists."""
    if state in ('absent', 'reused'):
        return True
    match = re.fullmatch(r'exited:([0-9]{1,10})', state) if isinstance(state, str) else None
    if not match:
        return False
    code = int(match[1])
    return code <= 0xffffffff and code != 259  # STILL_ACTIVE


def identity_states(pairs, timeout=40):
    """pairs: list of (label, pid, creation). Returns {label: alive|exited:N|absent|reused|unavailable:*}."""
    calls = ';'.join("$o['%s']=[S7Identity]::State([uint32]%d,'%s')" % (label, int(pid), str(int(creation))) for label, pid, creation in pairs)
    return ps_command(IDENTITY_TYPE + "$o=@{};" + calls + ";$o|ConvertTo-Json -Compress", timeout=timeout)


STOP_TYPE = r'''
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class S7ExactStop {
  [StructLayout(LayoutKind.Sequential)] struct FileTime { public uint Low, High; }
  [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process, out uint code);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr process, out FileTime c, out FileTime e, out FileTime k, out FileTime u);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr process, uint code);
  [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle, uint ms);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
  // Identity is proven and the process terminated through the same handle: no PID-reuse window.
  public static string Stop(uint pid, string creation) {
    IntPtr p=OpenProcess(0x1000|0x0001|0x00100000,false,pid);
    if(p==IntPtr.Zero) { int e=Marshal.GetLastWin32Error(); return e==87 ? "absent" : "unavailable:"+e; }
    try {
      FileTime c,x,k,u;
      if(!GetProcessTimes(p,out c,out x,out k,out u)) return "unavailable:times";
      string created=(((ulong)c.High<<32)|c.Low).ToString(System.Globalization.CultureInfo.InvariantCulture);
      if(created!=creation) return "reused-not-stopped";
      uint code; if(!GetExitCodeProcess(p,out code)) return "unavailable:exit";
      if(code!=259) return "already-exited:"+code;
      if(!TerminateProcess(p,1)) return "terminate-failed:"+Marshal.GetLastWin32Error();
      return WaitForSingleObject(p,10000)==0 ? "stopped" : "stop-unconfirmed";
    } finally { CloseHandle(p); }
  }
}
'@
'''


def stop_exact(label, pid, creation, timeout=40):
    """Terminate only the exact owned identity (PID + creation FILETIME on one handle)."""
    script = STOP_TYPE + "@{label='%s';result=[S7ExactStop]::Stop([uint32]%d,'%s')}|ConvertTo-Json -Compress" % (label, int(pid), str(int(creation)))
    return ps_command(script, timeout=timeout)
