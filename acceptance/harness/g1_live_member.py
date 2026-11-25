#!/usr/bin/env python3
"""G1: owned live-member job fixture (interop only; no install, no app, no WSL units).

An owned root joins a fresh named job, listens on an owned loopback ephemeral port and starts one
owned child (job member, no breakaway). The exact product probe bytes then observe:
  P1 control    root + child alive, root holds the job handle       -> active(count>=2) / owned
  P2 closed     root alive, root closed its only job handle         -> identityMismatch (07c Minor 1)
  P3 rootExited root gone, child (job member) still alive           -> I1 premise finding
  P4 reaped     child released                                      -> notFound | empty
Only owned processes are created; each is released by its own protocol and, only if that fails,
terminated by exact PID + creation FILETIME. Nothing else on Windows is touched.
"""
import argparse
import base64
import json
import sys
import time
import uuid
from pathlib import Path

from common import (BASE, evidence_root, harness_hashes, LineProcess, assigned_receipt, cgroup_evidence, dump, identity_states,
                    native_class_source, probe_script, ps_command, require_lock, run_probe, stop_exact)

CHILD = r'''$ErrorActionPreference='Stop'
$e=New-Object System.Threading.EventWaitHandle($false,[System.Threading.EventResetMode]::ManualReset,'EVENT')
try { [void]$e.WaitOne(180000) } finally { $e.Dispose() }
'''

HELPER = r'''
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class S7G1 {
  [StructLayout(LayoutKind.Sequential)] struct FileTime { public uint Low, High; }
  [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)] static extern IntPtr OpenJobObject(uint access, bool inherit, string name);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool IsProcessInJob(IntPtr process, IntPtr job, out bool member);
  [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetProcessTimes(IntPtr process, out FileTime c, out FileTime e, out FileTime k, out FileTime u);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  public static int OpenJobError(string name) { IntPtr j=OpenJobObject(4,false,name); if(j==IntPtr.Zero) return Marshal.GetLastWin32Error(); CloseHandle(j); return 0; }
  public static string InJob(IntPtr process, IntPtr job) { bool m; if(!IsProcessInJob(process,job,out m)) return "unavailable:"+Marshal.GetLastWin32Error(); return m ? "member" : "notMember"; }
  public static string Creation(IntPtr process) { FileTime c,x,k,u; if(!GetProcessTimes(process,out c,out x,out k,out u)) throw new Exception("times"); return (((ulong)c.High<<32)|c.Low).ToString(System.Globalization.CultureInfo.InvariantCulture); }
  [StructLayout(LayoutKind.Sequential)] struct SECURITY_ATTRIBUTES { public int length; public IntPtr descriptor; public int inherit; }
  [StructLayout(LayoutKind.Sequential)] struct STARTUPINFO { public uint cb; public IntPtr reserved,desktop,title; public uint x,y,xSize,ySize,xChars,yChars,fill,flags; public ushort show,reservedCount; public IntPtr reservedBytes,input,output,error; }
  [StructLayout(LayoutKind.Sequential)] struct STARTUPINFOEX { public STARTUPINFO startup; public IntPtr attributes; }
  [StructLayout(LayoutKind.Sequential)] struct PROCESS_INFORMATION { public IntPtr process,thread; public uint pid,tid; }
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFile(string name,uint access,uint share,ref SECURITY_ATTRIBUTES security,uint creation,uint flags,IntPtr template);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,int flags,ref IntPtr size);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,IntPtr size,IntPtr previous,IntPtr returned);
  [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcess(string executable,System.Text.StringBuilder command,IntPtr ps,IntPtr ts,bool inherit,uint flags,IntPtr environment,string directory,ref STARTUPINFOEX startup,out PROCESS_INFORMATION info);
  // The child inherits exactly one NUL handle (PROC_THREAD_ATTRIBUTE_HANDLE_LIST), never the WSL relay
  // pipes, so the root's exit stays observable. No breakaway flag: the child joins the root's job.
  public static IntPtr StartIsolated(string executable,string arguments,out uint pid) {
    IntPtr nul=IntPtr.Zero,attributes=IntPtr.Zero,handles=IntPtr.Zero; bool initialized=false; var info=new PROCESS_INFORMATION();
    try {
      var security=new SECURITY_ATTRIBUTES{length=Marshal.SizeOf(typeof(SECURITY_ATTRIBUTES)),inherit=1};
      nul=CreateFile("NUL",0xC0000000,3,ref security,3,0x80,IntPtr.Zero);
      if(nul==new IntPtr(-1)) { nul=IntPtr.Zero; throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); }
      IntPtr bytes=IntPtr.Zero; InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref bytes);
      attributes=Marshal.AllocHGlobal(bytes);
      if(!InitializeProcThreadAttributeList(attributes,1,0,ref bytes)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
      initialized=true; handles=Marshal.AllocHGlobal(IntPtr.Size); Marshal.WriteIntPtr(handles,0,nul);
      if(!UpdateProcThreadAttribute(attributes,0,new IntPtr(0x20002),handles,new IntPtr(IntPtr.Size),IntPtr.Zero,IntPtr.Zero)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
      var startup=new STARTUPINFOEX(); startup.startup.cb=(uint)Marshal.SizeOf(typeof(STARTUPINFOEX));
      startup.startup.flags=0x100; startup.startup.input=nul; startup.startup.output=nul; startup.startup.error=nul; startup.attributes=attributes;
      if(!CreateProcess(executable,new System.Text.StringBuilder("\""+executable+"\" "+arguments),IntPtr.Zero,IntPtr.Zero,true,0x00080000|0x08000000|0x00000200,IntPtr.Zero,null,ref startup,out info))
        throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
      pid=info.pid; IntPtr process=info.process; info.process=IntPtr.Zero; return process;
    } finally {
      if(info.thread!=IntPtr.Zero) CloseHandle(info.thread);
      if(info.process!=IntPtr.Zero) CloseHandle(info.process);
      if(initialized) DeleteProcThreadAttributeList(attributes);
      if(attributes!=IntPtr.Zero) Marshal.FreeHGlobal(attributes);
      if(handles!=IntPtr.Zero) Marshal.FreeHGlobal(handles);
      if(nul!=IntPtr.Zero) CloseHandle(nul);
    }
  }
  public static void Release(IntPtr process) { if(process!=IntPtr.Zero) CloseHandle(process); }
}
'@
'''

ROOT = r'''
function Say($value) { [Console]::Out.WriteLine(($value | ConvertTo-Json -Compress)); [Console]::Out.Flush() }
$job=[DevboxObservation]::AssociateFresh('JOB')
$listener=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0)
$child=[IntPtr]::Zero
try {
  $listener.Start()
  $childPid=[uint32]0
  $child=[S7G1]::StartIsolated('C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','-NoProfile -NonInteractive -EncodedCommand CHILD',[ref]$childPid)
  $deadline=[DateTime]::UtcNow.AddSeconds(30); $ready=$null
  while(-not [System.Threading.EventWaitHandle]::TryOpenExisting('EVENT',[ref]$ready)) {
    if([DateTime]::UtcNow -gt $deadline) { throw 'owned child not ready' }
    Start-Sleep -Milliseconds 100
  }
  $ready.Dispose()
  Say @{phase='started';pid=$PID;creationTime=[DevboxObservation]::CreationTime();sid=[DevboxObservation]::Sid();sessionId=[DevboxObservation]::Session();
        port=$listener.LocalEndpoint.Port;childPid=$childPid;childCreationTime=[S7G1]::Creation($child);
        childInJob=[S7G1]::InJob($child,$job);openJobErrorBeforeClose=[S7G1]::OpenJobError('JOB');childStdHandles='NUL-only handle list'}
  while($true) {
    $read=[Console]::In.ReadLineAsync()
    if(-not $read.Wait(120000)) { Say @{phase='self-deadline'}; break }
    $line=$read.Result
    if($null -eq $line -or $line -eq 'exit') { break }
    if($line -eq 'close') {
      [void][DevboxObservation]::CloseHandle($job); $job=[IntPtr]::Zero
      Say @{phase='closed';openJobErrorAfterClose=[S7G1]::OpenJobError('JOB')}
    }
  }
} finally {
  if($job -ne [IntPtr]::Zero) { [void][DevboxObservation]::CloseHandle($job) }
  $listener.Stop()
  [S7G1]::Release($child)  # Only this handle; the child keeps running until its release event.
}
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--worktree', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--out', required=True)
    parser.add_argument('--evidence-root')
    args = parser.parse_args()
    script, binding = probe_script(args.worktree, args.source_sha)
    if not Path(args.out).resolve().is_relative_to(evidence_root(args.evidence_root, create=False) / 'runs'):
        parser.error('--out must be this run\'s evidence directory under <evidence-root>/runs (changes §4)')
    owner = require_lock()
    nonce = uuid.uuid4().hex
    job = 'Local\\Devbox.Observe.' + nonce
    event = 'Local\\Devbox.S7G1.' + nonce + '.release'
    run_dir = Path(args.out) / ('g1-' + args.source_sha[:8] + '-' + nonce[:8])
    run_dir.mkdir(parents=True, mode=0o700)
    child_b64 = base64.b64encode(CHILD.replace('EVENT', event).encode('utf-16-le')).decode()
    root_script = native_class_source(args.worktree, args.source_sha) + '\n' + HELPER + ROOT.replace('JOB', job).replace('CHILD', child_b64).replace('EVENT', event)
    result = {'case': 'G1', 'sourceSha': args.source_sha, 'probeBinding': binding, 'harnessSha256': harness_hashes('g1_live_member.py'), 'lockOwnerPid': owner, 'cgroup': cgroup_evidence(),
              'nonce': nonce, 'jobName': job, 'releaseEvent': event, 'started': time.time(), 'steps': [], 'cleanup': [],
              'owned': {}, 'status': 'RUNNING'}
    save = lambda: dump(run_dir / 'g1.json', result)
    save()
    root = None
    identities = []
    try:
        root = LineProcess(root_script)
        started = root.read_json(0, 60)
        if started.get('phase') != 'started':
            raise RuntimeError('Owned root did not start: %r' % started)
        result['owned'] = {'root': {'pid': started['pid'], 'creationTime': started['creationTime']},
                           'child': {'pid': started['childPid'], 'creationTime': started['childCreationTime']},
                           'listenerPort': started['port']}
        identities = [('root', started['pid'], started['creationTime']), ('child', started['childPid'], started['childCreationTime'])]
        result['rootStarted'] = started
        save()
        assigned = assigned_receipt(nonce, started['pid'], started['creationTime'], started['sid'], started['sessionId'])

        def step(name, expect_obs, expect_listen, extra=None):
            row = {'step': name, 'observation': run_probe(script, {'type': 'observation', 'assigned': assigned}),
                   'listeners': run_probe(script, {'type': 'listeners', 'assigned': assigned, 'ports': [started['port']]}),
                   'identities': identity_states(identities), 'expected': {'observation': expect_obs, 'listeners': expect_listen}}
            if extra:
                row.update(extra)
            result['steps'].append(row)
            save()
            return row

        p1 = step('P1-control-root-holds-job', ['active'], ['owned'])
        root.send('close')
        closed = root.read_json(1, 30)
        p2 = step('P2-root-alive-job-handle-closed', ['identityMismatch'], ['identityMismatch'], {'rootReport': closed})
        root.send('exit')
        exit_started = time.monotonic()
        root_exit = root.close(30)
        result['rootExitWaitSeconds'] = round(time.monotonic() - exit_started, 3)
        p3 = step('P3-root-exited-child-member-alive', ['notFound'], ['notFound'], {'rootExit': root_exit})
        try:
            released = ps_command("$e=[System.Threading.EventWaitHandle]::OpenExisting('%s');[void]$e.Set();$e.Dispose();@{released=$true}|ConvertTo-Json -Compress" % event, timeout=30)
        except Exception as error:
            released = {'released': False, 'error': str(error)[:400]}
        result['childRelease'] = released
        deadline = time.monotonic() + 20
        while True:
            states = identity_states(identities)
            if states.get('child') != 'alive' or time.monotonic() >= deadline:
                break
            time.sleep(1)
        p4 = step('P4-all-owned-released', ['notFound', 'empty'], ['notFound', 'empty'])

        def raw(row, key):
            return row[key]['rawState']
        control = (raw(p1, 'observation') == 'active' and p1['observation']['reply']['state'].get('count', 0) >= 2
                   and raw(p1, 'listeners') == 'owned' and started['childInJob'] == 'member'
                   and started['openJobErrorBeforeClose'] == 0 and p1['identities'] == {'root': 'alive', 'child': 'alive'})
        name_vanished = closed.get('openJobErrorAfterClose') == 2
        root_alive_p2 = p2['identities'].get('root') == 'alive'
        if not control:
            branch = 'FAIL'
            branch_reason = 'Control did not prove an active job with the owned child as member'
        elif not name_vanished and raw(p2, 'observation') == 'active':
            branch = 'NOT_RUN'
            branch_reason = 'Job name persisted after the root closed its handle; the STILL_ACTIVE(259) branch was not reached'
        elif name_vanished and root_alive_p2 and raw(p2, 'observation') == 'identityMismatch' and raw(p2, 'listeners') == 'identityMismatch':
            branch = 'PASS'
            branch_reason = 'Root alive (exit code 259) with job name gone -> identityMismatch for observation and listeners'
        else:
            branch = 'FAIL'
            branch_reason = 'Unexpected P2 states'
        child_alive_p2 = p2['identities'].get('child') == 'alive'
        child_alive_p3 = p3['identities'].get('child') == 'alive'
        result['i1Premise'] = {
            'jobNameGoneWhileMembersAlive': bool(name_vanished and child_alive_p2 and root_alive_p2),
            'productSaysNotFoundWhileDescendantAlive': bool(raw(p3, 'observation') == 'notFound' and child_alive_p3),
            'interpretation': ('CONFIRMED: the named job is unobservable once the last job handle closes, while its member processes still run; '
                               'after the root exits the product reports notFound for a live descendant (I1 stays release-gating)')
            if name_vanished and child_alive_p3 and raw(p3, 'observation') == 'notFound' else 'NOT_CONFIRMED (see steps)',
        }
        final_ok = raw(p4, 'observation') in ('notFound', 'empty') and all(v != 'alive' for v in p4['identities'].values())
        result['verdicts'] = {
            'control': 'PASS' if control else 'FAIL',
            'identityMismatchBranch': {'status': branch, 'reason': branch_reason},
            'reaped': 'PASS' if final_ok else 'FAIL',
            'budgets': {'allWithinProductBudget': all(r[k]['withinBudget'] for r in result['steps'] for k in ('observation', 'listeners')
                                                      if r[k]['rawState'] is not None)},
        }
        result['status'] = 'PASS' if control and branch == 'PASS' and final_ok else ('NOT_RUN' if branch == 'NOT_RUN' and control else 'FAIL')
    except Exception as error:
        result['status'] = 'FAIL'
        result['error'] = '%s: %s' % (type(error).__name__, str(error)[:1500])
    finally:
        if root is not None:
            result['rootExitCode'] = root.close(20)
            result['rootStderr'] = root.stderr[-20:]
            result['rootStdout'] = root.lines[-10:]
        try:
            ps_command("try{$e=[System.Threading.EventWaitHandle]::OpenExisting('%s');[void]$e.Set();$e.Dispose();@{set=$true}|ConvertTo-Json -Compress}catch{@{set=$false}|ConvertTo-Json -Compress}" % event, timeout=30)
        except Exception as error:
            result['cleanup'].append({'action': 'release-event', 'status': 'FAIL', 'error': str(error)[:400]})
        for label, pid, creation in identities:
            try:
                states = identity_states([(label, pid, creation)])
                if states.get(label) == 'alive':
                    time.sleep(3)
                    states = identity_states([(label, pid, creation)])
                if states.get(label) == 'alive':
                    stopped = stop_exact(label, pid, creation)
                    result['cleanup'].append({'action': 'exact-stop', 'label': label, 'status': 'PASS' if stopped['result'] in ('stopped', 'absent') or stopped['result'].startswith('already-exited') else 'FAIL', 'result': stopped})
                else:
                    result['cleanup'].append({'action': 'verify-gone', 'label': label, 'status': 'PASS', 'state': states.get(label)})
            except Exception as error:
                result['cleanup'].append({'action': 'verify-gone', 'label': label, 'status': 'FAIL', 'error': str(error)[:400]})
        result['retainedResources'] = [row['label'] for row in result['cleanup'] if row.get('status') == 'FAIL' and 'label' in row]
        if result['retainedResources'] or any(row.get('status') == 'FAIL' for row in result['cleanup']):
            result['status'] = 'FAIL'
            result['cleanupFailure'] = True
        result['ended'] = time.time()
        save()
    print(json.dumps({'status': result['status'], 'evidence': str(run_dir / 'g1.json'), 'verdicts': result.get('verdicts'),
                      'i1Premise': result.get('i1Premise'), 'error': result.get('error')}, indent=2))
    return 0 if result['status'] in ('PASS', 'NOT_RUN') else 1


if __name__ == '__main__':
    sys.exit(main())
