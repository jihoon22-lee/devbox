import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from concurrent.futures import ThreadPoolExecutor
import unittest

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / 'scripts/verify-resources.py'

class VerificationLocks(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        subprocess.run(['git', 'init', '-q', self.root], check=True)
        self.state = self.root / '.git/devbox-verification'
        self.state.mkdir()
        self.env = {k: v for k, v in os.environ.items() if not k.startswith('DEVBOX_VERIFY_')}
        self.env.update(DEVBOX_VERIFY_CGROUP='off', DEVBOX_VERIFY_WAIT_SECONDS='0.3')

    def run_child(self, *options, cwd=None):
        return subprocess.run([sys.executable, str(RUNNER), *options, '--', sys.executable, '-c',
                               'from pathlib import Path; Path("child").touch()'], cwd=cwd or self.root,
                              env=self.env, capture_output=True, text=True, timeout=5)

    def hold(self, name='lock'):
        lock = (self.state / name).open('w')
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.addCleanup(lock.close)
        return lock

    def test_wait_timeout_never_launches_child(self):
        self.hold()
        result = self.run_child('--wait')
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertIn('검사 대기 초과', result.stderr)
        self.assertFalse((self.root / 'child').exists())

    def test_wait_runs_after_release(self):
        lock = self.hold()
        timer = threading.Timer(0.1, lambda: fcntl.flock(lock, fcntl.LOCK_UN))
        timer.start()
        self.addCleanup(timer.join)
        result = self.run_child('--wait')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / 'child').exists())

    def test_named_and_unlocked_reports_preserve_default_report(self):
        self.hold()
        report = self.state / 'latest.json'
        report.write_text('preserve')
        with ThreadPoolExecutor(max_workers=2) as workers:
            results = list(workers.map(lambda options: self.run_child(*options),
                                      [('--lock', 'windows-acceptance'), ('--no-lock',), ('--no-lock',)]))
        for result in results:
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(report.read_text(), 'preserve')
        reports = list(self.state.glob('run-*.json'))
        self.assertEqual(len(reports), 3)
        self.assertTrue(all(json.loads(path.read_text())['exit_code'] == 0 for path in reports))

    def test_default_and_named_contention_and_invalid_options_launch_nothing(self):
        self.hold()
        self.hold('windows-acceptance.lock')
        for options, expected in [((), 75), (('--lock', 'windows-acceptance'), 75),
                                  (('--no-lock', '--wait'), 2), (('--no-lock', '--lock', 'x'), 2),
                                  (('--lock', '../escape'), 2), (('--lock', ''), 2)]:
            result = self.run_child(*options)
            self.assertEqual(result.returncode, expected, result.stderr)
            self.assertFalse((self.root / 'child').exists())

    def test_wait_deadline_cannot_exceed_one_hour_or_be_nonpositive(self):
        for deadline in ['0', '-1', '3601', 'nan', 'inf']:
            self.env['DEVBOX_VERIFY_WAIT_SECONDS'] = deadline
            self.assertEqual(self.run_child('--wait').returncode, 2)
            self.assertFalse((self.root / 'child').exists())

    def test_wrappers_forward_once_and_full_checks_cannot_skip_common_lock(self):
        scripts = self.root / 'scripts'
        scripts.mkdir()
        for name in ['check.sh', 'e2e.sh', 'dogfood.sh', 'verify-entry.sh']:
            (scripts / name).write_bytes((ROOT / 'scripts' / name).read_bytes())
        (self.root / '.cargo').mkdir()
        (self.root / '.cargo/env').touch()
        stub = self.root / 'python3'
        stub.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$HOME/args"\n')
        stub.chmod(0o700)
        env = dict(self.env, HOME=str(self.root), PATH=str(self.root) + os.pathsep + self.env['PATH'])
        for entry, options, expected in [
            ('check', [], ['scripts/verify-resources.py', '--wait', '--', 'bash', 'scripts/check-inner.sh']),
            ('e2e', ['--wait', '--', 'runs.spec.ts'], ['scripts/verify-resources.py', '--wait', '--wait', '--', 'pnpm', '--filter', 'app', 'exec', 'playwright', 'test', 'runs.spec.ts']),
            ('dogfood', ['fixture'], ['scripts/dogfood.py', 'fixture']),
            ('dogfood', ['--wait', '--lock', 'windows-acceptance', '--', 'fixture'], ['scripts/verify-resources.py', '--wait', '--lock', 'windows-acceptance', '--', 'python3', 'scripts/dogfood.py', 'fixture']),
        ]:
            result = subprocess.run(['bash', str(scripts / f'{entry}.sh'), *options], env=env, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((self.root / 'args').read_text().splitlines(), expected)
        for entry in ['check', 'e2e']:
            for options in [('--no-lock',), ('--lock', 'windows-acceptance')]:
                (self.root / 'args').unlink(missing_ok=True)
                result = subprocess.run(['bash', str(scripts / f'{entry}.sh'), *options], env=env, capture_output=True)
                self.assertEqual(result.returncode, 2)
                self.assertFalse((self.root / 'args').exists())

    def test_limits_exit_and_metrics(self):
        probe = 'import os; assert len(os.sched_getaffinity(0)) <= 4; assert os.getpriority(os.PRIO_PROCESS, 0) >= 10; assert os.environ["CARGO_BUILD_JOBS"] == "2"; assert os.environ["RUST_TEST_THREADS"] == "2"; assert os.environ["VITEST_MAX_WORKERS"] == "2"; raise SystemExit(7)'
        result = subprocess.run([sys.executable, str(RUNNER), '--', sys.executable, '-c', probe], cwd=self.root,
                                env={k: v for k, v in self.env.items() if not k.startswith('DEVBOX_VERIFY_')}, capture_output=True)
        self.assertEqual(result.returncode, 7, result.stdout + result.stderr)
        report = json.loads((self.state / 'latest.json').read_text())
        self.assertEqual(report['exit_code'], 7)
        self.assertIn('sampled_group_rss_peak_bytes', report)

    def test_common_dir_lock_is_nonblocking(self):
        subprocess.run(['git', '-C', self.root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '--allow-empty', '-qm', 'fixture'], check=True)
        linked = self.root / 'linked'
        subprocess.run(['git', '-C', self.root, 'worktree', 'add', '-q', '--detach', str(linked)], check=True)
        self.hold()
        result = self.run_child(cwd=linked)
        self.assertEqual(result.returncode, 75, result.stderr)
        self.assertFalse((linked / 'child').exists())

    def test_check_collects_failures_and_does_not_reenter_supervisor(self):
        root = self.root
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
