"""1.0.1 (D5, gate G1): the accept app's own update from the installed baseline to the candidate installer (flow `update`).

What the product allows (read from the bound source; README section 7 has the file:line list):
- An accept build reads DEVBOX_UPDATE_FIXTURE once at start. The folder holds owner.json, metadata.json and installer.exe.
  owner.json names the source SHA of the *running* build; nothing ties the installer to a source SHA, only its size and
  sha256 (through metadata.json and owner.json).
- After the installer ran, the accept runner starts the app again only when the installed devbox-version.txt equals the
  runner's own version (it is a copy of the old executable). So the baseline and the candidate must have the same
  version: the shape `same-version`. The gates refuse any other pair unless the run names `next-version`, which only a
  product build that compares against the release version could pass.

The update is driven with the commands the Settings screen calls, in its order (update_status, update_check,
update_download, update_install_summary, update_install_confirm), through native-cdp.ps1. update_check is sent only
while the phase is idle or failed: the app checks by itself at start, so the release is often already `available`
(run ceda4811-e2f2d5b7 never sent it). No window gets input: the confirm command starts the product's own quit round,
and every document window's own code answers it.

Each native call is one PowerShell start through WSL interop, so the watch of the product's runner sees the Windows
side only every 4 to 6 s. Its times (`oldExitedAfter`, `newAppAfter`) are those of the first poll that saw the event:
upper bounds. The connection gap is measured apart from that, inside WSL, every 100 ms.

This module holds the connection gap, the command order, the decisions and the run itself; update_fixture.py holds
the fixture and the shape. Windows work goes through the session's reviewed helpers only.
"""
import os
from pathlib import Path
import threading
import time

from common import dump, redact_text
import foreign_files
from update_fixture import build_fixture, check_shape, debug_port

PHASES = ('idle', 'checking', 'available', 'downloading', 'ready', 'installing', 'failed')
# Seconds. Each native call is one PowerShell start (measured 4 to 6 s apart in run ceda4811-e2f2d5b7).
TIMEOUTS = {'check': 60, 'download': 300, 'exit': 120, 'install': 600, 'adoptWait': 60, 'reply': 20, 'newApp': 90}
CASE = 'UP1'
PROJECT = 'P-U1'
TMUX_IDENTITY = '#{session_created} #{pane_pid} #{pid}'
BRIDGE_INTERVAL = 0.1


class UpdateFailed(RuntimeError):
    """The app's update did not go the way the Settings screen's own path goes; `detail` is kept as evidence."""

    def __init__(self, message, detail=None):
        super().__init__(message)
        self.detail = detail


# --- the connection gap, read inside WSL ----------------------------------------------------------------
def accept_bridges(proc=Path('/proc'), uid=None):
    """{(pid, start time)} of this user's accept bridge processes: argv [<...>/devbox, 'bridge'] with DEVBOX_INSTANCE=accept
    in its environment (app/src-tauri/src/platform/supervisor.rs:159-169 and core/deploy.rs:430-433). Read-only."""
    uid = os.getuid() if uid is None else uid
    found = set()
    for entry in proc.iterdir():
        if not entry.name.isdigit():
            continue
        try:
            if entry.stat().st_uid != uid:
                continue
            argv = (entry / 'cmdline').read_bytes().split(b'\0')
            if len(argv) < 2 or argv[1] != b'bridge' or argv[0].rsplit(b'/', 1)[-1] != b'devbox' or any(argv[2:]):
                continue
            if b'DEVBOX_INSTANCE=accept' not in (entry / 'environ').read_bytes().split(b'\0'):
                continue
            start = (entry / 'stat').read_text().rsplit(')', 1)[1].split()[19]
        except (OSError, IndexError):
            continue
        found.add((int(entry.name), start))
    return found


class BridgeSampler:
    """Every 100 ms, the accept bridge processes (accept_bridges). Only the samples where the set changed are kept."""

    def __init__(self, read=accept_bridges, clock=time, interval=BRIDGE_INTERVAL):
        self.read, self.clock, self.interval = read, clock, interval
        self.changes, self.loops, self.max_step, self.errors = [], 0, 0.0, 0
        self.stop = threading.Event()
        self.thread = None

    def sample(self, last=None):
        now = self.clock.monotonic()
        try:
            found = self.read()
        except Exception:
            self.errors += 1
            return last
        if last is not None:
            self.max_step = max(self.max_step, now - last[0])
        if last is None or found != last[1]:
            self.changes.append((now, found))
        self.loops += 1
        return now, found

    def _loop(self):
        last = None
        while not self.stop.is_set():
            last = self.sample(last)
            self.stop.wait(self.interval)

    def start(self):
        self.thread = threading.Thread(target=self._loop, daemon=True)
        self.thread.start()
        return self

    def result(self):
        self.stop.set()
        if self.thread is not None:
            self.thread.join(timeout=5)
        return {'loops': self.loops, 'errors': self.errors, 'maxStepSeconds': round(self.max_step, 3), 'intervalSeconds': self.interval,
                **connection_gap(self.changes)}


def connection_gap(changes):
    """From the change list [(t, {(pid, start)})]: the bridges of the first sample are the old app's. The gap runs from the
    first sample without any of them to the first later sample with a bridge that is not one of them. None when the old
    bridge never went or no new one came; the sampling step bounds the error on each side."""
    if not changes:
        return {'gapSeconds': None, 'why': 'no sample'}
    old = set(changes[0][1])
    if not old:
        return {'gapSeconds': None, 'why': 'no accept bridge was connected at the start', 'changes': len(changes)}
    gone = next((t for t, found in changes if not found & old), None)
    if gone is None:
        return {'gapSeconds': None, 'why': 'the old bridge never went', 'oldBridges': len(old), 'changes': len(changes)}
    back = next((t for t, found in changes if t >= gone and found - old), None)
    if back is None:
        return {'gapSeconds': None, 'why': 'no new bridge connected', 'oldBridges': len(old), 'changes': len(changes)}
    return {'gapSeconds': round(back - gone, 3), 'oldBridges': len(old), 'changes': len(changes)}


def wsl_witness(proc=Path('/proc')):
    """What stays the same only while this WSL instance keeps running: the kernel boot id and the start of PID 1."""
    return {'bootId': (proc / 'sys/kernel/random/boot_id').read_text().strip(), 'uptimeSeconds': float((proc / 'uptime').read_text().split()[0]),
            'pid1Start': (proc / '1/stat').read_text().rsplit(')', 1)[1].split()[19]}


def daemon_replaced(before, after):
    """Whether the owned daemon after the update is another process than before it: True when the PID or the systemd
    InvocationID of the two instance proofs differ, False when both are the same, None when a proof lacks them.

    The app deploys and restarts the daemon only when its bundled Linux binary differs from the installed one. With two
    builds that bundle the same binary the daemon is not touched, and the run proves the app's restart only."""
    keys = ('runningPid', 'invocationId')
    if not all(isinstance(proof, dict) and proof.get(key) for proof in (before, after) for key in keys):
        return None
    return any(before[key] != after[key] for key in keys)


SAME_DAEMON = ('The daemon was not replaced (same PID and InvocationID before and after): both builds bundle the same Linux binary, so this run '
               'proves the app restart only, not a daemon swap.')


def distro_stayed_up(before, after):
    return (bool(before.get('bootId')) and before.get('bootId') == after.get('bootId') and before.get('pid1Start') == after.get('pid1Start')
            and after.get('uptimeSeconds', 0) > before.get('uptimeSeconds', 0))


# --- the app's own commands ------------------------------------------------------------------------------
def answered(reply, command):
    """The value a command answered in the main window. A rejected command raises with its code. The window must name
    itself `main`: an answer without a window label is not accepted."""
    if not isinstance(reply, dict) or reply.get('ok') is not True:
        raise UpdateFailed('%s was rejected: %s' % (command, redact_text(str((reply or {}).get('error')), 200)), reply)
    if reply.get('label') != 'main':
        raise UpdateFailed('%s was not answered by the main window (%s)' % (command, reply.get('label')), reply)
    return reply.get('value')


def snapshot_of(reply, command):
    """The snapshot a status, check, download or confirm command answered."""
    value = answered(reply, command)
    if not isinstance(value, dict) or value.get('phase') not in PHASES or type(value.get('generation')) is not int:
        raise UpdateFailed('%s answered no update snapshot' % command, reply)
    return value


def drive_update(call, target_version, clock=time, log=None):
    """The Settings screen's own order (Update.tsx:96-113, nativeUpdate.ts:202-226, UpdateConfirmation.tsx:97, 124-128):
    status, [새 버전 확인] update_check, [업데이트 다운로드] update_download, [업데이트] update_install_summary, [설치하고 다시 시작]
    update_install_confirm. The check is sent only while the phase is idle or failed; a release the app's own startup
    check already found is downloaded as it is. `call(command, **integers)` returns the page's reply. Returns the step
    list; raises UpdateFailed."""
    steps = log if log is not None else []

    def step(command, **integers):
        started = clock.monotonic()
        reply = call(command, **integers)
        steps.append({'command': command, 'arguments': integers, 't': round(started, 3), 'reply': reply})
        return reply

    def wait(wanted, bound, what):
        deadline = clock.monotonic() + bound
        while True:
            snapshot = snapshot_of(step('update_status'), 'update_status')
            if snapshot['phase'] in wanted:
                return snapshot
            if snapshot['phase'] == 'failed' or clock.monotonic() >= deadline:
                raise UpdateFailed('%s ended in phase %s (error %s)' % (what, snapshot['phase'], snapshot.get('error')), snapshot)
            clock.sleep(1)

    snapshot = snapshot_of(step('update_status'), 'update_status')
    if snapshot.get('enabled') is not True:
        raise UpdateFailed('The app did not take the update fixture (enabled false, %s)' % snapshot.get('disabledReason'), snapshot)
    if snapshot['phase'] in ('idle', 'failed'):
        snapshot_of(step('update_check'), 'update_check')
    snapshot = wait(('available', 'ready'), TIMEOUTS['check'], 'The check')
    if snapshot.get('availableVersion') != target_version:
        raise UpdateFailed('The app offers %s, not %s' % (snapshot.get('availableVersion'), target_version), snapshot)
    if snapshot['phase'] == 'available':
        snapshot_of(step('update_download', Generation=snapshot['generation']), 'update_download')
        snapshot = wait(('ready',), TIMEOUTS['download'], 'The download')
    if snapshot.get('nextAction') != 'install' or snapshot.get('availableVersion') != target_version:
        raise UpdateFailed('The download did not end ready to install', snapshot)
    reply = step('update_install_summary', Generation=snapshot['generation'])
    summary = answered(reply, 'update_install_summary')
    if (not isinstance(summary, dict) or summary.get('blocked') is not None or type(summary.get('confirmationId')) is not int
            or summary['confirmationId'] <= 0 or summary.get('version') != target_version or summary.get('generation') != snapshot['generation']):
        raise UpdateFailed('The install summary does not allow the install (%s)' % ((summary or {}).get('blocked') or 'no summary'), reply)
    installing = snapshot_of(step('update_install_confirm', Generation=snapshot['generation'], ConfirmationId=summary['confirmationId']),
                             'update_install_confirm')
    if installing['phase'] != 'installing':
        raise UpdateFailed('The confirm did not start the install (phase %s)' % installing['phase'], installing)
    return steps


def watch_update(observe, status, clock=time, bound=None, rows=None):
    """Poll update-observe (one PowerShell start each, 4 to 6 s apart; the summary's times are those of the first poll that
    saw the event) until the product's runner has finished and its relaunched app runs. While the old app still
    runs, `status()` (the app's own update_status, or None when it cannot be asked) shows a cancelled or refused install.
    Returns the summary; the rows (bounded) are kept as evidence."""
    rows = rows if rows is not None else []
    start = clock.monotonic()
    deadline = start + (TIMEOUTS['install'] if bound is None else bound)
    summary = {'oldExitedAfter': None, 'runnerSeen': False, 'installerSeen': False, 'keepAlive': [], 'outcomes': [], 'newAppAfter': None,
               'stageSeen': False, 'installed': None}
    while True:
        row = observe()
        now = round(clock.monotonic() - start, 3)
        if len(rows) < 600:
            rows.append({'t': now, **row})
        names = [str(item.get('name', '')).lower() for item in row.get('staged') or []]
        summary['runnerSeen'] |= 'runner.exe' in names
        summary['installerSeen'] |= 'installer.exe' in names
        for item in row.get('keepAlive') or []:
            if item not in summary['keepAlive'] and len(summary['keepAlive']) < 8:
                summary['keepAlive'].append(item)
        for item in row.get('outcomes') or []:
            label = '%s:%s' % (item.get('outcome'), item.get('error') or '')
            if item.get('outcome') and label not in summary['outcomes']:
                summary['outcomes'].append(label)
        if not row.get('oldAlive') and summary['oldExitedAfter'] is None:
            summary['oldExitedAfter'] = now
        # A failed, unknown or cancelled record ends the watch first, whatever else runs.
        failed = [label for label in summary['outcomes'] if label.split(':')[0] in ('failed', 'unknown', 'cancelled')]
        if failed and not row.get('staged'):
            raise UpdateFailed('The update runner recorded %s' % ', '.join(failed), {**summary, 'last': row})
        # Success needs the runner's own `installed` record. The relaunched app removes the stage folder when it finds
        # that record and no other (platform/update.rs:74-79, runner_recovery.rs:157-168, 237-308), so a folder that was
        # seen and is gone while that app runs is the same record, read by the product (`acknowledged`).
        summary['stageSeen'] |= bool(row.get('outcomes'))
        if 'installed:' in summary['outcomes']:
            summary['installed'] = 'recorded'
        elif summary['stageSeen'] and not row.get('outcomes') and row.get('newApps') and not failed:
            summary['installed'] = 'acknowledged'
        if summary['oldExitedAfter'] is not None and row.get('newApps') and not row.get('staged') and summary['installed']:
            summary['newAppAfter'] = now
            summary['last'] = row
            return summary
        if row.get('oldAlive'):
            # The app still runs: it is preparing, or it gave the install up (a refused quit round, a runner error).
            snapshot = status()
            if isinstance(snapshot, dict) and snapshot.get('phase') != 'installing':
                raise UpdateFailed('The app left the install (%s)' % describe(snapshot), {**summary, 'last': row, 'snapshot': snapshot})
            if now >= TIMEOUTS['exit']:
                raise UpdateFailed('The app did not exit for the install within %d s (%s)' % (TIMEOUTS['exit'], describe(snapshot)), {**summary, 'last': row})
        if clock.monotonic() >= deadline:
            raise UpdateFailed('The update did not finish within %d s' % (deadline - start), {**summary, 'last': row})
        clock.sleep(1)


def describe(snapshot):
    if not isinstance(snapshot, dict):
        return 'the app did not answer'
    return 'phase %s, error %s' % (snapshot.get('phase'), snapshot.get('error'))


# In the shape same-version both builds report the same version, so `version` cannot tell them apart there: it is
# recorded and must still hold, but it is listed apart from the parts that decide which build runs.
RECORD_ONLY = {'same-version': ('version',)}


def decide(parts, shape=None):
    """UP1: PASS only when every part is True. A None part was not observed, which is a FAIL here: the update ran.
    Which build runs afterwards is decided by `installerRan` and `identity`; both must be there and True."""
    failed = sorted(key for key, value in parts.items() if value is not True)
    missing = [key for key in ('installerRan', 'identity') if key not in parts]
    if failed or missing:
        return 'FAIL', 'Failed or unobserved parts: ' + ', '.join(failed + missing)
    return 'PASS', None


def deciding(parts, shape):
    """The part names that discriminate in this shape (evidence only; decide() needs every part)."""
    return sorted(key for key in parts if key not in RECORD_ONLY.get(shape, ()))


# --- the run ------------------------------------------------------------------------------------------------
class UpdateRun:
    """The `update` flow after the gates: baseline install, fixture, launch with the debug port, the update, the checks."""

    def __init__(self, session, clock=time, sampler=BridgeSampler, witness=wsl_witness):
        self.s, self.clock, self.sampler, self.witness = session, clock, sampler, witness
        self.data = {}

    def native(self, script, timeout=200, **params):
        return self.s.native(script, timeout=timeout, Ownership=self.s.windows(self.s.ownership), **params)

    def cdp(self, command, **integers):
        value = self.native('native-cdp.ps1', timeout=TIMEOUTS['reply'] + 40, Action='invoke', Port=self.port, Command=command,
                            TimeoutMs=TIMEOUTS['reply'] * 1000, **integers)
        return value.get('reply')

    def status_or_none(self):
        try:
            return snapshot_of(self.cdp('update_status'), 'update_status')
        except Exception:
            return None

    def save(self, name, value):
        dump(self.s.evidence / (name + '.json'), value)
        return name + '.json'

    def prepare(self):
        """The baseline install, the fixture folder in the owned stage, and the baseline app with the port and the fixture."""
        s = self.s
        s.install('baseline')
        baseline, candidate = s.sides['baseline'], s.sides['candidate']
        chain = s.chains['candidate']
        fixture_dir = Path(s.stage_dir) / 'update'
        self.fixture = s.observed('update-fixture', lambda: build_fixture(fixture_dir, baseline['sha'], chain['installerPath'], chain['installerSha256'],
                                                                           baseline['version'], candidate['version']), needs=('stage-installer',))
        self.port = debug_port(s.token)
        s.outcome['debugPort'] = self.port
        proof, sc2 = s.launch_and_prove('baseline', CdpPort=self.port, UpdateFixture=s.windows_stage + '\\update')
        self.baseline_instance = proof
        s.matrix['gates']['native_identity'] = {'status': 'PASS', 'evidence': ['native-launch-baseline.json', 'installed-identity-baseline.json']}
        s.matrix['gates']['fixture'] = {'status': 'PASS', 'evidence': ['instance-proof-baseline.json']}
        s.matrix['gates']['trust_answer'] = {'status': 'PASS', 'evidence': ['review-approval-used.json']}
        s.save()

    def tmux_task(self, rows, pane):
        """One owned shell task (profile s7-shell: bash, no tool) whose tmux session must outlive the update."""
        task = rows.shell_task(CASE, PROJECT)
        deadline = self.clock.monotonic() + 60
        while not task.get('terminalId') and self.clock.monotonic() < deadline:
            self.clock.sleep(0.5)
            task = self.s.rpc('agents.get', {'id': task['id']})
        if not task.get('terminalId'):
            raise UpdateFailed('The owned shell task has no terminal', {'state': task.get('state')})
        self.task, self.pane = task, pane
        return self.tmux_state()

    def tmux_state(self):
        tid = self.task['terminalId']
        current = self.s.rpc('agents.get', {'id': self.task['id']})
        alive = self.pane.has_session(tid)
        return {'terminalId': tid, 'session': alive, 'identity': self.pane.display(TMUX_IDENTITY, tid) if alive else None,
                'taskTerminalId': current.get('terminalId'), 'state': current.get('state')}

    def update(self):
        """UP1's update: witnesses, the app's commands, the wait for the product's runner, the adoption, the new app."""
        s, data = self.s, self.data
        data['fixture'] = {key: self.fixture[key] for key in ('sourceSha', 'currentVersion', 'targetVersion', 'installerSha256', 'installerBytes')}
        data['shape'] = s.args.update_shape
        data['wslBefore'] = self.witness()
        data['daemonAnswered'] = bool(s.rpc('agents.list', {}).get('items') is not None)  # the dialog's own precondition (UpdateConfirmation.tsx:93-97, 117-123)
        sampler = self.sampler().start()
        try:
            s.observed('native-update-begin', lambda: self.native('native-lifecycle.ps1', Action='update-begin'))
            s.update_begun = True
            steps = []
            try:
                drive_update(self.cdp, self.fixture['targetVersion'], self.clock, steps)
            finally:
                data['commands'] = self.save('update-commands', steps)
            observed = []
            try:
                summary = watch_update(lambda: self.native('native-lifecycle.ps1', Action='update-observe'), self.status_or_none, self.clock, rows=observed)
            finally:
                data['observations'] = self.save('update-observe', observed)
            data['runner'] = {key: value for key, value in summary.items() if key != 'last'}
            adoption = s.observed('native-update', lambda: self.native('native-lifecycle.ps1', timeout=TIMEOUTS['adoptWait'] + 80, Action='update-adopt',
                                                                       WaitSeconds=TIMEOUTS['adoptWait']), needs=('native-update-begin',))
            data['adoption'] = adoption
            s.set_phase('candidate')
            data['instance'] = s.instance('candidate')
            s.seal_after_launch(reseal=True)
            deadline = self.clock.monotonic() + TIMEOUTS['newApp']
            reported = self.status_or_none()
            while reported is None and self.clock.monotonic() < deadline:
                self.clock.sleep(2)
                reported = self.status_or_none()
            data['reported'] = reported
            identity = s.observed('installed-identity-candidate', lambda: self.native('native-lifecycle.ps1', Action='installed-identity',
                                                                                     LinuxSha256=s.chains['candidate']['linux']['sha256']))
            data['identity'] = identity
            data['prodComparison'] = s.prod_compare('after-update')
        finally:
            data['connection'] = sampler.result()
            data['wslAfter'] = self.witness()
        return data

    def parts(self, tmux_before, tmux_after):
        data, candidate = self.data, self.s.sides['candidate']
        adoption, reported, identity = data.get('adoption') or {}, data.get('reported') or {}, data.get('identity') or {}
        same_task = (tmux_after.get('session') is True and tmux_after.get('identity') and tmux_after.get('identity') == tmux_before.get('identity')
                     and tmux_after.get('taskTerminalId') == tmux_before.get('terminalId') and tmux_after.get('state') not in ('failed', 'closed', None))
        linux, instance = self.s.chains['candidate']['linux'], data.get('instance') or {}
        # The product shows its source SHA nowhere at run time, so the relaunched build is identified by three things:
        # (1) the installed tree's devbox-linux is byte for byte the candidate run's Linux artifact, whose receipt names
        # the candidate SHA (verify_linux_artifact); (2) the daemon that app deployed and runs is that same artifact
        # (instance_proof, phase candidate); (3) the app refuses the fixture it inherited, whose owner.json names the
        # baseline SHA, so its compiled SOURCE_SHA is not the baseline's (fixture.rs:48; `enabled` false).
        data['identityHow'] = {'bundledLinuxSha256': identity.get('bundledLinuxSha256'), 'candidateLinuxSha256': linux.get('sha256'),
                               'candidateLinuxReceiptSourceSha': linux.get('source_sha'), 'candidateSha': candidate['sha'],
                               'daemonProofSourceSha': instance.get('sourceSha'), 'daemonProofBinarySha256': instance.get('binarySha256'),
                               'relaunchedAppRefusedBaselineFixture': reported.get('enabled') is False}
        # Recorded, never judged: whether the daemon is another process after the update. It must be known either way.
        before = getattr(self, 'baseline_instance', None) or {}
        data['daemon'] = {side: {key: proof.get(key) for key in ('runningPid', 'invocationId', 'binarySha256')}
                          for side, proof in (('before', before), ('after', instance))}
        data['daemonReplaced'] = daemon_replaced(before, instance)
        return {'installerRan': adoption.get('exeChanged') is True and (data.get('runner') or {}).get('installed') in ('recorded', 'acknowledged'),
                'daemonRecorded': data['daemonReplaced'] is not None,
                'newAppStarted': len(adoption.get('adopted') or []) == 1 and bool(reported),
                'version': reported.get('currentVersion') == candidate['version'] and adoption.get('versionAfter') == candidate['version']
                           and identity.get('bundledVersion') == candidate['version'],
                'identity': (bool(linux.get('sha256')) and identity.get('bundledLinuxSha256') == linux['sha256'] and linux.get('source_sha') == candidate['sha']
                             and instance.get('sourceSha') == candidate['sha'] and instance.get('binarySha256') == linux['sha256']
                             and reported.get('enabled') is False),
                'distroUp': distro_stayed_up(data.get('wslBefore') or {}, data.get('wslAfter') or {}),
                'tmuxSurvived': bool(same_task),
                'connectionGapMeasured': (data.get('connection') or {}).get('gapSeconds') is not None,
                'prodUnchanged': (data.get('prodComparison') or {}).get('prodUnchanged') is True}


def run(session, clock=time, sampler=BridgeSampler, witness=wsl_witness):
    """The `update` flow's cases UP1 and UP2 (A7 and M4 are concluded by the driver's teardown)."""
    import daemon_rows
    import evidence as ev
    import w1_windows
    s = session
    flow = UpdateRun(s, clock, sampler, witness)
    flow.prepare()
    if CASE not in s.cases:
        return flow
    live = s.live()
    rows = daemon_rows.DaemonRows(s.rpc, s.load, s.case_result, s.evidence, s.flow_name, pane=live.pane, counter=s.counter, clock=clock)
    install = w1_windows.drvfs(s.load()['nativePreflight']['installPath'])
    state = {}

    def up1(_selected):
        m = s.load()
        tmux_before = flow.tmux_task(rows, live.pane)
        if 'UP2' in s.cases:
            try:  # a plant that fails is UP2's own failure; the update of UP1 still runs
                state['planted'] = foreign_files.plant(install, m['token'], 'update', m['sourceSha'])
                flow.save('foreign-update-planted', state['planted'])
            except Exception as error:
                state['plantError'] = redact_text(str(error), 300)
        try:
            flow.update()
        except Exception as error:
            flow.data['failure'] = {'type': type(error).__name__, 'message': redact_text(str(error), 600), 'detail': getattr(error, 'detail', None)}
            raise
        finally:
            flow.data['tmuxBefore'] = tmux_before
            try:
                flow.data['tmuxAfter'] = flow.tmux_state()
            except Exception as error:
                flow.data['tmuxAfter'] = {'error': redact_text(str(error), 300)}
            flow.save('cases/' + CASE, flow.data)
        parts = flow.parts(tmux_before, flow.data['tmuxAfter'])
        flow.data['parts'] = parts
        flow.save('cases/' + CASE, flow.data)
        status, reason = decide(parts, flow.data.get('shape'))
        if flow.data.get('daemonReplaced') is False:
            reason = ' '.join(filter(None, [reason, SAME_DAEMON]))
        flow.data['decidingParts'] = deciding(parts, flow.data.get('shape'))
        flow.save('cases/' + CASE, flow.data)
        s.case_result(CASE, status, ['cases/%s.json' % CASE, 'update-fixture.json', 'update-commands.json', 'update-observe.json', 'native-update.json',
                                     'installed-identity-candidate.json', 'instance-proof-candidate.json', 'prod-after-update.json'], reason,
                      surface='installed-native', parts=parts, shape=flow.data.get('shape'), decidingParts=flow.data['decidingParts'],
                      daemonReplaced=flow.data.get('daemonReplaced'),
                      measurement={'connectionGapSeconds': flow.data['connection'].get('gapSeconds'), 'connection': flow.data['connection'],
                                   'runner': flow.data.get('runner'), 'reportedVersion': (flow.data.get('reported') or {}).get('currentVersion')},
                      keepAlive=ev.ROW_TEXT['UP1.keepalive'] if (flow.data.get('runner') or {}).get('keepAlive') else None)

    try:
        s.guarded((CASE, 'UP2'), up1)
    finally:
        up2(s, flow, install, state, ev)
        if getattr(flow, 'task', None):
            try:
                live.end('update-cleanup', flow.task['id'])
            except Exception as error:
                s.outcome['updateTaskEnd'] = redact_text(str(error), 300)
    return flow


def up2(s, flow, install, state, ev):
    """UP2: read the planted `update` set back once the product's installer ran, then remove exactly that set."""
    planted = state.get('planted')
    if planted is None:
        if 'plantError' in state:
            s.case_result('UP2', 'FAIL', [], 'The foreign files could not be planted before the update: ' + state['plantError'])
        return
    result = {'planted': planted}
    try:
        ran = (flow.data.get('adoption') or {}).get('exeChanged') is True
        result['observed'] = foreign_files.observe(install, planted)
        result['comparison'] = foreign_files.compare(planted, result['observed'])
        result['cleanup'] = foreign_files.remove(install, planted)
        if not ran:
            status, reason = 'NOT_RUN', ev.ROW_TEXT['UP2.noupdate']
        else:
            status, reason = foreign_files.decision(result['comparison'], result['cleanup'])
    except Exception as error:
        result['error'] = redact_text(str(error), 300)
        status, reason = 'FAIL', 'The foreign files could not be read back or removed: ' + result['error']
    dump(s.evidence / 'cases' / 'UP2.json', result)
    # What was read back is UP2's own observation, also when UP1 ended early: it replaces the group's "not reached".
    s.case_result('UP2', status, ['cases/UP2.json', 'foreign-update-planted.json'], reason, surface='installed-native',
                  comparison=result.get('comparison'))
