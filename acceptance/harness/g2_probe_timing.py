#!/usr/bin/env python3
"""G2: cold timing of the exact product Listeners/Observation probes against the 5 s / 3 s budgets.

Each sample is a fresh PowerShell process (PowerShell start + Add-Type compile), invoked exactly like
query_with_timeout over WSL interop. No warm-up, no dropped samples, fixed order. One owned root joins
a fresh job and listens on an owned loopback ephemeral port; it is released on stdin EOF.
"""
import argparse
import json
import statistics
import sys
import time
import uuid
from pathlib import Path

from common import (BASE, evidence_root, harness_hashes, BUDGET_SECONDS, LineProcess, assigned_receipt, cgroup_evidence, dump, identity_states,
                    native_class_source, probe_script, require_lock, run_probe, stop_exact)

ROOT = r'''
$job=[DevboxObservation]::AssociateFresh('JOB')
$listener=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0)
try {
  $listener.Start()
  [Console]::Out.WriteLine((@{pid=$PID;creationTime=[DevboxObservation]::CreationTime();sid=[DevboxObservation]::Sid();sessionId=[DevboxObservation]::Session();port=$listener.LocalEndpoint.Port} | ConvertTo-Json -Compress))
  [Console]::Out.Flush()
  $read=[Console]::In.ReadLineAsync(); [void]$read.Wait(600000)
} finally { $listener.Stop(); [void][DevboxObservation]::CloseHandle($job) }
'''


def summary(rows):
    values = [r['afterSpawnSeconds'] for r in rows]
    walls = [r['wallSeconds'] for r in rows]
    return {'n': len(rows), 'afterSpawnSeconds': values, 'wallSeconds': walls,
            'min': min(values), 'median': statistics.median(values), 'max': max(values),
            'wallMax': max(walls)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--worktree', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--samples', type=int, default=5)
    parser.add_argument('--out', required=True)
    parser.add_argument('--evidence-root')
    args = parser.parse_args()
    if not 1 <= args.samples <= 10:
        parser.error('1..10 samples')
    if not Path(args.out).resolve().is_relative_to(evidence_root(args.evidence_root, create=False) / 'runs'):
        parser.error('--out must be this run\'s evidence directory under <evidence-root>/runs (changes §4)')
    script, binding = probe_script(args.worktree, args.source_sha)
    owner = require_lock()
    nonce = uuid.uuid4().hex
    run_dir = Path(args.out) / ('g2-' + args.source_sha[:8] + '-' + nonce[:8])
    run_dir.mkdir(parents=True, mode=0o700)
    result = {'case': 'G2', 'sourceSha': args.source_sha, 'probeBinding': binding, 'harnessSha256': harness_hashes('g2_probe_timing.py'), 'lockOwnerPid': owner, 'cgroup': cgroup_evidence(),
              'nonce': nonce, 'budgetsSeconds': BUDGET_SECONDS, 'order': 'listeners x N, then observation x N; sequential; no warm-up',
              'measurement': 'afterSpawnSeconds = Popen return -> probe exit (product tokio timeout scope); wallSeconds includes interop spawn',
              'started': time.time(), 'samples': {'listeners': [], 'observation': []}, 'cleanup': [], 'status': 'RUNNING'}
    save = lambda: dump(run_dir / 'g2.json', result)
    save()
    root = None
    identity = None
    try:
        root = LineProcess(native_class_source(args.worktree, args.source_sha) + '\n' + ROOT.replace('JOB', 'Local\\Devbox.Observe.' + nonce))
        started = root.read_json(0, 60)
        identity = ('root', started['pid'], started['creationTime'])
        result['owned'] = {'root': {'pid': started['pid'], 'creationTime': started['creationTime']}, 'listenerPort': started['port']}
        assigned = assigned_receipt(nonce, started['pid'], started['creationTime'], started['sid'], started['sessionId'])
        for kind, request, expected in [('listeners', {'type': 'listeners', 'assigned': assigned, 'ports': [started['port']]}, 'owned'),
                                        ('observation', {'type': 'observation', 'assigned': assigned}, 'active')]:
            for index in range(args.samples):
                row = run_probe(script, request)
                row['index'] = index
                row['expectedState'] = expected
                row['countsAsSample'] = row['rawState'] == expected and row['exit'] == 0
                result['samples'][kind].append(row)
                save()
        verdicts = {}
        for kind in ('listeners', 'observation'):
            rows = result['samples'][kind]
            ok = all(r['countsAsSample'] and r['withinBudget'] for r in rows)
            verdicts[kind] = {'status': 'PASS' if ok else 'FAIL', 'budgetSeconds': BUDGET_SECONDS[kind], **summary(rows),
                              'overBudget': [r['index'] for r in rows if not r['withinBudget']],
                              'unexpectedState': [r['index'] for r in rows if not r['countsAsSample']]}
        result['verdicts'] = verdicts
        result['status'] = 'PASS' if all(v['status'] == 'PASS' for v in verdicts.values()) else 'FAIL'
    except Exception as error:
        result['status'] = 'FAIL'
        result['error'] = '%s: %s' % (type(error).__name__, str(error)[:1500])
    finally:
        if root is not None:
            result['rootExitCode'] = root.close(20)
            result['rootStderr'] = root.stderr[-20:]
        if identity:
            try:
                state = identity_states([identity]).get('root')
                if state == 'alive':
                    stopped = stop_exact(*identity)
                    result['cleanup'].append({'action': 'exact-stop', 'status': 'PASS' if stopped['result'] in ('stopped', 'absent') else 'FAIL', 'result': stopped})
                else:
                    result['cleanup'].append({'action': 'verify-gone', 'status': 'PASS', 'state': state})
            except Exception as error:
                result['cleanup'].append({'action': 'verify-gone', 'status': 'FAIL', 'error': str(error)[:400]})
        if any(row['status'] == 'FAIL' for row in result['cleanup']):
            result['status'] = 'FAIL'
            result['retainedResources'] = ['root']
        else:
            result['retainedResources'] = []
        result['ended'] = time.time()
        save()
    print(json.dumps({'status': result['status'], 'evidence': str(run_dir / 'g2.json'), 'verdicts': result.get('verdicts'), 'error': result.get('error')}, indent=2))
    return 0 if result['status'] == 'PASS' else 1


if __name__ == '__main__':
    sys.exit(main())
