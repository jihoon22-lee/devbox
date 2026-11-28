#!/usr/bin/env python3
"""S7-14 accept-channel acceptance entry point (one windows-acceptance lock, stage to cleanup).

Flows (changes §1.1, §6, §7, §8.0):
- `wsl-fresh`, `wsl-upgrade`: the owned accept daemon from the pinned Linux artifact, with the product's
  own install/exec scripts and setup/doctor argv. These paths never start a Windows program and never
  call native() or the interop probe.
- `fresh`, `upgrade`: the accept NSIS installer(s) on Windows; they need Q0 and a passing interop probe.
- `update` (1.0.1): the baseline accept install updates itself to the candidate installer with the app's own
  update commands (update_flow.py); the same needs as the other Windows flows.

Never downloads, dispatches or selects a workflow run. Fails closed before the lock unless the
two-part approval binds every helper byte (helperSha256) and this run's inputs (binding: source
binding, flow, cases, answers, SHAs, runs, branches, artifact folders and digests, windowsLocal).
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import uuid

# M13: no bytecode beside the helpers (S6 C3). This process writes none, every child Python gets -B, and a
# __pycache__ already in $H refuses the run before any helper module could be loaded from it.
sys.dont_write_bytecode = True


def refuse_bytecode(base):
    cache = Path(base) / '__pycache__'
    if cache.exists() or cache.is_symlink():
        raise SystemExit('Refusing to start: %s exists; remove it (the helpers run with python3 -B)' % cache)


if __name__ == '__main__':
    refuse_bytecode(Path(__file__).resolve().parent)

from common import (BASE, PS, cgroup_evidence, dump, error_evidence, evidence_root, git, interop_probe, redact_text, require_lock, run_checked,
                    update_manifest)
import foreign_files
from coordinator import Coordinator
import evidence as ev
from evidence import (BRANCHES, FLOWS, NEEDS, REPO, UPGRADE_FLOWS, WINDOWS_FLOWS, WSL_FLOWS, Unavailable, approval_binding,
                      artifact_digest, initial, parse_answers, record, verify_artifact_dir, verify_review, verify_run, wait_sc2)
from fixture import BINDING_FILE, MARKER, delegate_windows, inspect_accept_absent, stage, verify_source
from teardown import cleanup_stage, compare_prod, finalize, worktree_orphans
import agents_live
import daemon_rows
import mcp_stdio
import wsl_daemon

# Read-only GitHub metadata only (`gh api repos/<repo>/actions/runs/<id>`). Absolute path: the supervisor
# environment may not include /snap/bin.
GH = '/snap/bin/gh' if Path('/snap/bin/gh').exists() else 'gh'
WORKTREE = '/home/jihoon/projects/devbox-wt/v1-s7-rc2'
SHA = re.compile('[0-9a-f]{40}')
DISTRO = re.compile('[A-Za-z0-9_.-]+')
# Recorded once per WSL-only run; every WSL case points to it (changes §7 "Differences from the app path").
WSL_DIFFERENCES = {
    'transport': 'no wsl.exe --exec: the product scripts run with sh -c inside WSL',
    'environment': 'setup captures bash -lc env -0 from the harness fixed env (wsl.exe would append Windows PATH entries); '
                   'only the env file PATH entry count and a /mnt/c flag are recorded',
    'noApp': ['no App-client Hello', 'no native broker', 'no toasts', 'no Windows runs',
              "the doctor's powershell.exe probe (crates/cli/src/doctor/os.rs:79-94) is Unavailable after its 2 s deadline"],
}
# Owned in memory by A4 (a4_windows_service mutates them); everything else is adopted from disk.
DRIVER_KEYS = ('a4', 'a4ServiceId', 'windowsProcesses')
# Both writers add to these; the merge keeps the union.
UNION_KEYS = ('projectIds', 'runIds', 'runUnits')
OPENCODE = ('H3', 'H3X-REPORT', 'H3X-CONFIG', 'H3X-SHELL', 'SN1', 'SN2', 'H3X-EXIT', 'H3X-ENV', 'H3X-NEST')
PI = ('K1X-SEND', 'H4', 'SN2')  # agents_live.pi_block's order (M8)
CLAUDE = ('H1X-TRUST', 'H1', 'H1X-SHELL', 'H1X-HELP', 'P1', 'SN2')
CODEX = ('H2X-TRUST', 'H2', 'H2X-RESUME', 'H2X-RESUME1S', 'H2X-FEEDBACK', 'H2X-SCREENS', 'H2X-SHELLMARK', 'P1X', 'SN2')


def _load_wsl_owned():
    spec = importlib.util.spec_from_file_location('wsl_owned', BASE / 'wsl-owned.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


wsl_owned = _load_wsl_owned()


def command(argv, timeout=60, check=True):
    """A harness child process. With check, a failure raises common.CommandFailed (bounded, redacted stderr)."""
    argv = list(map(str, argv))
    if not check:
        return subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    return run_checked(argv, timeout=timeout, text=True)


def timed_out(error):
    return isinstance(error, (TimeoutError, subprocess.TimeoutExpired)) or bool(getattr(error, 'timed_out', False))


def selected_cases(flow, value=None):
    """A unique, flow-ordered subset of the flow's case list whose NEEDS are selected too (changes §8.0)."""
    allowed = FLOWS[flow]
    cases = list(allowed) if not value else value.split(',')
    if not cases or any(c not in allowed for c in cases) or cases != [c for c in allowed if c in cases] or len(set(cases)) != len(cases):
        raise ValueError('Explicit unique ordered case IDs from ' + ','.join(allowed))
    for case in cases:
        missing = NEEDS[flow].get(case, set()) - set(cases)
        if missing:
            raise ValueError(case + ' requires ' + ','.join(sorted(missing)))
    return cases


def parse(argv=None):
    parser = argparse.ArgumentParser(description='S7-14 accept acceptance (run only under verify-resources.py --lock windows-acceptance)')
    parser.add_argument('--flow', required=True, choices=list(FLOWS))
    parser.add_argument('--candidate-sha', required=True)
    parser.add_argument('--candidate-run', required=True, type=int)
    parser.add_argument('--baseline-sha')
    parser.add_argument('--baseline-run', type=int)
    # 1.0.1 (D1): each side's artifact folder (<evidence root>/<name>/artifacts) and dispatch branch are run arguments
    # that the approval binding pins; no helper names a candidate.
    parser.add_argument('--candidate-artifacts', required=True)
    parser.add_argument('--baseline-artifacts')
    parser.add_argument('--candidate-branch', default=BRANCHES['candidate'])
    parser.add_argument('--baseline-branch')
    # 1.0.1 (D5): the update flow's version pair, bound by the approval. `same-version` is what the accept runner accepts.
    parser.add_argument('--update-shape', choices=list(ev.UPDATE_SHAPES))
    parser.add_argument('--evidence-root')
    parser.add_argument('--worktree', default=WORKTREE)
    parser.add_argument('--cases')
    parser.add_argument('--answers', default=str(BASE / 'answers.json'))
    parser.add_argument('--review', default=str(BASE / 'review-approval.json'))
    parser.add_argument('--distro', default=os.environ.get('WSL_DISTRO_NAME'))
    # The drvfs path of Windows LocalAppData (/mnt/c/Users/<user>/AppData/Local): bound by the approval, never guessed.
    parser.add_argument('--windows-local', required=True)
    args = parser.parse_args(argv)
    problems = []
    if not SHA.fullmatch(args.candidate_sha) or args.candidate_run <= 0:
        problems.append('exact 40-hex candidate SHA and a positive run')
    upgrade = args.flow in UPGRADE_FLOWS
    if upgrade != (args.baseline_sha is not None) or upgrade != (args.baseline_run is not None):
        problems.append('--baseline-sha/--baseline-run are required for the upgrade flows and refused for the others')
    elif upgrade and (not SHA.fullmatch(args.baseline_sha) or args.baseline_run <= 0 or args.baseline_sha == args.candidate_sha):
        problems.append('exact 40-hex baseline SHA (not the candidate) and a positive run')
    if upgrade != (args.baseline_artifacts is not None):
        problems.append('--baseline-artifacts is required for the two-sided flows and refused for the others')
    if not upgrade and args.baseline_branch is not None:
        problems.append('--baseline-branch is refused for a flow without a baseline')
    if args.flow == 'update' and args.baseline_branch is None:
        problems.append('--baseline-branch is required for the update flow (its baseline is not the upgrade flows\' rc.1 branch)')
    if args.flow == 'update':
        args.update_shape = args.update_shape or ev.UPDATE_SHAPES[0]
    elif args.update_shape is not None:
        problems.append('--update-shape is only for the update flow')
    if upgrade and args.baseline_branch is None:
        args.baseline_branch = BRANCHES['baseline']
    try:
        ev.artifact_dir_name(args.candidate_artifacts)
        ev.branch_name(args.candidate_branch)
        if upgrade and args.baseline_artifacts is not None:
            ev.artifact_dir_name(args.baseline_artifacts)
            ev.branch_name(args.baseline_branch)
            if args.baseline_artifacts == args.candidate_artifacts:
                raise ValueError('The baseline and the candidate need different artifact folders')
    except ValueError as error:
        problems.append(str(error))
    if args.flow in WINDOWS_FLOWS and (not args.distro or not DISTRO.fullmatch(args.distro)):
        problems.append('--distro is required for the Windows flows')
    if not wsl_daemon.WINDOWS_LOCAL.fullmatch(args.windows_local):
        problems.append('--windows-local must be /mnt/c/Users/<user>/AppData/Local')
    try:
        args.cases = selected_cases(args.flow, args.cases)
    except ValueError as error:
        problems.append(str(error))
    if problems:
        parser.error('; '.join(problems))
    args.answers = str(Path(args.answers).resolve())
    args.review = str(Path(args.review).resolve())
    if args.flow in WSL_FLOWS:
        args.distro = None  # WSL-only flows start no Windows program
    return args


def sides_of(args):
    sides = {'candidate': {'sha': args.candidate_sha, 'runId': args.candidate_run, 'branch': args.candidate_branch,
                           'artifacts': args.candidate_artifacts}}
    if args.flow in UPGRADE_FLOWS:
        sides['baseline'] = {'sha': args.baseline_sha, 'runId': args.baseline_run, 'branch': args.baseline_branch,
                             'artifacts': args.baseline_artifacts}
    return sides


def gates(args, root=None):
    """Everything checked before the lock (changes §1.1): pure file reads, no subprocess, no host action."""
    root = root or evidence_root(args.evidence_root, create=False)
    if BINDING_FILE.is_symlink() or not BINDING_FILE.is_file():
        raise ValueError('source-binding.json is UNBOUND; the controller binds the exact candidate with bind_source.py')
    binding_bytes = BINDING_FILE.read_bytes()
    binding = json.loads(binding_bytes)
    sides = sides_of(args)
    if (binding.get('schema') != 2 or (binding.get('candidate') or {}).get('sha') != args.candidate_sha
            or (binding.get('baseline') or {}).get('sha') != (sides.get('baseline') or {}).get('sha')):
        raise ValueError('Source binding differs from this flow (candidate/baseline SHA)')
    answers_bytes = Path(args.answers).read_bytes()
    answers = parse_answers(answers_bytes)
    for side, row in sides.items():
        row['version'] = binding[side]['version']
        row['dir'] = str(ev.artifact_dir(root, row['artifacts']))  # D1: the bound folder name, checked under the evidence root
        row['artifactSha256'] = artifact_digest(row['dir'])
    shape = None
    if args.flow == 'update':
        import update_fixture  # pure; the version pair is refused here, before the lock
        shape = update_fixture.check_shape(args.update_shape, sides['baseline']['version'], sides['candidate']['version'])
    approval = approval_binding(binding_bytes, args.flow, args.cases, answers_bytes, sides['candidate'], sides.get('baseline'), args.windows_local,
                                update_shape=shape, distro=args.distro)
    review = verify_review(BASE, args.review, approval)
    return {'binding': binding, 'answers': answers, 'sides': sides, 'approval': approval, 'review': review, 'root': str(root)}


def merge_manifest(path, owned):
    """Write A4's in-memory keys and the union of the shared id lists; adopt every other key from disk.
    Driver-owned values keep their object identity in memory, so later in-place mutations (A4's dict)
    reach disk on the next save."""
    def change(disk):
        if disk.get('token') != owned.get('token'):
            raise ValueError('Ownership manifest token changed')
        for key in DRIVER_KEYS:
            if key in owned:
                disk[key] = owned[key]
        for key in UNION_KEYS:
            rows = list(disk.get(key) or [])
            rows += [v for v in owned.get(key) or [] if v not in rows]
            disk[key] = rows
    disk = update_manifest(path, change)
    for key, value in disk.items():
        if key not in DRIVER_KEYS:
            owned[key] = value


def probe_marker(script):
    """A fixed printable line of the probe script that a Windows command line carries unchanged (W1 (아))."""
    rows = [line.strip() for line in script.splitlines()]
    rows = [r for r in rows if re.fullmatch(r'[A-Za-z0-9 _.$=(){}\[\]:,;+-]{24,}', r)]
    if not rows:
        raise ValueError('No probe marker line')
    return max(rows, key=len)[:64]


PENDING = 'Not executed in this run'


def classify(error, journal=lambda: []):
    """(status, reason, failure) for an error that ended a case or a group of cases.

    A step failure is FAIL with its own failure (interop attribution applies in evidence.record); a guard
    refusal, an unexpected tool screen or a missing precondition is NOT_RUN; anything else is FAIL.
    """
    if isinstance(error, wsl_daemon.StepFailed):
        return 'FAIL', 'Step failed: ' + str(error)[:500], error.failure
    if isinstance(error, agents_live.StopPrompt):
        return 'NOT_RUN', ev.reason('R-PROMPT') + ' (' + str(error)[:120] + ')', None
    if isinstance(error, agents_live.Refused):
        return 'NOT_RUN', 'Harness guard refused: ' + str(error)[:300], None
    if isinstance(error, Unavailable):
        return 'NOT_RUN', 'Precondition unavailable: ' + str(error)[:500], None
    return 'FAIL', '%s: %s' % (type(error).__name__, str(error)[:500]), ev.failure(
        'case', error, stderr=getattr(error, 'stderr', None), journal=journal(), timeout=timed_out(error))


def a6_decision(outcome):
    """A6 (matrix S6 묶음 A6, changes §5): every teardown step passed or was not needed, nothing retained,
    the prod comparison PASS, and the fresh accept-absent proof (native acceptAbsent and WSL) PASS."""
    absent = outcome.get('acceptAbsentAfter') or {}
    problems = []
    if (outcome.get('prodComparison') or {}).get('status') != 'PASS':
        problems.append('prod comparison is not PASS')
    if outcome.get('retainedResources'):
        problems.append('resources retained')
    if any(row.get('status') not in ('PASS', 'NOT_RUN') for row in outcome.get('cleanup', [])):
        problems.append('a teardown step failed')
    if absent.get('status') != 'PASS' or (absent.get('native') or {}).get('acceptAbsent') is not True or not absent.get('wsl'):
        problems.append('the accept-absent proof after cleanup failed')
    if not any(row.get('action') == 'windows' and row.get('status') == 'PASS' for row in outcome.get('cleanup', [])):
        problems.append('the owned native uninstall did not pass')
    return ('PASS', None) if not problems else ('FAIL', '; '.join(problems))


def m4_decision(outcome, native_flow):
    """M4: prod unchanged and nothing retained. A WSL-only run proves only the WSL half (R-IOP½)."""
    absent = outcome.get('acceptAbsentAfter') or {}
    clean = ((outcome.get('prodComparison') or {}).get('status') == 'PASS' and not outcome.get('retainedResources')
             and all(row.get('status') in ('PASS', 'NOT_RUN') for row in outcome.get('cleanup', [])) and absent.get('status') == 'PASS')
    if not clean:
        return 'FAIL', 'Prod changed, resources retained or the accept-absent proof failed', None
    if native_flow:
        return 'PASS', None, None
    return 'NOT_RUN', ev.reason('R-IOP½'), ['wsl']


def tmux_decision(before, after):
    """K1 추가(tmux 업그레이드), Linux half. A missing precondition (the user's change) is NOT_RUN, never FAIL."""
    version = lambda text: tuple(int(x) for x in re.findall(r'\d+', text or '')[:2]) or (0,)
    on = lambda text: (text or '').split()[-1:] in (['on'], ['always'])
    if version(before.get('version')) >= (3, 5):
        return 'NOT_RUN', 'Precondition: the running owned tmux server was not on the old (<3.5) version before the restart'
    if version(after.get('version')) < (3, 5):
        return 'NOT_RUN', 'Precondition: after the restart the host tmux is still <3.5 (the user change was not made)'
    if on(before.get('extendedKeys')) or not on(after.get('extendedKeys')):
        return 'FAIL', 'extended-keys was on before the restart, or devbox did not turn it on after it'
    return 'PASS', None


class DaemonOps:
    """The owned accept setup/doctor for daemon_rows (wsl_daemon with the candidate's own scripts)."""

    def __init__(self, session):
        self.s = session

    def setup(self, trigger):
        t0 = time.time()
        return wsl_daemon.setup(self.s.load(), 'candidate', trigger, self.s.scripts['candidate'], journal=lambda: self.s.journal_since(t0))

    def startup_doctor(self, sampled=False):
        t0 = time.time()
        return wsl_daemon.startup_doctor(self.s.load(), 'candidate', self.s.scripts['candidate'], sampler=wsl_daemon.Sampler() if sampled else None,
                                         journal=lambda: self.s.journal_since(t0))


def native_command(windows, script, params):
    """The -Command text for a reviewed helper: the file read and run as a script block, each parameter quoted. The
    execution policy is never set, overridden or bypassed by flag."""
    quote = lambda value: "'" + str(value).replace("'", "''") + "'"
    parts = ['& ([ScriptBlock]::Create([IO.File]::ReadAllText(' + quote(windows(BASE / script)) + ')))']
    for key, value in {**params, **({'HelperRoot': windows(BASE)} if script in ('native-lifecycle.ps1', 'native-window.ps1') else {})}.items():
        if not re.fullmatch('[A-Za-z0-9]+', key):
            raise ValueError('Invalid helper parameter name')
        parts.append('-' + key + ' ' + quote(value))
    return '[Console]::OutputEncoding=New-Object Text.UTF8Encoding; ' + ' '.join(parts)


class NativeOps:
    """Windows operations for daemon_rows and w1_windows, built only in the Windows flows."""

    def __init__(self, session):
        self.s = session

    def stop_owned(self):
        return self.s.native('native-lifecycle.ps1', Action='stop-owned', Ownership=self.s.windows(self.s.ownership))

    def launch(self):
        self.s.launched = True
        return self.s.native('native-lifecycle.ps1', Action='launch', Ownership=self.s.windows(self.s.ownership))

    def sc2(self, record, clock=time):
        """C1 (1.0.1): the SC2 lines of one launch record's stderr log, start phases included (evidence.wait_sc2). Recorded
        only, so a failed read is kept as its error."""
        try:
            return wait_sc2(Path(command(['wslpath', '-u', record['stderr']]).stdout.strip()), clock=clock)
        except Exception as error:
            return {'error': redact_text(str(error), 400)}

    def settings_drvfs(self):
        import w1_windows
        return w1_windows.drvfs(self.s.load()['nativePreflight']['appLocalData']) / 'settings.json'

    def stage_drvfs(self):
        return self.s.stage_dir

    def install_drvfs(self):
        import w1_windows
        return w1_windows.drvfs(self.s.load()['nativePreflight']['installPath'])

    def window_agent(self, seconds):
        """A8: the long-lived owned window agent (native-window.ps1): one command per stdin line, one JSON line back."""
        from common import LineProcess
        params = {'Ownership': self.s.windows(self.s.ownership), 'WaitSeconds': seconds}
        return LineProcess(native_command(self.s.windows, 'native-window.ps1', params), limit=64)

    def console_windows(self, nonce, marker):
        return self.s.native('native-observe.ps1', Action='console-windows', Nonce=nonce, ProbeMarker=marker)

    def console_watch(self, nonce, seconds):
        """I3: one long-lived read-only watcher (native-observe.ps1 -Action console-watch); W1 closes its stdin to stop it."""
        import w1_windows
        from common import LineProcess
        interval = w1_windows.WATCH['intervalMs']
        params = {'Action': 'console-watch', 'Nonce': nonce, 'WatchSeconds': seconds, 'IntervalMs': interval}
        return LineProcess(native_command(self.s.windows, 'native-observe.ps1', params), limit=seconds * 1000 // interval + 100)

    def stop_exact(self, label, pid, creation):
        from common import stop_exact
        return stop_exact(label, pid, creation)


class Session:
    def __init__(self, args, ctx, owner, root):
        self.args, self.ctx, self.root = args, ctx, Path(root)
        self.worktree_stat = os.stat(args.worktree)
        self.start_tick = time.clock_gettime_ns(time.CLOCK_BOOTTIME) * os.sysconf('SC_CLK_TCK') // 1_000_000_000
        self.flow_name, self.cases = args.flow, args.cases
        self.answers, self.binding, self.sides = ctx['answers'], ctx['binding'], ctx['sides']
        runs = self.root / 'runs'
        if runs.is_symlink():
            raise ValueError('runs must not be a symlink')
        runs.mkdir(mode=0o700, exist_ok=True)
        self.token = (args.candidate_sha[:8] + '-' + uuid.uuid4().hex[:8]).lower()
        self.evidence = runs / self.token
        self.evidence.mkdir(mode=0o700)
        (self.evidence / 'cases').mkdir(mode=0o700)
        self.outcome = {'flow': args.flow, 'candidate': {k: v for k, v in self.sides['candidate'].items()}, 'baseline': self.sides.get('baseline'),
                        'selectedCases': self.cases, 'status': 'RUNNING', 'evidenceDirectory': str(self.evidence), 'started': time.time(),
                        'lockOwnerPid': owner, 'cgroup': cgroup_evidence(), 'answers': self.answers, 'windowsLocal': args.windows_local,
                        'phases': [], 'cleanup': []}
        self.matrix = initial(self.binding, args.flow, self.cases, self.answers)
        for case in FLOWS[args.flow]:
            if case not in self.cases:
                self.matrix['cases'][case]['reason'] = 'Not selected for this run'
        self.coordinator = Coordinator(self.outcome, self.save)
        self.ownership = None
        self.before = {'wsl': None, 'windows': None}
        self.scripts, self.chains, self.mcp_clients, self.closed_mcp = {}, {}, [], []
        self.counter = iter(range(1, 900)).__next__
        self.stage_dir = None
        self.installed = self.launched = False
        self.legacy_prefixes = []
        self.local = None
        self.phase_started = time.time()
        # 1.0.1 (D5): set once the update flow recorded update-begin; A7's planted set or its plant error.
        self.update_begun, self.a7 = False, None

    # --- evidence ----------------------------------------------------------------------------------
    def save(self):
        dump(self.evidence / 'driver-result.json', self.outcome)
        dump(self.evidence / 'acceptance.json', self.matrix)

    def observed(self, name, operation, needs=()):
        def act():
            self.phase_started = time.time()
            value = operation()
            dump(self.evidence / (name + '.json'), value)
            return value
        return self.coordinator.phase(name, act, needs)

    def case_result(self, case, status, files, reason=None, partial=None, failure=None, **details):
        if self.flow_name in WSL_FLOWS:
            details.setdefault('wslDifferences', 'driver-result.json#wslDifferences')
        try:
            record(self.matrix, case, status, files, reason, failure=failure, partial=partial, **details)
        except ValueError as error:
            # Only a PASS can be refused: the evidence does not support it, which is not a product failure.
            record(self.matrix, case, 'NOT_RUN', files, 'Evidence gate refused the %s (harness): %s' % (status, error))
        self.save()

    def pending(self, case):
        return case in self.cases and self.matrix['cases'][case]['status'] == 'NOT_RUN' and self.matrix['cases'][case]['reason'] == PENDING

    def journal_since(self, t0):
        """Owned daemon journal lines since t0, for the attribution counts only (never stored). Item 6: a failed read keeps
        its exit code and redacted stderr in the outcome (`journalErrors`, at most 20), since no lines means no attribution."""
        try:
            result = subprocess.run(['journalctl', '--user', '-u', 'devbox-accept@accept.service', '--since', '@%d' % int(t0), '-o', 'cat', '--no-pager',
                                     '-n', '2000'], capture_output=True, text=True, timeout=20,
                                    env={'XDG_RUNTIME_DIR': f'/run/user/{os.getuid()}', 'PATH': '/usr/bin:/bin'})
        except Exception as error:
            self.journal_failed(error_evidence(error))
            return []
        if result.returncode:
            self.journal_failed({'exit': result.returncode, 'stderr': redact_text(result.stderr, 400)})
        return result.stdout.splitlines()

    def journal_failed(self, row):
        errors = self.outcome.setdefault('journalErrors', [])
        if len(errors) < 20:
            errors.append(row)

    def guarded(self, cases, function):
        """Run one case or one group; an error ends only the cases still pending (classify()).

        The first pending case takes the error's status; the others are NOT_RUN "not reached" unless the
        error itself is a NOT_RUN kind (then they share its reason). A recorded FAIL is never overwritten.
        """
        selected = [c for c in cases if c in self.cases]
        if not selected:
            return
        t0 = time.time()
        try:
            function(set(selected))
        except Exception as error:
            status, reason, failure = classify(error, lambda: self.journal_since(t0))
            files = []
            if isinstance(error, wsl_daemon.StepFailed):
                name = 'cases/' + selected[0] + '-step.json'
                dump(self.evidence / name, error.row)
                files.append(name)
            elif isinstance(error, agents_live.StopPrompt):
                # Fix round 1 item 7 and round 2 item 1: the pattern, the matched lines with context, the last 15 lines.
                name = 'cases/' + selected[0] + '-stop.json'
                dump(self.evidence / name, agents_live.stop_evidence(error))
                files.append(name)
            elif status == 'FAIL':
                # Fix round 1 item 6: the error's bounded, redacted stderr (CommandFailed) or message, never raw text.
                name = 'cases/' + selected[0] + '-error.json'
                dump(self.evidence / name, error_evidence(error))
                files.append(name)
            first = True
            for case in selected:
                if not self.pending(case):
                    continue
                if first or status == 'NOT_RUN':
                    self.case_result(case, status, files, reason, failure=failure)
                else:
                    self.case_result(case, 'NOT_RUN', [], 'Not reached: an earlier case of this group ended (%s)' % reason[:200])
                first = False

    def run_case(self, case, function):
        self.guarded([case], lambda _selected: function())

    # --- owned instance ----------------------------------------------------------------------------
    def rpc(self, method, params=None):
        return wsl_owned.rpc_owned(self.ownership, method, params or {})

    def load(self):
        return json.loads(Path(self.ownership).read_text())

    def set_phase(self, side):
        row = self.sides[side]
        update_manifest(self.ownership, lambda m: m.update(phase=side, phaseSha=row['sha'], phaseVersion=row['version']))

    def remote_run(self, side):
        row = self.sides[side]
        try:
            run = json.loads(command([GH, 'api', f'repos/{REPO}/actions/runs/{row["runId"]}'], timeout=60).stdout)
        except Exception as error:
            raise Unavailable('Read-only GitHub run metadata unavailable; provenance unproven: ' + str(error)[:300])
        verify_run(run, row['sha'], row['runId'], row['branch'])
        return {k: run.get(k) for k in ('id', 'head_sha', 'head_branch', 'path', 'event', 'status', 'conclusion', 'html_url', 'created_at', 'updated_at')}

    def installer_gate(self):
        files = []
        for side, row in self.sides.items():
            chain = self.observed('artifact-chain-' + side, lambda row=row: verify_artifact_dir(row['dir'], row['sha'], row['runId'], row['version']))
            if artifact_digest(row['dir']) != row['artifactSha256']:
                raise ValueError('Artifact bytes changed after the approval gate: ' + side)
            chain['linux'] = dict(chain['linux'], path=str(Path(row['dir']) / 'devbox-linux' / 'devbox-linux'))
            self.chains[side] = chain
            self.observed('verified-workflow-' + side, lambda side=side: self.remote_run(side))
            files += ['artifact-chain-' + side + '.json', 'verified-workflow-' + side + '.json']
        self.matrix['gates']['installer'] = {'status': 'PASS', 'evidence': files}
        self.save()

    def stage_fixture(self, mode, proof):
        extra = {'flow': self.flow_name, 'answers': self.answers, 'baselineSha': (self.sides.get('baseline') or {}).get('sha'),
                 'linuxArtifacts': {side: chain['linux'] for side, chain in self.chains.items()}, 'phase': None, 'phaseSha': None, 'phaseVersion': None}
        self.ownership = Path(self.coordinator.phase('stage-fixture', lambda: stage(self.args.candidate_sha, self.evidence, mode, proof, root=self.root,
                                                                                    upgrade=self.flow_name in UPGRADE_FLOWS, extra=extra)))
        self.outcome['ownership'] = str(self.ownership)
        for side, row in self.sides.items():
            self.scripts[side] = wsl_daemon.scripts(self.args.worktree, row['sha'], self.binding[side]['files'][wsl_daemon.DEPLOY_CORE])
        dump(self.evidence / 'product-scripts.json', {side: value['sha256'] for side, value in self.scripts.items()})

    def instance(self, side, timeout=120):
        deadline = time.monotonic() + timeout
        while True:
            try:
                proof = wsl_owned.instance_proof(self.load())
                break
            except Exception as error:
                if time.monotonic() >= deadline:
                    raise RuntimeError('Owned accept instance not proven within %d s: %s' % (timeout, str(error)[:400]))
                time.sleep(2)
        dump(self.evidence / ('instance-proof-' + side + '.json'), proof)
        return proof

    def mcp(self, baseline=False):
        def record_process(row):
            update_manifest(self.ownership, lambda m: m.setdefault('mcpProcesses', []).append(row))
        sha = self.chains['baseline']['linux']['sha256'] if baseline else None
        client = mcp_stdio.McpClient(self.load, wsl_daemon.HOME, record_process, baseline_sha256=sha)
        self.mcp_clients.append(client)
        return client.start()

    def close_mcp(self):
        rows = []
        for client in self.mcp_clients:
            try:
                rows.append(client.close())
            except Exception as error:
                rows.append({'error': str(error)[:300]})
        dump(self.evidence / 'mcp-closed.json', {'blocks': self.closed_mcp, 'atEnd': rows})

    def live(self):
        m = self.load()
        native_flow = self.flow_name in WINDOWS_FLOWS
        pane = agents_live.Pane(self.load, self.answers, m['nonce'])
        return agents_live.Live(self.rpc, self.load, self.case_result, self.evidence, self.answers, m['nonce'], pane, None, counter=self.counter,
                                surface='installed-native' if native_flow else 'wsl-artifact', windows_half=None if native_flow else ev.reason('R-IOP½'))

    def with_mcp(self, function, baseline=False):
        """One owned MCP process per tool block (changes §8.1.3): SIGTERM, then SIGKILL, when the block ends.

        M10: a server that does not start ends the block's rows NOT_RUN with that reason (Unavailable), not FAIL."""
        try:
            client = self.mcp(baseline=baseline)
        except Exception as error:
            self.closed_mcp.append({'startError': error_evidence(error)})
            raise Unavailable('The owned MCP server did not start (%s); the block was not run' % redact_text(str(error), 300)) from None
        try:
            return function(client)
        finally:
            self.closed_mcp.append(client.close())

    def validate(self, parse):
        argv = ['python3', '-B', BASE / 'validate.py', '--worktree', self.args.worktree, '--source-sha', self.args.candidate_sha]
        if parse:
            argv.append('--powershell-parse')
        return json.loads(command(argv, timeout=240).stdout)

    # --- WSL-only flows (changes §7): no native(), no interop probe ------------------------------------
    def wsl_start(self, side, sampled=False):
        """The product's install script, then its Launch setup and startup doctor, for one phase."""
        self.observed('install-' + side, lambda: wsl_daemon.install_binary(self.load(), side, self.scripts[side]))
        self.set_phase(side)
        t0 = time.time()
        self.observed('setup-' + side, lambda: wsl_daemon.setup(self.load(), side, 'launch', self.scripts[side], journal=lambda: self.journal_since(t0)))
        sampler = wsl_daemon.Sampler() if sampled else None
        return self.observed('startup-doctor-' + side, lambda: wsl_daemon.startup_doctor(self.load(), side, self.scripts[side], sampler=sampler,
                                                                                         journal=lambda: self.journal_since(t0)))

    def wsl_flow(self):
        self.outcome['wslDifferences'] = WSL_DIFFERENCES
        self.outcome['powershellParse'] = 'NOT_RUN: WSL-only flow; the .ps1 files are not executed (changes §13)'
        self.outcome['windowsProd'] = 'NOT_RUN: ' + ev.reason('R-IOP')
        self.observed('validate', lambda: self.validate(parse=False))
        self.installer_gate()
        self.before['wsl'] = self.observed('prod-wsl-before', wsl_owned.fingerprint)
        self.observed('wsl-preflight', inspect_accept_absent)
        drvfs = self.observed('drvfs-preflight', lambda: wsl_daemon.drvfs_absent(self.args.windows_local))
        self.stage_fixture('wsl', drvfs)
        if self.flow_name == 'wsl-fresh':
            doctor = self.wsl_start('candidate', sampled=True)
            self.observed('seal-owned-resources', lambda: wsl_owned.seal(self.ownership, self.load()))
            self.instance('candidate')
            self.identity_gates('wsl_identity')
            self.wave_w(doctor)
        else:
            self.wsl_start('baseline')
            self.observed('seal-owned-resources', lambda: wsl_owned.seal(self.ownership, self.load()))
            self.instance('baseline')
            live = self.live()
            # The rc.1 MCP process starts after the tasks and stays open across the upgrade (closed in close_mcp).
            state = agents_live.u1_prepare(live, {}, lambda: self.mcp(baseline=True))
            self.wsl_start('candidate')
            self.observed('reseal-owned-resources', lambda: wsl_owned.reseal(self.ownership, self.load()))
            self.instance('candidate')
            self.identity_gates('wsl_identity', 'upgrade_identity')
            self.upgrade_rows(live, state, win=None)

    def identity_gates(self, *names):
        for name in names:
            self.matrix['gates'][name] = {'status': 'PASS', 'evidence': ['instance-proof-candidate.json']}
        # The answers are bound by the approval's binding part (answersSha256).
        self.matrix['gates']['trust_answer'] = {'status': 'PASS', 'evidence': ['review-approval-used.json']}
        self.save()

    def wave_w(self, doctor):
        """Matrix Wave W, steps 2-12 (changes §8.0 wsl-fresh). C1 runs last before cleanup; K1X-TMUXUP after it."""
        live = self.live()
        rows = daemon_rows.DaemonRows(self.rpc, self.load, self.case_result, self.evidence, self.flow_name, daemon=DaemonOps(self), pane=live.pane,
                                      counter=self.counter, worktree=self.args.worktree)
        self.run_case('CS4', lambda: rows.cs4(sampled=doctor))
        self.run_case('CS3', rows.cs3)
        self.run_case('SV1', rows.sv1)
        self.run_case('NT1', rows.nt1)
        for cases, block in ((OPENCODE, agents_live.opencode_block), (PI, agents_live.pi_block), (CLAUDE, agents_live.claude_block),
                             (CODEX, agents_live.codex_block)):
            chosen = {c for c in cases if c in self.cases}
            if not chosen:
                continue
            # SN2 is concluded once by record_sn2(); an error that escapes a block ends only its own pending rows.
            rows_of_block = [c for c in cases if c in chosen and c != 'SN2'] or ['SN2']
            self.guarded(rows_of_block, lambda _selected, block=block, chosen=chosen: self.with_mcp(lambda client: block(live, chosen, client)))
        if 'SN2' in self.cases:
            agents_live.record_sn2(live)
        for case, function in (('CL1', rows.cl1), ('CL2', rows.cl2), ('CL3', rows.cl3), ('C1', rows.c1)):
            self.run_case(case, function)
        self.run_case('K1X-TMUXUP', lambda: self.tmux_upgrade(live))

    def tmux_upgrade(self, live):
        """K1 추가(tmux 업그레이드), Linux half, only with Q3: the user changed the host tmux; the owned unit restarts."""
        key_half = ev.ROW_TEXT['K1X-TMUXUP.key']
        if self.answers.get('Q3') is not True:
            self.case_result('K1X-TMUXUP', 'NOT_RUN', [], ev.ROW_TEXT['K1X-TMUXUP.default'] + ' ' + key_half)
            return
        read = lambda: {'version': live.pane.display('#{version}'), 'extendedKeys': live.pane.show_extended_keys()}
        before = read()
        restart = wsl_owned.restart_tmux(self.load(), 'K1X-TMUXUP')
        after, deadline = {}, time.monotonic() + 30
        while time.monotonic() < deadline:
            try:
                after = read()
            except RuntimeError:  # the restarted server may not answer yet
                after = {}
            if tmux_decision(before, after)[0] == 'PASS':
                break
            time.sleep(1)
        status, reason = tmux_decision(before, after)
        dump(self.evidence / 'cases' / 'K1X-TMUXUP.json', {'before': before, 'restart': restart, 'after': after, 'linuxHalf': status})
        files = ['cases/K1X-TMUXUP.json']
        if status == 'PASS':
            self.case_result('K1X-TMUXUP', 'NOT_RUN', files, key_half, partial=['linux'])
        else:
            self.case_result('K1X-TMUXUP', status, files, reason + ' ' + key_half)

    def upgrade_rows(self, live, state, win):
        """U1 (a)-(d), U1 추가 and CS5 once the candidate phase has started (win: NativeOps or None)."""
        cs5 = {'CS5'} if 'CS5' in self.cases else set()
        self.guarded(('U1', 'U1X'), lambda selected: self.with_mcp(lambda client: agents_live.u1_rows(live, state, selected | cs5, client)))
        if cs5 and not ({'U1', 'U1X'} & set(self.cases)):
            # CS5 alone: its hook half (U1 (b)) was not run in this selection.
            state.setdefault('cs5HookHalf', None)
        rows = daemon_rows.DaemonRows(self.rpc, self.load, self.case_result, self.evidence, self.flow_name, daemon=DaemonOps(self), win=win,
                                      counter=self.counter)
        self.run_case('CS5', lambda: rows.cs5(state.get('cs5HookHalf')))
        for task in state.get('tasks', []):
            live.end('upgrade-cleanup', task)

    # --- Windows flows (changes §5, §6): Q0 and a passing interop probe first ----------------------------
    def windows(self, path):
        return command(['wslpath', '-w', str(path)]).stdout.strip()

    def native(self, script, timeout=200, **params):
        """Run a reviewed helper via -Command (native_command); the execution policy is never set, overridden or bypassed by flag."""
        args = [PS, '-NoProfile', '-NonInteractive', '-Command', native_command(self.windows, script, params)]
        return json.loads(command(args, timeout=timeout).stdout.lstrip('\ufeff'))

    def windows_flow(self):
        import a4_windows_service
        import w1_windows
        from common import probe_script
        if self.answers.get('Q0') is not True:
            raise Unavailable(ev.reason('R-IOP'))
        try:
            self.outcome['interopProbe'] = interop_probe()
        except Unavailable as error:
            self.outcome['interopProbe'] = {'status': 'FAIL', 'reason': str(error)[:300]}
            raise Unavailable(ev.reason('R-IOP'))
        self.save()
        self.observed('validate', lambda: self.validate(parse=True))
        for case, script in (('G1', 'g1_live_member.py'), ('G2', 'g2_probe_timing.py')):
            if case in self.cases:
                result = command(['python3', '-B', BASE / script, '--worktree', self.args.worktree, '--source-sha', self.args.candidate_sha, '--out', self.evidence,
                                  '--evidence-root', self.root],
                                 timeout=600, check=False)
                dump(self.evidence / (case.lower() + '-driver.json'), {'exit': result.returncode, 'stdout': result.stdout[-20000:],
                                                                         'stderr': redact_text(result.stderr, 4000)})
                summary = json.loads(result.stdout) if result.stdout.strip().startswith('{') else {'status': 'FAIL'}
                self.case_result(case, summary.get('status', 'FAIL'), [x for x in [case.lower() + '-driver.json', summary.get('evidence')] if x],
                                 summary.get('error'), verdicts=summary.get('verdicts'), i1Premise=summary.get('i1Premise'))
        if self.flow_name == 'fresh' and 'A1' not in self.cases:
            raise Unavailable('Only interop gates selected')
        self.before['wsl'] = self.observed('prod-wsl-before', wsl_owned.fingerprint)
        self.before['windows'] = self.observed('prod-windows-before', lambda: self.native('native-observe.ps1', Action='fingerprint'))
        self.installer_gate()
        self.local = command([PS, '-NoProfile', '-NonInteractive', '-Command', "[Environment]::GetFolderPath('LocalApplicationData')"]).stdout.strip()
        if command(['wslpath', '-u', self.local]).stdout.strip() != self.args.windows_local:
            raise ValueError('The bound windowsLocal differs from this Windows LocalAppData')
        # Out of v1 prod scope (FLAGGED for review): the legacy v0.x catalog dir's integration snapshots are
        # rewritten by the user's running legacy apps; reported as legacyExternalWriterChanged, never dropped.
        self.legacy_prefixes = [self.local + '\\Devbox\\integration']
        self.outcome['legacyExternalPrefixes'] = self.legacy_prefixes
        self.observed('wsl-preflight', inspect_accept_absent)
        pre = self.observed('native-preflight', lambda: self.native('native-observe.ps1', Action='preflight', SourceSha=self.args.candidate_sha,
                                                                    WindowsRoot='C:\\dev\\devbox-s7-accept-' + self.token,
                                                                    InstallPath=self.local + '\\DevboxAccept-s7-' + self.token))
        self.stage_fixture('windows', pre)
        self.observed('stage-installer', self.stage_installers, needs=('stage-fixture',))
        script, _ = probe_script(self.args.worktree, self.args.candidate_sha)
        self.probe_marker = probe_marker(script)
        if self.flow_name == 'fresh':
            self.fresh_flow(a4_windows_service, w1_windows)
        elif self.flow_name == 'update':
            import update_flow
            update_flow.run(self)
        else:
            self.upgrade_flow()

    def stage_installers(self):
        windows_stage = self.local + '\\DevboxAccept-s7-stage-' + self.token
        target = Path(command(['wslpath', '-u', windows_stage]).stdout.strip())
        self.stage_dir, self.windows_stage = target, windows_stage
        self.outcome['stageDirectory'] = str(target)
        self.save()
        target.mkdir(mode=0o700)
        m = self.load()
        # Same owned token as the fixture manifest: native-lifecycle Check-Owner compares the stage root marker.
        (target / MARKER).write_text(json.dumps({'token': m['token'], 'sourceSha': m['sourceSha']}))
        staged = {}
        for side in self.sides:
            sub = target / side
            sub.mkdir(mode=0o700)
            source = Path(self.chains[side]['installerPath'])
            shutil.copyfile(self.chains[side]['receiptPath'], sub / 'receipt.json')
            shutil.copyfile(source, sub / source.name)
            staged[side] = {'stage': str(sub), 'windowsStage': windows_stage + '\\' + side, 'installer': source.name}
        return staged

    def install(self, side):
        """A1 rules (native-lifecycle install) for one side; the prod comparison around it must PASS."""
        self.installed = True
        ownership = self.windows(self.ownership)
        self.observed('native-install-' + side, lambda: self.native('native-lifecycle.ps1', timeout=300, Action='install', Ownership=ownership,
                                                                    Stage=self.windows_stage + '\\' + side, Distro=self.args.distro), needs=('stage-installer',))
        identity = self.observed('installed-identity-' + side, lambda: self.native('native-lifecycle.ps1', Action='installed-identity', Ownership=ownership,
                                                                                   LinuxSha256=self.chains[side]['linux']['sha256']))
        return identity, self.prod_compare('after-install-' + side)

    def prod_compare(self, name):
        after = {'wsl': wsl_owned.fingerprint(), 'windows': self.native('native-observe.ps1', Action='fingerprint')}
        dump(self.evidence / ('prod-' + name + '.json'), after)
        comparison = compare_prod(self.before, after, self.legacy_prefixes)
        if not comparison['prodUnchanged']:
            raise RuntimeError('Prod fingerprint changed (' + name + '); stopping before the next mutation')
        return comparison

    def launch_and_prove(self, side, reseal=False, **launch_params):
        """`launch_params` (1.0.1, the update flow): CdpPort and UpdateFixture of native-lifecycle's launch action."""
        def launch():
            self.launched = True
            value = self.native('native-lifecycle.ps1', Action='launch', Ownership=self.windows(self.ownership), **launch_params)
            value['stderrWsl'] = command(['wslpath', '-u', value['stderr']]).stdout.strip()
            return value
        launch_row = self.observed('native-launch-' + side, launch)
        self.set_phase(side)
        # S8-5 (Ruling 147): prove the instance first: the daemon answering means the app's setup wrote the units.
        # Sealing right after the launch missed the unit files written 3 s later (run 70b1b4f6-e228bf7f).
        proof = self.instance(side)
        self.seal_after_launch(reseal)
        sc2 = wait_sc2(Path(launch_row['stderrWsl']))
        dump(self.evidence / ('sc2-' + side + '.json'), sc2)
        return proof, sc2

    def seal_after_launch(self, reseal):
        """Seal (or reseal) once the started app has deployed and run its setup; retried for at most 120 s."""
        deadline = time.monotonic() + 120
        while True:  # the app deploys and runs setup first
            try:
                if reseal:
                    self.observed('reseal-owned-resources', lambda: wsl_owned.reseal(self.ownership, self.load()))
                else:
                    self.observed('seal-owned-resources', lambda: wsl_owned.seal(self.ownership, self.load()))
                break
            except Exception:
                if time.monotonic() >= deadline:
                    raise
                time.sleep(2)

    def fresh_flow(self, a4_windows_service, w1_windows):
        identity, comparison = self.install('candidate')
        self.case_result('A1', 'PASS', ['artifact-chain-candidate.json', 'verified-workflow-candidate.json', 'native-install-candidate.json',
                                        'installed-identity-candidate.json', 'prod-after-install-candidate.json'], surface='installed-native',
                         installerSha256=self.chains['candidate']['installerSha256'], bundledLinuxSha256=identity['bundledLinuxSha256'], prodComparison=comparison)
        if 'A2' not in self.cases:
            return
        proof, sc2 = self.launch_and_prove('candidate')
        self.matrix['gates']['native_identity'] = {'status': 'PASS', 'evidence': ['native-launch-candidate.json', 'installed-identity-candidate.json']}
        self.matrix['gates']['fixture'] = {'status': 'PASS', 'evidence': ['instance-proof-candidate.json']}
        self.matrix['gates']['trust_answer'] = {'status': 'PASS', 'evidence': ['review-approval-used.json']}
        first = sc2['firstDataPainted']
        self.case_result('A2', 'PASS' if first else 'FAIL', ['native-launch-candidate.json', 'instance-proof-candidate.json', 'sc2-candidate.json'],
                         None if first else 'No first-data-painted line within 60 s of an owned launch', surface='installed-native',
                         measurement={'firstDataPainted': first, 'startPhases': sc2['startPhases'],
                                      'condition': 'cold app process, WSL running, accept daemon first start after setup'})
        self.sc2 = ('1.0.0', first)  # S8-5 (Ruling 146): the fresh wave's candidate is 1.0.0, not rc.2
        win = NativeOps(self)
        # 1.0.1 (#678): A8 needs the window as A2 left it (shown, first data painted) and leaves it so for the rows after it.
        import a8_window
        self.run_case('A8', lambda: a8_window.run(self, win, wsl_owned.unit_state))
        w1 = w1_windows.W1(self.rpc, self.load, self.case_result, self.evidence, win, self.answers, self.probe_marker, self.commit_p_win)
        # W1.run's execution order, so an error lands on the row that was running.
        self.guarded(('W1-D', 'W1-AB', 'W1-G', 'W1-E', 'W1-F', 'W1-H'), w1.run)
        self.run_case('A4', lambda: self.a4(a4_windows_service))
        rows = daemon_rows.DaemonRows(self.rpc, self.load, self.case_result, self.evidence, self.flow_name, daemon=DaemonOps(self), win=win,
                                      counter=self.counter)
        show = lambda spec: git(self.args.worktree, 'show', spec)
        self.run_case('N1-ACCEPT', lambda: rows.n1_accept(self.args.candidate_sha, show))
        self.run_case('CS3', rows.cs3)
        self.run_case('CS10', rows.cs10)
        self.run_case('C1', rows.c1)

    def a4(self, a4_windows_service):
        owned = self.load()
        row = a4_windows_service.run(owned, lambda: merge_manifest(self.ownership, owned), self.rpc, self.evidence,
                                     lambda name, value: dump(self.evidence / (name + '.json'), value))
        dump(self.evidence / 'a4-windows-service.json', row)
        self.case_result('A4', row['status'], ['a4-windows-service.json'], row.get('reason'), surface='installed-native',
                         measurement={k: row.get(k) for k in ('timeline', 'readyAfterGoSeconds', 'port')})

    def commit_p_win(self, text):
        """W1 (마): the one owned P-WIN edit (fixture.W1_EDITED_SERVICE), committed in the owned project."""
        project = Path(self.load()['projectPaths']['P-WIN'])
        (project / '.devbox/devbox.toml').write_text(text)
        daemon_rows.git_owned(project, 'add', '.devbox/devbox.toml')
        daemon_rows.git_owned(project, 'commit', '-qm', 's7 w1 edited service')

    def upgrade_flow(self):
        """Changes §6: the rc.1 installer, the rc.1 app's first start, U1 preparation, then the rc.2 installer over it."""
        _, comparison = self.install('baseline')
        proof, sc2 = self.launch_and_prove('baseline')
        self.sc2 = ('rc.1', sc2['firstDataPainted'])
        self.matrix['gates']['native_identity'] = {'status': 'PASS', 'evidence': ['native-launch-baseline.json', 'installed-identity-baseline.json']}
        self.matrix['gates']['fixture'] = {'status': 'PASS', 'evidence': ['instance-proof-baseline.json']}
        live = self.live()
        state = agents_live.u1_prepare(live, {}, lambda: self.mcp(baseline=True))
        ownership = self.windows(self.ownership)
        self.observed('stop-rc1-app', lambda: self.native('native-lifecycle.ps1', Action='stop-owned', Ownership=ownership))
        self.observed('assert-rc1-stopped', lambda: self.native('native-lifecycle.ps1', Action='assert-stopped', Ownership=ownership))
        self.observed('native-upgrade', lambda: self.native('native-lifecycle.ps1', timeout=300, Action='upgrade', Ownership=ownership,
                                                            Stage=self.windows_stage + '\\candidate'), needs=('assert-rc1-stopped',))
        self.observed('installed-identity-candidate', lambda: self.native('native-lifecycle.ps1', Action='installed-identity', Ownership=ownership,
                                                                          LinuxSha256=self.chains['candidate']['linux']['sha256']))
        self.prod_compare('after-upgrade')
        self.launch_and_prove('candidate', reseal=True)
        self.matrix['gates']['upgrade_identity'] = {'status': 'PASS', 'evidence': ['native-upgrade.json', 'instance-proof-candidate.json']}
        self.matrix['gates']['trust_answer'] = {'status': 'PASS', 'evidence': ['review-approval-used.json']}
        self.save()
        self.upgrade_rows(live, state, win=NativeOps(self))

    # --- the run -----------------------------------------------------------------------------------
    def run(self):
        delegation_fd, _ = delegate_windows(self.evidence, self.args.candidate_sha, self.root)
        try:
            dump(self.evidence / 'review-approval-used.json', self.ctx['review'])
            if self.flow_name in WSL_FLOWS:
                self.wsl_flow()
            else:
                self.windows_flow()
            self.outcome['status'] = 'EXECUTED'
        except Unavailable as error:
            self.outcome['status'] = 'NOT_RUN'
            self.outcome['reason'] = str(error)
            for case in self.cases:
                if self.pending(case):
                    self.matrix['cases'][case]['reason'] = str(error)[:1000]
        except Exception as error:
            failure = getattr(error, 'failure', None) or ev.failure(self.outcome['phases'][-1]['name'] if self.outcome['phases'] else 'driver', error,
                                                                    stderr=getattr(error, 'stderr', None), journal=self.journal_since(self.phase_started),
                                                                    timeout=timed_out(error))
            attributed = ev.attribute(self.matrix, None, failure)
            self.outcome['reason'] = str(error)[:2000]
            self.outcome['errorType'] = type(error).__name__
            if attributed:
                # Changes §1.6: an environment stall under the recorded broken interop is NOT_RUN, never FAIL.
                self.outcome['status'] = 'NOT_RUN'
                self.outcome['originalFailure'] = attributed
                for case in self.cases:
                    if self.pending(case):
                        self.matrix['cases'][case].update(reason=ev.reason('R-IOP-STALL'), originalFailure=attributed)
            else:
                self.outcome['status'] = 'FAIL'
                for case in self.cases:
                    if self.pending(case):
                        self.matrix['cases'][case]['reason'] = 'Not reached: an earlier phase failed (%s)' % (self.outcome['phases'][-1]['name']
                                                                                                          if self.outcome['phases'] else 'driver')
        finally:
            try:
                self.close_mcp()
            except Exception as error:
                self.outcome['mcpCloseError'] = str(error)[:300]
            self.teardown()
            self.outcome['cases'] = self.matrix['cases']
            if any(row['status'] == 'FAIL' for row in self.matrix['cases'].values()):
                self.outcome['status'] = 'FAIL'
            self.save()
            os.close(delegation_fd)
        print(json.dumps({'status': self.outcome['status'], 'evidenceDirectory': str(self.evidence), 'cases': self.matrix['cases'],
                          'prodComparison': self.outcome.get('prodComparison'), 'retainedResources': self.outcome.get('retainedResources')},
                         indent=2, ensure_ascii=False))
        return 1 if self.outcome['status'] == 'FAIL' else 0

    def seal_before_cleanup(self):
        """Seal (or, after an upgrade's candidate phase, reseal) what the run created, so cleanup removes it by the rule."""
        attempted = {row['name'] for row in self.outcome['phases']}
        completed = self.coordinator.completed
        if not self.ownership or not any(n.startswith(('install-', 'setup-', 'native-install-', 'native-launch-', 'native-upgrade', 'native-update'))
                                         for n in attempted):
            return
        if 'seal-owned-resources' not in completed:
            name, action = 'seal-owned-resources', wsl_owned.seal
        elif (self.flow_name in UPGRADE_FLOWS and {'install-candidate', 'native-upgrade', 'native-update-begin', 'native-update'} & attempted
              and 'reseal-owned-resources' not in completed):
            name, action = 'reseal-owned-resources', wsl_owned.reseal
        else:
            return
        try:
            self.observed(name, lambda: action(self.ownership, self.load()))
        except Exception as error:
            self.outcome['sealFailure'] = str(error)[:500]

    def teardown(self):
        if self.ownership is None:
            found = list(self.evidence.glob('ownership-owned-*.json'))
            self.ownership = found[0] if len(found) == 1 else None
        native_flow = self.flow_name in WINDOWS_FLOWS
        self.seal_before_cleanup()
        if self.before['wsl'] is None:
            return  # nothing was staged: no prod fingerprint was taken before the first mutation
        m = self.load() if self.ownership else None
        wsl_resources = [*m['absentBefore'], *(m.get('absentParentsBefore') or []), m['fixtureRoot']] if m else []
        operations = {'wsl_cleanup': lambda: wsl_owned.cleanup(self.ownership, self.load()), 'fingerprint_wsl': wsl_owned.fingerprint}
        if m:
            operations['absent_after'] = lambda: self.absent_after(native_flow)
        windows_resources = []
        if native_flow:
            operations['worktree_orphans'] = lambda: worktree_orphans(self.worktree_stat, self.start_tick)
            windows_resources = self.native_teardown(operations, m)
        context = {'ownership': str(self.ownership) if self.ownership else None, 'launched': self.launched and bool(self.ownership),
                   'installed': self.installed and bool(self.ownership), 'staged': self.stage_dir is not None,
                   'stageDirectory': str(self.stage_dir) if self.stage_dir is not None else None,
                   'retainedResources': [*wsl_resources, *windows_resources], 'wslResources': wsl_resources, 'windowsResources': windows_resources,
                   'legacyExternalPrefixes': self.legacy_prefixes, 'sides': ('wsl', 'windows') if native_flow else ('wsl',)}
        finalize(self.outcome, context, self.before, operations, lambda name, value: dump(self.evidence / (name + '.json'), value))
        self.after_teardown(native_flow)

    def native_teardown(self, operations, m):
        """Windows flows only: quit, the stopped proof, the uninstall (§5 poll), the Windows fingerprint, the stage."""
        ownership = lambda: self.windows(self.ownership)

        def quit_owned():
            return quit_after_update(lambda timeout=200, **params: self.native('native-lifecycle.ps1', timeout=timeout, Ownership=ownership(), **params),
                                     self.update_begun, lambda rows: dump(self.evidence / 'teardown-update-adopt.json', rows))

        def uninstall_owned():
            # 1.0.1 (D5, A7): with A7 selected, plant the `uninstall` foreign file set first. native-lifecycle reads it back
            # between the uninstaller's exit and the owned tree removal, and removes exactly that set.
            params = {}
            if 'A7' in self.cases and m:
                try:
                    import w1_windows
                    planted = foreign_files.plant(w1_windows.drvfs(m['nativePreflight']['installPath']), m['token'], 'uninstall', m['sourceSha'])
                    dump(self.evidence / 'foreign-uninstall-planted.json', planted)
                    self.a7 = {'planted': planted}
                    params['Foreign'] = self.windows(self.evidence / 'foreign-uninstall-planted.json')
                except Exception as error:
                    self.a7 = {'plantError': redact_text(str(error), 300)}
            return self.native('native-lifecycle.ps1', timeout=300, Action='uninstall', Ownership=ownership(), **params)

        operations.update({
            'quit': quit_owned,
            'assert_stopped': lambda: self.native('native-lifecycle.ps1', Action='assert-stopped', Ownership=ownership()),
            'windows_cleanup': uninstall_owned,
            'fingerprint_windows': lambda: self.native('native-observe.ps1', Action='fingerprint'),
        })
        if m:
            operations['stage_cleanup'] = lambda: cleanup_stage(self.stage_dir, m['token'], m['sourceSha'])
        return [m['nativePreflight'][k] for k in ['installPath', 'appLocalData', 'appRoamingData', 'windowsRoot']] if m else []

    def absent_after(self, native_flow):
        """C5 (changes §5): after every cleanup step, the WSL proof plus a fresh native preflight (Windows flows) or drvfs."""
        value = {'wsl': inspect_accept_absent()}
        if native_flow:
            value['native'] = self.native_absent()
        else:
            value['drvfs'] = wsl_daemon.drvfs_absent(self.args.windows_local)
        value['status'] = 'PASS' if not native_flow or (value['native'] or {}).get('acceptAbsent') is True else 'FAIL'
        return value

    def native_absent(self):
        fresh = (self.args.candidate_sha[:8] + '-' + uuid.uuid4().hex[:8]).lower()
        return self.native('native-observe.ps1', Action='preflight', SourceSha=self.args.candidate_sha,
                           WindowsRoot='C:\\dev\\devbox-s7-accept-' + fresh, InstallPath=self.local + '\\DevboxAccept-s7-' + fresh)

    def a7_result(self, out):
        """Pure reads only: teardown's `windows` row and the native record this run's helper wrote in the evidence folder."""
        recorded = None
        try:
            token = json.loads(Path(self.ownership).read_text())['token']
            recorded = json.loads((self.evidence / ('native-' + token + '.json')).read_text(encoding='utf-8-sig'))
        except Exception:
            recorded = None
        row = next((r for r in out.get('cleanup', []) if r.get('action') == 'windows'), None)
        status, reason, comparison = a7_decision(self.a7, row, recorded)
        dump(self.evidence / 'cases' / 'A7.json', {'case': 'A7', 'status': status, 'reason': reason, 'state': self.a7, 'comparison': comparison})
        self.case_result('A7', status, ['cases/A7.json', 'teardown-windows.json'], reason, surface='installed-native', comparison=comparison)

    def after_teardown(self, native_flow):
        out = self.outcome
        files = ['driver-result.json', 'prod-after.json']
        if 'A6' in self.cases:
            status, reason = a6_decision(out)
            self.case_result('A6', status, files, reason, surface='installed-native', retainedResources=out.get('retainedResources'),
                             prodComparison=out.get('prodComparison'), acceptAbsentAfter=out.get('acceptAbsentAfter'))
        if 'A7' in self.cases:
            self.a7_result(out)
        if 'M4' in self.cases:
            status, reason, partial = m4_decision(out, native_flow)
            self.case_result('M4', status, files, reason, partial=partial, surface='installed-native' if native_flow else 'wsl-artifact',
                             prodComparison=out.get('prodComparison'), legacyExternalWriterChanged=out.get('legacyExternalWriterChanged'))
        if 'R-SC2' in self.cases and getattr(self, 'sc2', None):
            label, first = self.sc2
            dump(self.evidence / 'r-sc2.json', {'wave': label, 'firstDataPainted': first})
            self.case_result('R-SC2', 'NOT_RUN', ['r-sc2.json'], 'Record only: this wave measured the %s cold value; the other wave records the other '
                             'value and the controller shows both side by side' % label, partial=[label])


QUIT_ROUNDS = 3
ADOPT_WAIT_SECONDS = 600  # native-lifecycle's own upper bound for update-adopt's wait
ADOPT_RETRY_SECONDS = 120


def quit_after_update(lifecycle, update_begun, save=lambda rows: None, rounds=QUIT_ROUNDS):
    """Teardown's quit (1.0.1). Without an update: the exact stop of the recorded processes, as before.

    Once an update began, each round first waits (at most ADOPT_WAIT_SECONDS, then ADOPT_RETRY_SECONDS, inside the helper) until the product's
    update runner and installer have ended, records the app the runner relaunched and pins the installed bytes
    (update-adopt), then stops every recorded process exactly (PID, executable path and start time). A relaunch can
    arrive after a stop, so update-observe is read again and the round repeats while a runner, an installer or an
    owned app process is present, at most `rounds` times. A failing adopt never skips the stop. The result is FAIL
    when a round's adopt or stop failed for good or a process is left: a debug-port app or a runner must never be left
    running unreported. `lifecycle(timeout=..., **params)` runs one native-lifecycle action."""
    if not update_begun:
        return lifecycle(Action='stop-owned')
    rows, left, error = [], None, None
    for number in range(1, rounds + 1):
        row = {'round': number}
        rows.append(row)
        error = None
        wait = ADOPT_WAIT_SECONDS if number == 1 else ADOPT_RETRY_SECONDS
        for name, call in (('adopt', lambda: lifecycle(timeout=wait + 80, Action='update-adopt', WaitSeconds=wait)),
                           ('stop', lambda: lifecycle(Action='stop-owned'))):
            try:
                row[name] = call()
            except Exception as failure:
                row[name + 'Error'] = redact_text(str(failure), 600)
                error = error or row[name + 'Error']
        try:
            seen = lifecycle(Action='update-observe')
            left = {key: seen.get(key) or [] for key in ('oldAlive', 'newApps', 'staged')}
        except Exception as failure:
            row['observeError'] = redact_text(str(failure), 600)
            error, left = error or row['observeError'], None
        row['left'] = left
        save(rows)
        if left is not None and not any(left.values()) and error is None:
            return {'status': 'PASS', 'stopped': True, 'rounds': rows}
    save(rows)
    raise RuntimeError('After %d rounds of update-adopt and stop-owned an owned process may still run (%s); retained'
                       % (rounds, error or 'left: ' + json.dumps(left)))


def a7_decision(state, cleanup_row, recorded):
    """A7 (1.0.1): the `uninstall` foreign file set after the product's uninstaller, before the owned tree removal.
    `state` is the planted set or its plant error, `cleanup_row` teardown's `windows` row, and `recorded` native-lifecycle's
    own record (used when the action failed after the read). Returns (status, reason, comparison)."""
    if not state:
        return 'NOT_RUN', 'The owned uninstall did not run, so no foreign file was planted', None
    if 'plantError' in state:
        return 'FAIL', ev.ROW_TEXT['A7.noplant'] + ' (' + state['plantError'] + ')', None
    result = (cleanup_row or {}).get('result')
    if not isinstance(result, dict) or result.get('foreignAfterUninstall') is None:
        result = recorded
    if not isinstance(result, dict) or result.get('foreignAfterUninstall') is None:
        return 'FAIL', 'The uninstall ended before the planted files were read back; they are retained with the owned install', None
    comparison = foreign_files.compare(state['planted'], result['foreignAfterUninstall'])
    # The read counts only after the uninstaller's own end (uninstall.exe and its Uninstall registration gone), and
    # only when two reads 1 s apart agree: a read during its file work could pass on files it was still going to remove.
    comparison.update(uninstallerEnded=result.get('foreignUninstallerEnded'), stable=result.get('foreignStable'), waitMs=result.get('foreignWaitMs'))
    if result.get('foreignUninstallerEnded') is not True:
        return 'FAIL', 'The uninstaller had not finished within 30 s of its exit when the planted files were read; the read proves nothing', comparison
    if result.get('foreignStable') is not True:
        return 'FAIL', 'Two reads of the planted files 1 s apart differ: the install folder was still changing', comparison
    status, reason = foreign_files.decision(comparison)
    return status, reason, comparison


def main(argv=None):
    args = parse(argv)
    ctx = gates(args)  # fails closed before any host action
    verify_source(args.worktree, args.candidate_sha, args.baseline_sha)  # read-only git show; re-checked under the lock
    os.chdir(args.worktree)
    owner = require_lock()
    head = command(['git', 'rev-parse', 'HEAD']).stdout.strip()
    dirty = command(['git', 'status', '--porcelain']).stdout.strip()
    # S8-5 (Ruling 146): the candidate is main, which the original checkout holds, so the check worktree is
    # detached at the candidate SHA. The run's own branch is still checked (verify_run: head_branch == 'main').
    if head != args.candidate_sha or dirty:
        raise ValueError('Clean exact reviewed SHA required')
    verify_source(args.worktree, args.candidate_sha, args.baseline_sha)
    if gates(args)['approval'] != ctx['approval']:
        raise ValueError('Run inputs changed between the gates and the lock')
    root = evidence_root(args.evidence_root, create=True)
    return Session(args, ctx, owner, root).run()


if __name__ == '__main__':
    sys.exit(main())
