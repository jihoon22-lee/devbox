"""WSL-only owned accept daemon from the pinned Linux artifact, without the app (changes §7).

The install and exec scripts are the product's own bytes, extracted from the bound
`app/src-tauri/src/core/deploy.rs` of the running phase's SHA; the setup and doctor argv are the ones
that SHA's app passes (core/deploy.rs, platform/deploy.rs). Only the `wsl.exe --exec` transport is
missing. This module never imports a native helper and never starts a Windows program.
"""
import hashlib
import json
import os
from pathlib import Path
import pwd
import re
import subprocess
import threading
import time

from common import CommandFailed, git, redact_text, require_lock
from installed_binary import installed_binary

DEPLOY_CORE = 'app/src-tauri/src/core/deploy.rs'
DEPLOY_PLATFORM = 'app/src-tauri/src/platform/deploy.rs'
PATHS_RE = re.compile(r'const PATHS_SCRIPT: &str = r#"(.*?)"#;', re.S)
INSTALL_RE = re.compile(r'const INSTALL_SCRIPT: &str = r#"(.*?)"#;', re.S)
STORAGE = 'devbox-accept'  # AppChannel::Accept storage_name (crates/protocol/src/channel.rs)
SETUP_ARGV = {('candidate', 'launch'): ['setup', '--json'],
              ('candidate', 'redeploy'): ['setup', '--json', '--restart'],
              ('baseline', 'launch'): ['setup', '--json']}
# platform/deploy.rs: install 120 s, setup 90 s, doctor 30 s.
TIMEOUTS = {'install': 120, 'setup': 90, 'doctor': 30}
INSTALL_REFUSALS = range(61, 67)  # the install script's own exits (61-66)
HOME = Path.home()
ENV_INSTALL_KEYS = ('HOME', 'PATH')
ENV_SETUP_KEYS = ('HOME', 'USER', 'LOGNAME', 'SHELL', 'LANG', 'PATH', 'XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS')
WINDOWS_LOCAL = re.compile(r'/mnt/c/Users/[^/]+/AppData/Local')
# The S7 and the S6 owned install/stage prefixes (changes §7). Test 1 forbids the S6 literal in helpers,
# so both are one character class.
OWNED_PREFIX = re.compile(r'DevboxAccept-s[67]-')


def env_install(home=HOME):
    return {'HOME': str(home), 'PATH': '/usr/bin:/bin'}


def env_setup(entry=None):
    """Nothing is inherited: the passwd identity plus the fixed locale, PATH and user-bus variables."""
    entry = entry or pwd.getpwuid(os.getuid())
    uid = entry.pw_uid
    return {'HOME': entry.pw_dir, 'USER': entry.pw_name, 'LOGNAME': entry.pw_name, 'SHELL': entry.pw_shell, 'LANG': 'C.UTF-8',
            'PATH': '/usr/local/bin:/usr/bin:/bin', 'XDG_RUNTIME_DIR': f'/run/user/{uid}', 'DBUS_SESSION_BUS_ADDRESS': f'unix:path=/run/user/{uid}/bus'}


ENV_INSTALL = env_install()
ENV_SETUP = env_setup()


def extract(source):
    """PATHS_SCRIPT and INSTALL_SCRIPT raw strings; exactly one match of each, else refuse."""
    text = source.decode('utf-8') if isinstance(source, bytes) else source
    found = {}
    for name, pattern in (('paths', PATHS_RE), ('install', INSTALL_RE)):
        matches = pattern.findall(text)
        if len(matches) != 1:
            raise ValueError('Expected exactly one %s raw string in deploy.rs, found %d' % (name, len(matches)))
        found[name] = matches[0]
    return found


def compose(found):
    """install_script_for / exec_script_for (core/deploy.rs) for the accept channel."""
    paths = found['paths'].replace('@STORAGE@', STORAGE)
    return {'install': paths + '\n' + found['install'], 'exec': paths + '\nexec "$d/devbox" "$@"'}


def scripts(worktree, sha, expected_sha256=None):
    """The phase's own deploy.rs bytes (git show); their sha256 must equal the source binding."""
    source = git(worktree, 'show', sha + ':' + DEPLOY_CORE)
    if expected_sha256 is not None and hashlib.sha256(source).hexdigest() != expected_sha256:
        raise ValueError('deploy.rs differs from the bound source for ' + sha)
    out = compose(extract(source))
    out['sha256'] = {k: hashlib.sha256(v.encode()).hexdigest() for k, v in out.items()}
    platform = git(worktree, 'show', sha + ':' + DEPLOY_PLATFORM)
    out['doctorArgv'] = doctor_argv(source.decode(), platform.decode())
    return out


def doctor_argv(core, platform):
    """Choose only known invocations from the exact app revision being measured."""
    if re.search(r'\.args\s*\(\s*STARTUP_DOCTOR_ARGS\s*\)', platform):
        definition = re.search(r'(?m)^\s*(?:pub\s+)?const\s+STARTUP_DOCTOR_ARGS\s*:[^=]+='
                               r'\s*\[\s*"doctor"\s*,\s*"--json"\s*,\s*"(--launch-check|--startup)"\s*,?\s*\]\s*;', core)
        if definition:
            return ['doctor', '--json', definition[1]]
    else:
        # rc.1 invoked the doctor inline in the wsl.exe argument array.
        direct = re.search(r'\bdoctor\.args\(\s*(\[[^\]]+\])\s*\)', platform)
        if direct and argv_in_source(['doctor', '--json'], direct[1]):
            return ['doctor', '--json']
    raise ValueError('Unsupported startup doctor invocation in bound deploy sources')


def argv_in_source(argv, text):
    """The quoted tokens appear in this order, separated only by commas and whitespace, and end the array."""
    pattern = r'\s*,\s*'.join(re.escape('"' + token + '"') for token in argv) + r'\s*,?\s*\]'
    return re.search(pattern, text) is not None


def read_artifact(artifact):
    data = Path(artifact['path']).read_bytes()
    if hashlib.sha256(data).hexdigest() != artifact['sha256']:
        raise ValueError('devbox-linux bytes differ from linux-receipt.json')
    return data


def install_binary(m, phase, phase_scripts, run=subprocess.run, home=HOME):
    """`sh -c <install> sh <sha256>` as the app's install runs it (app/src-tauri/src/platform/deploy.rs:14-27, no wsl.exe). The
    app writes the binary only after the script asks with `send` (core/deploy.rs:513-528, offer); here it is on stdin from
    the start, and a script that answers `kept` exits without reading it."""
    require_lock()
    artifact = m['linuxArtifacts'][phase]
    data = read_artifact(artifact)
    started = time.monotonic()
    result = run(['sh', '-c', phase_scripts['install'], 'sh', artifact['sha256']], input=data, env=env_install(home), capture_output=True,
                 timeout=TIMEOUTS['install'])
    row = {'phase': phase, 'exit': result.returncode, 'seconds': round(time.monotonic() - started, 3),
           'stderr': redact_text(result.stderr, 2000), 'scriptSha256': phase_scripts['sha256']['install']}
    if result.returncode:
        # M7 and item 6: the failure keeps the bounded, redacted stderr; the install script's own exits are refusals.
        what = 'refused' if result.returncode in INSTALL_REFUSALS else 'failed'
        raise CommandFailed(['product-install-script', what, phase], result.returncode, result.stderr)
    with installed_binary(Path(m['acceptBinary']), artifact['sha256'], home=home) as (fd, target):
        row['target'] = target
    row['status'] = 'PASS'
    return row


class StepFailed(RuntimeError):
    """A setup or doctor run that did not succeed; `failure` is what evidence.attribute() reads."""

    def __init__(self, message, failure, row):
        super().__init__(message)
        self.failure, self.row = failure, row


def exec_argv(phase_scripts, argv):
    return ['env', 'DEVBOX_INSTANCE=accept', 'sh', '-c', phase_scripts['exec'], 'sh', *argv]


def _run(phase_scripts, argv, timeout, run):
    started = time.monotonic()
    try:
        result = run(exec_argv(phase_scripts, argv), env=env_setup(), capture_output=True, timeout=timeout)
    except subprocess.TimeoutExpired as error:
        return None, {'argv': argv, 'timedOut': True, 'seconds': round(time.monotonic() - started, 3),
                      'stderr': redact_text(error.stderr, 4000) or ''}
    stdout = (result.stdout or b'').decode('utf-8', 'replace')
    # M7: setup and doctor stderr through redact(); UtilAcceptVsock lines stay for the interop attribution.
    return stdout, {'argv': argv, 'exit': result.returncode, 'timedOut': False, 'seconds': round(time.monotonic() - started, 3),
                    'stderr': redact_text(result.stderr, 4000) or ''}


def setup(m, phase, trigger, phase_scripts, run=subprocess.run, journal=lambda: []):
    """`setup --json[ --restart]` as the phase's app runs it; the report is parsed whatever the exit code."""
    require_lock()
    argv = SETUP_ARGV[(phase, trigger)]
    stdout, row = _run(phase_scripts, argv, TIMEOUTS['setup'], run)
    row.update(phase=phase, trigger=trigger)
    if stdout is None:
        # M14: setup runs the login shell (env capture); a hang there under broken interop is R-IOP-STALL (evidence.attribute).
        raise StepFailed('setup timed out after %d s' % TIMEOUTS['setup'],
                         {'step': 'setup', 'reason': 'setup timed out', 'stderr': row['stderr'], 'journal': journal(), 'timeout': True,
                          'shellInit': 'setup-timeout', 'message': 'setup timed out after %d s' % TIMEOUTS['setup']}, row)
    try:
        steps = json.loads(stdout).get('steps') or []
        row['steps'] = [{'id': s.get('id'), 'ok': s.get('ok'), 'message': str(s.get('message', ''))[:300]} for s in steps]
    except (ValueError, AttributeError):
        row['steps'] = None
    failed = [s for s in row['steps'] or [] if s['ok'] is not True]
    row['restartMessage'] = next((s['message'] for s in row['steps'] or [] if s['id'] == 'restart'), None)
    row['env'] = env_summary(m)
    if row['steps'] is None or failed or row['exit']:
        first = failed[0] if failed else {'id': 'report', 'message': 'no setup report'}
        failure = {'step': 'setup:' + str(first['id']), 'reason': 'setup step %s failed: %s' % (first['id'], first['message']),
                   'stderr': row['stderr'], 'journal': journal(), 'timeout': False, 'message': first['message'],
                   'shellInit': 'setup-env' if first['id'] == 'env' else None}
        raise StepFailed(failure['reason'], failure, row)
    row['status'] = 'PASS'
    return row


def descendants(root, proc=Path('/proc')):
    """PIDs whose parent chain reaches root (read from /proc/<pid>/stat only)."""
    parents = {}
    for entry in proc.iterdir():
        if entry.name.isdigit():
            try:
                parents[int(entry.name)] = int((entry / 'stat').read_text().rsplit(')', 1)[1].split()[1])
            except (OSError, IndexError, ValueError):
                continue
    found = set()
    for pid in parents:
        current, seen = pid, 0
        while current in parents and seen < 64:
            current, seen = parents[current], seen + 1
            if current == root:
                found.add(pid)
                break
    return found


def flagged(argv):
    """CS4: the product's command probe, whose argv holds the word `devbox-probe` (its $0 since P-DEF-1:
    `bash -lic … devbox-probe <program>`, crates/agents/src/command_path.rs:181), `bash -ic ...`, or an agent CLI
    `--version` run. Round 2 item 6: `bash -ic` alone would miss a probe run under another shell shape."""
    words = [w.decode('utf-8', 'replace') if isinstance(w, bytes) else w for w in argv if w]
    if not words:
        return None
    if 'devbox-probe' in words:
        return 'devbox-probe'
    name = words[0].rsplit('/', 1)[-1]
    if name == 'bash':
        # Short options before the first operand, combined (`-ic`, `-lic`) or separate (`-i -c`); `-o x` takes a value.
        letters, index = '', 1
        while index < len(words):
            word = words[index]
            if word in ('-o', '+o', '-O', '+O'):
                index += 2
                continue
            if word == '--' or not word.startswith(('-', '+')):
                break
            if not word.startswith('--'):
                letters += word[1:]
            index += 1
        if 'i' in letters and 'c' in letters:
            return 'bash -ic'
    for index, word in enumerate(words[:3]):
        tool = word.rsplit('/', 1)[-1]
        if tool in ('claude', 'codex', 'opencode', 'pi') and '--version' in words[index:]:
            return tool + ' --version'
    return None


class Sampler:
    """Every 50 ms, the argv of each descendant of one PID (read-only, /proc/<pid>/cmdline only).

    M1: `loops` counts the completed passes; CS4 needs at least one."""

    def __init__(self, interval=0.05, proc=Path('/proc')):
        self.interval, self.proc = interval, proc
        self.pid = None
        self.rows = {}
        self.loops = 0
        self.thread = None
        self.stop = threading.Event()

    def watch(self, pid):
        self.pid = pid
        self.thread = threading.Thread(target=self._loop, daemon=True)
        self.thread.start()

    def _loop(self):
        while not self.stop.is_set():
            for child in descendants(self.pid, self.proc):
                try:
                    argv = (self.proc / str(child) / 'cmdline').read_bytes().split(b'\0')
                except OSError:
                    continue
                key = (child, argv[0] if argv else b'')
                if key not in self.rows:
                    self.rows[key] = {'pid': child, 'flag': flagged(argv), 'argv0': (argv[0] if argv else b'').decode('utf-8', 'replace')[:200]}
            self.loops += 1
            self.stop.wait(self.interval)

    def result(self):
        self.stop.set()
        if self.thread is not None:
            self.thread.join(timeout=5)
        rows = list(self.rows.values())
        return {'samples': len(rows), 'loops': self.loops, 'flagged': [r for r in rows if r['flag']], 'argv0': sorted({r['argv0'] for r in rows})}


def popen_run(argv, env, timeout, sampler):
    """Like subprocess.run, with the sampler watching the child's descendants."""
    child = subprocess.Popen(argv, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    sampler.watch(child.pid)
    try:
        out, err = child.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        child.kill()
        out, err = child.communicate()
        raise subprocess.TimeoutExpired(argv, timeout, output=out, stderr=err)
    return subprocess.CompletedProcess(argv, child.returncode, out, err)


def startup_doctor(m, phase, phase_scripts, run=subprocess.run, sampler=None, journal=lambda: []):
    """The startup doctor the phase's bound app source invokes. tool.* rows keep a short message."""
    require_lock()
    argv = phase_scripts['doctorArgv']
    if sampler is not None:
        run = lambda full, env, capture_output, timeout: popen_run(full, env, timeout, sampler)
    stdout, row = _run(phase_scripts, argv, TIMEOUTS['doctor'], run)
    row['phase'] = phase
    if sampler is not None:
        row['descendants'] = sampler.result()
    if stdout is None:
        raise StepFailed('doctor timed out after %d s' % TIMEOUTS['doctor'],
                         {'step': 'doctor', 'reason': 'doctor timed out', 'stderr': row['stderr'], 'journal': journal(), 'timeout': True}, row)
    try:
        checks = json.loads(stdout).get('checks') or []
    except (ValueError, AttributeError):
        raise StepFailed('doctor printed no report', {'step': 'doctor', 'reason': 'no doctor report', 'stderr': row['stderr'], 'journal': journal()}, row)
    row['checks'] = [dict({'id': c.get('id'), 'status': c.get('status')},
                          **({'message': str(c.get('message', ''))[:200]} if str(c.get('id', '')).startswith('tool.') else {})) for c in checks]
    row['status'] = 'PASS'
    return row


def env_summary(m, home=HOME):
    """After setup: the number of PATH entries in the owned env file and whether one starts with /mnt/c."""
    path = Path(home) / '.config/devbox-accept/accept/env'
    try:
        lines = path.read_text().splitlines()
    except OSError:
        return {'present': False}
    value = next((line[5:] for line in reversed(lines) if line.startswith('PATH=')), None)
    if value is None:
        return {'present': True, 'pathEntries': 0, 'mntC': False}
    entries = [e for e in value.strip('"').split(':') if e]
    return {'present': True, 'pathEntries': len(entries), 'mntC': any(e.startswith('/mnt/c') for e in entries)}


def drvfs_absent(windows_local):
    """No owned-prefix install path exists under the bound Windows LocalAppData (drvfs read only).

    Registry keys cannot be read without interop; the evidence says so.
    """
    root = Path(windows_local)
    if not WINDOWS_LOCAL.fullmatch(str(windows_local)) or root.is_symlink() or not root.is_dir():
        raise ValueError('The bound Windows LocalAppData drvfs path is required')
    found = sorted(p.name for p in root.iterdir() if p.name.startswith('DevboxAccept-'))
    owned = [name for name in found if OWNED_PREFIX.match(name)]
    if owned:
        raise RuntimeError('Owned-prefix accept install paths exist; preserve and ask: ' + repr(owned))
    return {'drvfsAbsent': True, 'windowsLocal': str(root), 'checkedPattern': OWNED_PREFIX.pattern,
            'otherAcceptNames': found, 'registry': 'NOT_RUN: registry keys are not readable without interop'}
