"""A4: Windows-target service readiness on the owned accept instance (driven by driver.py).

Sequence (all resources owned and recorded before use):
 1. An owned unrelated Windows listener binds 127.0.0.1:0 -> port P.
 2. The owned project's .devbox/devbox.toml defines one Windows service with ports_hint=[P]; its command
    waits for an owned 'go' event, then starts a child PowerShell that listens on P until an owned 'stop'
    event (hard self-deadlines everywhere).
 3. projects.add (owned path) -> trust_preview -> trust_approve with the digest just previewed (the same
    approval the UI performs; FLAGGED for reviewer acceptance) -> runtime.start.
    runtime.start of a service awaits readiness (product wait_ready, up to ready_timeout_sec), so it runs
    on a background thread with a deadline above ready_timeout_sec. The run id is discovered at once with
    the read-only owned runtime.runs query and recorded in the ownership manifest before any runtime.get
    (run 1 never recorded it because the start RPC hit its 15 s deadline).
 4. Phase N (not-ready): while P is held by the unrelated listener, runtime.get must report ready=false.
 5. Release the unrelated listener, set 'go'; Phase R: ready=true must follow; times recorded. The start
    thread must then return the same ready run (its result or error is evidence).
 6. Teardown: set 'stop', join the start thread (bounded), adopt every run of the owned project (even after
    a start error) so cleanup can stop it, runtime.stop, verify the recorded Windows child identity is gone
    (exact stop only).
"""
import base64
import json
import os
import re
import threading
import time
import uuid
from pathlib import Path

from common import (A4_READY_TIMEOUT_SEC, IDENTITY_TYPE, START_RPC_DEADLINE_SECONDS, LineProcess, ensure_trust, identity_states, ps_command,
                    run_checked, stop_exact, trust_context)

LIVE = ('starting', 'running', 'restarting')

PS_ABS = r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe'
UNRELATED = r'''$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class S7A4Self { [StructLayout(LayoutKind.Sequential)] struct FT { public uint L, H; }
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool GetProcessTimes(IntPtr p, out FT c, out FT e, out FT k, out FT u);
 public static string Creation() { FT c,e,k,u; GetProcessTimes(GetCurrentProcess(),out c,out e,out k,out u); return (((ulong)c.H<<32)|c.L).ToString(System.Globalization.CultureInfo.InvariantCulture); } }
'@
$l=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0)
$l.Server.ExclusiveAddressUse=$true
try {
  $l.Start()
  [Console]::Out.WriteLine((@{port=$l.LocalEndpoint.Port;pid=$PID;creationTime=[S7A4Self]::Creation()}|ConvertTo-Json -Compress)); [Console]::Out.Flush()
  $read=[Console]::In.ReadLineAsync(); [void]$read.Wait(600000)
} finally { $l.Stop() }
[Console]::Out.WriteLine('{"released":true}'); [Console]::Out.Flush()
'''
CHILD = r'''$ErrorActionPreference='Stop'
$stop=New-Object System.Threading.EventWaitHandle($false,[System.Threading.EventResetMode]::ManualReset,'STOP')
$l=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,PORT)
try { $l.Start(); [Console]::Out.WriteLine('s7a4-child-listening NONCE '+$PID); [Console]::Out.Flush(); [void]$stop.WaitOne(300000) }
finally { $l.Stop(); $stop.Dispose() }
'''
# Runs inside the product bootstrap (DevboxObservation is already loaded there). Both events exist before
# any wait, so a teardown 'stop' always releases it; the bootstrap root records its own identity in the
# owned Windows copy dir so teardown can prove it gone by exact PID + creation FILETIME.
SERVICE = ("$go=New-Object System.Threading.EventWaitHandle($false,[System.Threading.EventResetMode]::ManualReset,'GO'); "
           "$stop=New-Object System.Threading.EventWaitHandle($false,[System.Threading.EventResetMode]::ManualReset,'STOP'); "
           "[IO.File]::WriteAllText((Join-Path (Get-Location) 'ROOTFILE'),(@{pid=$PID;creationTime=[DevboxObservation]::CreationTime()}|ConvertTo-Json -Compress)); "
           "try { $i=[System.Threading.WaitHandle]::WaitAny([System.Threading.WaitHandle[]]@($go,$stop),240000); if($i -ne 0) { exit 3 } "
           "& 'PSABS' -NoProfile -NonInteractive -EncodedCommand CHILDB64; exit $LASTEXITCODE } finally { $go.Dispose(); $stop.Dispose() }")


def plan(token):
    nonce = uuid.uuid4().hex
    return {'nonce': nonce, 'serviceId': 's7win' + nonce[:8], 'go': 'Local\\Devbox.S7A4.' + nonce + '.go',
            'stop': 'Local\\Devbox.S7A4.' + nonce + '.stop', 'token': token}


def service_command(a4, port):
    child = CHILD.replace('STOP', a4['stop']).replace('PORT', str(int(port))).replace('NONCE', a4['nonce'])
    b64 = base64.b64encode(child.encode('utf-16-le')).decode()
    a4['childB64'] = b64
    a4['rootFile'] = '.s7a4-root-' + a4['nonce'] + '.json'
    return (SERVICE.replace("'GO'", "'" + a4['go'] + "'").replace("'STOP'", "'" + a4['stop'] + "'").replace('ROOTFILE', a4['rootFile'])
            .replace('PSABS', PS_ABS).replace('CHILDB64', b64))


def project_toml(a4, port):
    command = service_command(a4, port)
    if "'''" in command:
        raise ValueError('Command must fit a TOML literal string')
    return ("[[service]]\nid = '%s'\ntarget = 'windows'\ncommand = '''%s'''\nports_hint = [%d]\nready_timeout_sec = %d\n"
            % (a4['serviceId'], command, int(port), A4_READY_TIMEOUT_SEC))


def set_event(name):
    return ps_command("try{$e=[System.Threading.EventWaitHandle]::OpenExisting('%s');[void]$e.Set();$e.Dispose();@{set=$true}|ConvertTo-Json -Compress}"
                      "catch{@{set=$false;reason=$_.Exception.GetType().Name}|ConvertTo-Json -Compress}" % name, timeout=30)


def port_owner(port, b64):
    """Read-only: the listener owner of P with creation time and whether its command line is our child."""
    script = IDENTITY_TYPE + ("$c=@(Get-NetTCPConnection -State Listen -LocalPort %d -ErrorAction SilentlyContinue);"
              "$rows=@($c|ForEach-Object{$p=Get-CimInstance Win32_Process -Filter ('ProcessId='+$_.OwningProcess);"
              "@{pid=$_.OwningProcess;address=$_.LocalAddress;creation=[S7Identity]::Creation([uint32]$_.OwningProcess);ours=$(if($p){$p.CommandLine -like '*%s*'}else{$false})}});"
              "@{rows=$rows}|ConvertTo-Json -Depth 4 -Compress") % (int(port), b64[:64])
    return ps_command(script, timeout=40)


class Background:
    """One owned call on a daemon thread. Only fills this holder; never touches the manifest."""

    def __init__(self, function):
        self.result = None
        self.error = None
        self.started = time.monotonic()
        self.ended = None
        self.done = threading.Event()
        threading.Thread(target=self._run, args=(function,), daemon=True).start()

    def _run(self, function):
        try:
            self.result = function()
        except Exception as error:
            self.error = '%s: %s' % (type(error).__name__, str(error)[:800])
        finally:
            self.ended = time.monotonic()
            self.done.set()

    def join(self, timeout):
        self.done.wait(max(0.0, timeout))
        return self.snapshot()

    def snapshot(self):
        done = self.done.is_set()
        return {'done': done, 'result': self.result if done else None, 'error': self.error if done else None,
                'seconds': round((self.ended if done else time.monotonic()) - self.started, 3)}


RUN_ID = re.compile('[0-9a-f]{32}')


def owned_runs(m, rpc):
    """Read-only: every run of a recorded owned project (runtime.runs is limited to owned project ids)."""
    rows = []
    for project_id in list(m.get('projectIds', [])):
        items = rpc('runtime.runs', {'projectId': project_id, 'limit': 100}).get('items') or []
        rows += [r for r in items if isinstance(r, dict) and r.get('projectId') == project_id]
    return rows


def adopt_runs(m, runs, save_manifest):
    """Record run ids/units of owned-project runs before any further call; refuse ids cleanup cannot map."""
    added, refused = [], []
    for row in runs:
        run_id, unit = row.get('id'), row.get('unit')
        if row.get('projectId') not in m.get('projectIds', []):
            refused.append({'id': run_id, 'reason': 'not an owned project'})
            continue
        if not isinstance(run_id, str) or not RUN_ID.fullmatch(run_id) or unit != 'devbox-accept-run-' + run_id + '.service':
            refused.append({'id': run_id, 'unit': unit, 'reason': 'id/unit is not the accept run unit form cleanup derives'})
            continue
        if run_id not in m['runIds']:
            m['runIds'].append(run_id)
            m.setdefault('runUnits', []).append(unit)
            added.append(run_id)
    if added:
        save_manifest()
    if refused:
        raise RuntimeError('Owned-project run could not be recorded; retained: ' + json.dumps(refused)[:600])
    return added


def discover_run(m, a4, rpc, save_manifest, start, timeout=30):
    """Poll the owned project's runs until the A4 service run appears; record it before returning."""
    deadline = time.monotonic() + timeout
    while True:
        runs = owned_runs(m, rpc)
        adopt_runs(m, runs, save_manifest)
        mine = [r for r in runs if r.get('defId') == a4['serviceId']]
        if not mine and start.done.is_set() and isinstance(start.result, dict):
            adopt_runs(m, [start.result], save_manifest)
            mine = [start.result] if start.result.get('defId') == a4['serviceId'] else []
        if mine:
            if len({r['id'] for r in mine}) != 1:
                raise RuntimeError('Expected exactly one owned A4 run, found %d (all recorded)' % len({r['id'] for r in mine}))
            return mine[0]
        if start.done.is_set():
            raise RuntimeError('runtime.start ended without an owned A4 run: ' + str(start.error)[:600])
        if time.monotonic() >= deadline:
            raise TimeoutError('Owned A4 run not visible within %d s of runtime.start' % timeout)
        time.sleep(0.5)


def run(m, save_manifest, rpc, evidence, record_step):
    """Returns a case row. rpc(method, params) is the bounded owned-only wsl-owned RPC."""
    a4 = plan(m['token'])
    m['a4'] = a4
    m['a4ServiceId'] = a4['serviceId']
    save_manifest()
    unrelated = None
    child_identity = None
    start = None
    row = {'case': 'A4', 'surface': 'installed-native', 'steps': [], 'timeline': {}}
    t0 = time.monotonic()
    mark = lambda name: row['timeline'].__setitem__(name, round(time.monotonic() - t0, 3))
    try:
        unrelated = LineProcess(UNRELATED)
        held = unrelated.read_json(0, 60)
        a4['unrelated'] = held
        save_manifest()
        port = int(held['port'])
        row['port'] = port
        # S7: P-A4 is a fixture project (git init, repo-local identity, README commit, fixture.make_project);
        # only its run-time toml is added and committed here.
        project_path = m['projectPaths']['P-A4']
        project = Path(project_path)
        (project / '.devbox').mkdir(mode=0o700)
        text = project_toml(a4, port)
        (project / '.devbox/devbox.toml').write_text(text)
        env = {'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_NOSYSTEM': '1', 'PATH': '/usr/bin:/bin'}
        for args in [['add', '.devbox/devbox.toml'], ['-c', 'commit.gpgSign=false', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'test: S7 owned A4 fixture']]:
            run_checked(['git', '-C', str(project), *args], timeout=15, env=env)  # a failure keeps its redacted stderr (item 6)
        mark('projectWritten')
        added = rpc('projects.add', {'path': project_path})
        # The owned RPC recorded the id on disk; the in-memory copy adopts it through save_manifest.
        if added['id'] not in m['projectIds']:
            m['projectIds'].append(added['id'])
        save_manifest()
        def owned_definition(preview):
            # Before any approval: the preview is the owned project's and shows exactly the owned service.
            record_step('a4-trust-preview', preview)
            if preview.get('canonicalPath') != os.path.realpath(project_path):
                raise RuntimeError('Trust preview canonical path is not the owned project')
            if [d.get('id') for d in preview.get('runDefinitions') or []] != [a4['serviceId']]:
                raise RuntimeError('Trust preview does not show exactly the owned service definition')
        # Item 5: the run context exactly as the app sends it, and the app's ensure() rule (asked before the start).
        row['trust'] = ensure_trust(rpc, trust_context(added['id'], 'run'), check=owned_definition)
        mark('trusted')
        start_params = {'projectId': added['id'], 'id': a4['serviceId']}
        start = Background(lambda: rpc('runtime.start', start_params))
        mark('startIssued')
        started = discover_run(m, a4, rpc, save_manifest, start)
        mark('runRecorded')
        row['runId'] = started['id']
        row['startPendingAtRecord'] = not start.done.is_set()
        record_step('a4-start', {'discovered': started, 'startRpc': start.snapshot()})
        not_ready = []
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            detail = rpc('runtime.get', {'runId': started['id']})['run']
            not_ready.append({'t': round(time.monotonic() - t0, 3), 'state': detail['state'], 'ready': detail['ready'],
                              'startPending': not start.done.is_set()})
            if detail['ready'] or detail['state'] not in LIVE:
                break
            time.sleep(0.5)
        row['phaseNotReady'] = not_ready
        root_identity = read_root_identity(m, a4, rpc, started['id'])
        row['bootstrapRoot'] = root_identity
        if root_identity:
            m['windowsProcesses'].append({'label': 'a4-bootstrap-root', 'pid': root_identity['pid'], 'creationTime': root_identity['creationTime']})
            save_manifest()
        held_ok = bool(not_ready) and not any(s['ready'] for s in not_ready) and not_ready[-1]['state'] in LIVE
        unrelated.send('release')
        unrelated.read_json(1, 30)
        unrelated.close(20)
        mark('unrelatedReleased')
        row['goEvent'] = set_event(a4['go'])
        mark('go')
        ready = []
        ready_at = None
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            detail = rpc('runtime.get', {'runId': started['id']})['run']
            ready.append({'t': round(time.monotonic() - t0, 3), 'state': detail['state'], 'ready': detail['ready']})
            if detail['ready']:
                ready_at = time.monotonic()
                break
            if detail['state'] not in LIVE:
                break
            time.sleep(0.25)
        row['phaseReady'] = ready
        owner = port_owner(port, a4['childB64'])
        row['portOwnerAtReady'] = owner
        ours = [r for r in owner.get('rows', []) if r.get('ours') and r.get('creation')]
        if ours:
            child_identity = ('a4-child', ours[0]['pid'], ours[0]['creation'])
            m['windowsProcesses'].append({'label': 'a4-child', 'pid': ours[0]['pid'], 'creationTime': ours[0]['creation']})
            save_manifest()
        if ready_at is not None:
            row['readyAfterGoSeconds'] = round(ready_at - t0 - row['timeline']['go'], 3)
            # wait_ready re-polls every second, so a ready run returns from runtime.start promptly.
            row['startRpc'] = start.join(30)
            mark('startReturned')
        start_run = (row.get('startRpc') or {}).get('result') or {}
        if not held_ok:
            row.update(status='FAIL', reason='Service reported ready (or ended) while only the unrelated listener held the port')
        elif ready_at is None:
            row.update(status='FAIL', reason='Service child listened on the hinted port but readiness never became true within 60 s')
        elif not ours:
            row.update(status='FAIL', reason='Ready reported but the port owner is not the owned service child')
        elif start_run.get('id') != started['id'] or start_run.get('ready') is not True:
            row.update(status='FAIL', reason='runtime.start did not return the same ready owned run: ' + json.dumps(row.get('startRpc'))[:600])
        else:
            row.update(status='PASS', reason='Unrelated listener -> not ready; owned service child listener -> ready')
    except Exception as error:
        row.update(status='FAIL', reason='%s: %s' % (type(error).__name__, str(error)[:800]))
    finally:
        row['teardown'] = teardown(m, a4, rpc, unrelated, child_identity, start, save_manifest)
        if start is not None:
            row['startRpc'] = start.snapshot()
        if any(step.get('status') == 'FAIL' for step in row['teardown']):
            row['status'] = 'FAIL'
            row['reason'] = (row.get('reason') or '') + ' | A4 teardown incomplete; resources retained'
    return row


def read_root_identity(m, a4, rpc, run_id):
    """The bootstrap root's self-reported identity from the owned Windows copy dir (bounded wait)."""
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        detail = rpc('runtime.get', {'runId': run_id})['run']
        directory = (detail.get('windows') or {}).get('directory')
        if directory and directory.startswith(m['windowsRoot'] + '\\'):
            path = Path(run_checked(['wslpath', '-u', directory], timeout=10, text=True).stdout.strip()) / a4['rootFile']
            if path.is_file() and not path.is_symlink() and path.stat().st_size < 512:
                value = json.loads(path.read_text(encoding='utf-8-sig'))
                return {'pid': int(value['pid']), 'creationTime': str(int(value['creationTime'])), 'directory': directory}
        time.sleep(1)
    return None


def teardown(m, a4, rpc, unrelated, child_identity, start=None, save_manifest=lambda: None):
    steps = []
    try:
        steps.append({'action': 'stop-event', 'status': 'PASS', 'result': set_event(a4['stop'])})
    except Exception as error:
        steps.append({'action': 'stop-event', 'status': 'FAIL', 'error': str(error)[:300]})
    if start is not None:
        # The stop event ends the service script, so a still-blocked start returns its exited run promptly.
        # Bound: the start RPC deadline plus the driver's subprocess margin.
        snap = start.join(start.started + START_RPC_DEADLINE_SECONDS + 60 - time.monotonic())
        steps.append({'action': 'runtime.start-join', 'status': 'PASS' if snap['done'] else 'FAIL', 'startRpc': snap,
                      **({} if snap['done'] else {'reason': 'runtime.start still pending; resources retained'})})
    if m.get('projectIds'):
        # Adopt every run of the owned project (also after a start error) so runtime.stop and cleanup see it.
        try:
            runs = owned_runs(m, rpc)
            if start is not None and isinstance(start.result, dict):
                runs.append(start.result)
            steps.append({'action': 'adopt-owned-runs', 'status': 'PASS', 'added': adopt_runs(m, runs, save_manifest), 'runIds': list(m['runIds'])})
        except Exception as error:
            steps.append({'action': 'adopt-owned-runs', 'status': 'FAIL', 'error': str(error)[:600]})
    for run_id in list(m.get('runIds', [])):
        try:
            result = rpc('runtime.stop', {'runId': run_id})
            steps.append({'action': 'runtime.stop', 'status': 'PASS', 'runId': run_id, 'state': result.get('state')})
        except Exception as error:
            # Already exited runs are fine; record the error text for review.
            steps.append({'action': 'runtime.stop', 'status': 'PASS', 'runId': run_id, 'note': str(error)[:300]})
    if unrelated is not None:
        code = unrelated.close(20)
        steps.append({'action': 'unrelated-listener', 'status': 'PASS' if code is not None else 'FAIL', 'exit': code})
        held = a4.get('unrelated')
        if held:
            state = identity_states([('unrelated', held['pid'], held['creationTime'])]).get('unrelated')
            if state == 'alive':
                result = stop_exact('unrelated', held['pid'], held['creationTime'])
                steps.append({'action': 'unrelated-exact-stop', 'status': 'PASS' if result['result'] in ('stopped', 'absent') else 'FAIL', 'result': result})
    identities = [(p['label'], p['pid'], p['creationTime']) for p in m.get('windowsProcesses', []) if p['label'].startswith('a4-')]
    if child_identity and child_identity[0] not in [i[0] for i in identities]:
        identities.append(child_identity)
    if any(p['label'] == 'a4-bootstrap-root' for p in m.get('windowsProcesses', [])) is False and m.get('runIds'):
        # Without the root's identity the Windows copy dir may still be in use: retain, never guess by name.
        steps.append({'action': 'a4-bootstrap-root-identity', 'status': 'FAIL', 'reason': 'Bootstrap root identity not recorded; retained'})
    for label, pid, creation in identities:
        deadline = time.monotonic() + 15
        state = identity_states([(label, pid, creation)]).get(label)
        while state == 'alive' and time.monotonic() < deadline:
            time.sleep(1)
            state = identity_states([(label, pid, creation)]).get(label)
        if state == 'alive':
            result = stop_exact(label, pid, creation)
            steps.append({'action': label + '-exact-stop', 'status': 'PASS' if result['result'] in ('stopped', 'absent') else 'FAIL', 'result': result})
        else:
            steps.append({'action': label + '-gone', 'status': 'PASS', 'state': state})
    return steps
