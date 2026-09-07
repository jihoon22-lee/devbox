#!/usr/bin/env python3
"""Bound local verification and record sampled process/cgroup resource evidence."""
from __future__ import annotations

import fcntl
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
        values = {}
        for file, key in [('memory.peak', 'cgroup_memory_peak_bytes'),
                          ('memory.swap.current', 'cgroup_swap_current_bytes')]:
            text = (root / file).read_text().strip()
            if text.isdecimal():
                values[key] = int(text)
        for line in (root / 'cpu.stat').read_text().splitlines():
            key, value = line.split()
            if key == 'usage_usec':
                values['cgroup_cpu_usec'] = int(value)
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
        'exit_code': 128 + interrupted if interrupted else (code if code >= 0 else 128 - code),
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


def supervise(command: list[str]) -> int:
    profile = os.environ.get('DEVBOX_VERIFY_PROFILE', 'ci' if os.environ.get('CI') == 'true' else 'local')
    if profile == 'ci':
        return subprocess.call(command)
    if profile != 'local':
        raise ValueError('DEVBOX_VERIFY_PROFILE must be local or ci')
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
    with (state / 'lock').open('a+') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            print('Another verification is running for this repository (including linked worktrees).', file=sys.stderr)
            return 75
        report = state / 'latest.json'
        report.unlink(missing_ok=True)  # A failed/killed run must not display a stale successful report.
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
            'DEVBOX_VERIFY_RUST_TEST_THREADS': str(budget['test_threads']),
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
            try:
                os.killpg(child.pid, signum)
            except ProcessLookupError:
                pass

        for signum in (signal.SIGINT, signal.SIGTERM):
            signal.signal(signum, forward)
        try:
            while child.poll() is None:
                if deadline is not None and time.monotonic() >= deadline:
                    try:
                        os.killpg(child.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    break
                time.sleep(0.1)
            code = child.wait()
        finally:
            # Keep the repository lock until the command's remaining descendants are stopped.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            if enforced:
                try:
                    subprocess.run(['systemctl', '--user', 'kill', '--signal=KILL', unit + '.scope'],
                                   capture_output=True, timeout=5)
                except (OSError, subprocess.TimeoutExpired) as error:
                    print(f'Could not confirm scope cleanup: {error}', file=sys.stderr)
        code = 128 + interrupted if interrupted else (code if code >= 0 else 128 - code)
        if code and not report.exists():
            report.write_text(json.dumps({'exit_code': code, 'budget': budget,
                                          'measurement_incomplete': True}) + '\n')
        print(f'Verification report: {report}', flush=True)
        return code


def main() -> int:
    args = sys.argv[1:]
    if args[:1] == ['--measure']:
        return measure(args[4:], Path(args[1]), json.loads(args[2]))
    if args[:1] != ['--'] or len(args) < 2:
        raise ValueError('usage: verify-resources.py -- command [args...]')
    return supervise(args[1:])


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f'Verification resource setup failed: {error}', file=sys.stderr)
        sys.exit(2)
