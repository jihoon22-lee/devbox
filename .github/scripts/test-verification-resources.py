#!/usr/bin/env python3
"""Exercise inherited budgets, failures, cancellation, and cross-worktree exclusion."""
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / '.github/scripts/verify-resources.py'
spec = importlib.util.spec_from_file_location('agent_metadata', ROOT / '.github/scripts/check-agent-metadata.py')
metadata = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metadata)
metadata.validate('policy:\n  allow_implicit_invocation: false\n')
for text in ('policy:\n  allow_implicit_invocation: true', 'policy:\n  \tallow_implicit_invocation: false', 'policy:\n  allow_implicit_invocation: false\ndependencies: {}', ''):
    try:
        metadata.validate(text)
    except ValueError:
        pass
    else:
        raise AssertionError('invalid metadata accepted')

with tempfile.TemporaryDirectory(prefix='devbox-verification-test-') as temporary:
    root = Path(temporary) / 'repo'
    root.mkdir()
    subprocess.run(['git', 'init', '-q', str(root)], check=True)
    subprocess.run(['git', '-C', str(root), '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid',
                    'commit', '-q', '--allow-empty', '-m', 'fixture'], check=True)
    linked = Path(temporary) / 'linked'
    subprocess.run(['git', '-C', str(root), 'worktree', 'add', '-q', '--detach', str(linked)], check=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('DEVBOX_VERIFY_')}
    env.update({'DEVBOX_VERIFY_CGROUP': 'off', 'DEVBOX_VERIFY_PROFILE': 'local'})

    def run(code, *, cwd=root, overrides=None):
        return subprocess.run([sys.executable, str(RUNNER), '--', sys.executable, '-c', code],
                              cwd=cwd, env={**env, **(overrides or {})}, capture_output=True, text=True, timeout=15)

    result = run("""import os, subprocess, sys
assert os.environ['CARGO_BUILD_JOBS']=='2'
assert os.environ['VITEST_MAX_WORKERS']=='2'
assert os.environ['DEVBOX_VERIFY_WORKSPACE_CONCURRENCY']=='1'
assert os.environ['DEVBOX_VERIFY_RUST_TEST_THREADS']=='2'
assert len(os.sched_getaffinity(0)) <= 4
assert os.getpriority(os.PRIO_PROCESS, 0) >= 10
subprocess.run([sys.executable, '-c', "import os; assert os.environ['CARGO_BUILD_JOBS']=='2'; assert len(os.sched_getaffinity(0))<=4"], check=True)
""")
    assert result.returncode == 0, result.stderr + result.stdout
    report = root / '.git/devbox-verification/latest.json'
    data = json.loads(report.read_text())
    assert data['exit_code'] == 0 and data['sampled_group_rss_peak_bytes'] > 0
    assert data['budget']['memory_enforced'] is False
    assert run('raise SystemExit(7)').returncode == 7
    assert json.loads(report.read_text())['exit_code'] == 7
    assert run('raise SystemExit(0)', overrides={'DEVBOX_VERIFY_WORKERS': '0'}).returncode == 2
    assert run('raise SystemExit(0)', overrides={'DEVBOX_VERIFY_MEMORY_HIGH_MB': '9000'}).returncode == 2
    assert run('raise SystemExit(9)', overrides={'DEVBOX_VERIFY_PROFILE': 'ci'}).returncode == 9

    fake_bin = Path(temporary) / 'bin'
    fake_bin.mkdir()
    fake_systemd = fake_bin / 'systemd-run'
    fake_systemd.write_text('#!/bin/sh\nexit 1\n')
    fake_systemd.chmod(0o755)
    unavailable = {'PATH': str(fake_bin) + os.pathsep + env['PATH'], 'DEVBOX_VERIFY_CGROUP': 'required'}
    assert run('raise SystemExit(0)', overrides=unavailable).returncode == 2
    assert run('raise SystemExit(0)', overrides={**unavailable, 'DEVBOX_VERIFY_CGROUP': 'auto'}).returncode == 0
    assert run("import os; assert os.environ['VITEST_MAX_WORKERS']=='1'; assert len(os.sched_getaffinity(0))==1",
               overrides={'DEVBOX_VERIFY_WORKERS': '1', 'DEVBOX_VERIFY_CPUS': '1'}).returncode == 0

    ready = Path(temporary) / 'ready'
    child_pid = Path(temporary) / 'child-pid'
    code = """import pathlib, subprocess, sys, time
child=subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
pathlib.Path(sys.argv[2]).write_text(str(child.pid))
pathlib.Path(sys.argv[1]).touch()
time.sleep(60)
"""
    holder = subprocess.Popen([sys.executable, str(RUNNER), '--', sys.executable, '-c', code,
                               str(ready), str(child_pid)], cwd=root, env=env,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        deadline = time.monotonic() + 10
        while not ready.exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        assert ready.exists(), 'lock holder failed to start'
        denied = run('raise SystemExit(0)', cwd=linked)
        assert denied.returncode == 75, denied.stdout + denied.stderr
        holder.send_signal(signal.SIGTERM)
        assert holder.wait(timeout=10) == 143
        pid = int(child_pid.read_text())
        status = Path(f'/proc/{pid}/stat')
        assert not status.exists() or status.read_text().rsplit(')', 1)[1].split()[0] == 'Z'
        assert run('raise SystemExit(0)', cwd=linked).returncode == 0
    finally:
        if holder.poll() is None:
            holder.terminate()
            holder.wait(timeout=10)
print('Verification resource and metadata regression tests passed')
