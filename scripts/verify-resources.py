#!/usr/bin/env python3
"""Bound local verification and record sampled process/cgroup resource evidence."""
from __future__ import annotations

import argparse
import fcntl
import re
from contextlib import nullcontext, suppress
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid

def number(name: str, default: int) -> int:
    raw = os.environ.get(name, str(default))
    if not raw.isdecimal() or not 1 <= int(raw) <= 65536:
        raise ValueError(f"{name} must be a positive integer <= 65536")
    return int(raw)

def cgroup_stats() -> dict[str, int]:
    try:
        relative = next(line[3:] for line in Path('/proc/self/cgroup').read_text().splitlines()
                        if line.startswith('0::'))
        root = Path('/sys/fs/cgroup') / relative.lstrip('/')
        values = {key: int((root / file).read_text()) for file, key in
                  [('memory.peak', 'cgroup_memory_peak_bytes'), ('memory.swap.current', 'cgroup_swap_current_bytes')]}
        cpu = dict(line.split() for line in (root / 'cpu.stat').read_text().splitlines())
        if 'usage_usec' in cpu:
            values['cgroup_cpu_usec'] = int(cpu['usage_usec'])
        return values
    except (OSError, StopIteration, ValueError):
        return {}

def group_memory(group: int) -> tuple[int, int]:
    """Sample summed RSS/Swap; shared pages may be counted more than once."""
    rss = swap = 0
    for entry in Path('/proc').iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            # comm may contain spaces and parentheses; fields after the last ')' start at state.
            fields = (entry / 'stat').read_text().rsplit(')', 1)[1].split()
            if int(fields[2]) != group:
                continue
            for line in (entry / 'status').read_text().splitlines():
                if line.startswith('VmRSS:'):
                    rss += int(line.split()[1]) * 1024
                elif line.startswith('VmSwap:'):
                    swap += int(line.split()[1]) * 1024
        except (OSError, ValueError, IndexError):
            continue
    return rss, swap

def exit_code(code, interrupted):
    return 128 + interrupted if interrupted else (code if code >= 0 else 128 - code)

def measure(command: list[str], report: Path, budget: dict) -> int:
    interrupted = 0

    def on_signal(signum, _frame):
        nonlocal interrupted
        interrupted = signum  # Outer supervisor signals the entire process group.

    for signum in (signal.SIGINT, signal.SIGTERM):
        signal.signal(signum, on_signal)
    os.nice(10)
    if hasattr(os, 'sched_getaffinity'):
        cpus = sorted(os.sched_getaffinity(0))[:budget['cpus']]
        os.sched_setaffinity(0, cpus)
    started = time.monotonic()
    peak_rss = peak_swap = 0
    child = subprocess.Popen(command)
    while True:
        rss, swap = group_memory(os.getpgrp())
        peak_rss, peak_swap = max(peak_rss, rss), max(peak_swap, swap)
        code = child.poll()
        if code is not None:
            break
        time.sleep(0.25)
    result = {
        'elapsed_seconds': round(time.monotonic() - started, 3),
        'exit_code': exit_code(code, interrupted),
        'budget': budget,
        'sampled_group_rss_peak_bytes': peak_rss,
        'sampled_group_swap_peak_bytes': peak_swap,
    }
    if budget['memory_enforced']:
        result.update(cgroup_stats())
    temporary = report.with_suffix('.tmp')
    temporary.write_text(json.dumps(result, indent=2) + '\n')
    temporary.replace(report)
    print('Verification resources: ' + json.dumps(result), flush=True)
    return result['exit_code']

def supervise(command: list[str], wait=False, lock_name="lock", no_lock=False) -> int:
    budget = {
        'packages': number('DEVBOX_VERIFY_PACKAGES', 1),
        'workers': number('DEVBOX_VERIFY_WORKERS', 2),
        'build_jobs': number('DEVBOX_VERIFY_BUILD_JOBS', 2),
        'test_threads': number('DEVBOX_VERIFY_TEST_THREADS', 2),
        'cpus': number('DEVBOX_VERIFY_CPUS', 4),
        'memory_high_mb': number('DEVBOX_VERIFY_MEMORY_HIGH_MB', 6144),
        'memory_max_mb': number('DEVBOX_VERIFY_MEMORY_MAX_MB', 8192),
        'swap_max_mb': number('DEVBOX_VERIFY_SWAP_MAX_MB', 1024),
    }
    if budget['memory_high_mb'] > budget['memory_max_mb']:
        raise ValueError('memory high must not exceed memory max')
    mode = os.environ.get('DEVBOX_VERIFY_CGROUP', 'auto')
    if mode not in {'auto', 'required', 'off'}:
        raise ValueError('DEVBOX_VERIFY_CGROUP must be auto, required or off')
    common = Path(subprocess.check_output(
        ['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'], text=True).strip())
    state = common / 'devbox-verification'
    state.mkdir(exist_ok=True)
    timeout = float(os.environ.get('DEVBOX_VERIFY_WAIT_SECONDS', '3600'))
    if not 0 < timeout <= 3600:
        raise ValueError('wait deadline must be positive and at most 3600 seconds')
    with nullcontext() if no_lock else (state / lock_name).open('a+') as lock:
        deadline = time.monotonic() + timeout
        while lock is not None:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if not wait or time.monotonic() >= deadline:
                    print('검사 대기 초과' if wait else 'Another verification is running for this repository (including linked worktrees).', file=sys.stderr)
                    return 75
                time.sleep(min(0.1, max(0, deadline - time.monotonic())))
        report = state / ('latest.json' if lock_name == 'lock' and not no_lock else f'run-{uuid.uuid4().hex}.json')
        report.unlink(missing_ok=True)  # Never present a previous successful run as this run.
        properties = [f'MemoryHigh={budget["memory_high_mb"]}M',
                      f'MemoryMax={budget["memory_max_mb"]}M',
                      f'MemorySwapMax={budget["swap_max_mb"]}M', f'CPUQuota={budget["cpus"] * 100}%']
        scope = ['systemd-run', '--user', '--scope', '--quiet', '--collect']
        for prop in properties:
            scope += ['--property', prop]
        enforced = False
        if mode != 'off':
            try:
                probe = subprocess.run(scope + ['--', '/usr/bin/true'], capture_output=True, timeout=5)
                enforced = probe.returncode == 0
            except (OSError, subprocess.TimeoutExpired):
                pass
            if not enforced and mode == 'required':
                raise ValueError('systemd user scope unavailable; required memory limits cannot be applied')
        budget['memory_enforced'] = enforced
        print('Local verification budget: ' + json.dumps(budget), flush=True)
        if not enforced:
            print('Memory cgroup limit unavailable/disabled; worker, CPU affinity and priority limits remain active.', flush=True)
        environment = os.environ.copy()
        environment.update({
            'DEVBOX_VERIFY_WORKSPACE_CONCURRENCY': str(budget['packages']),
            'VITEST_MAX_WORKERS': str(budget['workers']),
            'CARGO_BUILD_JOBS': str(budget['build_jobs']),
            'RUST_TEST_THREADS': str(budget['test_threads']),
        })
        inner = [sys.executable, str(Path(__file__).resolve()), '--measure', str(report), json.dumps(budget), '--', *command]
        unit = 'devbox-verify-' + uuid.uuid4().hex
        invocation = scope + ['--unit', unit, '--', *inner] if enforced else inner
        child = subprocess.Popen(invocation, env=environment, start_new_session=True)
        interrupted = 0
        deadline = None

        def forward(signum, _frame):
            nonlocal interrupted, deadline
            interrupted, deadline = signum, time.monotonic() + 5
            with suppress(ProcessLookupError):
                os.killpg(child.pid, signum)

        for signum in (signal.SIGINT, signal.SIGTERM):
            signal.signal(signum, forward)
        try:
            while child.poll() is None:
                if deadline is not None and time.monotonic() >= deadline:
                    with suppress(ProcessLookupError):
                        os.killpg(child.pid, signal.SIGKILL)
                    break
                time.sleep(0.1)
            code = child.wait()
        finally:
            # Keep the repository lock until the command's remaining descendants are stopped.
            with suppress(ProcessLookupError):
                os.killpg(child.pid, signal.SIGKILL)
            if enforced:
                try:
                    subprocess.run(['systemctl', '--user', 'kill', '--signal=KILL', unit + '.scope'],
                                   capture_output=True, timeout=5)
                except (OSError, subprocess.TimeoutExpired) as error:
                    print(f'Could not confirm scope cleanup: {error}', file=sys.stderr)
        code = exit_code(code, interrupted)
        if code and not report.exists():
            report.write_text(json.dumps({'exit_code': code, 'budget': budget,
                                          'measurement_incomplete': True}) + '\n')
        print(f'Verification report: {report}', flush=True)
        return code

def main() -> int:
    args = sys.argv[1:]
    if args[:1] == ['--measure']:
        return measure(args[4:], Path(args[1]), json.loads(args[2]))
    parser = argparse.ArgumentParser()
    parser.add_argument('--wait', action='store_true')
    parser.add_argument('--lock')
    parser.add_argument('--no-lock', action='store_true')
    if '--' not in args:
        raise ValueError('usage: verify-resources.py [--wait] [--lock name | --no-lock] -- command [args...]')
    split = args.index('--')
    options = parser.parse_args(args[:split])
    if not args[split + 1:] or (options.no_lock and (options.wait or options.lock is not None)):
        raise ValueError('a command is required; --no-lock cannot be combined with --wait or --lock')
    if options.lock is not None and not re.fullmatch(r'[a-z][a-z0-9-]{0,63}', options.lock):
        raise ValueError('invalid lock name')
    name = 'lock' if options.lock in (None, 'verification') else options.lock + '.lock'
    return supervise(args[split + 1:], options.wait, name, options.no_lock)

if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f'Verification resource setup failed: {error}', file=sys.stderr)
        sys.exit(2)
