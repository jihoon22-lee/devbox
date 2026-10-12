import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SUPERVISOR = ROOT / 'scripts/verify-resources.py'


class Verification(unittest.TestCase):
    def test_limits_exit_and_metrics(self):
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run(['git', 'init', '-q', directory], check=True)
            probe = 'import os; assert len(os.sched_getaffinity(0)) <= 4; assert os.getpriority(os.PRIO_PROCESS, 0) >= 10; assert os.environ["CARGO_BUILD_JOBS"] == "2"; assert os.environ["RUST_TEST_THREADS"] == "2"; assert os.environ["VITEST_MAX_WORKERS"] == "2"; raise SystemExit(7)'
            result = subprocess.run([sys.executable, str(SUPERVISOR), '--', sys.executable, '-c', probe], cwd=directory, env={key: value for key, value in os.environ.items() if not key.startswith('DEVBOX_VERIFY_')}, capture_output=True)
            self.assertEqual(result.returncode, 7, result.stdout + result.stderr)
            report = json.loads((Path(directory) / '.git/devbox-verification/latest.json').read_text())
            self.assertEqual(report['exit_code'], 7)
            self.assertIn('sampled_group_rss_peak_bytes', report)

    def test_common_dir_lock_is_nonblocking(self):
        with tempfile.TemporaryDirectory() as directory:
            subprocess.run(['git', 'init', '-q', directory], check=True)
            subprocess.run(['git', '-C', directory, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '--allow-empty', '-qm', 'fixture'], check=True)
            linked = Path(directory) / 'linked'
            subprocess.run(['git', '-C', directory, 'worktree', 'add', '-q', '--detach', str(linked)], check=True)
            state = Path(directory) / '.git/devbox-verification'
            state.mkdir()
            with (state / 'lock').open('w') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                result = subprocess.run([sys.executable, str(SUPERVISOR), '--', sys.executable, '-c', 'raise SystemExit(99)'], cwd=linked, capture_output=True, timeout=5)
                self.assertEqual(result.returncode, 75, result.stderr)


    def test_check_collects_failures_and_does_not_reenter_supervisor(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            log = root / 'commands'
            for command in ['cargo', 'pnpm', 'python3', 'bash']:
                stub = root / command
                stub.write_text('#!/bin/sh\nprintf "%s\\n" "$0 $*" >> "$COMMAND_LOG"\ncase "$*" in *clippy*) exit 17;; esac\n')
                stub.chmod(0o700)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'], COMMAND_LOG=str(log))
            result = subprocess.run(['/bin/bash', str(ROOT / 'scripts/check-inner.sh')], env=env, capture_output=True)
            self.assertEqual(result.returncode, 1)
            commands = log.read_text()
            self.assertIn('clippy', commands)
            self.assertIn('playwright test', commands)
            self.assertIn('--no-fail-fast', commands)
            self.assertNotIn('verify-resources', commands)
            self.assertIn(b'Failed:', result.stderr)


if __name__ == '__main__':
    unittest.main()
