"""S7-14 pure tests (changes §12): injected fakes and temporary folders only.

Nothing here starts a tool, tmux, systemctl, devbox, gh, PowerShell or a Windows program. Every Live and
trust_probe gets a fake HOME, so ~/.claude.json and ~/.codex/config.toml are never opened. Worktree reads
are `git show` only. The live-artifact test runs only with S7_LIVE_ARTIFACTS=1 and the controller's
<common>/devbox-verification/s7/test-binding.json.
"""
import ast
import contextlib
import copy
import hashlib
import importlib.util
import inspect
import io
import json
import os
from pathlib import Path
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import tomllib
import unittest
from unittest.mock import patch
import urllib.parse

import a4_windows_service
import agents_live
import common
import daemon_rows
import driver
import evidence
import fixture
import mcp_stdio
import teardown
import validate
import w1_windows
import wsl_daemon

BASE = Path(__file__).resolve().parent
WORKTREE = '/home/jihoon/projects/devbox-wt/v1-s7-rc2'
# rc.1 (Ruling 79 D2). A test may name it; a helper never names a SHA.
BASELINE_SHA = '28eb2a9111c83f4345e1b7ed2ae04d6849fa116e'
NONCE = '0123456789ab'
C_SHA, B_SHA = 'c' * 40, 'b' * 40
LOCAL = '/mnt/c/Users/u/AppData/Local'
spec = importlib.util.spec_from_file_location('wsl_owned_test', BASE / 'wsl-owned.py')
wsl_owned = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wsl_owned)
NO_LOCK = lambda: os.getpid()


def candidate_sha():
    return subprocess.run(['git', '-C', WORKTREE, 'show', '-s', '--format=%H', 'HEAD'], check=True, capture_output=True, text=True,
                          timeout=30).stdout.strip()


def answers(**values):
    row = {'Q0': False, 'Q1': False, 'Q2': False, 'Q3': False, 'Q4': False, 'Q5': False, 'interopBroken': True, 'source': 'user',
           'recordedBy': 'controller', 'recordedAt': '2026-11-02T12:00:00+09:00'}
    row.update(values)
    return row


def matrix(flow='wsl-fresh', cases=None, **values):
    binding = {'candidate': {'sha': C_SHA, 'version': '1.0.0-rc.2'}, 'baseline': None}
    return evidence.initial(binding, flow, cases or evidence.FLOWS[flow], answers(**values))


class FakeClock:
    """Virtual monotonic time; sleep advances it and yields to threads."""

    def __init__(self):
        self.now = 1000.0

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds
        time.sleep(0.001)


class Fake:
    def __init__(self, **kw):
        self.__dict__.update(kw)


def receipt(directory, sha, run, channel, version):
    name = ('Devbox Accept_' if channel == 'accept' else 'Devbox_') + version + '_x64-setup.exe'
    data = ('installer %s %s' % (channel, version)).encode()
    directory.mkdir(parents=True)
    (directory / name).write_bytes(data)
    (directory / 'receipt.json').write_text(json.dumps({'repository': evidence.REPO, 'workflow': evidence.WORKFLOW, 'source_sha': sha,
                                                        'run_id': str(run), 'channel': channel, 'version': version,
                                                        'installers': {name: hashlib.sha256(data).hexdigest()}}))


def make_artifacts(root, sha, run, version):
    receipt(root / 'devbox-setup', sha, run, 'prod', version)
    receipt(root / 'devbox-setup-accept', sha, run, 'accept', version)
    linux = root / 'devbox-linux'
    linux.mkdir()
    data = b'linux ' + version.encode()
    (linux / 'devbox-linux').write_bytes(data)
    (linux / 'linux-receipt.json').write_text(json.dumps({'source_sha': sha, 'sha256': hashlib.sha256(data).hexdigest()}))
    (linux / 'devbox-version.txt').write_text(version)
    return root


def fake_common_dir(tmp):
    """<tmp>/repo.git with objects/ and HEAD: a Git common dir, with devbox-verification/s7/harness inside."""
    common_dir = Path(tmp) / 'repo.git'
    (common_dir / 'objects').mkdir(parents=True)
    (common_dir / 'HEAD').write_text('ref: refs/heads/main\n')
    base = common_dir / 'devbox-verification' / 's7' / 'harness'
    base.mkdir(parents=True)
    return base, base.parent


# --- 1, 2 and §13 scratch: literals -------------------------------------------------------------------
class Literals(unittest.TestCase):
    S6 = ('19cff21e', '40305797593', 'v1/s6-release', 'v1-s6-release', 'devbox-s6-accept', 'DevboxAccept-s6', '.s6-accept-owner',
          '90-s6-owned', 's6-accept-fixture', 's6-vault', 'S6Identity', 'S6ExactStop', 'S6G1', 'S6A4')
    CANDIDATE = ('240430cc', '22731033', '40d97a83')

    def files(self):
        return [p for p in sorted(BASE.iterdir()) if p.is_file() and (p.suffix in {'.py', '.ps1'} or p.name in ('matrix-text.json', 'README.md'))
                and not p.name.startswith('test_')]

    def test_no_s6_literal(self):
        for path in self.files():
            text = path.read_text(encoding='utf-8-sig')
            for needle in self.S6:
                self.assertNotIn(needle, text, '%s holds %s' % (path.name, needle))

    def test_no_candidate_literal(self):
        for path in self.files():
            text = path.read_text(encoding='utf-8-sig')
            for needle in self.CANDIDATE:
                self.assertNotIn(needle, text, '%s holds %s' % (path.name, needle))

    def test_no_scratch_path_in_helpers(self):
        for name, text in validate.helpers().items():
            self.assertNotIn(validate.SCRATCH, text, name)


# --- 3, 5, 6: provenance --------------------------------------------------------------------------------
class Provenance(unittest.TestCase):
    def run_record(self, sha=C_SHA, run=17, branch=evidence.BRANCHES['candidate']):
        return {'repository': {'full_name': evidence.REPO}, 'head_repository': {'full_name': evidence.REPO}, 'head_branch': branch,
                'path': evidence.WORKFLOW, 'head_sha': sha, 'status': 'completed', 'conclusion': 'success', 'event': 'workflow_dispatch', 'id': run}

    def test_branches(self):
        evidence.verify_run(self.run_record(), C_SHA, 17, evidence.BRANCHES['candidate'])
        evidence.verify_run(self.run_record(branch='v1/s7-rc1-accept'), C_SHA, 17, evidence.BRANCHES['baseline'])
        refused = [('head_branch', 'v1/s7-rc1-accept'), ('conclusion', 'failure'), ('event', 'push'), ('status', 'in_progress'), ('id', 18),
                   ('head_sha', B_SHA), ('path', '.github/workflows/ci.yml'), ('repository', {'full_name': 'x/y'})]
        for field, value in refused:
            bad = dict(self.run_record(), **{field: value})
            with self.assertRaises(ValueError, msg=field):
                evidence.verify_run(bad, C_SHA, 17, evidence.BRANCHES['candidate'])
        with self.assertRaises(ValueError):
            evidence.verify_run(self.run_record(branch='v1/s7-rc2'), C_SHA, 17, 'v1/s7-rc2')  # S8-5: rc.2's branch is no longer an artifact branch

    def test_artifact_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = make_artifacts(Path(tmp) / 'rc2', C_SHA, 7, '1.0.0-rc.2')
            chain = evidence.verify_artifact_dir(root, C_SHA, 7, '1.0.0-rc.2')
            self.assertEqual(chain['installerName'], 'Devbox Accept_1.0.0-rc.2_x64-setup.exe')
            self.assertEqual((chain['prod']['channel'], chain['accept']['channel'], chain['linux']['version']), ('prod', 'accept', '1.0.0-rc.2'))
            for args in [(root, C_SHA, 7, '1.0.0-rc.1'), (root, B_SHA, 7, '1.0.0-rc.2'), (root, C_SHA, 8, '1.0.0-rc.2')]:
                with self.assertRaises(ValueError, msg=repr(args[1:])):
                    evidence.verify_artifact_dir(*args)
            with self.assertRaises(ValueError):
                evidence.verify_receipt(root / 'devbox-setup', C_SHA, 7, channel='accept')
            with self.assertRaises(ValueError):
                evidence.verify_receipt(root / 'devbox-setup-accept', C_SHA, 7, channel='prod')
            digest = evidence.artifact_digest(root)
            prod = next((root / 'devbox-setup').glob('*.exe'))
            prod.write_bytes(prod.read_bytes() + b'x')
            self.assertNotEqual(evidence.artifact_digest(root), digest)
            (root / 'extra').mkdir()
            with self.assertRaises(ValueError):
                evidence.verify_artifact_dir(root, C_SHA, 7, '1.0.0-rc.2')
            rc1 = make_artifacts(Path(tmp) / 'rc1', B_SHA, 5, '1.0.0-rc.1')
            self.assertEqual(evidence.verify_artifact_dir(rc1, B_SHA, 5, '1.0.0-rc.1')['version'], '1.0.0-rc.1')

    @unittest.skipUnless(os.environ.get('S7_LIVE_ARTIFACTS') == '1', 'live artifacts: set S7_LIVE_ARTIFACTS=1 with s7/test-binding.json')
    def test_live_artifacts(self):
        binding_file = common.verification_root() / 's7' / 'test-binding.json'
        if not binding_file.is_file():
            self.skipTest('s7/test-binding.json absent')
        binding = json.loads(binding_file.read_text())
        for side in ('candidate', 'baseline'):
            row = binding.get(side)
            if not row:
                continue
            directory = common.verification_root() / 's7' / evidence.ARTIFACT_DIRS[side] / 'artifacts'
            chain = evidence.verify_artifact_dir(directory, row['sha'], row['runId'], row['version'])
            self.assertEqual(chain['installerSha256'], row.get('installerSha256', chain['installerSha256']))


# --- 4: evidence root -----------------------------------------------------------------------------------
class EvidenceRoot(unittest.TestCase):
    def test_root_rules(self):
        with tempfile.TemporaryDirectory() as tmp:
            base, s7 = fake_common_dir(tmp)
            s7.chmod(0o755)
            with self.assertRaises(ValueError):
                common.evidence_root(None, create=False, base=base)  # mode 755: the controller chmods 700 first
            s7.chmod(0o700)
            self.assertEqual(common.evidence_root(None, create=False, base=base), s7)
            for bad in [str(Path(tmp) / 'outside'), str(s7) + '/../s7', 'relative/s7']:
                with self.assertRaises(ValueError, msg=bad):
                    common.evidence_root(bad, create=True, base=base)
            (s7 / 'real').mkdir(mode=0o700)
            (s7 / 'link').symlink_to(s7 / 'real')
            with self.assertRaises(ValueError):
                common.evidence_root(str(s7 / 'link'), create=False, base=base)
            made = common.evidence_root(str(s7 / 'sub'), create=True, base=base)
            self.assertEqual(made.stat().st_mode & 0o777, 0o700)

    def test_stage_and_delegation_refuse_outside_runs(self):
        with tempfile.TemporaryDirectory() as tmp:
            base, s7 = fake_common_dir(tmp)
            s7.chmod(0o700)
            outside = Path(tmp) / 'evidence'
            outside.mkdir()
            root = lambda arg=None, create=True: common.evidence_root(arg, create, base=base)
            with patch.object(fixture, 'require_lock', NO_LOCK), patch.object(fixture, 'evidence_root', root), \
                    patch.object(fixture, 'inspect_accept_absent', side_effect=AssertionError('host read')):
                with self.assertRaises(ValueError):
                    fixture.stage(C_SHA, outside, 'wsl', {'drvfsAbsent': True}, root=str(s7))
                with self.assertRaises(ValueError):
                    fixture.delegate_windows(outside, C_SHA, str(s7))
            self.assertEqual(list(outside.iterdir()), [])


# --- 7: approval in two parts ---------------------------------------------------------------------------
class Approval(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)
        self.s7 = self.tmp / 's7'
        make_artifacts(self.s7 / evidence.ARTIFACT_DIRS['candidate'] / 'artifacts', C_SHA, 7, '1.0.0-rc.2')
        self.binding = self.tmp / 'source-binding.json'
        self.binding.write_text(json.dumps({'schema': 2, 'candidate': {'sha': C_SHA, 'version': '1.0.0-rc.2', 'files': {}}, 'baseline': None}))
        self.answers = self.tmp / 'answers.json'
        self.answers.write_text(json.dumps(answers()))
        self.review = self.tmp / 'review-approval.json'
        self.argv = ['--flow', 'wsl-fresh', '--candidate-sha', C_SHA, '--candidate-run', '7', '--windows-local', LOCAL,
                     '--answers', str(self.answers), '--review', str(self.review)]
        args = driver.parse(self.argv)
        sides = driver.sides_of(args)
        sides['candidate']['artifactSha256'] = evidence.artifact_digest(self.s7 / evidence.ARTIFACT_DIRS['candidate'] / 'artifacts')
        binding = evidence.approval_binding(self.binding.read_bytes(), 'wsl-fresh', args.cases, self.answers.read_bytes(), sides['candidate'], None, LOCAL)
        self.good = {'status': 'APPROVE', 'helperSha256': evidence.helper_hashes(BASE), 'binding': binding}
        self.review.write_text(json.dumps(self.good))

    def gates(self, argv=None):
        with patch.object(driver, 'BINDING_FILE', self.binding):
            return driver.gates(driver.parse(argv or self.argv), root=self.s7)

    def test_matching_approval_passes(self):
        ctx = self.gates()
        self.assertEqual(ctx['approval'], self.good['binding'])
        self.assertIsNone(ctx['approval']['baseline'])

    def test_helper_part_refuses_a_code_change(self):
        bad = copy.deepcopy(self.good)
        bad['helperSha256']['driver.py'] = '0' * 64
        self.review.write_text(json.dumps(bad))
        with self.assertRaisesRegex(ValueError, '^helperSha256:'):
            self.gates()
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / 'a.py').write_text('x = 1\n')
            review = Path(tmp) / 'r.json'
            review.write_text(json.dumps({'status': 'APPROVE', 'helperSha256': evidence.helper_hashes(tmp), 'binding': {}}))
            evidence.verify_review(tmp, review, {})
            (Path(tmp) / 'a.py').write_text('x = 2\n')
            with self.assertRaisesRegex(ValueError, '^helperSha256:'):
                evidence.verify_review(tmp, review, {})

    def test_binding_part_refuses_a_new_run_sha_or_answer(self):
        argv = list(self.argv)
        argv[argv.index('--candidate-run') + 1] = '8'
        with self.assertRaisesRegex(ValueError, '^binding:'):
            self.gates(argv)
        self.answers.write_text(json.dumps(answers(Q3=True)))
        with self.assertRaisesRegex(ValueError, '^binding:'):
            self.gates()
        self.answers.write_text(json.dumps(answers()))
        self.gates()
        argv = list(self.argv)
        argv[argv.index('--candidate-sha') + 1] = B_SHA
        with self.assertRaisesRegex(ValueError, 'Source binding differs'):
            self.gates(argv)  # the bound source still names the old SHA
        self.binding.write_text(json.dumps({'schema': 2, 'candidate': {'sha': B_SHA, 'version': '1.0.0-rc.2', 'files': {}}, 'baseline': None}))
        with self.assertRaisesRegex(ValueError, '^binding:'):
            self.gates(argv)  # rebound to a new SHA: the helper part still matches, only the binding part refuses

    def test_unbound_refuses(self):
        with patch.object(driver, 'BINDING_FILE', self.tmp / 'absent.json'):
            with self.assertRaisesRegex(ValueError, 'UNBOUND'):
                driver.gates(driver.parse(self.argv), root=self.s7)

    def test_main_refuses_before_any_host_action(self):
        guards = [patch.object(driver, 'require_lock', side_effect=AssertionError('lock')),
                  patch.object(driver, 'command', side_effect=AssertionError('host command')),
                  patch.object(driver, 'verify_source', side_effect=AssertionError('reached verify_source')),
                  patch.object(driver.os, 'chdir', side_effect=AssertionError('chdir')),
                  patch.object(driver, 'Session', side_effect=AssertionError('session')),
                  patch.object(driver, 'BINDING_FILE', self.binding),
                  patch.object(driver, 'evidence_root', lambda arg=None, create=True: self.s7)]
        for guard in guards:
            guard.start()
            self.addCleanup(guard.stop)
        self.answers.write_text(json.dumps(answers(Q1=True)))  # answers.json differs from the approved bytes
        with self.assertRaisesRegex(ValueError, '^binding:'):
            driver.main(self.argv)
        self.answers.write_text(json.dumps(answers()))
        bad = copy.deepcopy(self.good)
        bad['helperSha256']['wsl_daemon.py'] = '0' * 64  # a code change
        self.review.write_text(json.dumps(bad))
        with self.assertRaisesRegex(ValueError, '^helperSha256:'):
            driver.main(self.argv)
        self.review.write_text(json.dumps(self.good))
        argv = list(self.argv)
        argv[argv.index('--candidate-run') + 1] = '9'  # a new run
        with self.assertRaisesRegex(ValueError, '^binding:'):
            driver.main(argv)

    def test_arguments(self):
        self.assertEqual(driver.parse(self.argv).answers, str(self.answers))
        defaults = driver.parse(['--flow', 'wsl-fresh', '--candidate-sha', C_SHA, '--candidate-run', '7', '--windows-local', LOCAL])
        self.assertEqual((defaults.answers, defaults.review), (str(BASE / 'answers.json'), str(BASE / 'review-approval.json')))
        self.assertEqual(defaults.worktree, WORKTREE)
        bad = [['--flow', 'wsl-upgrade', '--candidate-sha', C_SHA, '--candidate-run', '7', '--windows-local', LOCAL],
               ['--flow', 'wsl-fresh', '--candidate-sha', C_SHA, '--candidate-run', '7', '--baseline-sha', B_SHA, '--baseline-run', '5',
                '--windows-local', LOCAL],
               ['--flow', 'wsl-fresh', '--candidate-sha', 'C' * 40, '--candidate-run', '7', '--windows-local', LOCAL],
               ['--flow', 'wsl-fresh', '--candidate-sha', C_SHA, '--candidate-run', '0', '--windows-local', LOCAL],
               ['--flow', 'wsl-fresh', '--candidate-sha', C_SHA, '--candidate-run', '7', '--windows-local', '/mnt/c/Users/u'],
               ['--flow', 'fresh', '--candidate-sha', C_SHA, '--candidate-run', '7', '--windows-local', LOCAL, '--distro', 'a b'],
               ['--flow', 'upgrade', '--candidate-sha', C_SHA, '--candidate-run', '7', '--baseline-sha', C_SHA, '--baseline-run', '5',
                '--windows-local', LOCAL, '--distro', 'Ubuntu']]
        for argv in bad:
            with self.assertRaises(SystemExit, msg=repr(argv)):
                with patch('sys.stderr'):
                    driver.parse(argv)
        up = driver.parse(['--flow', 'wsl-upgrade', '--candidate-sha', C_SHA, '--candidate-run', '7', '--baseline-sha', B_SHA,
                           '--baseline-run', '5', '--windows-local', LOCAL])
        self.assertEqual(sorted(driver.sides_of(up)), ['baseline', 'candidate'])


class CaseSelection(unittest.TestCase):
    def test_selection(self):
        self.assertEqual(driver.selected_cases('wsl-fresh'), evidence.FLOWS['wsl-fresh'])
        self.assertEqual(driver.selected_cases('fresh', 'G1,G2'), ['G1', 'G2'])
        self.assertEqual(driver.selected_cases('fresh', 'A1,A2,W1-AB,A6'), ['A1', 'A2', 'W1-AB', 'A6'])
        self.assertEqual(driver.selected_cases('wsl-upgrade', 'U1,CS5'), ['U1', 'CS5'])
        for flow, bad in [('fresh', 'W1-AB'), ('fresh', 'A2'), ('fresh', 'A2,A1'), ('fresh', 'A1,A1'), ('fresh', 'A3'), ('wsl-fresh', 'A1'),
                          ('wsl-upgrade', 'CS3'), ('upgrade', 'G1')]:
            with self.assertRaises(ValueError, msg=bad):
                driver.selected_cases(flow, bad)


# --- 8, 9: A6 poll, upgrade, reseal and Hello -----------------------------------------------------------
class NativeScripts(unittest.TestCase):
    """Static only (no Windows): the parser check runs in validate.py --powershell-parse."""
    LIFECYCLE = (BASE / 'native-lifecycle.ps1').read_text()

    def block(self, action):
        start = self.LIFECYCLE.index("if ($Action -eq '%s')" % action)
        end = self.LIFECYCLE.find("\nif ($Action -eq '", start + 1)
        return self.LIFECYCLE[start:end if end > 0 else len(self.LIFECYCLE)]

    def test_uninstall_poll_before_throw(self):
        text = self.block('uninstall')
        throw = text.index("throw ('Uninstaller left accept registration")
        self.assertLess(text.index('AddSeconds(30)'), throw)
        self.assertLess(text.index('Start-Sleep -Milliseconds 200'), throw)
        self.assertLess(text.index('registryWaitMs'), throw)
        self.assertFalse([line for line in text.splitlines() if 'Remove-Item' in line and 'Uninstall' in line])
        self.assertNotRegex(self.LIFECYCLE, r"Remove-Item[^\n]*Uninstall\\Devbox Accept")
        for name in ('state', 'removed', 'nsisKey', 'registryLeft'):
            self.assertIn('Add-Member -NotePropertyName ' + name + ' ', text)
        self.assertIn('[string]::Equals($value,$install,[StringComparison]::Ordinal)', self.LIFECYCLE)

    def test_upgrade_preconditions_before_the_installer(self):
        text = self.block('upgrade')
        start = text.index('Start-Process -FilePath $staged.setup')
        for check in ('$installs.Count -ne 1', '[string]::Equals([string]$native.install,$install', '$native.exeSha256 -ne $installs[0].exeSha256',
                      'An owned process is running', 'An unrecorded process runs the owned executable', '$staged=Check-Stage $Stage',
                      "$staged.side -ne 'candidate'"):
            self.assertLess(text.index(check), start, check)
        self.assertGreater(text.index('Second install did not change the executable'), start)
        self.assertIn('markerRecreated', text)
        self.assertIn('uninstallerReplaced', text)
        self.assertIn("Add-Member -NotePropertyName installs -NotePropertyValue (@($installs)+@($row))", text)
        stage = self.LIFECYCLE[self.LIFECYCLE.index('function Check-Stage'):self.LIFECYCLE.index('function Install-Row')]
        self.assertIn('Installer hash mismatch', stage)
        self.assertIn('\\\\(baseline|candidate)$', stage)
        install = self.block('install')
        self.assertLess(install.index('This run already installed'), install.index('Check-Stage $Stage'))

    def test_reseal_accepts_only_setup_managed_changes(self):
        with tempfile.TemporaryDirectory() as tmp:
            unit = Path(tmp) / 'user'
            unit.mkdir()
            managed = unit / 'devbox-accept@.service'
            other = unit / 'devbox-accept-extra.service'
            for item in (managed, other):
                item.write_bytes(wsl_owned.SETUP_MARKER + b'v1\n')
            sha = hashlib.sha256(wsl_owned.SETUP_MARKER + b'v1\n').hexdigest()
            m = {'token': 'owned-123456abcdef', 'sourceSha': C_SHA, 'absentBefore': [str(managed), str(other)],
                 'sealedPaths': [{'path': str(managed), 'kind': 'file', 'sha256': sha}, {'path': str(other), 'kind': 'file', 'sha256': sha}]}
            path = Path(tmp) / 'ownership.json'
            with patch.object(wsl_owned, 'UNIT', unit):
                managed.write_bytes(wsl_owned.SETUP_MARKER + b'v2\n')
                result = wsl_owned.reseal(path, copy.deepcopy(m))
                self.assertEqual([c['path'] for c in result['resealed']], [str(managed)])
                managed.write_bytes(b'unmarked\n')
                with self.assertRaises(ValueError):
                    wsl_owned.reseal(path, copy.deepcopy(m))
                managed.write_bytes(wsl_owned.SETUP_MARKER + b'v1\n')
                other.write_bytes(wsl_owned.SETUP_MARKER + b'v2\n')
                with self.assertRaises(ValueError):
                    wsl_owned.reseal(path, copy.deepcopy(m))

    def test_hello_follows_phase_version(self):
        self.assertEqual(wsl_owned.hello({'phaseVersion': '1.0.0-rc.1'})['version'], '1.0.0-rc.1')
        self.assertEqual(wsl_owned.hello({'phaseVersion': '1.0.0-rc.2'}),
                         {'type': 'hello', 'version': '1.0.0-rc.2', 'protocol': 1, 'client': 'cli', 'instance': 'accept'})
        for m in ({}, {'phaseVersion': ''}, {'phaseVersion': None}):
            with self.assertRaises(ValueError):
                wsl_owned.hello(m)

    def test_preflight_counts_nsis_key_as_accept(self):
        text = (BASE / 'native-observe.ps1').read_text()
        keys = text[text.index('$acceptKeys=@('):text.index('function OwnedShape')]
        self.assertIn("'HKCU:\\Software\\github\\Devbox Accept'", keys)
        self.assertIn("DevboxAccept-s7-", text)
        residue = text[text.index("if ($Action -eq 'residue')"):text.index("if ($Action -eq 'preflight')")]
        self.assertFalse(any(verb in residue for verb in ('Remove-', 'Set-', 'New-Item', 'Stop-')))

    def test_native_command_has_no_policy_flag(self):
        seen = {}

        def fake(argv, timeout=60, check=True):
            seen['argv'] = argv
            return Fake(stdout='{"ok":true}', returncode=0)
        stub = Fake(windows=lambda p: 'C:\\h\\' + Path(p).name)
        with patch.object(driver, 'command', fake):
            self.assertEqual(driver.Session.native(stub, 'native-lifecycle.ps1', Action='install', Distro="Ub'untu"), {'ok': True})
            with self.assertRaises(ValueError):
                driver.Session.native(stub, 'native-observe.ps1', **{'Action; rm': 'x'})
        argv = seen['argv']
        self.assertEqual(argv[:4], [common.PS, '-NoProfile', '-NonInteractive', '-Command'])
        self.assertFalse(any('ExecutionPolicy' in a or a == '-File' for a in argv))
        self.assertIn("-Distro 'Ub''untu'", argv[4])
        self.assertIn("-HelperRoot 'C:\\h\\harness'", argv[4])

    def test_console_watch_is_one_long_lived_process(self):
        seen = {}

        class Started:
            def __init__(self, script, limit=200):
                seen.update(script=script, limit=limit)
        stub = Fake(windows=lambda p: 'C:\\h\\' + Path(p).name)
        with patch.object(common, 'LineProcess', Started):
            driver.NativeOps(stub).console_watch('240f00aa-1234abcd', 3600)
        for part in ("native-observe.ps1')", "-Action 'console-watch'", "-Nonce '240f00aa-1234abcd'", "-WatchSeconds '3600'", "-IntervalMs '250'"):
            self.assertIn(part, seen['script'])
        self.assertNotIn('ExecutionPolicy', seen['script'])
        self.assertEqual(seen['limit'], 3600 * 1000 // 250 + 100)  # every sample of the longest watch is kept


class Decisions(unittest.TestCase):
    def clean(self, **extra):
        out = {'prodComparison': {'status': 'PASS'}, 'retainedResources': [], 'acceptAbsentAfter': {'status': 'PASS', 'wsl': {'absentPaths': []},
                                                                                                        'native': {'acceptAbsent': True}},
               'cleanup': [{'action': 'quit', 'status': 'PASS'}, {'action': 'windows', 'status': 'PASS'}, {'action': 'stage', 'status': 'NOT_RUN'}]}
        out.update(extra)
        return out

    def test_a6_needs_the_accept_absent_proof(self):
        self.assertEqual(driver.a6_decision(self.clean()), ('PASS', None))
        for bad in [self.clean(acceptAbsentAfter=None), self.clean(acceptAbsentAfter={'status': 'PASS', 'wsl': {}, 'native': {'acceptAbsent': False}}),
                    self.clean(acceptAbsentAfter={'status': 'PASS', 'native': {'acceptAbsent': True}}),
                    self.clean(acceptAbsentAfter={'status': 'PASS', 'wsl': {'absentPaths': []}}),
                    self.clean(acceptAbsentAfter={'status': 'FAIL'}), self.clean(prodComparison={'status': 'FAIL'}),
                    self.clean(retainedResources=['x']), self.clean(cleanup=[{'action': 'windows', 'status': 'FAIL'}]),
                    self.clean(cleanup=[{'action': 'windows', 'status': 'NOT_RUN'}])]:
            self.assertEqual(driver.a6_decision(bad)[0], 'FAIL')

    def test_m4(self):
        self.assertEqual(driver.m4_decision(self.clean(), True), ('PASS', None, None))
        status, reason, partial = driver.m4_decision(self.clean(), False)
        self.assertEqual((status, reason, partial), ('NOT_RUN', evidence.reason('R-IOP½'), ['wsl']))
        self.assertEqual(driver.m4_decision(self.clean(retainedResources=['x']), False)[0], 'FAIL')

    def test_tmux_linux_half(self):
        old, new = {'version': '3.4', 'extendedKeys': 'extended-keys off'}, {'version': '3.5a', 'extendedKeys': 'extended-keys on'}
        self.assertEqual(driver.tmux_decision(old, new), ('PASS', None))
        self.assertEqual(driver.tmux_decision(new, new)[0], 'NOT_RUN')
        self.assertEqual(driver.tmux_decision(old, old)[0], 'NOT_RUN')
        self.assertEqual(driver.tmux_decision(old, {'version': '3.5', 'extendedKeys': 'extended-keys off'})[0], 'FAIL')
        self.assertEqual(driver.tmux_decision({'version': '3.4', 'extendedKeys': 'extended-keys on'}, new)[0], 'FAIL')

    def test_k1x_default_and_w1_d_texts(self):
        recorded = []
        stub = Fake(answers=answers(), case_result=lambda *a, **k: recorded.append(a))
        driver.Session.tmux_upgrade(stub, None)
        self.assertEqual(recorded[0][:2], ('K1X-TMUXUP', 'NOT_RUN'))
        self.assertIn(evidence.ROW_TEXT['K1X-TMUXUP.default'], recorded[0][3])
        self.assertIn(evidence.ROW_TEXT['K1X-TMUXUP.key'], recorded[0][3])
        with tempfile.TemporaryDirectory() as tmp, patch.object(w1_windows, 'require_lock', NO_LOCK):
            for q4, expected in ((False, evidence.ROW_TEXT['W1-D.default']), (True, None)):
                rows = []
                w1 = w1_windows.W1(None, None, lambda *a, **k: rows.append(a), tmp, None, answers(Q4=q4), 'm' * 20, None)
                w1.run({'W1-D'})
                self.assertEqual(rows[0][:2], ('W1-D', 'NOT_RUN'))
                if expected:
                    self.assertEqual(rows[0][3], expected)
                self.assertNotIn('winget', rows[0][3])


# --- 10: wsl_daemon --------------------------------------------------------------------------------------
class WslDaemon(unittest.TestCase):
    @unittest.skipUnless(Path(WORKTREE).is_dir(), 'worktree absent')
    def test_scripts_at_both_bound_shas(self):
        for sha in (candidate_sha(), BASELINE_SHA):
            source = common.git(WORKTREE, 'show', sha + ':' + wsl_daemon.DEPLOY_CORE)
            out = wsl_daemon.scripts(WORKTREE, sha, hashlib.sha256(source).hexdigest())
            found = wsl_daemon.extract(source)
            paths = found['paths'].replace('@STORAGE@', 'devbox-accept')
            self.assertEqual(out['install'], paths + '\n' + found['install'])
            self.assertEqual(out['exec'], paths + '\nexec "$d/devbox" "$@"')
            text = source.decode()
            self.assertIn('format!("{paths}\\n{INSTALL_SCRIPT}")', text)
            self.assertIn('format!("{paths}\\nexec \\"$d/devbox\\" \\"$@\\"")', text)
            with self.assertRaises(ValueError):
                wsl_daemon.scripts(WORKTREE, sha, '0' * 64)

    def test_extract_needs_exactly_one_of_each(self):
        one = 'const PATHS_SCRIPT: &str = r#"p"#;\nconst INSTALL_SCRIPT: &str = r#"i"#;\n'
        self.assertEqual(wsl_daemon.extract(one), {'paths': 'p', 'install': 'i'})
        for bad in ('const PATHS_SCRIPT: &str = r#"p"#;\n', one + 'const INSTALL_SCRIPT: &str = r#"j"#;\n'):
            with self.assertRaises(ValueError):
                wsl_daemon.extract(bad)
        self.assertEqual(wsl_daemon.compose({'paths': 'X=@STORAGE@', 'install': 'I'}),
                         {'install': 'X=devbox-accept\nI', 'exec': 'X=devbox-accept\nexec "$d/devbox" "$@"'})

    @unittest.skipUnless(Path(WORKTREE).is_dir(), 'worktree absent')
    def test_argv_literals_in_each_sha(self):
        show = lambda sha, name: common.git(WORKTREE, 'show', sha + ':' + name).decode()
        shas = {'candidate': candidate_sha(), 'baseline': BASELINE_SHA}
        sources = {side: show(sha, wsl_daemon.DEPLOY_CORE) + '\n' + show(sha, wsl_daemon.DEPLOY_PLATFORM) for side, sha in shas.items()}
        rows = [(side, argv) for (side, _), argv in wsl_daemon.SETUP_ARGV.items()] + list(wsl_daemon.DOCTOR_ARGV.items())
        self.assertEqual(len(rows), 5)
        for side, argv in rows:
            self.assertTrue(wsl_daemon.argv_in_source(argv, sources[side]), (side, argv))
        self.assertTrue(wsl_daemon.argv_in_source(['setup', '--json', '--restart'], show(shas['candidate'], wsl_daemon.DEPLOY_CORE)))
        self.assertFalse(wsl_daemon.argv_in_source(['doctor', '--json', '--startup'], sources['baseline']))  # rc.1 has no --startup
        self.assertFalse(wsl_daemon.argv_in_source(['doctor', '--json', '--verbose'], sources['candidate']))
        for sha in shas.values():  # platform/deploy.rs: install 120 s, setup 90 s, doctor 30 s at both SHAs
            platform = show(sha, wsl_daemon.DEPLOY_PLATFORM)
            for step in ('install', 'setup', 'doctor'):
                self.assertIn('Duration::from_secs(%d)' % wsl_daemon.TIMEOUTS[step], platform, (sha, step))

    def test_environments(self):
        self.assertEqual(set(wsl_daemon.ENV_INSTALL), set(wsl_daemon.ENV_INSTALL_KEYS))
        self.assertEqual(set(wsl_daemon.ENV_SETUP), set(wsl_daemon.ENV_SETUP_KEYS))
        self.assertEqual(wsl_daemon.ENV_INSTALL['PATH'], '/usr/bin:/bin')
        self.assertEqual((wsl_daemon.ENV_SETUP['PATH'], wsl_daemon.ENV_SETUP['LANG']), ('/usr/local/bin:/usr/bin:/bin', 'C.UTF-8'))
        self.assertEqual(wsl_daemon.exec_argv({'exec': 'E'}, ['setup', '--json']), ['env', 'DEVBOX_INSTANCE=accept', 'sh', '-c', 'E', 'sh', 'setup', '--json'])

    def test_imports_nothing_native(self):
        self.assertEqual(validate.scan_wsl_modules(validate.helpers()), [])

    def test_flagged(self):
        for argv in (['bash', '-ic', 'x'], ['/bin/bash', '-i', '-c', 'x'], ['bash', '-lic', 'x'], ['bash', '-o', 'pipefail', '-i', '-c', 'x'],
                     ['claude', '--version'], ['/usr/bin/codex', '--version'], ['node', '/x/opencode', '--version']):
            self.assertIsNotNone(wsl_daemon.flagged(argv), argv)
        for argv in (['bash', '-lc', 'env -0'], ['bash', 'script.sh', '-i', '-c'], ['opencode', 'run'], ['sh', '-c', 'x'], []):
            self.assertIsNone(wsl_daemon.flagged(argv), argv)

    def test_install_refusal_and_setup_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            data = Path(tmp) / 'devbox-linux'
            data.write_bytes(b'bin')
            m = {'linuxArtifacts': {'candidate': {'path': str(data), 'sha256': hashlib.sha256(b'bin').hexdigest()}}, 'acceptBinary': '/x'}
            calls = []

            def run(argv, input=None, env=None, capture_output=True, timeout=None):
                calls.append((argv, input, env, timeout))
                return subprocess.CompletedProcess(argv, 63, b'', b'refused')
            with patch.object(wsl_daemon, 'require_lock', NO_LOCK):
                with self.assertRaisesRegex(RuntimeError, 'refused'):
                    wsl_daemon.install_binary(m, 'candidate', {'install': 'I', 'sha256': {'install': 'h'}}, run=run, home=Path(tmp))
            self.assertEqual(calls[0][0], ['sh', '-c', 'I', 'sh', m['linuxArtifacts']['candidate']['sha256']])
            self.assertEqual((calls[0][1], calls[0][2], calls[0][3]), (b'bin', {'HOME': tmp, 'PATH': '/usr/bin:/bin'}, 120))
            data.write_bytes(b'other')
            with patch.object(wsl_daemon, 'require_lock', NO_LOCK), self.assertRaises(ValueError):
                wsl_daemon.install_binary(m, 'candidate', {'install': 'I', 'sha256': {'install': 'h'}}, run=run, home=Path(tmp))

    def setup_with(self, stdout, code=0, raises=None):
        seen = {}

        def run(argv, env=None, capture_output=True, timeout=None):
            seen.update(argv=argv, env=env, timeout=timeout)
            if raises:
                raise raises
            return subprocess.CompletedProcess(argv, code, json.dumps(stdout).encode(), b'')
        with patch.object(wsl_daemon, 'require_lock', NO_LOCK), patch.object(wsl_daemon, 'env_summary', lambda m: {'present': False}):
            try:
                return wsl_daemon.setup({}, 'candidate', 'redeploy', {'exec': 'E'}, run=run), seen
            except wsl_daemon.StepFailed as error:
                return error, seen

    def test_setup_parses_steps_and_marks_env_failures(self):
        ok, seen = self.setup_with({'steps': [{'id': 'env', 'ok': True, 'message': 'ok'}, {'id': 'restart', 'ok': True, 'message': '재시작'}]})
        self.assertEqual((ok['status'], ok['restartMessage'], seen['timeout']), ('PASS', '재시작', 90))
        self.assertEqual(seen['argv'][-3:], ['setup', '--json', '--restart'])
        self.assertEqual(seen['env'], wsl_daemon.env_setup())
        env_fail, _ = self.setup_with({'steps': [{'id': 'env', 'ok': False, 'message': 'bash timed out'}]}, code=1)
        self.assertIsInstance(env_fail, wsl_daemon.StepFailed)
        self.assertEqual((env_fail.failure['step'], env_fail.failure['shellInit']), ('setup:env', 'setup-env'))
        other, _ = self.setup_with({'steps': [{'id': 'units', 'ok': False, 'message': 'denied'}]}, code=1)
        self.assertIsNone(other.failure['shellInit'])
        timeout, _ = self.setup_with(None, raises=subprocess.TimeoutExpired(['x'], 90))
        self.assertEqual((timeout.failure['step'], timeout.failure['timeout']), ('setup', True))

    def test_startup_doctor_rows(self):
        report = {'checks': [{'id': 'tool.claude', 'status': 'warn', 'message': 'x' * 500}, {'id': 'os.wslconfig', 'status': 'ok', 'message': 'secret'}]}
        run = lambda argv, env=None, capture_output=True, timeout=None: subprocess.CompletedProcess(argv, 0, json.dumps(report).encode(), b'')
        with patch.object(wsl_daemon, 'require_lock', NO_LOCK):
            row = wsl_daemon.startup_doctor({}, 'baseline', {'exec': 'E'}, run=run)
        self.assertEqual(row['argv'], ['doctor', '--json'])
        self.assertEqual(row['checks'], [{'id': 'tool.claude', 'status': 'warn', 'message': 'x' * 200}, {'id': 'os.wslconfig', 'status': 'ok'}])

    def test_drvfs_absent(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(wsl_daemon, 'WINDOWS_LOCAL', re.compile(re.escape(tmp))):
            (Path(tmp) / 'DevboxAccept-other').mkdir()
            self.assertEqual(wsl_daemon.drvfs_absent(tmp)['otherAcceptNames'], ['DevboxAccept-other'])
            for name in ('DevboxAccept-s7-ab', 'DevboxAccept-s' + '6-cd'):
                (Path(tmp) / name).mkdir()
                with self.assertRaises(RuntimeError):
                    wsl_daemon.drvfs_absent(tmp)
                (Path(tmp) / name).rmdir()
        with self.assertRaises(ValueError):
            wsl_daemon.drvfs_absent('/tmp')


# --- 11: RPC allowlist -----------------------------------------------------------------------------------
class OwnedRpc(unittest.TestCase):
    RUN = 'ab' * 16

    def m(self, **extra):
        paths = {name: '/f/' + name for name in ('P-TOOLS', 'P-CL', 'P-SUB', 'sub-src', 'P-SV')}
        ids = {name: 'id-' + name.lower() for name in ('P-TOOLS', 'P-CL', 'P-SUB', 'P-SV')}
        m = {'nonce': NONCE, 'projectPaths': paths, 'projectIdByName': ids, 'projectIds': list(ids.values()), 'agentIds': ['a1', 'a2', 'a3'],
             'clAgentIds': ['a2', 'a3'], 'runIds': [self.RUN], 'terminalIds': ['t1'], 'answers': answers(), 'a4ServiceId': None,
             'lastPreview': {'id-p-sv': {'expectedCanonicalPath': '/f/P-SV', 'expectedDigest': 'd', 'expectedScopeVersion': 3}},
             'lastDiscardCheck': {'a1': {'headOid': 'h1', 'unmergedCommits': 0, 'fingerprint': None}},
             'lastChanges': {'a2': {'headOid': 'h2', 'baseOid': 'b2'}},
             'lastCandidates': {'id-p-cl': {'worktrees': ['/f/P-CL-agents/x'], 'branches': ['s7-%s-2' % NONCE], 'offBase': [], 'submodules': []},
                                'id-p-sub': {'worktrees': [], 'branches': [], 'offBase': [], 'submodules': ['/f/P-SUB-agents/y']}},
             'agents': {'a3': {'worktreePath': '/f/P-SUB-agents/y', 'branch': 's7-%s-3' % NONCE}}, 'trustObservation': {}}
        m.update(extra)
        return m

    def create(self, **extra):
        params = {'projectId': 'id-p-tools', 'title': 's7-opencode-1', 'profile': 'opencode', 'worktree': True,
                  'prompt': agents_live.text_for(NONCE, 'P-OK'), 'branch': 's7-%s-1' % NONCE}
        params.update(extra)
        return params

    def test_owned_values_are_accepted(self):
        m = self.m()
        ok = [('system.info', {}), ('system.doctor', {}), ('projects.list', {}), ('agents.list', {}), ('agents.list', {'includeClosedHours': 24}),
              ('agents.get', {'id': 'a1'}), ('agents.usage', {'id': 'a1'}), ('agents.discard_check', {'id': 'a1'}), ('agents.changes', {'id': 'a2'}),
              ('agents.cleanup_candidates', {'projectId': 'id-p-cl'}), ('runtime.get', {'runId': self.RUN}),
              ('runtime.runs', {'projectId': 'id-p-sv', 'limit': 10}),
              ('logs.search', {'sources': [{'kind': 'run', 'runId': self.RUN}], 'query': '한글 s7', 'regex': False, 'limit': 10}),
              ('search.notes', {'text': 's7-nt1-' + NONCE, 'limit': 10}), ('projects.add', {'path': '/f/P-SV'}),
              ('projects.trust_preview', {'id': 'id-p-sv', 'operation': 'run', 'agentId': None, 'worktrees': [], 'worktree': None}),
              ('projects.trust_approve', {'id': 'id-p-sv', 'expectedCanonicalPath': '/f/P-SV', 'expectedDigest': 'd', 'expectedScopeVersion': 3}),
              ('runtime.start', {'projectId': 'id-p-sv', 'id': 's7sv1'}), ('runtime.stop', {'runId': self.RUN}), ('agents.create', self.create()),
              ('agents.create', self.create(profile='claude', title='s7-claude-1')),  # the one trust-observation task in P-TOOLS
              ('agents.send', {'id': 'a1', 'text': agents_live.text_for(NONCE, 'T-ZW1')}), ('agents.resume', {'id': 'a1'}),
              ('agents.review', {'id': 'a2'}), ('agents.merge', {'id': 'a2', 'strategy': 'squash', 'message': None, 'expect': {'headOid': 'h2', 'baseOid': 'b2'}}),
              ('agents.discard', {'id': 'a1', 'expect': {'headOid': 'h1', 'unmergedCommits': 0}}),
              ('agents.cleanup', {'projectId': 'id-p-cl', 'worktrees': ['/f/P-CL-agents/x'], 'branches': ['s7-%s-2' % NONCE]}),
              ('agents.cleanup', {'projectId': 'id-p-sub', 'worktrees': ['/f/P-SUB-agents/y'], 'branches': ['s7-%s-3' % NONCE]}),
              ('terminal.create', {'projectId': 'id-p-sub', 'cwd': '/f/P-SUB', 'title': 's7-cl3'}), ('terminal.close', {'id': 't1'})]
        for method, params in ok:
            wsl_owned.authorize(m, method, params)
        self.assertEqual({method for method, _ in ok}, wsl_owned.READ_ONLY | wsl_owned.OWNED_MUTATIONS)

    def test_foreign_or_extra_values_are_refused(self):
        m = self.m()
        bad = [('agents.get', {'id': 'foreign'}), ('agents.get', {'id': 'a1', 'x': 1}), ('agents.changes', {'id': 'a1'}), ('runtime.get', {'runId': 'cd' * 16}),
               ('runtime.runs', {'projectId': 'foreign'}), ('runtime.runs', {'projectId': 'id-p-sv', 'limit': 101}), ('agents.list', {'includeClosedHours': 169}),
               ('logs.search', {'sources': [{'kind': 'run', 'runId': self.RUN}], 'query': 'x', 'regex': True, 'limit': 10}),
               ('logs.search', {'sources': [{'kind': 'run', 'runId': self.RUN}], 'query': 'x', 'regex': False, 'limit': 10, 'before': 1}),
               ('search.notes', {'text': 'other', 'limit': 10}), ('projects.add', {'path': '/home/u/real'}),
               ('projects.trust_preview', {'id': 'id-p-sv', 'operation': 'everything'}),
               ('projects.trust_preview', {'id': 'id-p-sv', 'operation': 'run'}),  # the old two-key shape (trust_context_invalid in run 1)
               ('projects.trust_approve', {'id': 'id-p-sv', 'expectedCanonicalPath': '/f/P-SV', 'expectedDigest': 'stale', 'expectedScopeVersion': 3}),
               ('runtime.start', {'projectId': 'id-p-sv', 'id': 'web'}), ('runtime.start', {'projectId': 'id-p-sv', 'id': 's7sv1', 'worktree': 'x'}),
               ('agents.create', self.create(profile='bash')), ('agents.create', self.create(prompt='rm -rf /')),
               ('agents.create', self.create(branch='agent/s7-x')), ('agents.create', self.create(baseBranch='main')),
               ('agents.create', self.create(projectId='id-p-cl', profile='claude')),  # claude beyond P-TOOLS without Q1
               ('agents.send', {'id': 'a1', 'text': 'free text'}), ('agents.send', {'id': 'foreign', 'text': 'exit'}),
               ('agents.merge', {'id': 'a2', 'strategy': 'squash', 'message': None, 'expect': {'headOid': 'old', 'baseOid': 'b2'}}),
               ('agents.merge', {'id': 'a1', 'strategy': 'squash', 'message': None, 'expect': {'headOid': 'h2', 'baseOid': 'b2'}}),
               ('agents.discard', {'id': 'a1', 'expect': {'headOid': 'stale', 'unmergedCommits': 0}}),
               ('agents.cleanup', {'projectId': 'id-p-cl', 'worktrees': ['/f/P-CL-agents/other'], 'branches': []}),
               ('agents.cleanup', {'projectId': 'id-p-cl', 'worktrees': [], 'branches': ['main']}),
               ('agents.cleanup', {'projectId': 'id-p-sub', 'worktrees': [], 'branches': ['s7-%s-3' % NONCE]}),  # held branch without its folder
               ('agents.cleanup', {'projectId': 'id-p-sv', 'worktrees': [], 'branches': []}),
               ('terminal.create', {'projectId': 'id-p-sub', 'cwd': '/f/P-CL', 'title': 's7-cl3'}),
               ('terminal.create', {'projectId': 'id-p-sub', 'cwd': '/f/P-SUB', 'title': 's7-cl3', 'command': 'bash'}),
               ('terminal.close', {'id': 'foreign'}), ('tools.support_bundle', {}), ('secrets.set', {}), ('projects.remove', {'id': 'id-p-sv'})]
        for method, params in bad:
            with self.assertRaises(ValueError, msg=method + repr(params)):
                wsl_owned.authorize(m, method, params)
        observed = self.m(trustObservation={'claude': 'a1'})
        with self.assertRaises(ValueError):
            wsl_owned.authorize(observed, 'agents.create', self.create(profile='claude', title='s7-claude-2'))
        wsl_owned.authorize(self.m(answers=answers(Q1=True), trustObservation={'claude': 'a1'}), 'agents.create',
                            self.create(projectId='id-p-cl', profile='claude'))

    def test_replies_are_recorded(self):
        m = self.m(agentIds=[], clAgentIds=[], agents={}, runIds=[], agentWorktrees=[])
        wsl_owned.record_reply(m, 'agents.create', self.create(projectId='id-p-cl', profile='claude'),
                               {'id': 'n1', 'projectId': 'id-p-cl', 'terminalId': 't9', 'worktreePath': '/f/P-CL-agents/n1', 'branch': 'b'})
        self.assertEqual((m['agentIds'], m['clAgentIds'], m['trustObservation'], m['agentWorktrees']), (['n1'], ['n1'], {'claude': 'n1'}, ['/f/P-CL-agents/n1']))
        self.assertEqual(m['agents']['n1']['terminalIds'], ['t9'])
        with self.assertRaises(ValueError):
            wsl_owned.record_reply(m, 'agents.create', self.create(), {'id': 'n2', 'projectId': 'other'})
        foreign = 'cd' * 16
        wsl_owned.record_reply(m, 'runtime.runs', {'projectId': 'id-p-sv'}, {'items': [
            {'id': self.RUN, 'projectId': 'id-p-sv', 'unit': 'devbox-accept-run-' + self.RUN + '.service'},
            {'id': foreign, 'projectId': 'other', 'unit': 'devbox-accept-run-' + foreign + '.service'},
            {'id': '../x', 'projectId': 'id-p-sv', 'unit': 'devbox-accept-run-../x.service'}]})
        self.assertEqual(m['runIds'], [self.RUN])
        wsl_owned.record_reply(m, 'agents.cleanup_candidates', {'projectId': 'id-p-sub'}, {'worktrees': [], 'branches': [], 'submodules': ['/f/s']})
        self.assertEqual(m['lastCandidates']['id-p-sub']['submodules'], ['/f/s'])

    def test_trust_preview_context_is_checked(self):
        """Fix round 1 item 5: the app's five keys, and a context that names only owned, recorded objects."""
        m = self.m(agents={'a2': {'projectId': 'id-p-cl', 'worktreePath': '/f/P-CL-agents/x'}, 'a1': {'projectId': 'id-p-tools'}})
        ok = [common.trust_context('id-p-tools', 'create', worktree=True), common.trust_context('id-p-sv', 'run'),
              common.trust_context('id-p-cl', 'discard', agent_id='a2'), common.trust_context('id-p-cl', 'merge', agent_id='a2'),
              common.trust_context('id-p-tools', 'discard', agent_id='a1'),
              common.trust_context('id-p-cl', 'cleanup', worktrees=['/f/P-CL-agents/x'])]
        for params in ok:
            wsl_owned.authorize(m, 'projects.trust_preview', params)
        base = common.trust_context('id-p-cl', 'discard', agent_id='a2')
        bad = [{'id': 'id-p-cl', 'operation': 'discard'}, dict(base, agentId='foreign'), dict(base, agentId='a1'),  # a1 is in another project
               dict(base, worktrees=['/f/P-CL-agents/x']), dict(base, worktree=True), dict(base, extra=1),
               dict(common.trust_context('id-p-tools', 'create', worktree=True), worktree=False),
               dict(common.trust_context('id-p-tools', 'create', worktree=True), worktree=None),
               dict(common.trust_context('id-p-sv', 'run'), agentId='a2'),
               dict(common.trust_context('id-p-cl', 'cleanup', worktrees=['/f/P-CL-agents/x']), worktrees=['/f/P-CL-agents/other']),
               dict(common.trust_context('id-p-cl', 'cleanup', worktrees=['/f/P-CL-agents/x']), worktrees=[]),
               dict(common.trust_context('id-p-sub', 'run'), operation='cleanup', worktrees=['/f/P-SUB-agents/y']),  # held, not a worktree candidate
               dict(common.trust_context('id-p-sv', 'run'), operation='launch'), dict(base, operation='test'), dict(base, operation='pr'),
               dict(common.trust_context('foreign', 'run'))]
        for params in bad:
            with self.assertRaises(ValueError, msg=repr(params)):
                wsl_owned.authorize(m, 'projects.trust_preview', params)

    def test_trust_observation_pending_before_create(self):
        """M9: the observation is on disk before agents.create, so a lost reply cannot buy a second trust task."""
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'ownership.json'
            common.dump(path, self.m(agentIds=[], agents={}))
            seen = []

            def lost(m, method, params):
                seen.append(json.loads(path.read_text()).get('trustObservation'))
                raise TimeoutError('RPC whole-request deadline')
            with patch.object(wsl_owned, 'load_owned', lambda p: (Path(p), json.loads(Path(p).read_text()))):
                with self.assertRaises(TimeoutError):
                    wsl_owned.rpc_owned(path, 'agents.create', self.create(profile='claude', title='s7-claude-1'), call=lost)
                self.assertEqual(seen, [{'claude': wsl_owned.PENDING}])
                with self.assertRaises(ValueError):  # the backstop refuses the second trust-observation create
                    wsl_owned.rpc_owned(path, 'agents.create', self.create(profile='claude', title='s7-claude-2'),
                                        call=lambda *a: self.fail('called'))
                common.dump(path, self.m(agentIds=[], agents={}))
                reply = {'id': 'n1', 'projectId': 'id-p-tools', 'terminalId': 't9'}
                wsl_owned.rpc_owned(path, 'agents.create', self.create(profile='claude', title='s7-claude-1'), call=lambda *a: reply)
                self.assertEqual(json.loads(path.read_text())['trustObservation'], {'claude': 'n1'})
                # an opencode create writes nothing
                wsl_owned.rpc_owned(path, 'agents.create', self.create(), call=lambda *a: dict(reply, id='n2'))
                self.assertEqual(json.loads(path.read_text())['trustObservation'], {'claude': 'n1'})


# --- 12: MCP --------------------------------------------------------------------------------------------
class Mcp(unittest.TestCase):
    def test_tool_names_and_texts(self):
        m = {'agentIds': ['a1'], 'nonce': NONCE}
        mcp_stdio.authorize_call(m, 'devbox_agents', {})
        mcp_stdio.authorize_call(m, 'devbox_agent_send', {'id': 'a1', 'text': agents_live.text_for(NONCE, 'T-FF')})
        for name, arguments in [('devbox_agents', {'x': 1}), ('devbox_agent_send', {'id': 'other', 'text': 'exit'}),
                                ('devbox_agent_send', {'id': 'a1', 'text': 'anything'}), ('devbox_agent_send', {'id': 'a1', 'text': 'exit', 'x': 1}),
                                ('devbox_agent_stop', {'id': 'a1'}), ('devbox_run', {})]:
            with self.assertRaises(ValueError, msg=name):
                mcp_stdio.authorize_call(m, name, arguments)
        self.assertEqual(mcp_stdio.TOOLS, ('devbox_agents', 'devbox_agent_send'))
        self.assertEqual(mcp_stdio.argv_for('/b/devbox')[1:], ['--instance', 'accept', 'mcp'])
        self.assertEqual(set(mcp_stdio.environment('/h')), {'HOME', 'PATH', 'XDG_RUNTIME_DIR'})

    def test_rc1_binary_must_hash_to_the_baseline_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp) / '.local/share/devbox-accept/bin'
            directory.mkdir(parents=True)
            data = b'rc1 binary'
            sha = hashlib.sha256(data).hexdigest()
            target = directory / ('devbox-' + sha)
            target.write_bytes(data)
            target.chmod(0o700)
            fd, path = mcp_stdio.open_baseline(tmp, sha)
            os.close(fd)
            self.assertEqual(path, str(target))
            target.write_bytes(b'changed')
            with self.assertRaises(ValueError):
                mcp_stdio.open_baseline(tmp, sha)
            with self.assertRaises(FileNotFoundError):
                mcp_stdio.open_baseline(tmp, '0' * 64)


# --- 13, 20: Pane and the Enter guard -------------------------------------------------------------------
class FakeTmux:
    def __init__(self):
        self.calls = []
        self.screen = ''
        self.buffers = ''

    def __call__(self, argv, capture_output=True, timeout=10):
        self.calls.append(list(argv))
        sub = argv[3]
        out = self.screen if sub == 'capture-pane' else self.buffers if sub == 'list-buffers' else ''
        return subprocess.CompletedProcess(argv, 0, out.encode(), b'')

    def sent(self):
        return [c[3:] for c in self.calls if c[3] in agents_live.TMUX_MUTATE]


CLAUDE_TRUST = ' Accessing workspace:\n /f/P-TOOLS\n ❯ No, exit\n   Yes, I trust this folder\n'
CLAUDE_TRUST_YES = ' Accessing workspace:\n /f/P-TOOLS\n   No, exit\n ❯ Yes, I trust this folder\n'
CODEX_TRUST = ' Folder access\n › 1. Trust and continue  \n   2. Back to Agent Command Center\n'
CODEX_RESTRICTED = ' Folder access\n › 1. Open restricted\n   2. Back to Agent Command Center\n'
APPROVAL = (' Bash command\n   touch s7-approved\n Do you want to proceed?\n ❯ 1. Yes\n'
            "   2. Yes, and don't ask again for touch commands\n   3. No, and tell Claude what to do differently (esc)\n")


class PaneGuards(unittest.TestCase):
    def pane(self, **values):
        self.tmux = FakeTmux()
        self.m = {'terminalIds': ['t1'], 'agents': {'a1': {'terminalIds': ['t2']}}, 'agentWorktrees': ['/f/P-SUB-agents/y']}
        return agents_live.Pane(lambda: self.m, answers(**values), NONCE, run=self.tmux, clock=FakeClock())

    def test_socket_and_target(self):
        pane = self.pane()
        pane.capture('t1')
        pane.display('#{version}')
        for tid in ('t1;x', 'foreign', '=t1:', None):
            with self.assertRaises(agents_live.Refused):
                pane.capture(tid)
        with self.assertRaises(agents_live.Refused):
            pane.display('#{pane_pid}')
        # The socket is always `-L devbox-accept` and the subcommand follows at once, so no global `-S <path>` can appear
        # (capture-pane's own `-S -60` is its start line, not a socket).
        self.assertTrue(all(c[:3] == ['/usr/bin/tmux', '-L', 'devbox-accept'] for c in self.tmux.calls))
        self.assertTrue(all(c[3] in agents_live.TMUX_READ + agents_live.TMUX_MUTATE for c in self.tmux.calls))
        self.assertEqual(self.tmux.calls[0][3:], ['capture-pane', '-p', '-J', '-S', '-60', '-t', '=t1:'])
        self.assertEqual(len(self.tmux.calls), 2)  # the refused targets and the refused format ran nothing

    def test_fixed_keys_texts_and_paste(self):
        pane = self.pane()
        for text in ('rm -rf /', 'exit\n', '/goal anything'):
            with self.assertRaises(agents_live.Refused):
                pane.type_text('t1', text)
        for key in ('C-c', 'Tab', 'q'):
            with self.assertRaises(agents_live.Refused):
                pane.key('t1', key)
        with self.assertRaises(agents_live.Refused):
            pane.paste('t1', 'echo hi')
        pane.paste('t1', '!')
        self.assertEqual([c[0] for c in self.tmux.sent()], ['set-buffer', 'paste-buffer'])
        self.assertEqual(self.tmux.sent()[0][2], 's7-%s-1' % NONCE)
        for text in agents_live.KEY_TEXTS:
            pane.type_text('t1', text)
        for size in ((200, 50), (40, 24), (80, 24)):
            pane.resize('t1', *size)
        with self.assertRaises(agents_live.Refused):
            pane.resize('t1', 100, 30)

    def test_enter_rule_i(self):
        pane = self.pane()
        with self.assertRaises(agents_live.Refused):
            pane.key('t1', 'Enter', 'command')  # nothing typed
        pane.type_text('t1', '?')
        with self.assertRaises(agents_live.Refused):
            pane.key('t1', 'Enter', 'command')
        for text in ('/exit', '!echo s7'):
            pane.type_text('t1', text)
            pane.key('t1', 'Enter', 'command')
        pane.key('t1', 'Down')
        with self.assertRaises(agents_live.Refused):
            pane.key('t1', 'Enter', 'command')

    def test_cl3_command_only_for_the_recorded_folder(self):
        pane = self.pane()
        command = "git worktree remove --force '/f/P-SUB-agents/y'"
        with self.assertRaises(agents_live.Refused):
            pane.type_text('t1', command, '/f/P-SUB-agents/y')  # not in CL3
        pane.begin('CL3')
        for text, folder in ((command, '/f/P-SUB-agents/z'), ("git worktree remove --force '/f/P-SUB-agents/z'", '/f/P-SUB-agents/z'),
                             (command + '; rm -rf /', '/f/P-SUB-agents/y')):
            with self.assertRaises(agents_live.Refused):
                pane.type_text('t1', text, folder)
        pane.type_text('t1', command, '/f/P-SUB-agents/y')
        pane.key('t1', 'Enter', 'command')

    def test_trust_enter(self):
        pane = self.pane(Q1=True, Q2=True)
        self.tmux.screen = CLAUDE_TRUST
        pane.capture('t2')
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'trust-claude', 'P-TOOLS')  # cursor on "No, exit"
        pane.key('t2', 'Down')
        self.tmux.screen = CLAUDE_TRUST_YES
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'trust-claude', 'P-TOOLS')  # stale capture after Down
        pane.capture('t2')
        pane.key('t2', 'Enter', 'trust-claude', 'P-TOOLS')
        pane.capture('t2')
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'trust-claude', 'P-TOOLS')  # second ask in the same project
        self.tmux.screen = CLAUDE_TRUST_YES.replace('❯ Yes', '❯ y. Yes')
        pane.capture('t2')
        pane.key('t2', 'Enter', 'trust-claude', 'P-CL')
        self.tmux.screen = CODEX_RESTRICTED
        pane.capture('t2')
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'trust-codex', 'P-TOOLS')
        self.tmux.screen = CODEX_TRUST
        pane.capture('t2')
        pane.key('t2', 'Enter', 'trust-codex', 'P-TOOLS')
        no = self.pane(Q1=False, Q2=False)
        for screen, rule in ((CLAUDE_TRUST_YES, 'trust-claude'), (CODEX_TRUST, 'trust-codex')):
            self.tmux.screen = screen
            no.capture('t2')
            with self.assertRaises(agents_live.Refused):
                no.key('t2', 'Enter', rule, 'P-TOOLS')

    def test_approval_enter(self):
        pane = self.pane(Q5=True)
        self.tmux.screen = APPROVAL
        pane.begin('H1')
        pane.capture('t2')
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'approve')  # not P1 or U1X
        pane.begin('P1')
        for _ in range(agents_live.APPROVALS_PER_CASE):
            pane.capture('t2')
            pane.key('t2', 'Enter', 'approve')
        pane.capture('t2')
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'approve')  # at most two per case
        pane.begin('U1X')
        self.tmux.screen = APPROVAL.replace(' ❯ 1. Yes', '   1. Yes').replace("   2. Yes, and don't", " ❯ 2. Yes, and don't")
        pane.capture('t2')
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'approve')  # "don't ask again" selected
        self.tmux.screen = APPROVAL.replace('touch s7-approved', 'touch other')
        pane.capture('t2')
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'approve')
        no = self.pane(Q5=False)
        no.begin('P1')
        self.tmux.screen = APPROVAL
        no.capture('t2')
        with self.assertRaises(agents_live.Refused):
            no.key('t2', 'Enter', 'approve')

    def test_stop_patterns_end_the_case(self):
        for screen in ('Update available! Run claude update', 'Please run /login', 'Enter your API key', 'Accept the terms of service',
                       ' Trust project folder?'):
            pane = self.pane()
            self.tmux.screen = screen
            with self.assertRaises(agents_live.StopPrompt, msg=screen):
                pane.capture('t1', 'pi')
            with self.assertRaises(agents_live.Refused):
                pane.type_text('t1', '/exit')
            self.assertEqual(self.tmux.sent(), [])

    def test_restore(self):
        pane = self.pane()
        pane.resize('t1', 200, 50)
        pane.paste('t1', '!')
        self.tmux.buffers = 's7-%s-1\nbuffer0\n' % NONCE
        done = pane.restore()
        self.assertIn(['resize-window', '-t', '=t1:', '-x', '80', '-y', '24'], self.tmux.sent())
        self.assertIn(['delete-buffer', '-b', 's7-%s-1' % NONCE], self.tmux.sent())
        self.assertNotIn(['delete-buffer', '-b', 'buffer0'], self.tmux.sent())
        self.assertEqual(len(done), 2)


class RpcError(RuntimeError):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


class LiveRows(unittest.TestCase):
    def live(self, rpc, **values):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.home = Path(tmp.name) / 'home'
        self.home.mkdir()
        self.tmux = FakeTmux()
        self.m = {'nonce': NONCE, 'terminalIds': [], 'agents': {'a1': {'terminalIds': ['t2']}}, 'projectIdByName': {'P-TOOLS': 'p1'},
                  'projectPaths': {'P-TOOLS': '/f/P-TOOLS'}}
        clock = FakeClock()
        pane = agents_live.Pane(lambda: self.m, answers(**values), NONCE, run=self.tmux, clock=clock)
        self.recorded = []
        return agents_live.Live(rpc, lambda: self.m, lambda *a, **k: self.recorded.append((a, k)), Path(tmp.name), answers(**values), NONCE, pane, None,
                                clock=clock, home=self.home)

    def test_q5_false_p1_sends_only_escape(self):
        calls = []

        def rpc(method, params):
            calls.append((method, params))
            if method == 'agents.get':
                return {'id': 'a1', 'terminalId': 't2', 'state': 'waiting', 'worktreePath': '/nonexistent'}
            if method == 'agents.send':
                if params['text'] == agents_live.text_for(NONCE, 'P-APPROVE'):
                    return {'state': 'running'}
                raise RpcError('agents.awaiting_input')
            raise AssertionError(method)
        live = self.live(rpc, Q1=True, Q5=False)
        live.pane.begin('P1')
        self.tmux.screen = APPROVAL
        client = Fake(pairs=[], send=lambda agent_id, text: ('refused', '에이전트가 선택을 기다리고 있습니다. 화면에서 답하세요.'))
        parts, reasons = agents_live.approval_row(live, 'P1', {'id': 'a1'}, client)
        self.assertEqual((parts['a'], parts['b'], parts['c'], parts['d'], parts['e']), (True, True, True, None, None))
        self.assertIn(evidence.reason('R-Q5'), reasons)
        self.assertEqual(self.tmux.sent(), [['send-keys', '-t', '=t2:', 'Escape']])
        self.assertFalse(any('Enter' in c for c in self.tmux.calls))

    def test_turn_budget_refuses_before_the_call(self):
        calls = []

        def rpc(method, params):
            calls.append(method)
            if method == 'agents.send' and params['text'] == 'quit':
                raise RpcError('agents.awaiting_input')
            if method == 'projects.trust_preview':
                return {'projectId': 'p1', 'requiresApproval': False}
            return {'id': 'n1', 'projectId': 'p1', 'state': 'running'}
        live = self.live(rpc)
        mcp_sent = []
        client = Fake(pairs=[], send=lambda agent_id, text: mcp_sent.append(text) or ('accepted', 'ok'))
        for key in ('free text', 'rm -rf /'):  # not a fixed text id: refused before any call, on both paths
            with self.assertRaises(ValueError):
                live.send('X', 'pi', 'a1', key)
            with self.assertRaises(ValueError):
                live.mcp_send('X', 'pi', client, 'a1', key)
        self.assertEqual((calls, mcp_sent), ([], []))
        live.send('X', 'pi', 'a1', 'quit')
        self.assertEqual(live.budget.turns['pi'], 0)  # a refused send does not count
        live.create('X', 'pi', 'P-TOOLS', 'P-OK')
        live.send('X', 'pi', 'a1', 'P-OK')
        live.send('X', 'pi', 'a1', 'P-OK')
        live.mcp_send('X', 'pi', client, 'a1', 'P-OK')
        live.mcp_send('X', 'pi', client, 'a1', 'P-OK')
        # M8: one count for the tool across agents.create, agents.send and MCP, against one cap.
        self.assertEqual((live.budget.turns['pi'], agents_live.BUDGET['pi']), (5, 5))
        self.assertEqual(live.budget.snapshot()['paths'], {'rpc': dict(claude=0, codex=0, opencode=0, pi=3), 'mcp': dict(claude=0, codex=0, opencode=0, pi=2),
                                                           'tmux': dict(claude=0, codex=0, opencode=0, pi=0)})
        before, sent_before = list(calls), list(mcp_sent)
        for attempt in (lambda: live.send('X', 'pi', 'a1', 'P-OK'), lambda: live.mcp_send('X', 'pi', client, 'a1', 'P-OK'),
                        lambda: live.create('X', 'pi', 'P-TOOLS', 'P-OK')):
            with self.assertRaises(agents_live.BudgetExceeded):
                attempt()
        self.assertEqual((calls, mcp_sent), (before, sent_before))
        live.budget.nest('opencode')
        with self.assertRaises(agents_live.BudgetExceeded):
            live.budget.nest('opencode')

    def test_sn2_keeps_the_sends_before_the_cap(self):
        def rpc(method, params):
            if method == 'agents.list':
                return {'items': [{'id': 'a1', 'state': 'idle'}]}
            return {'id': 'a1', 'terminalId': 't2', 'state': 'idle'}
        live = self.live(rpc)
        live.pane.begin('SN2')
        mcp_sent = []
        client = Fake(pairs=[{}], send=lambda agent_id, text: mcp_sent.append(text) or ('accepted', 'ok'))
        live.budget.turns['pi'] = 2  # the pi block's create and K1X-SEND
        agents_live.sn2_part(live, 'pi', 'a1', client)
        self.assertEqual([(r['path'], r['text'], r['result']) for r in live.sn2['pi']],
                         [('agents.send', 'T-ZW1', 'accepted'), ('agents.send', 'T-ZW2', 'accepted'), ('mcp', 'T-ZW1', 'accepted'), ('mcp', 'T-ZW2', 'not sent')])
        self.assertEqual((live.budget.turns['pi'], len(mcp_sent)), (agents_live.BUDGET['pi'], 1))  # the sixth turn was never sent
        live.sn2.update(opencode=[{'result': 'accepted'}], claude=None)
        agents_live.record_sn2(live)
        (case, status, _, reason), extra = self.recorded[-1]
        self.assertEqual((case, status, extra['partial']), ('SN2', 'NOT_RUN', ['opencode']))
        self.assertIn('pi part partial: a send was over the turn budget (M8)', reason)

    def test_typed_prompt_counts_against_the_same_cap(self):
        live = self.live(lambda method, params: {'id': 'a1', 'terminalId': 't2', 'state': 'idle'})
        self.m['agents']['a1']['profile'] = 'codex'
        live.pane.begin('H2X-RESUME')
        live.budget.turns['codex'] = agents_live.BUDGET['codex'] - 1
        live.type('H2X-RESUME', 't2', '/goal Reply with OK')  # starts a model turn once Enter submits it
        self.assertEqual((live.budget.turns['codex'], live.budget.snapshot()['paths']['tmux']['codex']), (agents_live.BUDGET['codex'], 1))
        typed = len(self.tmux.sent())
        with self.assertRaises(agents_live.BudgetExceeded):
            live.type('H2X-RESUME', 't2', '/goal Reply with OK')
        self.assertEqual(len(self.tmux.sent()), typed)  # refused before tmux
        live.type('H2X-RESUME', 't2', '/quit')  # a command, not a prompt: never counted
        self.assertEqual(live.budget.turns['codex'], agents_live.BUDGET['codex'])
        self.m['agents']['a1'].pop('profile')
        with self.assertRaises(agents_live.Refused):  # a prompt for a terminal of no known tool cannot be counted: refused
            live.type('H2X-RESUME', 't2', '/goal Reply with OK')

    def test_fixed_texts(self):
        texts = agents_live.send_texts(NONCE)
        self.assertEqual(set(agents_live.prompt_texts(NONCE)), {'P-OK', 'P-APPROVE', 'P-APPROVE-X', 'P-ASK', 'P-ENV', 'P-NEST'})
        self.assertIn('Run the shell command `touch /tmp/devbox-s7-p1x-%s` (it is outside this folder), then reply OK.' % NONCE, texts)
        self.assertTrue({'exit', 'quit', ':q', 's7 line\u000cend', '\u001b[31ms7 red\u001b[0m', '\u200b!echo devbox-ok', '\u2060!echo devbox-ok'} <= texts)
        for bad in ('P-UNKNOWN', 'rm'):
            with self.assertRaises(ValueError):
                agents_live.text_for(NONCE, bad)
        with self.assertRaises(ValueError):
            agents_live.prompt_texts('not-a-nonce')

    def test_create_and_end_preview_as_the_app(self):
        """Fix round 1 item 5: create previews {create, worktree: <the create's value>}; the end previews the discard."""
        calls = []

        def rpc(method, params):
            calls.append((method, params))
            if method == 'projects.trust_preview':
                return {'projectId': 'p1', 'requiresApproval': False, 'approved': False}
            if method == 'agents.discard_check':
                return {'headOid': 'h', 'unmergedCommits': 0, 'fingerprint': None}
            return {'id': 'n1', 'projectId': 'p1', 'state': 'preparing'}
        live = self.live(rpc)
        live.create('H3', 'opencode', 'P-TOOLS', 'P-OK')
        self.assertEqual(calls[0], ('projects.trust_preview', common.trust_context('p1', 'create', worktree=True)))
        self.assertEqual(calls[0][1], {'id': 'p1', 'operation': 'create', 'agentId': None, 'worktrees': [], 'worktree': True})
        self.assertEqual(calls[1][0], 'agents.create')
        self.assertIs(calls[1][1]['worktree'], calls[0][1]['worktree'])
        del calls[:]
        live.end('H3', 'n1')
        self.assertEqual([m for m, _ in calls], ['projects.trust_preview', 'agents.discard_check', 'agents.discard'])
        self.assertEqual(calls[0][1], {'id': 'p1', 'operation': 'discard', 'agentId': 'n1', 'worktrees': [], 'worktree': None})


# --- 14: fixture -----------------------------------------------------------------------------------------
class FixtureTests(unittest.TestCase):
    def test_plan_both_modes(self):
        for mode, root in (('windows', 'C:\\dev\\devbox-s7-accept-x1'), ('wsl', None)):
            for upgrade in (False, True):
                plan = fixture.fixture_plan(C_SHA, 'owned-123456abcdef', mode, root, home=Path('/h'), upgrade=upgrade)
                fixture.validate_plan(plan)
                config = tomllib.loads(plan['configText'])
                self.assertIs(config['mcp']['write'], True)
                self.assertIs(config['agents']['cleanup_after_close'], True)
                self.assertEqual(config['agents']['profiles'], [fixture.PROFILE])
                self.assertNotIn('windows', config)
                names = set(plan['projectPaths'])
                self.assertEqual('P-U1' in names, upgrade)
                self.assertEqual({'P-WIN', 'P-A4'} <= names, mode == 'windows')
                if mode == 'windows':
                    self.assertEqual(tomllib.loads(plan['configLocalText']), {'windows': {'root': root}})
                else:
                    self.assertIsNone(plan['configLocalText'])
        plan = fixture.fixture_plan(C_SHA, 'owned-123456abcdef', 'wsl', home=Path('/h'))
        for bad in [dict(plan, configLocalText='[windows]\nroot = "C:\\\\dev\\\\devbox-s7-accept-x"\n'),
                    dict(plan, configText=plan['configText'].replace('write = true', 'write = false')),
                    dict(plan, configText=plan['configText'] + '[windows]\nroot = "C:\\\\x"\n'),
                    dict(plan, projectPaths=dict(plan['projectPaths'], extra='/e/x'))]:
            with self.assertRaises(ValueError):
                fixture.validate_plan(bad)
        for root in ['C:\\dev', 'D:\\dev\\devbox-s7-accept-x', 'C:\\dev\\devbox-s' + '6-accept-x']:
            with self.assertRaises(ValueError, msg=root):
                fixture.fixture_plan(C_SHA, 'owned-123456abcdef', 'windows', root)
        with self.assertRaises(ValueError):
            fixture.fixture_plan(C_SHA, 'owned-123456abcdef', 'wsl', 'C:\\dev\\devbox-s7-accept-x')

    def test_projects_have_local_identity_and_fixed_texts(self):
        with tempfile.TemporaryDirectory() as tmp:
            calls = []
            fixture.make_project(Path(tmp) / 'P-WIN', fixture.project_files('P-WIN'), run=lambda argv, **kw: calls.append((argv, kw)))
            argv = [c[0] for c in calls]
            self.assertEqual(argv[0][:5], ['git', 'init', '-q', '-b', 'main'])
            for key, value in fixture.IDENTITY:
                self.assertIn(['git', '-C', str(Path(tmp) / 'P-WIN'), 'config', key, value], argv)
            self.assertFalse(any('--global' in a for a in argv))
            self.assertTrue(all(c[1]['env'] == fixture.GIT_ENV for c in calls))
            toml = tomllib.loads((Path(tmp) / 'P-WIN/.devbox/devbox.toml').read_text())
            entries = toml['task'] + toml['service']
            self.assertEqual(sorted(e['id'] for e in entries), ['s7w1a', 's7w1b', 's7w1e', 's7w1h', 's7w1u', 's7w1x'])
            self.assertTrue(all(e['target'] == 'windows' for e in entries))
        self.assertEqual(tomllib.loads(fixture.P_SV_TOML)['service'][0]['id'], 's7sv1')
        self.assertEqual(len(tomllib.loads(fixture.P_CL_TOML)['agent']['teardown']), 1)
        self.assertEqual(fixture.project_files('P-TOOLS'), {'README.md': fixture.README})

    def test_accept_paths_are_accept_only(self):
        for path in fixture.accept_paths():
            self.assertIn('accept', str(path))
            self.assertNotRegex(str(path), r'devbox@|/devbox/prod|devbox-tmux@')

    @unittest.skipUnless(Path(WORKTREE).is_dir(), 'worktree absent')
    def test_bind_and_verify_source(self):
        cand = candidate_sha()
        binding = fixture.bind_source(WORKTREE, cand, BASELINE_SHA)
        self.assertEqual((binding['candidate']['version'], binding['baseline']['version']), ('1.0.0-rc.2', '1.0.0-rc.1'))
        self.assertIn(wsl_daemon.DEPLOY_CORE, binding['candidate']['files'])
        self.assertIn(wsl_daemon.DEPLOY_PLATFORM, binding['baseline']['files'])
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp) / 'source-binding.json'
            with patch.object(fixture, 'BINDING_FILE', target):
                with self.assertRaises(ValueError):
                    fixture.verify_source(WORKTREE, cand)  # UNBOUND
                common.dump(target, fixture.bind_source(WORKTREE, cand))
                self.assertEqual(fixture.verify_source(WORKTREE, cand)['candidate']['sha'], cand)
                with self.assertRaises(ValueError):
                    fixture.verify_source(WORKTREE, cand, BASELINE_SHA)
                tampered = json.loads(target.read_text())
                tampered['candidate']['files']['crates/cli/src/setup.rs'] = '0' * 64
                common.dump(target, tampered)
                with self.assertRaises(ValueError):
                    fixture.verify_source(WORKTREE, cand)


# --- 15: cleanup ownership -------------------------------------------------------------------------------
class CleanupOwnership(unittest.TestCase):
    def test_worktrees(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / 'owned-123456abcdef'
            (root / 'P-CL/.git').mkdir(parents=True)
            good = str(root / 'P-CL-agents' / 'x')
            m = {'fixtureRoot': str(root), 'projectPaths': {'P-CL': str(root / 'P-CL'), 'P-SV': str(root / 'P-SV')}, 'agentWorktrees': [good]}
            self.assertEqual(wsl_owned.owned_worktrees(m, lambda p: [str(p), good]), [(root / 'P-CL', Path(good))])
            for bad in (str(root / 'P-CL-agents' / 'unrecorded'), str(Path(tmp) / 'elsewhere')):
                with self.assertRaises(ValueError):
                    wsl_owned.owned_worktrees(dict(m, agentWorktrees=[good, str(Path(tmp) / 'elsewhere')]), lambda p: [str(p), bad])

    def cleanup_env(self, units, m, tmp):
        calls = []
        run = lambda argv, **kw: calls.append(argv) or Fake(returncode=0, stdout='', stderr='')
        state = lambda name: {'LoadState': 'loaded' if name in units else 'not-found', 'ActiveState': 'failed' if name in units else 'inactive'}
        listings = [list(units), []]
        patches = [patch.object(wsl_owned, 'accept_paths', lambda: []), patch.object(wsl_owned.subprocess, 'run', run),
                   patch.object(wsl_owned, 'parent_candidates', lambda: []),
                   patch.object(wsl_owned, 'unit_state', state), patch.object(wsl_owned, 'accept_runtime_units', lambda: listings.pop(0) if listings else [])]
        return calls, patches

    def test_cleanup_resets_only_recorded_units_and_refuses_unrecorded_scopes(self):
        rid = 'ab' * 16
        unit = 'devbox-accept-run-' + rid + '.service'
        with tempfile.TemporaryDirectory() as tmp, patch.object(fixture, 'FIXTURE_PARENT', Path(tmp)):
            token = 'owned-123456abcdef'
            root = fixture.fixture_root(token)
            path = Path(tmp) / ('ownership-' + token + '.json')
            m = {'channel': 'accept', 'token': token, 'sourceSha': C_SHA, 'absentBefore': [], 'runIds': [rid], 'agentIds': ['a1'], 'sealedPaths': [],
                 'fixtureRoot': str(root), 'createdFiles': [], 'projectPaths': {'P-CL': str(root / 'P-CL')}, 'socketPath': str(Path(tmp) / 'no.sock'),
                 'mcpProcesses': []}
            common.dump(path, m)

            def build():
                fixture.create_root(root, token, C_SHA)
                (root / 'P-CL').mkdir()
            build()
            calls, patches = self.cleanup_env([unit], m, tmp)
            for item in patches:
                item.start()
            try:
                self.assertEqual(wsl_owned.cleanup(path, m)['status'], 'PASS')
            finally:
                for item in patches:
                    item.stop()
            self.assertEqual([c for c in calls if 'reset-failed' in c], [['systemctl', '--user', 'reset-failed', unit]])
            self.assertFalse(root.exists())
            build()
            for foreign in ('devbox-accept-agent-zz.scope', 'devbox-accept-run-' + 'cd' * 16 + '.service'):
                calls, patches = self.cleanup_env([foreign], m, tmp)
                for item in patches:
                    item.start()
                try:
                    with self.assertRaisesRegex(ValueError, 'Unrecorded accept jobs'):
                        wsl_owned.cleanup(path, m)
                finally:
                    for item in patches:
                        item.stop()
                self.assertFalse([c for c in calls if 'stop' in c or 'reset-failed' in c])
            self.assertTrue(root.exists())

    def test_tmux_restart_only_in_k1x(self):
        with tempfile.TemporaryDirectory() as tmp:
            fragment = Path(tmp) / 'devbox-accept-tmux@.service'
            fragment.write_bytes(wsl_owned.SETUP_MARKER + b'unit\n')
            row = {'path': str(fragment), 'kind': 'file', 'sha256': hashlib.sha256(fragment.read_bytes()).hexdigest()}
            m = {'flow': 'wsl-fresh', 'answers': answers(Q3=True), 'sealedPaths': [row]}
            state = lambda unit: {'FragmentPath': str(fragment), 'ActiveState': 'active', 'MainPID': '9', 'InvocationID': 'i'}
            calls = []
            run = lambda argv, **kw: calls.append(argv) or subprocess.CompletedProcess(argv, 0, b'', b'')
            for bad_m, case in ((m, 'C1'), (dict(m, flow='fresh'), 'K1X-TMUXUP'), (dict(m, answers=answers()), 'K1X-TMUXUP')):
                with self.assertRaises(ValueError):
                    wsl_owned.restart_tmux(bad_m, case, state=state, run=run)
            wsl_owned.restart_tmux(m, 'K1X-TMUXUP', state=state, run=run)
            self.assertEqual(calls, [['systemctl', '--user', 'restart', 'devbox-accept-tmux@accept.service']])
            fragment.write_bytes(b'changed\n')
            with self.assertRaises(ValueError):
                wsl_owned.restart_tmux(m, 'K1X-TMUXUP', state=state, run=run)
            self.assertEqual(len(calls), 1)

    def test_stage_cleanup_marker_and_entries(self):
        with tempfile.TemporaryDirectory() as tmp:
            stage = Path(tmp) / 'DevboxAccept-s7-stage-x'

            def build(extra=None, sub_extra=None):
                (stage / 'candidate').mkdir(parents=True)
                (stage / teardown.MARKER).write_text(json.dumps({'token': 'tok', 'sourceSha': C_SHA}))
                (stage / 'candidate/receipt.json').write_text('{}')
                (stage / 'candidate/Devbox Accept_1.0.0-rc.2_x64-setup.exe').write_bytes(b'x')
                (stage / 'settings.json.s7-backup').write_text('{}')
                if extra:
                    (stage / extra).write_text('x')
                if sub_extra:
                    (stage / 'candidate' / sub_extra).write_text('x')
            build()
            self.assertEqual(teardown.cleanup_stage(stage, 'other', C_SHA)['status'], 'FAIL')
            self.assertTrue(stage.exists())
            self.assertEqual(teardown.cleanup_stage(stage, 'tok', C_SHA)['status'], 'PASS')
            self.assertFalse(stage.exists())
            for extra, sub_extra in (('notes.txt', None), (None, 'evil.dll')):
                build(extra, sub_extra)
                self.assertEqual(teardown.cleanup_stage(stage, 'tok', C_SHA)['status'], 'FAIL')
                self.assertTrue(stage.exists())
                __import__('shutil').rmtree(stage)
            self.assertEqual(teardown.cleanup_stage(stage, 'tok', C_SHA)['removed'], False)

    def test_wsl_only_finalize_runs_no_native_operation(self):
        saved = {}
        ops = {'wsl_cleanup': lambda: {'status': 'PASS'}, 'fingerprint_wsl': lambda: {'w': 1}, 'absent_after': lambda: {'status': 'PASS', 'wsl': {}}}
        out = teardown.finalize({'status': 'EXECUTED', 'phases': []}, {'ownership': 'o', 'sides': ('wsl',), 'launched': True, 'installed': True,
                                                                       'staged': False}, {'wsl': {'w': 1}, 'windows': None}, ops,
                                lambda n, v: saved.__setitem__(n, v))
        self.assertEqual((out['status'], out['prodComparison']['status'], out['prodComparison']['sides']), ('EXECUTED', 'PASS', ['wsl']))
        self.assertEqual({r['action']: r['status'] for r in out['cleanup']}['windows'], 'NOT_RUN')
        self.assertEqual(out['acceptAbsentAfter'], {'status': 'PASS', 'wsl': {}})
        ops['fingerprint_wsl'] = lambda: {'w': 2}
        out = teardown.finalize({'status': 'EXECUTED'}, {'ownership': 'o', 'sides': ('wsl',)}, {'wsl': {'w': 1}}, ops, lambda n, v: None)
        self.assertEqual((out['status'], out['prodComparison']['status']), ('FAIL', 'FAIL'))


# --- 16: trust_probe -------------------------------------------------------------------------------------
class TrustProbe(unittest.TestCase):
    def test_reads_rb_and_returns_only_a_bool_or_a_hash(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            (home / '.claude.json').write_text(json.dumps({'projects': {'/p': {'hasTrustDialogAccepted': True, 'token': 'secret'}, '/q': {}}}))
            (home / '.codex').mkdir()
            (home / '.codex/config.toml').write_bytes(b'model = "x"\n')
            modes = []
            real = open

            def spy(file, mode='r', *args, **kwargs):
                modes.append(mode)
                return real(file, mode, *args, **kwargs)
            with patch('builtins.open', spy):
                claude, codex = agents_live.trust_probe(home)
                self.assertIs(claude('/p'), True)
                self.assertIs(claude('/q'), False)
                self.assertIsNone(claude('/r'))
                self.assertEqual(codex(), hashlib.sha256(b'model = "x"\n').hexdigest())
            self.assertEqual(set(modes), {'rb'})
            empty = home / 'empty'
            empty.mkdir()
            claude, codex = agents_live.trust_probe(empty)
            self.assertIsNone(claude('/p'))
            self.assertIsNone(codex())
            self.assertEqual(validate.scan_tool_paths(validate.helpers()), [])


# --- 17: console-windows ---------------------------------------------------------------------------------
class ConsoleWindows(unittest.TestCase):
    def test_only_read_only_user32(self):
        text = (BASE / 'native-observe.ps1').read_text()
        self.assertEqual(sorted(re.findall(r'\[DllImport\("user32\.dll"[^\]]*\)\]\s*static extern \w+ (\w+)\(', text)),
                         ['EnumWindows', 'GetClassName', 'GetWindowThreadProcessId', 'IsWindowVisible'])
        self.assertEqual(text.count('DllImport'), 4)
        for path in BASE.glob('*.ps1'):
            source = path.read_text()
            for name in ('SendMessage', 'PostMessage', 'SetForegroundWindow', 'SendInput', 'keybd_event', 'mouse_event'):
                self.assertNotIn(name, source, path.name)
        self.assertEqual(w1_windows.run_token('C:\\dev\\devbox-s7-accept-240f00aa-1234abcd'), '240f00aa-1234abcd')
        with self.assertRaises(ValueError):
            w1_windows.run_token('C:\\dev\\other')


# --- 18: every live entry point takes the lock first -----------------------------------------------------
class Locking(unittest.TestCase):
    ENTRY = {'wsl_daemon.py': ('install_binary', 'setup', 'startup_doctor'), 'agents_live.py': ('run_block', 'u1_prepare', 'u1_rows'),
             'daemon_rows.py': ('c1', 'cs3', 'cs4', 'cs5', 'sv1', 'nt1', 'cl1', 'cl2', 'cl3', 'cs10', 'n1_accept'), 'mcp_stdio.py': ('start',),
             'fixture.py': ('stage', 'delegate_windows'), 'w1_windows.py': ('run',), 'wsl-owned.py': ('main',), 'driver.py': ('main',),
             'g1_live_member.py': ('main',), 'g2_probe_timing.py': ('main',)}
    PURE = {'ArgumentParser', 'add_argument', 'parse_args', 'error', 'print', 'dumps', 'fingerprint', 'parse', 'gates', 'verify_source',
            'chdir', 'probe_script', 'Path', 'resolve', 'is_relative_to', 'evidence_root'}

    def test_lock_before_any_host_call(self):
        for name, functions in self.ENTRY.items():
            tree = ast.parse((BASE / name).read_text())
            for function in functions:
                defs = [f for f in ast.walk(tree) if isinstance(f, ast.FunctionDef) and f.name == function]
                self.assertTrue(defs, name + ':' + function)
                for node in defs:
                    calls = sorted((c for c in ast.walk(node) if isinstance(c, ast.Call)), key=lambda c: (c.lineno, c.col_offset))
                    names = [validate.callee(c) for c in calls]
                    self.assertIn('require_lock', names, name + ':' + function)
                    before = names[:names.index('require_lock')]
                    self.assertTrue(set(before) <= self.PURE, '%s:%s calls %s before the lock' % (name, function, set(before) - self.PURE))


# --- 19: interop attribution -----------------------------------------------------------------------------
class Attribution(unittest.TestCase):
    VSOCK = 'UtilAcceptVsock:273: accept4 failed 110\n'
    TOOL_TIMEOUT = {'step': 'doctor:tool.claude', 'reason': 'x', 'shellInit': 'doctor-tool', 'message': '확인하지 못함: 2초 시한을 초과했습니다', 'timeout': True}

    def env_failure(self):
        error, _ = WslDaemon.setup_with(WslDaemon(), {'steps': [{'id': 'env', 'ok': False, 'message': 'bash timed out'}]}, code=1)
        return error.failure

    def test_broken_interop_turns_stalls_into_not_run(self):
        for failure in (evidence.failure('setup', 'boom', stderr=self.VSOCK), evidence.failure('runtime.start', 'boom', journal=['x', self.VSOCK]),
                        self.env_failure(), self.TOOL_TIMEOUT):
            m = matrix(interopBroken=True)
            evidence.record(m, 'CS3', 'FAIL', [], 'boom', failure=failure)
            row = m['cases']['CS3']
            self.assertEqual((row['status'], row['reason']), ('NOT_RUN', evidence.reason('R-IOP-STALL')), failure['step'])
            self.assertEqual(row['originalFailure']['status'], 'FAIL')
            self.assertNotIn('UtilAcceptVsock:273', json.dumps(row))  # counts only
            m = matrix(interopBroken=False)
            evidence.record(m, 'CS3', 'FAIL', [], 'boom', failure=failure)
            self.assertEqual(m['cases']['CS3']['status'], 'FAIL')

    def test_other_failures_stay_fail(self):
        for failure in (dict(self.TOOL_TIMEOUT, message='없음: 실행 파일을 찾지 못했습니다', timeout=False), evidence.failure('setup', 'boom', timeout=True),
                        {'step': 'setup:units', 'shellInit': None, 'message': 'denied'}, None):
            for broken in (True, False):
                m = matrix(interopBroken=broken)
                evidence.record(m, 'CS3', 'FAIL', [], 'boom', failure=failure)
                self.assertEqual(m['cases']['CS3']['status'], 'FAIL', repr(failure))

    def test_conclude_offers_only_a_sole_doctor_part(self):
        finished = []
        live = Fake(data={}, put=lambda case, key, value: live.data.setdefault(case, {}).__setitem__(key, value),
                    finish=lambda case, status, reason=None, partial=None, failure=None, **k: finished.append((status, failure)))
        live.data['H1'] = {'failure': dict(self.TOOL_TIMEOUT)}
        agents_live.conclude(live, 'H1', {'a': True, 'b': True, 'c': True, 'd': False}, [])
        self.assertEqual(finished[-1][1]['shellInit'], 'doctor-tool')
        live.data['H1'] = {'failure': dict(self.TOOL_TIMEOUT)}
        agents_live.conclude(live, 'H1', {'a': False, 'b': True, 'c': True, 'd': False}, [])
        self.assertIsNone(finished[-1][1].get('shellInit'))

    def test_driver_groups(self):
        m = matrix(interopBroken=True)
        with tempfile.TemporaryDirectory() as tmp:
            stub = Fake(cases=evidence.FLOWS['wsl-fresh'], matrix=m, evidence=Path(tmp), journal_since=lambda t0: [])
            stub.pending = lambda case: driver.Session.pending(stub, case)
            stub.case_result = lambda case, status, files, reason=None, partial=None, failure=None, **d: evidence.record(
                m, case, status, files, reason, failure=failure, partial=partial, **d)
            (Path(tmp) / 'cases').mkdir()

            def raise_(error):
                def run(selected):
                    raise error
                return run
            driver.Session.guarded(stub, ['CL1', 'CL2'], raise_(RuntimeError('boom')))
            self.assertEqual((m['cases']['CL1']['status'], m['cases']['CL2']['status']), ('FAIL', 'NOT_RUN'))
            self.assertIn('Not reached', m['cases']['CL2']['reason'])
            env = wsl_daemon.StepFailed('setup step env failed', self.env_failure(), {'steps': []})
            driver.Session.guarded(stub, ['CS3'], raise_(env))
            self.assertEqual(m['cases']['CS3']['reason'], evidence.reason('R-IOP-STALL'))
            self.assertTrue((Path(tmp) / 'cases/CS3-step.json').is_file())
            driver.Session.guarded(stub, ['SV1'], raise_(agents_live.StopPrompt('update')))
            self.assertTrue(m['cases']['SV1']['reason'].startswith(evidence.reason('R-PROMPT')))
            driver.Session.guarded(stub, ['NT1', 'CL3'], raise_(agents_live.Refused('guard')))
            self.assertTrue(all(m['cases'][c]['reason'].startswith('Harness guard refused') for c in ('NT1', 'CL3')))
            driver.Session.guarded(stub, ['CL1'], raise_(evidence.Unavailable('x')))
            self.assertEqual(m['cases']['CL1']['status'], 'FAIL')  # a recorded FAIL is never overwritten


# --- answers gating --------------------------------------------------------------------------------------
class Answers(unittest.TestCase):
    def test_answers_file(self):
        # M2: the controller owns the values (Ruling 79 set Q5 true); the file must parse, every answer is a bool, and it is the user's.
        value = evidence.parse_answers((BASE / 'answers.json').read_bytes())
        self.assertTrue(all(type(value[q]) is bool for q in evidence.ANSWER_KEYS))
        self.assertIs(type(value['interopBroken']), bool)
        self.assertEqual((value['source'], value['recordedBy']), ('user', 'controller'))
        for bad in (dict(answers(), Q1='yes'), dict(answers(), extra=1), dict(answers(), source='agent'), dict(answers(), recordedBy='harness'),
                    dict(answers(), recordedAt='today'), {k: v for k, v in answers().items() if k != 'Q5'}, dict(answers(), notes={'Q9': 'x'})):
            with self.assertRaises(ValueError):
                evidence.parse_answers(json.dumps(bad))

    def test_pass_needs_the_answer_and_the_gates(self):
        m = matrix()
        m['gates'].update(installer={'status': 'PASS'}, wsl_identity={'status': 'PASS'}, trust_answer={'status': 'PASS'})
        with self.assertRaises(ValueError):
            evidence.record(m, 'H1', 'PASS', ['x'], surface='wsl-artifact')  # Q1 false
        with self.assertRaises(ValueError):
            evidence.record(m, 'CS3', 'PASS', ['x'], surface='installed-native')  # a WSL flow records wsl-artifact
        evidence.record(m, 'CS3', 'PASS', ['x'], surface='wsl-artifact')
        m = matrix(Q1=True)
        with self.assertRaises(ValueError):
            evidence.record(m, 'H1', 'PASS', ['x'], surface='wsl-artifact')  # gates missing
        m = matrix('fresh')
        m['gates'].update(installer={'status': 'PASS'})
        with self.assertRaises(ValueError):
            evidence.record(m, 'A1', 'PASS', ['x'], surface='wsl-artifact')
        evidence.record(m, 'A1', 'FAIL', ['x'], 'boom')
        evidence.record(m, 'A1', 'PASS', ['x'], surface='installed-native')
        self.assertEqual(m['cases']['A1']['status'], 'FAIL')
        stub = Fake(flow_name='wsl-fresh', matrix=matrix(), save=lambda: None)
        driver.Session.case_result(stub, 'H1', 'PASS', ['x'], surface='wsl-artifact')  # a refused PASS records NOT_RUN, never PASS
        self.assertEqual(stub.matrix['cases']['H1']['status'], 'NOT_RUN')
        self.assertIn('Evidence gate refused', stub.matrix['cases']['H1']['reason'])

    def test_partial_display(self):
        m = matrix()
        evidence.record(m, 'C1', 'NOT_RUN', ['x'], evidence.reason('R-IOP½'), partial=['a', 'b', 'c'])
        self.assertEqual(m['cases']['C1']['display'], 'NOT_RUN(부분: a, b, c)')


# --- driver units ----------------------------------------------------------------------------------------
class DriverUnits(unittest.TestCase):
    def test_merge_manifest_keeps_the_seal(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'ownership-owned-x.json'
            staged = {'token': 'owned-x', 'sealedPaths': [], 'projectIds': [], 'runIds': []}
            common.dump(path, staged)
            owned = json.loads(path.read_text())
            common.dump(path, dict(staged, sealedPaths=[{'path': '/a', 'kind': 'directory'}], runIds=['r1']))
            owned['projectIds'].append('p1')
            driver.merge_manifest(path, owned)
            disk = json.loads(path.read_text())
            self.assertEqual((len(disk['sealedPaths']), disk['projectIds'], disk['runIds']), (1, ['p1'], ['r1']))
            a4 = {'nonce': 'n'}
            owned['a4'] = a4
            driver.merge_manifest(path, owned)
            self.assertIs(owned['a4'], a4)
            a4['unrelated'] = {'pid': 7}
            driver.merge_manifest(path, owned)
            self.assertEqual(json.loads(path.read_text())['a4']['unrelated'], {'pid': 7})
            with self.assertRaises(ValueError):
                driver.merge_manifest(path, {'token': 'owned-y'})

    def test_classify(self):
        step = wsl_daemon.StepFailed('x', {'step': 'setup'}, {})
        self.assertEqual(driver.classify(step)[0::2], ('FAIL', {'step': 'setup'}))
        for error, status in ((agents_live.StopPrompt('u'), 'NOT_RUN'), (agents_live.BudgetExceeded('b'), 'NOT_RUN'),
                              (evidence.Unavailable('u'), 'NOT_RUN'), (RuntimeError('r'), 'FAIL'), (TimeoutError('t'), 'FAIL')):
            self.assertEqual(driver.classify(error)[0], status, repr(error))
        self.assertTrue(driver.classify(TimeoutError('t'))[2]['timeout'])

    def test_probe_marker(self):
        script = '$a=1\n[Console]::Out.WriteLine($json); exit 0\n#\n'
        self.assertEqual(driver.probe_marker(script), '[Console]::Out.WriteLine($json); exit 0')
        with self.assertRaises(ValueError):
            driver.probe_marker('a\nb\n')

    def test_w1_helpers(self):
        self.assertEqual(str(w1_windows.drvfs('C:\\Users\\u\\AppData\\Local\\x')), '/mnt/c/Users/u/AppData/Local/x')
        self.assertEqual(w1_windows.spaced([0, 60, 120])[0], True)
        self.assertEqual(w1_windows.spaced([0, 30, 60])[0], False)
        self.assertEqual(w1_windows.exit_code({'exit': {'code': 7}}), 7)


# --- validate.py checks ----------------------------------------------------------------------------------
class Validate(unittest.TestCase):
    def test_forbidden_raw_and_list_forms(self):
        self.assertEqual(validate.scan_forbidden({'ok.py': 'x = 1\n'}), [])
        for source in ("run(['git', 'push'])\n", "x = 'winget install'\n", "run(['systemctl', '--user', 'disable', 'u'])\n", "# SendInput\n",
                       "p = '" + validate.SCRATCH + "x'\n"):
            self.assertTrue(validate.scan_forbidden({'bad.py': source}), source)
        self.assertTrue(validate.scan_forbidden({'bad.ps1': '-ExecutionPolicy Bypass'}))

    def test_gh_only_read_only_api(self):
        self.assertEqual(validate.scan_gh({'d.py': "command([GH, 'api', f'repos/{REPO}/actions/runs/{i}'])\n"}), [])
        for source in ("command([GH, 'run', 'list'])\n", "command(['gh', 'api', f'repos/{REPO}/pulls', '-X', 'POST'])\n"):
            self.assertTrue(validate.scan_gh({'d.py': source}), source)

    def test_tool_paths(self):
        self.assertTrue(validate.scan_tool_paths({'x.py': "p = home / '.claude.json'\n"}))
        self.assertTrue(validate.scan_tool_paths({'x.py': "p = home / '.claude' / 'settings.json'\n"}))
        self.assertTrue(validate.scan_tool_paths({'agents_live.py': "def trust_probe(home):\n    open(home / '.claude.json', 'w')\n"}))
        self.assertTrue(validate.scan_tool_paths({'agents_live.py': "def trust_probe(home):\n    (home / '.bashrc').write_text('x')\n"}))
        self.assertEqual(validate.scan_tool_paths({'agents_live.py': "def trust_probe(home):\n    open(home / '.claude.json', 'rb')\n"}), [])
        self.assertTrue(validate.scan_tool_paths({'x.ps1': '$p = "$HOME\\.claude.json"'}))

    def test_allowlists(self):
        allowed = wsl_owned.READ_ONLY | wsl_owned.OWNED_MUTATIONS
        self.assertTrue(validate.scan_allowlists({'x.py': "rpc('projects.remove', {})\n"}, allowed))
        self.assertTrue(validate.scan_allowlists({'x.py': "self.rpc('tools.support_bundle', {})\n"}, allowed))
        self.assertTrue(validate.scan_allowlists({'x.py': "client.call('devbox_agent_stop', {})\n"}, allowed))
        self.assertTrue(validate.scan_allowlists({'x.py': "run(['tmux', 'kill-server'])\n"}, allowed))
        self.assertTrue(validate.scan_allowlists({'agents_live.py': "class Pane:\n    def k(self):\n        self._tmux(['kill-session', '-t', 'x'])\n"}, allowed))
        self.assertEqual(validate.scan_allowlists({'x.py': "rpc('agents.get', {'id': i})\n"}, allowed), [])
        self.assertEqual(validate.scan_allowlists(validate.helpers(), allowed), [])

    def test_wsl_purity(self):
        sources = dict(validate.helpers())
        sources['daemon_rows.py'] = 'from common import ps_command\n'
        self.assertTrue(validate.scan_wsl_modules(sources))
        sources['daemon_rows.py'] = "import w1_windows\n"
        self.assertTrue(validate.scan_wsl_modules(sources))
        sources['daemon_rows.py'] = "x = 'native-observe.ps1'\n"
        self.assertTrue(validate.scan_wsl_modules(sources))
        findings, methods = validate.scan_driver_wsl_paths((BASE / 'driver.py').read_text())
        self.assertEqual(findings, [])
        self.assertIn('wave_w', methods)
        self.assertNotIn('windows_flow', methods)
        self.assertNotIn('native_teardown', methods)
        synthetic = ('class Session:\n    def wsl_flow(self):\n        self.helper()\n    def helper(self):\n        self.native("x")\n'
                     '    def teardown(self):\n        native_flow = False\n        if native_flow:\n            self.native("y")\n'
                     '    def close_mcp(self):\n        pass\n    def native(self, s):\n        pass\n')
        self.assertTrue(validate.scan_driver_wsl_paths(synthetic)[0])
        self.assertEqual(validate.scan_driver_wsl_paths(synthetic.replace('self.native("x")', 'pass'))[0], [])

    def test_manifest_excludes_docs_tests_and_approvals(self):
        with tempfile.TemporaryDirectory() as tmp:
            for name in ('README.md', 'test_s7_harness.py', 'review-approval.json', 'helper-manifest.json', 'driver.py', 'answers.json', 'x.json.tmp'):
                (Path(tmp) / name).write_text('x')
            (Path(tmp) / 'runs').mkdir()
            self.assertEqual(sorted(validate.manifest_files(tmp)), ['answers.json', 'driver.py'])

    def test_fixture_check(self):
        rows = validate.check_fixture()
        self.assertEqual(sorted(rows), ['windows', 'windows-upgrade', 'wsl', 'wsl-upgrade'])


# --- S6 regressions, rebound ------------------------------------------------------------------------------
class FakeListener:
    def __init__(self, script):
        pass

    def read_json(self, index, timeout):
        return {'port': 50123, 'pid': 7, 'creationTime': '8'} if index == 0 else {'released': True}

    def send(self, line):
        pass

    def close(self, timeout=20):
        return 0


class A4NonBlockingStart(unittest.TestCase):
    """Run-1 A4 FAIL: runtime.start awaits readiness, so its id must be recorded while it is still pending."""
    RID = 'ab' * 16

    def harness(self, start_behavior, runs_visible=lambda state: state['startCalled'].is_set()):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        project = Path(tmp.name) / 'P-A4'
        project.mkdir()
        m = {'token': 'owned-123456abcdef', 'projectPaths': {'P-A4': str(project)}, 'projectIds': [], 'runIds': [], 'windowsProcesses': [],
             'windowsRoot': 'C:\\dev\\devbox-s7-accept-x'}
        state = {'startCalled': threading.Event(), 'go': threading.Event(), 'disk': {'runIds': []}, 'gets': [], 'stops': [], 'calls': []}
        run_row = lambda ready=False: {'id': self.RID, 'projectId': 'p1', 'defId': m['a4ServiceId'], 'unit': 'devbox-accept-run-' + self.RID + '.service',
                                       'state': 'running', 'ready': ready}

        def save_manifest():
            state['disk'] = json.loads(json.dumps({k: m.get(k) for k in ('runIds', 'runUnits', 'projectIds')}))

        def rpc(method, params):
            state['calls'].append(method)
            if method == 'projects.add':
                return {'id': 'p1'}
            if method == 'projects.trust_preview':
                self.assertEqual(params, common.trust_context('p1', 'run'))  # item 5: the app's run context (useRunActions.tsx:35)
                return {'canonicalPath': str(project.resolve()), 'runDefinitions': [{'id': m['a4ServiceId']}], 'requiresApproval': False}
            if method == 'runtime.start':
                state['startCalled'].set()
                return start_behavior(state, run_row)
            if method == 'runtime.runs':
                return {'items': [run_row(state['go'].is_set())] if runs_visible(state) else []}
            if method == 'runtime.get':
                self.assertIn(params['runId'], state['disk']['runIds'])
                return {'run': run_row(state['go'].is_set())}
            if method == 'runtime.stop':
                state['stops'].append(params['runId'])
                return {'state': 'stopped'}
            raise AssertionError(method)

        def set_event(name):
            if name == m['a4']['go']:
                state['go'].set()
            return {'set': True}
        patches = [patch.object(a4_windows_service, 'time', FakeClock()), patch.object(a4_windows_service, 'LineProcess', FakeListener),
                   patch.object(a4_windows_service, 'set_event', set_event),
                   patch.object(a4_windows_service, 'port_owner', lambda port, b64: {'rows': [{'pid': 20, 'creation': '21', 'ours': True}]}),
                   patch.object(a4_windows_service, 'read_root_identity', lambda m, a4, rpc, rid: {'pid': 10, 'creationTime': '11', 'directory': 'C:\\dev\\x'}),
                   patch.object(a4_windows_service, 'identity_states', lambda pairs, timeout=40: {pairs[0][0]: 'absent'}),
                   patch.object(a4_windows_service, 'stop_exact', side_effect=AssertionError('no stop needed')),
                   # the fixture commit and wslpath go through common.run_checked, which needs a finished process
                   patch.object(subprocess, 'run', lambda argv, **k: subprocess.CompletedProcess(argv, 0, b'', b''))]
        for item in patches:
            item.start()
            self.addCleanup(item.stop)
        row = a4_windows_service.run(m, save_manifest, rpc, None, lambda name, value: None)
        return row, m, state

    def test_run_id_recorded_while_start_pending_then_pass(self):
        def blocking(state, run_row):
            self.assertTrue(state['go'].wait(10))
            return run_row(True)
        row, m, state = self.harness(blocking)
        self.assertEqual(row['status'], 'PASS', row.get('reason'))
        self.assertEqual(m['runIds'], [self.RID])
        self.assertTrue(row['startPendingAtRecord'])
        self.assertEqual(state['stops'], [self.RID])
        self.assertTrue(m['a4ServiceId'].startswith('s7win'))

    def test_start_error_is_fail_but_run_still_recorded_and_stopped(self):
        def not_ready(state, run_row):
            raise RuntimeError('{"code": "runtime.not_ready"}')
        row, m, state = self.harness(not_ready, runs_visible=lambda state: True)
        self.assertEqual(row['status'], 'FAIL')
        self.assertEqual((m['runIds'], state['stops']), ([self.RID], [self.RID]))

    def test_project_toml_and_deadline(self):
        a4 = a4_windows_service.plan('owned-123456abcdef')
        service = tomllib.loads(a4_windows_service.project_toml(a4, 50123))['service'][0]
        self.assertEqual((service['id'], service['target'], service['ports_hint']), (a4['serviceId'], 'windows', [50123]))
        self.assertIn('Local\\Devbox.S7A4.', a4['go'])
        self.assertEqual(service['ready_timeout_sec'], common.A4_READY_TIMEOUT_SEC)
        self.assertGreater(common.rpc_deadline('runtime.start'), common.A4_READY_TIMEOUT_SEC)
        self.assertEqual(common.rpc_deadline('runtime.get'), 15)


class ProdScope(unittest.TestCase):
    LEGACY = 'C:\\Users\\U\\AppData\\Local\\Devbox\\integration'

    def fingerprint(self, legacy_hash='A', core_hash='B'):
        return {'wsl': {'files': [1]}, 'windows': {'installs': [{'root': 'C:\\Users\\U\\AppData\\Local\\Devbox', 'exists': True, 'files': [
            {'path': 'C:\\Users\\U\\AppData\\Local\\Devbox\\catalog.json', 'sha256': core_hash},
            {'path': 'C:\\Users\\U\\AppData\\Local\\Devbox\\integration\\wsl-desktop\\v1\\summary.json', 'sha256': legacy_hash}]}], 'protocol': False}}

    def test_legacy_writer_reported_not_dropped(self):
        result = teardown.compare_prod(self.fingerprint(), self.fingerprint(legacy_hash='C'), [self.LEGACY])
        self.assertTrue(result['prodUnchanged'] and result['legacyExternalWriterChanged'])
        self.assertFalse(teardown.compare_prod(self.fingerprint(), self.fingerprint(legacy_hash='C'))['prodUnchanged'])
        self.assertFalse(teardown.compare_prod(self.fingerprint(), self.fingerprint(core_hash='X'), [self.LEGACY])['prodUnchanged'])
        for bad in ['C:\\Users\\U\\AppData\\Local\\Devbox', 'C:\\Users\\U\\AppData\\Local\\Devbox\\integration\\..', 'D:\\x\\Devbox\\integration']:
            with self.assertRaises(ValueError, msg=bad):
                teardown.split_legacy(self.fingerprint()['windows'], [bad])

    def test_windows_finalize(self):
        ops = {'quit': lambda: {'stopped': True}, 'assert_stopped': lambda: {'stopped': True}, 'wsl_cleanup': lambda: {'status': 'PASS'},
               'windows_cleanup': lambda: {'state': 'ok'}, 'fingerprint_wsl': lambda: {'files': [1]},
               'fingerprint_windows': lambda: self.fingerprint(legacy_hash='C')['windows'], 'stage_cleanup': lambda: {'status': 'PASS'}}
        context = {'ownership': 'o', 'launched': True, 'installed': True, 'staged': True, 'legacyExternalPrefixes': [self.LEGACY]}
        out = teardown.finalize({'status': 'EXECUTED'}, context, self.fingerprint(), ops, lambda n, v: None)
        self.assertEqual((out['status'], out['prodComparison']['status'], out['legacyExternalWriterChanged']), ('EXECUTED', 'PASS', True))
        ops['assert_stopped'] = lambda: (_ for _ in ()).throw(RuntimeError('alive'))
        out = teardown.finalize({'status': 'EXECUTED'}, dict(context, retainedResources=['r']), self.fingerprint(), ops, lambda n, v: None)
        self.assertEqual(out['status'], 'FAIL')
        self.assertEqual([r['status'] for r in out['cleanup'] if r['action'] in ('wsl', 'windows')], ['NOT_RUN', 'NOT_RUN'])



# --- Fix round 1 (Ruling 80) ------------------------------------------------------------------------------
def completed(args=(), out='', code=0, err=''):
    return subprocess.CompletedProcess(list(args), code, out, err)


class FixC1(unittest.TestCase):
    """Item 4: the product omits an empty startupProblems (crates/protocol/src/system.rs:29-31)."""

    def rows(self, info_fails_after_restore=False):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        config = root / 'config.toml'
        text = '[pc]\nname = "s7-accept-fixture"\n'
        config.write_text(text)
        m = {'configPath': str(config), 'configText': text, 'fixtureRoot': str(root)}
        state = {'problems': None, 'restored': False}

        def setup(trigger):
            if trigger == 'redeploy':  # only the restart reloads the config
                broken = config.read_text() != text
                state['problems'] = [{'kind': 'config', 'path': str(config)}] if broken else None
                state['restored'] = not broken
            return {'trigger': trigger, 'status': 'PASS'}

        def rpc(method, params):
            self.assertEqual(method, 'system.info')
            if info_fails_after_restore and state['restored']:
                raise RpcError('internal')
            return {'instance': 'accept', **({'startupProblems': state['problems']} if state['problems'] else {})}
        rows = daemon_rows.DaemonRows(rpc, lambda: m, lambda *a, **k: None, root, 'wsl-fresh', daemon=Fake(setup=setup), clock=FakeClock())
        ids = {'MainPID': '7', 'InvocationID': 'i', 'ActiveState': 'active'}
        with patch.object(daemon_rows, 'require_lock', NO_LOCK), patch.object(daemon_rows, 'unit_ids', lambda: ids):
            rows.c1()
        return json.loads((root / 'cases' / 'C1.json').read_text()), str(config)

    def test_absent_field_reads_as_empty(self):
        row, config = self.rows()
        self.assertEqual(row['parts'], {'a': True, 'b': True, 'c': True})
        self.assertEqual(row['launchOnlyStartupProblems'], [])
        self.assertEqual([s['startupProblems'] for s in row['steps']], [[], [{'kind': 'config', 'path': config}], []])
        self.assertEqual(row['status'], 'NOT_RUN')  # WSL-only: the Windows half (R-IOP½)

    def test_no_answer_is_not_an_empty_list(self):
        row, _ = self.rows(info_fails_after_restore=True)
        self.assertIs(row['parts']['c'], False)
        self.assertIsNone(row['steps'][-1]['startupProblems'])
        self.assertTrue(row['steps'][-1]['infoError'])


class FixTrustContext(unittest.TestCase):
    """Item 5: the app's projects.trust_preview params per operation, and the app's ensure() rule."""

    def test_params_per_operation(self):
        ctx = common.trust_context
        self.assertEqual(ctx('p', 'create', worktree=True), {'id': 'p', 'operation': 'create', 'agentId': None, 'worktrees': [], 'worktree': True})
        for op in ('run', 'launch'):
            self.assertEqual(ctx('p', op), {'id': 'p', 'operation': op, 'agentId': None, 'worktrees': [], 'worktree': None})
        for op in ('test', 'discard', 'merge', 'pr'):
            self.assertEqual(ctx('p', op, agent_id='a1'), {'id': 'p', 'operation': op, 'agentId': 'a1', 'worktrees': [], 'worktree': None})
        self.assertEqual(ctx('p', 'cleanup', worktrees=['/w/1']), {'id': 'p', 'operation': 'cleanup', 'agentId': None, 'worktrees': ['/w/1'],
                                                                   'worktree': None})
        for bad in (dict(operation='create'), dict(operation='create', worktree='yes'), dict(operation='run', agent_id='a1'),
                    dict(operation='launch', worktree=True), dict(operation='discard'), dict(operation='merge', agent_id='a1', worktrees=['/w']),
                    dict(operation='cleanup'), dict(operation='cleanup', worktrees=['/w'], agent_id='a1'), dict(operation='everything')):
            with self.assertRaises(ValueError, msg=repr(bad)):
                ctx('p', **bad)

    def test_ensure_mirrors_the_app(self):
        def daemon(reply):
            calls = []

            def rpc(method, params):
                calls.append((method, params))
                return dict(reply) if method == 'projects.trust_preview' else {}
            return calls, rpc
        context = common.trust_context('p', 'run')
        base = {'projectId': 'p', 'canonicalPath': '/f/p', 'scopeVersion': 1}
        for reply in ({'requiresApproval': False, 'approved': False}, {'requiresApproval': True, 'approved': True, 'digest': 'd', 'config': {}}):
            calls, rpc = daemon(dict(base, **reply))
            row = common.ensure_trust(rpc, context)
            self.assertEqual(calls, [('projects.trust_preview', context)])
            self.assertFalse(row['approvedNow'])
        calls, rpc = daemon({'projectId': 'p', 'canonicalPath': '/f/p', 'digest': 'd9', 'scopeVersion': 4, 'approved': False, 'requiresApproval': True,
                             'config': {'agent': {}}})
        row = common.ensure_trust(rpc, context)
        self.assertEqual(calls[1], ('projects.trust_approve', {'id': 'p', 'expectedCanonicalPath': '/f/p', 'expectedDigest': 'd9',
                                                               'expectedScopeVersion': 4}))
        self.assertTrue(row['approvedNow'])
        for reply in ({'digest': None, 'config': {}}, {'digest': 'd', 'config': None}, {'projectId': 'other', 'digest': 'd', 'config': {}}):
            calls, rpc = daemon(dict(base, approved=False, requiresApproval=True, **reply))
            with self.assertRaises(RuntimeError, msg=repr(reply)):
                common.ensure_trust(rpc, context)
            self.assertEqual([m for m, _ in calls], ['projects.trust_preview'])  # refused: no approval, and the caller makes no call


class FixWindowsRunTrust(unittest.TestCase):
    """Item 5 on the Windows rows: W1 and A4 preview a run with the app's run context; the owned RPC refuses the old shape."""

    def test_w1_previews_and_approves_as_the_app(self):
        calls = []

        def rpc(method, params):
            calls.append((method, params))
            if method == 'projects.trust_preview':
                return {'projectId': 'pw', 'canonicalPath': '/f/pw', 'scopeVersion': 2, 'digest': 'd2', 'config': {}, 'requiresApproval': True,
                        'approved': False, 'runDefinitions': [{'id': 's7w1a'}, {'id': 's7w1e'}]}
            return {}
        w1 = w1_windows.W1(rpc, None, None, tempfile.mkdtemp(), None, answers(), 'm' * 20, None)
        self.addCleanup(shutil.rmtree, str(w1.evidence))
        w1.project_id = 'pw'
        row = w1.approve()
        self.assertEqual(calls, [('projects.trust_preview', common.trust_context('pw', 'run')),
                                 ('projects.trust_approve', {'id': 'pw', 'expectedCanonicalPath': '/f/pw', 'expectedDigest': 'd2', 'expectedScopeVersion': 2})])
        self.assertEqual((row['runDefinitions'], row['approvedNow']), (['s7w1a', 's7w1e'], True))
        calls.clear()
        w1.rpc = lambda method, params: calls.append(method) or {'projectId': 'pw', 'requiresApproval': True, 'approved': False, 'digest': None}
        with self.assertRaises(RuntimeError):
            w1.approve()  # the app refuses an approval it cannot show; nothing is approved
        self.assertEqual(calls, ['projects.trust_preview'])

    def test_owned_rpc_takes_only_the_app_run_context(self):
        m = {'projectIds': ['pw'], 'agentIds': [], 'runIds': []}
        wsl_owned.authorize(m, 'projects.trust_preview', common.trust_context('pw', 'run'))
        with self.assertRaises(ValueError):
            wsl_owned.authorize(m, 'projects.trust_preview', {'id': 'pw', 'operation': 'run'})  # what W1 and A4 sent before


class FakeAgents:
    """A daemon for the CL rows. agents.create replies `preparing` and the folder appears on the second agents.get, as the
    product's background prepare does (crates/agents/src/create.rs:177; actions.rs:15 refuses an action while preparing)."""

    def __init__(self, root):
        self.root = Path(root)
        self.calls, self.replies, self.tasks = [], {}, {}

    def __call__(self, method, params):
        self.calls.append((method, copy.deepcopy(params)))
        reply = getattr(self, method.replace('.', '_'))(params)
        self.replies[len(self.calls) - 1] = copy.deepcopy(reply)
        return reply

    def failing(self):
        return 'exit 3' in (self.root / 'P-CL' / '.devbox' / 'devbox.toml').read_text()

    def view(self, agent_id):
        return {k: v for k, v in self.tasks[agent_id].items() if k != 'polls'}

    def projects_trust_preview(self, params):
        return {'projectId': params['id'], 'canonicalPath': str(self.root / 'P-CL'), 'digest': 'd%d' % len(self.calls), 'scopeVersion': 2,
                'approved': False, 'requiresApproval': True, 'config': {'agent': {}}}

    def projects_trust_approve(self, params):
        return {'approved': True}

    def agents_create(self, params):
        agent_id = 'a%d' % (len(self.tasks) + 1)
        tree = self.root / 'P-CL-agents' / params['branch']
        self.tasks[agent_id] = {'id': agent_id, 'projectId': params['projectId'], 'state': 'preparing', 'worktreePath': str(tree),
                                'branch': 'agent/' + params['branch'], 'polls': 0, 'cleanupError': None}
        return self.view(agent_id)

    def agents_get(self, params):
        task = self.tasks[params['id']]
        if task['state'] == 'preparing':
            task['polls'] += 1
            if task['polls'] >= 2:
                tree = Path(task['worktreePath'])
                tree.mkdir(parents=True)
                (tree / '.git').write_text('gitdir: x\n')
                task['state'] = 'running'
        return self.view(params['id'])

    def agents_discard_check(self, params):
        return {'headOid': 'h', 'unmergedCommits': 0, 'fingerprint': 'f'}

    def close(self, agent_id):
        task = self.tasks[agent_id]
        task['state'] = 'closed'
        if self.failing():
            task['cleanupError'] = json.dumps({'code': 'agents.teardown_failed'})
        else:
            shutil.rmtree(task['worktreePath'])
        return self.view(agent_id)

    def agents_discard(self, params):
        return self.close(params['id'])

    def agents_review(self, params):
        self.tasks[params['id']]['state'] = 'review'
        return self.view(params['id'])

    def agents_changes(self, params):
        return {'headOid': 'h2', 'baseOid': 'b2'}

    def agents_merge(self, params):
        return self.close(params['id'])

    def agents_cleanup_candidates(self, params):
        left = [t for t in self.tasks.values() if t['state'] == 'closed' and Path(t['worktreePath']).exists()]
        trees, branches = [t['worktreePath'] for t in left], [t['branch'] for t in left]
        return {'worktrees': trees, 'branches': branches, 'offBase': trees + branches, 'submodules': []}

    def agents_cleanup(self, params):
        for tree in params['worktrees']:
            shutil.rmtree(tree)
        return {}


class FixClRows(unittest.TestCase):
    """Item 5 (CL1, CL2 trust_context_invalid in run 1) and item 6 (the `preparing` folder): one preview and approval per
    action, right before it, with the app's params; only the action's own read may sit between."""
    ACTIONS = {'agents.create': 'create', 'agents.discard': 'discard', 'agents.merge': 'merge', 'agents.cleanup': 'cleanup'}
    READS = {'create': set(), 'discard': {'agents.discard_check'}, 'merge': {'agents.changes'}, 'cleanup': {'agents.cleanup_candidates'}}

    def run_row(self, name):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        (root / 'P-CL' / '.devbox').mkdir(parents=True)
        (root / 'P-CL' / '.devbox' / 'devbox.toml').write_text(fixture.P_CL_TOML)
        m = {'nonce': NONCE, 'projectPaths': {'P-CL': str(root / 'P-CL')}, 'projectIdByName': {'P-CL': 'id-p-cl'}}
        daemon = FakeAgents(root)
        rows = daemon_rows.DaemonRows(daemon, lambda: m, lambda *a, **k: None, root, 'wsl-fresh', clock=FakeClock())
        with patch.object(daemon_rows, 'require_lock', NO_LOCK), patch.object(daemon_rows, 'git_owned', lambda path, *args, **kw: completed(args)):
            getattr(rows, name)()
        return daemon, json.loads((root / 'cases' / (name.upper() + '.json')).read_text()), root

    def previewed(self, daemon):
        calls, seen = daemon.calls, []
        for index, (method, params) in enumerate(calls):
            if method not in self.ACTIONS:
                continue
            operation, back = self.ACTIONS[method], index - 1
            while calls[back][0] in self.READS[operation]:
                back -= 1
            self.assertEqual(calls[back][0], 'projects.trust_approve', 'an approval right before ' + method)
            self.assertEqual(calls[back - 1][0], 'projects.trust_preview', 'a preview right before ' + method)
            reply = daemon.replies[back - 1]
            self.assertEqual(calls[back][1], {'id': calls[back - 1][1]['id'], 'expectedCanonicalPath': reply['canonicalPath'],
                                              'expectedDigest': reply['digest'], 'expectedScopeVersion': reply['scopeVersion']})
            self.assertEqual(calls[back - 1][1]['operation'], operation)
            seen.append(calls[back - 1][1])
        self.assertEqual(sum(1 for method, _ in calls if method == 'projects.trust_preview'), len(seen))
        return seen

    def test_cl2(self):
        daemon, row, root = self.run_row('cl2')
        tree = str(root / 'P-CL-agents' / ('s7-%s-500' % NONCE))
        ctx = common.trust_context
        self.assertEqual(self.previewed(daemon), [ctx('id-p-cl', 'create', worktree=True), ctx('id-p-cl', 'merge', agent_id='a1'),
                                                  ctx('id-p-cl', 'cleanup', worktrees=[tree])])
        self.assertEqual((row['status'], row['parts']), ('PASS', {'a': True, 'b': True, 'c': True}))
        self.assertTrue((Path(tree).parent).is_dir() and not Path(tree).exists())

    def test_shell_task_waits_for_the_prepared_folder(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        m = {'nonce': NONCE, 'projectPaths': {'P-CL': str(root / 'P-CL')}, 'projectIdByName': {'P-CL': 'id-p-cl'}}
        daemon = FakeAgents(root)
        rows = daemon_rows.DaemonRows(daemon, lambda: m, lambda *a, **k: None, root, 'wsl-fresh', clock=FakeClock())
        task = rows.shell_task('CL1', 'P-CL')
        self.assertEqual(task['state'], 'running')
        self.assertTrue((Path(task['worktreePath']) / '.git').is_file())
        self.assertEqual([c[0] for c in daemon.calls], ['projects.trust_preview', 'projects.trust_approve', 'agents.create', 'agents.get', 'agents.get'])
        for state, error in (('preparing', TimeoutError), ('failed', RuntimeError)):
            stuck = FakeAgents(root)
            stuck.agents_get = lambda params, state=state: {'id': params['id'], 'state': state, 'worktreePath': str(root / 'never')}
            rows = daemon_rows.DaemonRows(stuck, lambda: m, lambda *a, **k: None, root, 'wsl-fresh', clock=FakeClock())
            with self.assertRaises(error, msg=state):
                rows.shell_task('CL1', 'P-CL')


class FixCl3Git(unittest.TestCase):
    """Item 6: CL3's exit 128 was `git -C` on a task folder that did not exist yet. Replayed on owned temp repos with the
    fixture's own setup, the product's prepare commands and the same env (fixture.GIT_ENV, no HOME)."""

    def test_layout(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = Path(tmp.name)
        source, project = root / 'sub-src', root / 'P-SUB'
        fixture.make_project(source, fixture.project_files('sub-src'))
        fixture.make_project(project, fixture.project_files('P-SUB'))
        daemon_rows.git_owned(project, '-c', 'protocol.file.allow=always', 'submodule', 'add', str(source), 'sub')  # cl3's own steps
        daemon_rows.git_owned(project, 'commit', '-qm', 's7 cl3 submodule')
        tree = root / 'P-SUB-agents' / ('s7-%s-1' % NONCE)
        update = ('-c', 'protocol.file.allow=always', 'submodule', 'update', '--init')
        with self.assertRaises(common.CommandFailed) as caught:  # run 1: the folder is not there yet
            daemon_rows.git_owned(tree, *update)
        self.assertEqual(caught.exception.returncode, 128)
        self.assertIn('cannot change to', caught.exception.stderr)
        # The product's prepare: crates/git/src/mutate.rs:175-191 (worktree add --no-checkout) and run/implicit_checkout.rs:90-97.
        oid = subprocess.run(['git', '-C', str(project), 'rev-parse', 'HEAD'], check=True, capture_output=True, text=True,
                             env=fixture.GIT_ENV).stdout.strip()
        tree.parent.mkdir()
        subprocess.run(['git', '-C', str(project), '-c', 'core.hooksPath=/dev/null', 'worktree', 'add', '--no-checkout', '-b',
                        'agent/s7-%s-1' % NONCE, '--', str(tree), oid], check=True, capture_output=True, env=fixture.GIT_ENV)
        subprocess.run(['git', '-C', str(tree), '-c', 'core.hooksPath=/dev/null', 'reset', '--hard', '--no-recurse-submodules', oid], check=True,
                       capture_output=True, env=fixture.GIT_ENV)
        self.assertEqual((tree / '.gitmodules').read_text().count('url = ' + str(source)), 1)  # an absolute URL, not a relative one
        daemon_rows.git_owned(tree, *update)
        status = daemon_rows.git_owned(tree, 'submodule', 'status').stdout
        self.assertTrue(status.startswith(' ') and status.split()[1] == 'sub', status)  # initialized and checked out
        self.assertTrue((tree / 'sub' / 'README.md').is_file())
        self.assertNotIn('.devbox/devbox.toml', fixture.project_files('P-SUB'))  # no project commands: its cleanup needs no approval

    def test_command_failure_is_bounded_and_redacted(self):
        error = common.CommandFailed(['git', '-C', '/x', 'push'], 128, 'x' * 9000 + '\nfatal: token=abc123 rejected\nmail a@b.example\n')
        self.assertEqual(error.returncode, 128)
        self.assertLessEqual(len(error.stderr), common.STDERR_LIMIT + 200)
        for secret in ('abc123', 'a@b.example'):
            self.assertNotIn(secret, error.stderr)
            self.assertNotIn(secret, str(error))
        self.assertIn('[redacted]', error.stderr)
        self.assertIn('exit 128', str(error))
        row = common.error_evidence(subprocess.CalledProcessError(1, ['git', 'x'], stderr=b'password: hunter2\nfatal: no\n'))
        self.assertNotIn('hunter2', json.dumps(row))
        self.assertEqual((row['exit'], row['type']), (1, 'CalledProcessError'))
        timed = common.CommandFailed.from_timeout(subprocess.TimeoutExpired(['git', 'x'], 30, stderr=b'secret=1\nslow\n'))
        self.assertTrue(timed.timed_out)
        self.assertNotIn('secret=1', timed.stderr)

    def test_guarded_writes_the_redacted_error(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        (Path(tmp.name) / 'cases').mkdir()
        results = []
        stub = Fake(cases=['CL3'], evidence=Path(tmp.name), pending=lambda case: True, journal_since=lambda t0: [],
                    case_result=lambda *a, **k: results.append((a, k)))

        def fail(_selected):
            raise common.CommandFailed(['git', '-C', '/t', 'submodule', 'update', '--init'], 128,
                                       "fatal: cannot change to '/t': No such file or directory\nsecret=s3cr3t\n")
        driver.Session.guarded(stub, ['CL3'], fail)
        (case, status, files, reason), _ = results[0]
        self.assertEqual((case, status), ('CL3', 'FAIL'))
        self.assertIn('cases/CL3-error.json', files)
        error = json.loads((Path(tmp.name) / 'cases' / 'CL3-error.json').read_text())
        self.assertIn("cannot change to '/t'", error['stderr'])
        self.assertNotIn('s3cr3t', json.dumps(error) + reason)


    def test_unchecked_commands_keep_redacted_stderr(self):
        secret = b'tmux: no server\ntoken=s3cr3t\n'
        failing = lambda argv, **k: subprocess.CompletedProcess(argv, 1, b'', secret)
        # tmux queries that do not raise: the case evidence keeps exit and redacted stderr (Live.finish).
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        m = {'nonce': NONCE, 'terminalIds': [], 'agents': {}}
        pane = agents_live.Pane(lambda: m, answers(), NONCE, run=failing, clock=FakeClock())
        recorded = []
        live = agents_live.Live(None, lambda: m, lambda *a, **k: recorded.append(a), Path(tmp.name), answers(), NONCE, pane, None, clock=FakeClock(),
                                home=Path(tmp.name))
        pane.begin('K1X-SEND')
        self.assertEqual(pane.show_extended_keys(), '')
        live.finish('K1X-SEND', 'FAIL', 'Failed parts: a')
        row = json.loads((Path(tmp.name) / 'cases' / 'K1X-SEND.json').read_text())
        self.assertEqual([(e['cmd'], e['exit']) for e in row['tmuxFailures']], [('show-options', 1)])
        self.assertIn('tmux: no server', row['tmuxFailures'][0]['stderr'])
        self.assertNotIn('s3cr3t', json.dumps(row))
        # systemctl status reads: the failure is kept beside the fields.
        for value in (daemon_rows.unit_ids(run=failing), daemon_rows.run_unit_state('r1', run=failing)):
            self.assertIn('systemctl exit 1: tmux: no server', value['error'])
            self.assertNotIn('s3cr3t', json.dumps(value))
        # The journal read for the interop attribution: no lines, and the failure is kept in the outcome.
        stub = Fake(outcome={})
        stub.journal_failed = lambda row: driver.Session.journal_failed(stub, row)
        with patch.object(driver.subprocess, 'run', lambda argv, **k: subprocess.CompletedProcess(argv, 1, '', secret.decode())):
            self.assertEqual(driver.Session.journal_since(stub, 0), [])
        self.assertEqual(stub.outcome['journalErrors'][0]['exit'], 1)
        self.assertNotIn('s3cr3t', json.dumps(stub.outcome))


class FixStopEvidence(unittest.TestCase):
    """Item 7: a stop keeps the last 15 captured lines, redacted with redact(), and the pattern name, in that case's evidence."""

    def test_capture_keeps_the_screen(self):
        tmux = FakeTmux()
        pane = agents_live.Pane(lambda: {'terminalIds': ['t1'], 'agents': {}}, answers(), NONCE, run=tmux, clock=FakeClock())
        lines = ['line %d' % i for i in range(30)] + ['token: abc', 'mail me@example.com', ' Enter your API key']
        tmux.screen = '\n'.join(lines)
        with self.assertRaises(agents_live.StopPrompt) as caught:
            pane.capture('t1')
        self.assertEqual(caught.exception.pattern, 'api-key')
        self.assertEqual(caught.exception.screen, agents_live.redact(lines[-15:]))
        self.assertEqual(len(caught.exception.screen), 15)
        self.assertIn('[redacted]', caught.exception.screen)
        self.assertIn('mail [email]', caught.exception.screen)
        self.assertEqual(caught.exception.screen[-1], ' Enter your API key')  # the matched line itself is kept for the controller

    def test_block_records_the_stop_on_its_case(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        tmux, clock = FakeTmux(), FakeClock()
        m = {'nonce': NONCE, 'terminalIds': ['t1'], 'agents': {}}
        pane = agents_live.Pane(lambda: m, answers(), NONCE, run=tmux, clock=clock)
        live = agents_live.Live(lambda method, params: {}, lambda: m, lambda *a, **k: None, Path(tmp.name), answers(), NONCE, pane, None,
                                clock=clock, home=tmp.name)
        tmux.screen = 'opencode 1.19\n Update available: 1.20 \n> '
        step = lambda live, tasks: live.capture('H3', 't1', 'opencode')
        with patch.object(agents_live, 'require_lock', NO_LOCK):
            agents_live.run_block(live, 'opencode', {'H3', 'H3X-REPORT'}, [('H3', step), ('H3X-REPORT', step)])
        row = json.loads((Path(tmp.name) / 'cases' / 'H3.json').read_text())
        self.assertEqual(row['status'], 'NOT_RUN')
        screen = agents_live.redact(['opencode 1.19', ' Update available: 1.20 ', '> '])
        self.assertEqual(row['stop'], {'pattern': 'update', 'lines': screen, 'matches': [{'index': 1, 'line': screen[1], 'context': screen}],
                                       'matchCount': 1})  # round 2 item 1 adds the matched lines
        self.assertNotIn('stop', json.loads((Path(tmp.name) / 'cases' / 'H3X-REPORT.json').read_text()))


class FixWaitingGate(unittest.TestCase):
    """I1: agents.send and the MCP send go out only after `waiting` was observed, and each one only when a fresh agents.get
    says `waiting` and a fresh capture shows no stop screen. Otherwise nothing is sent and (b) is false."""

    def live(self, states, screen, **values):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmux, clock = FakeTmux(), FakeClock()
        self.tmux.screen = screen
        self.m = {'nonce': NONCE, 'terminalIds': [], 'agents': {'a1': {'terminalIds': ['t2']}}, 'projectIdByName': {'P-TOOLS': 'p1'},
                  'projectPaths': {'P-TOOLS': '/f/P-TOOLS'}}
        self.calls, self.mcp_sent = [], []
        states = iter(states)
        last = {'state': None}

        def rpc(method, params):
            self.calls.append(method)
            if method == 'agents.get':
                last['state'] = next(states, last['state'])
                return {'id': 'a1', 'terminalId': 't2', 'state': last['state'], 'worktreePath': '/f/P-TOOLS-agents/x'}
            if method == 'agents.send':
                raise RpcError('agents.awaiting_input')
            raise AssertionError(method)
        pane = agents_live.Pane(lambda: self.m, answers(**values), NONCE, run=self.tmux, clock=clock)
        self.client = Fake(pairs=[], send=lambda agent_id, text: self.mcp_sent.append(text) or ('refused', '에이전트가 선택을 기다리고 있습니다. 화면에서 답하세요.'))
        return agents_live.Live(rpc, lambda: self.m, lambda *a, **k: None, Path(tmp.name), answers(**values), NONCE, pane, None, clock=clock,
                                home=Path(tmp.name)), Path(tmp.name)

    def test_trust_rows_send_nothing_while_running(self):
        for tool, screen, case in (('claude', CLAUDE_TRUST, 'H1X-TRUST'), ('codex', CODEX_TRUST, 'H2X-TRUST')):
            live, root = self.live(['running'] * 1000, screen)  # Q1 and Q2 false: the trust rows only observe
            live.pane.begin(case)
            self.assertTrue(agents_live.trust_row(live, case, tool, {'id': 'a1'}, self.client, {}))
            self.assertNotIn('agents.send', self.calls, tool)
            self.assertEqual(self.mcp_sent, [], tool)
            self.assertEqual(self.tmux.sent(), [], tool)  # no key, no text, no paste
            row = json.loads((root / 'cases' / (case + '.json')).read_text())
            self.assertEqual((row['status'], row['parts']['b']), ('FAIL', False), tool)
            self.assertEqual(row['sendGate'], [{'before': 'agents.send', 'state': 'running', 'sent': False, 'why': 'waiting was never observed'}])

    def test_each_send_rechecks_the_state(self):
        live, _ = self.live(['waiting', 'waiting', 'running'], APPROVAL)
        live.pane.begin('P1X')
        checks = agents_live.waiting_checks(live, 'P1X', 'codex', 'a1', 't2', self.client)
        self.assertEqual(self.calls.count('agents.send'), 1)  # the first gate held; the second saw `running`
        self.assertEqual(self.mcp_sent, [])
        self.assertEqual((checks['send'], checks['mcp'], checks['mcpRefused']), (True, False, False))
        gates = live.data['P1X']['sendGate']
        self.assertEqual([(g['before'], g['state'], g['sent']) for g in gates], [('agents.send', 'waiting', True), ('mcp', 'running', False)])
        self.assertEqual(self.calls.index('agents.send'), 2)  # attention get, then the gate's own fresh get, then the send

    def test_stop_screen_at_the_gate_sends_nothing(self):
        live, _ = self.live(['waiting'] * 10, 'Accessing workspace\n Update available! Run the update\n')
        live.pane.begin('H1X-SHELL')
        with self.assertRaises(agents_live.StopPrompt):
            agents_live.waiting_checks(live, 'H1X-SHELL', 'claude', 'a1', 't2', self.client)
        self.assertNotIn('agents.send', self.calls)
        self.assertEqual(self.mcp_sent, [])
        self.assertEqual(live.data['H1X-SHELL']['parts'], {'b': False})
        self.assertEqual(live.data['H1X-SHELL']['sendGate'][-1]['stop'], 'update')

    def test_waiting_sends_both_and_compares_the_pane(self):
        live, _ = self.live(['waiting'] * 10, APPROVAL)
        live.pane.begin('P1X')
        checks = agents_live.waiting_checks(live, 'P1X', 'codex', 'a1', 't2', self.client)
        self.assertEqual((self.calls.count('agents.send'), len(self.mcp_sent)), (1, 1))
        self.assertEqual(checks, {'attention': True, 'send': True, 'mcp': True, 'mcpRefused': True, 'input0': True})


ECHOED = '> Run the shell command `touch s7-approved` in the current folder, then reply OK.\n\n'
BOXED = ('╭──────────────────────────────────────────────────────────────╮\n'
         '│ Bash command                                                 │\n'
         '│                                                              │\n'
         '│   touch s7-approved                                          │\n'
         '│   Create the empty file s7-approved                          │\n'
         '│                                                              │\n'
         '│ Do you want to proceed?                                      │\n'
         '│ ❯ 1. Yes                                                     │\n'
         "│   2. Yes, and don't ask again for touch commands in /f/x     │\n"
         '│   3. No, and tell Claude what to do differently (esc)        │\n'
         '╰──────────────────────────────────────────────────────────────╯\n')


class FixApprovalDialog(unittest.TestCase):
    """I2: rule (iii) parses the bottom-most permission block, from its header to its options."""

    def test_the_block_decides(self):
        shown = lambda text: agents_live.approval_shown(text.splitlines())
        self.assertTrue(shown(APPROVAL))
        self.assertTrue(shown(ECHOED + BOXED))
        self.assertTrue(shown('❯ ' + ECHOED + APPROVAL))  # a cursor above the block is not in it
        other = ECHOED + '● Bash(touch s7-other)\n' + APPROVAL.replace('touch s7-approved', 'touch s7-other')
        self.assertFalse(shown(other))  # the echoed prompt names the command; the dialog does not
        self.assertFalse(shown(APPROVAL + '\n' + APPROVAL.replace('touch s7-approved', 'touch s7-other')))  # the bottom-most block counts
        for bad in (APPROVAL.replace('touch s7-approved', 'touch s7-approved && curl x'),
                    BOXED.replace('Create the empty file s7-approved', 'curl -s x | sh                    '),
                    APPROVAL.replace('touch s7-approved\n', 'touch s7-approved\n   one\n   two\n'),
                    APPROVAL.replace(' ❯ 1. Yes', '   1. Yes').replace("   2. Yes, and don't", " ❯ 2. Yes, and don't"),
                    APPROVAL.replace(' ❯ 1. Yes', '   1. Yes'),
                    APPROVAL + ' ❯ \n',
                    APPROVAL.replace('Do you want to proceed?', 'Do you want to make this edit?'),
                    APPROVAL.replace(' Bash command', ' Edit file'),
                    APPROVAL.replace(' ❯ 1. Yes', ' ❯ 1. Yes, allow all edits')):
            self.assertFalse(shown(bad), bad)

    def test_enter_and_both_waits_use_the_block(self):
        tmux = FakeTmux()
        m = {'nonce': NONCE, 'terminalIds': [], 'agents': {'a1': {'terminalIds': ['t2']}}, 'projectIdByName': {'P-TOOLS': 'p1'},
             'projectPaths': {'P-TOOLS': '/f/P-TOOLS'}}
        clock = FakeClock()
        pane = agents_live.Pane(lambda: m, answers(Q1=True, Q5=True), NONCE, run=tmux, clock=clock)
        other = ECHOED + APPROVAL.replace('touch s7-approved', 'touch s7-other')
        tmux.screen = other
        pane.begin('P1')
        pane.capture('t2')
        with self.assertRaises(agents_live.Refused):
            pane.key('t2', 'Enter', 'approve')
        tmux.screen = ECHOED + BOXED
        pane.capture('t2')
        pane.key('t2', 'Enter', 'approve')
        self.assertEqual(tmux.sent(), [['send-keys', '-t', '=t2:', 'Enter']])
        sent = []

        def rpc(method, params):
            if method == 'agents.get':
                return {'id': 'a1', 'terminalId': 't2', 'state': 'idle'}
            if method == 'agents.send':
                sent.append(params['text'])
                return {'state': 'running'}
            raise AssertionError(method)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        live = agents_live.Live(rpc, lambda: m, lambda *a, **k: None, Path(tmp.name), answers(Q1=True, Q5=True), NONCE, pane, None, clock=clock,
                                home=Path(tmp.name))
        tmux.screen = other
        pane.begin('U1X')
        self.assertIsNone(agents_live.wait_approval(live, 'U1X', 'a1', 't2'))
        pane.begin('P1')
        parts, reasons = agents_live.approval_row(live, 'P1', {'id': 'a1'}, Fake(pairs=[]))
        self.assertEqual((parts, reasons), ({'prompt': None}, [evidence.reason('R-NOAPPROVAL')]))
        self.assertEqual(sent, [agents_live.text_for(NONCE, 'P-APPROVE')])
        self.assertEqual(len(tmux.sent()), 1)  # still only the one Enter on the matching block


TOKEN = 'owned-123456abcdef'


class FixFixtureRoot(unittest.TestCase):
    """M12: the owned fixture projects live in an owned /tmp/devbox-s7-fixture-<token>/, not under the user's repo .git."""

    def test_plan_root(self):
        plan = fixture.fixture_plan(C_SHA, TOKEN, 'wsl', home=Path('/h'))
        root = '/tmp/devbox-s7-fixture-' + TOKEN
        self.assertEqual(plan['fixtureRoot'], root)
        self.assertTrue(all(path.startswith(root + '/') for path in plan['projectPaths'].values()))
        self.assertTrue(plan['vaultPath'].startswith(root + '/'))
        self.assertEqual(tomllib.loads(plan['configText'])['projects'], {'roots': [root]})
        fixture.validate_plan(plan)
        for moved in ('/e/' + TOKEN, '/tmp/x/devbox-s7-fixture-' + TOKEN, '/tmp/devbox-s7-fixture-owned-000000000000', '/tmp/devbox-s7-fixture-' + TOKEN + '/..'):
            bad = dict(plan, fixtureRoot=moved)
            with self.assertRaises(ValueError, msg=moved):
                fixture.validate_plan(bad)

    def test_create_root(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(fixture, 'FIXTURE_PARENT', Path(tmp)):
            root = fixture.fixture_root(TOKEN)
            self.assertEqual(root, Path(tmp) / ('devbox-s7-fixture-' + TOKEN))
            fixture.create_root(root, TOKEN, C_SHA)
            st = root.lstat()
            self.assertEqual((st.st_mode & 0o777, st.st_uid), (0o700, os.getuid()))
            self.assertEqual(json.loads((root / fixture.MARKER).read_text()), {'token': TOKEN, 'sourceSha': C_SHA})
            fixture.check_root(root, TOKEN, C_SHA)
            with self.assertRaises(RuntimeError):  # proven absent first: an existing one is preserved
                fixture.create_root(root, TOKEN, C_SHA)
            self.assertEqual(sorted(p.name for p in root.iterdir()), [fixture.MARKER])
            other = fixture.fixture_root('owned-' + 'f' * 12)
            other.symlink_to(tmp)
            with self.assertRaises(RuntimeError):
                fixture.create_root(other, 'owned-' + 'f' * 12, C_SHA)
            root.chmod(0o755)
            with self.assertRaises(ValueError):
                fixture.check_root(root, TOKEN, C_SHA)
            root.chmod(0o700)
            with self.assertRaises(ValueError):
                fixture.check_root(root, 'owned-' + 'e' * 12, C_SHA)
            with self.assertRaises(ValueError):  # only directly under the fixed parent
                fixture.create_root(Path(tmp) / 'sub' / ('devbox-s7-fixture-' + TOKEN), TOKEN, C_SHA)

    def test_parents_proven_absent(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp) / 'home'
            (home / '.config').mkdir(parents=True)
            with patch.object(fixture, 'HOME', home), patch.object(fixture, 'UNIT', home / '.config/systemd/user'):
                candidates = {str(p) for p in fixture.parent_candidates()}
                absent = fixture.absent_parents()
            for name in ('.local/share/devbox', '.local/state/devbox', '.config/systemd/user/sockets.target.wants',
                         '.config/systemd/user/default.target.wants', '.config/systemd/user', '.local'):
                self.assertIn(str(home / name), candidates)
                self.assertIn(str(home / name), absent)
            self.assertNotIn(str(home / '.config'), absent)
            self.assertNotIn(str(home), candidates)
            self.assertFalse(any(Path(p).is_relative_to(home / '.config/devbox-accept') for p in candidates))  # accept paths are not parents
            depth = [len(Path(p).parts) for p in absent if Path(p).is_relative_to(home)]
            self.assertEqual(depth, sorted(depth, reverse=True))  # deepest first


class FixCleanupChecks(unittest.TestCase):
    """M5: cleanup runs the load_owned checks itself before any removal. M6: recorded parents, removed only when empty."""

    def owned(self, tmp, **extra):
        root = fixture.fixture_root(TOKEN)
        fixture.create_root(root, TOKEN, C_SHA)
        (root / 'P-CL').mkdir()
        evidence = Path(tmp) / 'runs' / 'r1'
        evidence.mkdir(parents=True)
        path = evidence / ('ownership-' + TOKEN + '.json')
        m = {'channel': 'accept', 'token': TOKEN, 'sourceSha': C_SHA, 'fixtureRoot': str(root), 'absentBefore': [], 'absentParentsBefore': [],
             'runIds': [], 'agentIds': [], 'sealedPaths': [], 'createdFiles': [], 'projectPaths': {'P-CL': str(root / 'P-CL')},
             'socketPath': str(Path(tmp) / 'no.sock'), 'mcpProcesses': [], **extra}
        common.dump(path, m)
        return path, m, root

    def env(self, calls, parents=()):
        run = lambda argv, **kw: calls.append(list(argv)) or completed(argv)
        return [patch.object(wsl_owned, 'accept_paths', lambda: []), patch.object(wsl_owned.subprocess, 'run', run),
                patch.object(wsl_owned, 'unit_state', lambda name: {'LoadState': 'not-found', 'ActiveState': 'inactive'}),
                patch.object(wsl_owned, 'accept_runtime_units', lambda: []), patch.object(wsl_owned, 'parent_candidates', lambda: [Path(p) for p in parents])]

    def run_cleanup(self, path, m, calls, parents=()):
        with contextlib.ExitStack() as stack:
            for item in self.env(calls, parents):
                stack.enter_context(item)
            return wsl_owned.cleanup(path, m)

    def test_checks_before_any_removal(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(fixture, 'FIXTURE_PARENT', Path(tmp)):
            path, m, root = self.owned(tmp)
            elsewhere = Path(tmp) / 'elsewhere'
            elsewhere.mkdir(mode=0o700)
            (elsewhere / fixture.MARKER).write_text(json.dumps({'token': TOKEN, 'sourceSha': C_SHA}))
            tampered = [('marker', lambda: (root / fixture.MARKER).write_text('{}'), m),
                        ('passed copy', lambda: None, dict(m, fixtureRoot=str(elsewhere))),
                        ('disk root', lambda: common.dump(path, dict(m, fixtureRoot=str(elsewhere))), dict(m, fixtureRoot=str(elsewhere))),
                        ('mode', lambda: root.chmod(0o755), m)]
            for name, damage, given in tampered:
                common.dump(path, m)
                (root / fixture.MARKER).write_text(json.dumps({'token': TOKEN, 'sourceSha': C_SHA}))
                root.chmod(0o700)
                damage()
                calls = []
                with self.assertRaises(ValueError, msg=name):
                    self.run_cleanup(path, given, calls)
                self.assertEqual(calls, [], name)  # refused before any process or removal
                self.assertTrue((root / 'P-CL').is_dir() and elsewhere.is_dir(), name)

    def test_parents_removed_only_when_empty_and_recorded(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(fixture, 'FIXTURE_PARENT', Path(tmp)):
            a, ab, c = Path(tmp) / 'p' / 'a', Path(tmp) / 'p' / 'a' / 'b', Path(tmp) / 'p' / 'c'
            path, m, root = self.owned(tmp, absentParentsBefore=[str(ab), str(a), str(c)])
            ab.mkdir(parents=True)
            c.mkdir()
            (c / 'keep').write_text('not ours')
            calls = []
            with self.assertRaisesRegex(ValueError, 'parent'):  # recorded, but not a parent the product creates
                self.run_cleanup(path, m, calls, parents=[ab, a])
            self.assertEqual(calls, [])
            self.assertTrue(ab.is_dir() and root.is_dir())
            result = self.run_cleanup(path, m, calls, parents=[ab, a, c])
            self.assertEqual(result['status'], 'PASS')
            self.assertFalse(ab.exists() or a.exists() or root.exists())
            self.assertTrue((c / 'keep').is_file())
            rows = json.loads((path.parent / 'fixture-cleanup.json').read_text())['parents']
            self.assertEqual({r['path']: r['state'] for r in rows}, {str(ab): 'removed', str(a): 'removed', str(c): 'retained'})


class FixMinor(unittest.TestCase):
    """M1, M7, M10, M13, M14."""

    def test_sampler_counts_its_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            proc = Path(tmp)
            (proc / '20').mkdir()
            (proc / '20' / 'stat').write_text('20 (bash) S 10 0 0\n')
            (proc / '20' / 'cmdline').write_bytes(b'bash\0-ic\0true\0')
            sampler = wsl_daemon.Sampler(interval=0.01, proc=proc)
            sampler.watch(10)
            deadline = time.monotonic() + 5
            while not sampler.rows and time.monotonic() < deadline:
                time.sleep(0.01)
            result = sampler.result()
            self.assertGreaterEqual(result['loops'], 1)
            self.assertEqual([r['flag'] for r in result['flagged']], ['bash -ic'])
            loops = result['loops']
            time.sleep(0.05)
            self.assertEqual(sampler.result()['loops'], loops)  # stopped and joined

    def test_cs4_needs_a_sampler_pass(self):
        tools = {'checks': [{'id': 'tool.' + t} for t in ('claude', 'codex', 'opencode', 'pi')]}
        for loops, status in ((0, 'NOT_RUN'), (3, 'PASS')):
            with tempfile.TemporaryDirectory() as tmp:
                rows = daemon_rows.DaemonRows(lambda method, params: tools, lambda: {}, lambda *a, **k: None, tmp, 'wsl-fresh')
                with patch.object(daemon_rows, 'require_lock', NO_LOCK):
                    rows.cs4(sampled={'descendants': {'samples': 0, 'loops': loops, 'flagged': [], 'argv0': []}})
                row = json.loads((Path(tmp) / 'cases' / 'CS4.json').read_text())
                self.assertEqual(row['status'], status, loops)
                self.assertNotIn('samples', json.dumps(row['parts']))

    def test_install_setup_and_mcp_evidence_is_redacted(self):
        with tempfile.TemporaryDirectory() as tmp:
            data = Path(tmp) / 'devbox-linux'
            data.write_bytes(b'bin')
            m = {'linuxArtifacts': {'candidate': {'path': str(data), 'sha256': hashlib.sha256(b'bin').hexdigest()}}, 'acceptBinary': '/x'}
            secret = b'export GH_TOKEN=ghp_abcdef\nmail jihoon@example.com\nfailed\n'
            run = lambda argv, **kw: subprocess.CompletedProcess(argv, 70, b'', secret)
            with patch.object(wsl_daemon, 'require_lock', NO_LOCK), self.assertRaises(common.CommandFailed) as caught:
                wsl_daemon.install_binary(m, 'candidate', {'install': 'I', 'sha256': {'install': 'h'}}, run=run, home=Path(tmp))
            self.assertNotIn('ghp_abcdef', caught.exception.stderr + str(caught.exception))
            self.assertNotIn('jihoon@example.com', caught.exception.stderr)
            self.assertIn('failed', caught.exception.stderr)
        report = json.dumps({'steps': [{'id': 'env', 'ok': True, 'message': ''}]}).encode()
        run = lambda argv, **kw: subprocess.CompletedProcess(argv, 0, report, b'password: hunter2\nUtilAcceptVsock:273: accept4 failed 110\n')
        with patch.object(wsl_daemon, 'require_lock', NO_LOCK), patch.object(wsl_daemon, 'env_summary', lambda m: {'present': False}):
            row = wsl_daemon.setup({}, 'candidate', 'launch', {'exec': 'E'}, run=run)
        self.assertNotIn('hunter2', row['stderr'])
        self.assertIn('UtilAcceptVsock:273: accept4 failed 110', row['stderr'])  # attribution still counts it
        client = mcp_stdio.McpClient(lambda: {}, '/h', lambda row: None)
        reply = {'jsonrpc': '2.0', 'id': 2, 'result': {'content': [{'type': 'text', 'text': json.dumps(
            {'items': [{'id': 'a1', 'tokens': {'input': 5}, 'note': 'me@example.com', 'key': 'sk-abcdefgh12'}]})}]}}
        client._keep(json.dumps({'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call'}), json.dumps(reply))
        kept = '\n'.join(client.pairs[-1]['response'])
        for secret in ('me@example.com', 'sk-abcdefgh12'):
            self.assertNotIn(secret, kept)
        self.assertIn('"input": 5', kept)  # one line per value: only the matching lines are blanked
        self.assertIn('"id": "a1"', kept)

    def test_mcp_start_failure_is_not_run(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        (Path(tmp.name) / 'cases').mkdir()
        results = []

        def no_server(baseline=False):
            raise RuntimeError('MCP reply deadline: initialize')
        stub = Fake(cases=['H1', 'H1X-SHELL'], evidence=Path(tmp.name), pending=lambda case: True, journal_since=lambda t0: [],
                    case_result=lambda *a, **k: results.append(a), mcp=no_server, closed_mcp=[])
        stub.with_mcp = lambda function, baseline=False: driver.Session.with_mcp(stub, function, baseline)
        driver.Session.guarded(stub, ['H1', 'H1X-SHELL'], lambda _selected: stub.with_mcp(lambda client: self.fail('block ran')))
        self.assertEqual([(r[0], r[1]) for r in results], [('H1', 'NOT_RUN'), ('H1X-SHELL', 'NOT_RUN')])
        self.assertTrue(all('MCP' in r[3] for r in results))
        self.assertEqual(len(stub.closed_mcp), 1)

    def test_bytecode_refusal_and_child_python(self):
        with tempfile.TemporaryDirectory() as tmp:
            driver.refuse_bytecode(tmp)
            (Path(tmp) / '__pycache__').mkdir()
            with self.assertRaises(SystemExit):
                driver.refuse_bytecode(tmp)
        source = (BASE / 'driver.py').read_text()
        self.assertLess(source.index('refuse_bytecode(Path(__file__)'), source.index('from common import'))  # before any helper import
        self.assertIn('sys.dont_write_bytecode = True', source[:source.index('from common import')])
        children = re.findall(r"\['python3'[^\]]*", source)
        self.assertEqual(len(children), 2)  # validate.py, and g1/g2
        self.assertTrue(all(c.startswith("['python3', '-B',") for c in children), children)

    def test_setup_timeout_under_broken_interop(self):
        def run(argv, **kw):
            raise subprocess.TimeoutExpired(argv, 90, stderr=b'stuck in the login shell\n')
        with patch.object(wsl_daemon, 'require_lock', NO_LOCK), self.assertRaises(wsl_daemon.StepFailed) as caught:
            wsl_daemon.setup({}, 'candidate', 'launch', {'exec': 'E'}, run=run)
        failure = caught.exception.failure
        self.assertEqual((failure['shellInit'], failure['timeout']), ('setup-timeout', True))
        broken = matrix(interopBroken=True)
        evidence.record(broken, 'C1', 'FAIL', [], 'Step failed', failure=failure)
        self.assertEqual((broken['cases']['C1']['status'], broken['cases']['C1']['reason']), ('NOT_RUN', evidence.reason('R-IOP-STALL')))
        working = matrix(interopBroken=False)
        evidence.record(working, 'C1', 'FAIL', [], 'Step failed', failure=failure)
        self.assertEqual(working['cases']['C1']['status'], 'FAIL')


class FakeWatch:
    """A started console watcher: the lines it printed, alive or not, closed once."""

    def __init__(self, lines, alive=True, first=True):
        self.lines, self.stderr, self.closed = list(lines), [], 0
        self.child = Fake(poll=lambda: None if alive else 0)
        self.first = first

    def read_json(self, index, timeout):
        if not self.first:
            raise TimeoutError('Owned fixture line 0 not received')
        return json.loads(self.lines[index])

    def close(self, timeout=20):
        self.closed += 1
        return 0


def sample(i, ms, *windows):
    return json.dumps({'i': i, 'ms': ms, 'windows': [{'hwnd': h, 'pid': p, 'class': 'ConsoleWindowClass', 'name': n} for h, p, n in windows]})


class FixConsoleWatch(unittest.TestCase):
    """I3: W1-F (a) from one long-lived watcher; any new console window fails (a); the achieved interval is recorded and
    (a) is not proven (NOT_RUN) when it exceeds the matrix bound (W1 (바): every 500 ms)."""
    TERMINAL = (11, 100, 'WindowsTerminal')

    def watched(self, lines, alive=True):
        return w1_windows.ConsoleWatch(FakeWatch(lines, alive)).result()

    def test_cadence_and_new_windows(self):
        steady = [sample(i, i * 250.0, self.TERMINAL) for i in range(8)]
        result = self.watched(steady)
        self.assertEqual((result['part'], result['samples'], result['achievedIntervalMs'], result['boundMs']), (True, 8, 250.0, 500))
        slow = steady[:4] + [sample(4, 1450.0, self.TERMINAL)] + [sample(i, 1450.0 + (i - 4) * 250, self.TERMINAL) for i in range(5, 8)]
        result = self.watched(slow)
        self.assertEqual((result['part'], result['achievedIntervalMs'], result['overBound']), (None, 700.0, 1))  # fail closed: not proven
        self.assertIsNone(self.watched(steady[:3] + steady[4:])['part'])  # a skipped sample
        self.assertIsNone(self.watched(steady[:1])['part'])  # fewer than two samples
        self.assertIsNone(self.watched(steady, alive=False)['part'])  # the watcher ended before the run did
        self.assertIsNone(self.watched(steady + ['{"i": 8, "ms": 20'])['part'])  # a cut line
        flash = steady[:3] + [sample(3, 750.0, self.TERMINAL, (42, 4321, 'cmd'))] + steady[4:]
        result = self.watched(flash)
        self.assertIs(result['part'], False)  # a short flash of any new console window fails (a)
        self.assertEqual(result['newWindows'], [{'hwnd': 42, 'pid': 4321, 'class': 'ConsoleWindowClass', 'name': 'cmd', 'firstMs': 750.0}])
        self.assertIs(self.watched(flash[:3] + [sample(3, 1400.0, self.TERMINAL, (42, 4321, 'cmd'))])['part'], False)  # also over the bound

    def test_one_watcher_for_the_run(self):
        started = []
        win = Fake(console_watch=lambda nonce, seconds: started.append((nonce, seconds)) or FakeWatch([sample(0, 0.0)]))
        w1 = w1_windows.W1(None, lambda: {'windowsRoot': 'C:\\dev\\devbox-s7-accept-240f00aa-1234abcd'}, None, tempfile.mkdtemp(), win, answers(),
                           'marker', None)
        self.addCleanup(shutil.rmtree, str(w1.evidence))
        w1.start_consoles()
        self.assertEqual(started, [('240f00aa-1234abcd', w1_windows.WATCH['seconds'])])
        self.assertIsNotNone(w1.consoles)
        dead = FakeWatch([], first=False)
        w1.win = Fake(console_watch=lambda nonce, seconds: dead)
        w1.start_consoles()
        self.assertIsNone(w1.consoles)
        self.assertIn('TimeoutError', w1.console_error)
        self.assertEqual(dead.closed, 1)  # a watcher that never sampled is closed at once

    def test_watch_action_is_bounded_and_read_only(self):
        text = (BASE / 'native-observe.ps1').read_text()
        self.assertIn("'console-watch'", text.split('\n', 2)[1])  # in the ValidateSet
        block = text[text.index("if ($Action -eq 'console-watch')"):]
        block = block[:block.index('\n}\n')]
        for needed in ('$WatchSeconds -gt 3600', '$IntervalMs -gt 500', 'Add-Type -TypeDefinition $consoleType', '[Diagnostics.Stopwatch]::StartNew()',
                       '[Console]::Out.Flush()', 'OpenStandardInput', 'IsCompleted'):
            self.assertIn(needed, block)
        self.assertEqual(text.count('Add-Type -TypeDefinition $consoleType'), 2)  # one type, used by both console actions


# --- Fix round 2 (Rulings 83-85) -------------------------------------------------------------------
# Run 2's pi screen, verbatim from its K1X-SEND stop evidence: a passive update banner while pi keeps working.
RUN2_PI = ['', '─' * 200, ' Update Available                                 ', ' New version 1.4.0 is available. Run pi update    ',
           ' Changelog: https://pi.dev/changelog              ', '─' * 200, '', ' ⠹ Working                                        ', '',
           ' Muse Spark 1.3 Free | think:med | dir s7-2f19f39676a1-2 | ⎇ agent/s7-2f19f39676a1-2 ?1 | ◫ 31k/1.0M (3.0%) AC | LSP Inactive  ·   '
           'bg ⬆ v2.6.10 /bg-update ', '─' * 200, ' ' * 50, '─' * 200, ' ↳ Reply with OK and stop.', '']
# Run 2's H2X-TRUST capture, verbatim: codex printed its config error and the product's end line before any trust screen.
RUN2_CODEX_EXIT = ['Error loading config.toml: invalid type: map, expected a boolean', 'in `features`', '',
                   '[devbox] 에이전트가 끝났습니다(종료 코드 1).', '[devbox] 작업이 끝났습니다. 재개 버튼으로 다시 시작하세요.'] + [''] * 45


def screen_pane(screen, m=None):
    tmux = FakeTmux()
    tmux.screen = screen if isinstance(screen, str) else '\n'.join(screen)
    return agents_live.Pane(lambda: m or {'terminalIds': ['t1'], 'agents': {}}, answers(), NONCE, run=tmux, clock=FakeClock()), tmux


class FixR2StopMatches(unittest.TestCase):
    """Round 2 item 1: a stop keeps the pattern name, every captured line that matched (at most 5, each with 2 lines of
    context above and below) and the last 15 lines, all through redact() and bounded. The patterns are unchanged."""

    def stopped(self, lines, tool='opencode'):
        pane, _ = screen_pane(lines)
        with self.assertRaises(agents_live.StopPrompt) as caught:
            pane.capture('t1', tool)
        return caught.exception

    def test_a_match_above_the_last_15_lines_is_kept(self):
        lines = ['row %d' % i for i in range(60)]
        lines[9], lines[10] = 'token: abc', '  Paste your API key below'  # run 2: opencode matched above the last 15 lines
        error = self.stopped(lines)
        self.assertEqual(error.pattern, 'api-key')
        self.assertEqual(error.screen, agents_live.redact(lines[-15:]))  # as before
        self.assertEqual(error.match_count, 1)
        self.assertEqual(error.matches, [{'index': 10, 'line': '  Paste your API key below',
                                          'context': ['row 8', '[redacted]', '  Paste your API key below', 'row 11', 'row 12']}])

    def test_at_most_five_clamped_and_bounded(self):
        lines = ['Enter your API key %d' % i for i in range(7)] + ['x'] * 10 + ['api key ' + 'y' * 5000]
        error = self.stopped(lines)
        self.assertEqual((error.match_count, [m['index'] for m in error.matches]), (8, [0, 1, 2, 3, 4]))
        self.assertEqual(error.matches[0]['context'], lines[0:3])  # clamped at the top edge
        last = self.stopped(['x'] * 3 + [lines[-1]]).matches[0]
        self.assertEqual(len(last['context']), 3)  # clamped at the bottom edge
        self.assertLessEqual(max(len(line) for line in last['context']), agents_live.STOP_WIDTH)
        self.assertLessEqual(max(len(line) for line in self.stopped([lines[-1]]).screen), agents_live.STOP_WIDTH)

    def test_the_block_and_the_driver_keep_the_matches(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        tmux, clock = FakeTmux(), FakeClock()
        m = {'nonce': NONCE, 'terminalIds': ['t1'], 'agents': {}}
        pane = agents_live.Pane(lambda: m, answers(), NONCE, run=tmux, clock=clock)
        live = agents_live.Live(lambda method, params: {}, lambda: m, lambda *a, **k: None, Path(tmp.name), answers(), NONCE, pane, None,
                                clock=clock, home=tmp.name)
        tmux.screen = 'opencode 1.19\n Your API key is missing \n> '
        with patch.object(agents_live, 'require_lock', NO_LOCK):
            agents_live.run_block(live, 'opencode', {'H3'}, [('H3', lambda live, tasks: live.capture('H3', 't1', 'opencode'))])
        stop = json.loads((Path(tmp.name) / 'cases' / 'H3.json').read_text())['stop']
        self.assertEqual((stop['pattern'], stop['matchCount'], stop['matches'][0]['index']), ('api-key', 1, 1))
        self.assertEqual(stop['lines'], agents_live.redact(['opencode 1.19', ' Your API key is missing ', '> ']))
        results = []
        stub = Fake(cases=['H3'], evidence=Path(tmp.name), pending=lambda case: True, journal_since=lambda t0: [],
                    case_result=lambda *a, **k: results.append(a))

        def stop_here(_selected):
            raise agents_live.StopPrompt('Unexpected tool screen (api-key)', 'api-key', ['l'], [{'index': 0, 'line': 'l', 'context': ['l']}], 1)
        driver.Session.guarded(stub, ['H3'], stop_here)
        written = json.loads((Path(tmp.name) / 'cases' / 'H3-stop.json').read_text())
        self.assertEqual(written, {'pattern': 'api-key', 'lines': ['l'], 'matches': [{'index': 0, 'line': 'l', 'context': ['l']}], 'matchCount': 1})

    def test_patterns_unchanged(self):
        self.assertEqual([(name, p.pattern) for name, p in agents_live.STOP_PATTERNS],
                         [('update', '(?i)update available|new version|update now'), ('login', '/login|(?i:\\bsign in\\b|\\blog in\\b)'),
                          ('terms', '(?i)terms of (?:service|use)|privacy policy|accept (?:the )?terms'),
                          ('onboarding', '(?i)choose the text style|select login method|onboarding'), ('api-key', '(?i)\\bapi key\\b')])


class FixR2PiBanner(unittest.TestCase):
    """Round 2 item 2 (Ruling 83): pi's passive update banner, exactly its three lines, is the one allowed notice, for pi
    only, recorded as `passiveNotices`. Any other update text stops, for every tool, and so does the banner when another
    line also matches."""

    def test_run2_pi_capture_is_not_a_stop(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        m = {'nonce': NONCE, 'terminalIds': ['t1'], 'agents': {}}
        pane, _ = screen_pane(RUN2_PI, m)
        live = agents_live.Live(lambda method, params: {}, lambda: m, lambda *a, **k: None, Path(tmp.name), answers(), NONCE, pane, None,
                                clock=FakeClock(), home=tmp.name)
        pane.begin('K1X-SEND')
        self.assertEqual(live.capture('K1X-SEND', 't1', 'pi'), RUN2_PI[:-1])
        live.capture('K1X-SEND', 't1', 'pi')
        self.assertIsNone(pane.stopped)
        live.finish('K1X-SEND', 'PASS')
        row = json.loads((Path(tmp.name) / 'cases' / 'K1X-SEND.json').read_text())
        self.assertEqual(row['passiveNotices'], [{'lines': agents_live.redact(RUN2_PI[2:5]), 'seen': 2}])

    def test_boxed_or_twice_is_still_the_banner(self):
        boxed = ['│ Update Available                         │', '│  New version 12.40.3 is available. Run pi update │',
                 '┃ Changelog: https://pi.dev/changelog ┃', '│ ⠹ Working │']
        pane, _ = screen_pane(boxed + ['x'] + RUN2_PI)
        pane.begin('H4')
        pane.capture('t1', 'pi')
        self.assertEqual(len(pane.notices), 2)

    def test_everything_else_still_stops(self):
        cases = [(RUN2_PI + ['Update now? (y/n)'], 'pi', 'update'), (RUN2_PI, 'codex', 'update'), (RUN2_PI, 'opencode', 'update'),
                 (['codex 0.166', '  ✨ Update available! 0.166.0 -> 0.170.0', '  Run npm i -g @openai/codex to update'], 'codex', 'update'),
                 (['  ✨ Update available! 0.166.0 -> 0.170.0'], 'pi', 'update'),
                 ([line.replace('1.4.0', '1.4') for line in RUN2_PI], 'pi', 'update'),
                 ([line.replace('1.4.0', 'v1.4.0') for line in RUN2_PI], 'pi', 'update'),
                 (RUN2_PI[:4] + ['x'] + RUN2_PI[4:], 'pi', 'update'),  # not three lines in a row
                 ([line.replace('Update Available', 'Update Available - press u to update now') for line in RUN2_PI], 'pi', 'update'),
                 (RUN2_PI + ['  /login to continue'], 'pi', 'login')]  # the other patterns are still checked after the exemption
        for lines, tool, pattern in cases:
            pane, _ = screen_pane(lines)
            with self.assertRaises(agents_live.StopPrompt, msg=(tool, lines[-1])) as caught:
                pane.capture('t1', tool)
            self.assertEqual(caught.exception.pattern, pattern, (tool, lines[-1]))
        pane, _ = screen_pane(RUN2_PI + ['Update now? (y/n)'])
        with self.assertRaises(agents_live.StopPrompt) as caught:
            pane.capture('t1', 'pi')
        self.assertEqual([m['index'] for m in caught.exception.matches], [2, 3, 15])  # the banner lines count when it stops

    def test_readme_names_the_one_allowed_notice(self):
        text = (BASE / 'README.md').read_text()
        self.assertIn('an allowed notice', text)
        self.assertIn('New version <x.y.z> is available. Run pi update', text)


class ClockTmux(FakeTmux):
    """A pane whose screen depends on the virtual time."""

    def __init__(self, clock, screens):
        super().__init__()
        self.clock, self.screens = clock, screens

    def __call__(self, argv, capture_output=True, timeout=10):
        self.screen = self.screens(self.clock.now)
        return super().__call__(argv, capture_output, timeout)


class TrustRowsHelper:
    def live(self, screens, waiting_after=None, task=None):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        clock = FakeClock()
        self.tmux = ClockTmux(clock, screens)
        self.m = {'nonce': NONCE, 'terminalIds': [], 'agents': {'a1': {'terminalIds': ['t2']}}, 'projectIdByName': {'P-TOOLS': 'p1'},
                  'projectPaths': {'P-TOOLS': '/f/P-TOOLS'}}
        self.calls, self.mcp_sent = [], []
        start = clock.now

        def rpc(method, params):
            self.calls.append(method)
            if method == 'projects.trust_preview':
                return {'projectId': 'p1', 'requiresApproval': False}
            if method == 'agents.create':
                return {'id': 'a1', 'projectId': 'p1', 'state': 'preparing'}
            if method == 'agents.get':
                state = 'waiting' if waiting_after is not None and clock.now - start >= waiting_after else 'running'
                return dict({'id': 'a1', 'terminalId': 't2', 'state': state, 'worktreePath': '/f/P-TOOLS-agents/x'}, **(task or {}))
            if method == 'agents.send':
                raise RpcError('agents.awaiting_input')
            raise AssertionError(method)
        pane = agents_live.Pane(lambda: self.m, answers(), NONCE, run=self.tmux, clock=clock)
        self.client = Fake(pairs=[], send=lambda agent_id, text: self.mcp_sent.append(text) or ('refused', '에이전트가 선택을 기다리고 있습니다. 화면에서 답하세요.'))
        self.root = Path(tmp.name)
        return agents_live.Live(rpc, lambda: self.m, lambda *a, **k: None, self.root, answers(), NONCE, pane, None, clock=clock, home=self.root)

    def row(self, case):
        return json.loads((self.root / 'cases' / (case + '.json')).read_text())


class FixR2TrustRows(TrustRowsHelper, unittest.TestCase):
    """Round 2 items 3 and 4 (Ruling 85, N2; the bound is 7.0 s since Ruling 86): the trust rows' (b) bound is measured
    from the first capture that showed the trust screen, hook-detected waits keep 2 s, and `checks` is in the evidence. A tool that ends before its trust screen
    is NOT_RUN with the toolExit sentence and its last screen lines. The codex hash is read before agents.create, and the
    claude flag and the codex hash are recorded on every path."""

    def test_bound_is_from_the_first_trust_screen(self):
        # The trust screen appears 1.0 s after the start. `waiting` 6.6 s after it (the product's "2 to about 6 s", with a
        # third watcher tick: review 3 R3-I1) is within 7.0 s; 7.4 s after it is not.
        screens = lambda now: CLAUDE_TRUST if now >= 1001.0 else ''
        for after, status in ((7.6, 'PASS'), (8.4, 'FAIL')):
            live = self.live(screens, waiting_after=after)
            live.pane.begin('H1X-TRUST')
            agents_live.trust_row(live, 'H1X-TRUST', 'claude', {'id': 'a1'}, self.client, {})
            row = self.row('H1X-TRUST')
            self.assertEqual(row['status'], status, after)
            self.assertEqual(row['attentionBoundSeconds'], agents_live.TIMEOUTS['trustAttention'])
            self.assertEqual(row['checks'], [{'attention': status == 'PASS', 'send': True, 'mcp': True, 'mcpRefused': True, 'input0': True}])
        self.assertEqual(agents_live.TIMEOUTS['trustAttention'], 7.0)

    def test_hook_waits_keep_two_seconds(self):
        live = self.live(lambda now: APPROVAL, waiting_after=2.5)
        live.pane.begin('P1')  # claude's permission prompt, which its hook reports (P1X's codex overlay is output-detected)
        checks = agents_live.waiting_checks(live, 'P1', 'claude', 'a1', 't2', self.client)
        self.assertFalse(checks['attention'])
        self.assertEqual(live.data['P1']['checks'], [checks])
        self.assertEqual(agents_live.TIMEOUTS['attention'], 2)

    def test_checks_recorded_when_nothing_is_sent(self):
        live = self.live(lambda now: CLAUDE_TRUST)
        live.pane.begin('H1X-TRUST')
        agents_live.trust_row(live, 'H1X-TRUST', 'claude', {'id': 'a1'}, self.client, {})
        self.assertEqual(self.row('H1X-TRUST')['checks'], [{'attention': False, 'send': False, 'mcp': False, 'mcpRefused': False, 'input0': False}])

    def test_tool_exit_before_the_trust_screen(self):
        live = self.live(lambda now: '\n'.join(RUN2_CODEX_EXIT))
        live.pane.begin('H2X-TRUST')
        self.assertFalse(agents_live.trust_row(live, 'H2X-TRUST', 'codex', {'id': 'a1'}, self.client, {}, config_before='h0'))
        row = self.row('H2X-TRUST')
        self.assertEqual((row['status'], row['reason']), ('NOT_RUN', evidence.ROW_TEXT['H2X-TRUST.toolExit'].format(code=1)))
        self.assertEqual(evidence.ROW_TEXT['H2X-TRUST.toolExit'], '도구가 신뢰 화면을 보이기 전에 끝났다(종료 코드 {code}). 마지막 화면 줄을 증거에 남겼다.')
        self.assertEqual(evidence.ROW_TEXT['H1X-TRUST.toolExit'], evidence.ROW_TEXT['H2X-TRUST.toolExit'])
        self.assertEqual(row['toolExit'], {'code': 1, 'lastLines': agents_live.redact(RUN2_CODEX_EXIT[:5])})
        self.assertEqual(row['configSha256'], {'before': 'h0', 'after': None, 'unchanged': False})  # no config file in the temp home
        self.assertLess(self.tmux.clock.now - 1000.0, agents_live.TIMEOUTS['screen'])  # it did not wait out the 30 s
        self.assertNotIn('agents.send', self.calls)
        # An ended task with no end line on the pane: the code comes from the task's reason.
        live = self.live(lambda now: 'codex\n', task={'state': 'failed', 'reason': 'exit:2', 'toolExited': True})
        live.pane.begin('H1X-TRUST')
        agents_live.trust_row(live, 'H1X-TRUST', 'claude', {'id': 'a1'}, self.client, {})
        row = self.row('H1X-TRUST')
        self.assertEqual((row['reason'], row['toolExit']['code']), (evidence.ROW_TEXT['H1X-TRUST.toolExit'].format(code=2), 2))
        self.assertEqual(row['trustEntryPresent'], {'project': False, 'worktree': False})
        self.assertFalse(row['trustFlagSet'])

    def test_absent_path_records_the_flag_and_the_hash(self):
        for tool, case in (('claude', 'H1X-TRUST'), ('codex', 'H2X-TRUST')):
            live = self.live(lambda now: 'still starting\n')
            live.pane.begin(case)
            agents_live.trust_row(live, case, tool, {'id': 'a1'}, self.client, {}, config_before='h0' if tool == 'codex' else None)
            row = self.row(case)
            self.assertEqual((row['status'], row['reason']), ('NOT_RUN', evidence.ROW_TEXT[case + '.absent']), tool)
            if tool == 'claude':
                self.assertEqual((row['trustEntryPresent'], row['trustFlagSet']), ({'project': False, 'worktree': False}, False))
            else:
                self.assertEqual(row['configSha256'], {'before': 'h0', 'after': None, 'unchanged': False})

    def test_codex_hash_is_read_before_create(self):
        live = self.live(lambda now: '\n'.join(RUN2_CODEX_EXIT))
        live.codex_config_sha256 = lambda: self.calls.append('hash') or 'h0'
        with patch.object(agents_live, 'require_lock', NO_LOCK):
            agents_live.codex_block(live, {'H2X-TRUST'}, self.client)
        self.assertLess(self.calls.index('hash'), self.calls.index('agents.create'))
        self.assertEqual(self.row('H2X-TRUST')['configSha256']['before'], 'h0')


START_SCOPED = """#!/usr/bin/env bash
# devbox 에이전트 시작 스크립트(자동 생성)
export DEVBOX_BIN='/b/devbox'
export DEVBOX_AGENT_ID='a1'
export DEVBOX_PROMPT_FILE='/d/prompt.md'
cd '/w/x y' || { printf 'setup:1\\n' > '/d/phase'; '/b/devbox' agent event --from setup --code 1; hold_terminal; }
printf 'tool\\n' > '/d/phase'
'/b/devbox' agent event --from setup --code 0
export PATH='{front}':"$PATH"
systemd-run --user --scope --quiet --expand-environment=no --unit 'devbox-accept-agent-a1' -- codex -c 'notify=["/b/devbox"]' -- "$(cat '/d/prompt.md')"
code=$?
printf 'exit:%s\\n' "$code" > '/d/phase'
echo "[devbox] 에이전트가 끝났습니다(종료 코드 $code)."
hold_terminal
"""


class FixR2ToolBinary(TrustRowsHelper, unittest.TestCase):
    """Round 2 item 5 (P-DEF-1 evidence): from the owned start.sh (crates/agents/src/prepare.rs:220, start.rs:28-136)
    only the `export PATH=` lines, the program the tool line runs, and the realpath of <PATH-front folder>/<program> for
    a bare name. Never executed; nothing else is recorded."""

    def test_shapes(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        front = Path(tmp.name) / 'node' / 'bin'
        front.mkdir(parents=True)
        (Path(tmp.name) / 'codex.js').write_text('x')
        (front / 'codex').symlink_to(Path(tmp.name) / 'codex.js')
        parsed = agents_live.start_script_binary(START_SCOPED.replace('{front}', str(front)))
        self.assertEqual(parsed, {'pathLines': ["export PATH='%s':\"$PATH\"" % front], 'program': 'codex',
                                  'resolved': os.path.realpath(Path(tmp.name) / 'codex.js')})
        shared = START_SCOPED.replace("export PATH='{front}':\"$PATH\"", "export PATH=\"$PATH\":'/usr/bin'").replace(
            '-- codex -c', "-- '/usr/bin/codex' -c")
        self.assertEqual(agents_live.start_script_binary(shared), {'pathLines': ["export PATH=\"$PATH\":'/usr/bin'"], 'program': '/usr/bin/codex',
                                                                   'resolved': None})
        bare = START_SCOPED.replace("export PATH='{front}':\"$PATH\"\n", '').replace(
            "systemd-run --user --scope --quiet --expand-environment=no --unit 'devbox-accept-agent-a1' -- ", '')
        self.assertEqual(agents_live.start_script_binary(bare), {'pathLines': [], 'program': 'codex', 'resolved': None})
        self.assertEqual(agents_live.start_script_binary('#!/bin/sh\necho x\n')['program'], None)

    def test_read_once_per_real_tool_task(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        home = Path(tmp.name)
        script = home / '.local/share/devbox/accept/agents/a1/start.sh'
        script.parent.mkdir(parents=True)
        script.write_text(START_SCOPED.replace('{front}', '/opt/x/bin') + 'export PATH=secret\n')
        m = {'nonce': NONCE, 'terminalIds': [], 'agents': {}}
        get = {'id': 'a1', 'state': 'preparing'}
        live = agents_live.Live(lambda method, params: dict(get), lambda: m, lambda *a, **k: None, home, answers(), NONCE, screen_pane('')[0], None,
                                clock=FakeClock(), home=home)
        live.task_tools['a1'], live.task_cases['a1'] = 'codex', 'H2X-TRUST'
        live.get('a1')
        self.assertNotIn('H2X-TRUST', live.data)  # no terminal yet
        get['terminalId'] = 't2'
        live.get('a1')
        script.write_text('changed\n')
        live.get('a1')
        self.assertEqual(live.data['H2X-TRUST']['toolBinary'], [{'agentId': 'a1', 'tool': 'codex', 'program': 'codex',
                                                                 'pathLines': ["export PATH='/opt/x/bin':\"$PATH\"", '[redacted]'],
                                                                 'resolved': '/opt/x/bin/codex'}])
        live.task_tools['b2'], live.task_cases['b2'] = 'pi', 'H4'
        live.read_tool_binary('b2')
        self.assertIn('error', live.data['H4']['toolBinary'][0])  # a missing file is recorded, never raised
        live.read_tool_binary('../a1')
        live.read_tool_binary('zz')
        self.assertEqual(set(live.data), {'H2X-TRUST', 'H4'})  # unknown ids are never read

    def test_tool_exit_row_keeps_the_binary(self):
        # Run 2's H2X-TRUST: the codex that exited is exactly what P-DEF-1 needs on record.
        live = self.live(lambda now: '\n'.join(RUN2_CODEX_EXIT))
        script = self.root / '.local/share/devbox/accept/agents/a1/start.sh'
        script.parent.mkdir(parents=True)
        script.write_text(START_SCOPED.replace('{front}', '/home/u/.local/lib/node-v24.18.0-linux-x64/bin'))
        live.task_tools['a1'], live.task_cases['a1'] = 'codex', 'H2X-TRUST'
        live.pane.begin('H2X-TRUST')
        agents_live.trust_row(live, 'H2X-TRUST', 'codex', {'id': 'a1'}, self.client, {}, config_before='h0')
        binary = self.row('H2X-TRUST')['toolBinary'][0]
        self.assertEqual((binary['program'], binary['resolved']), ('codex', '/home/u/.local/lib/node-v24.18.0-linux-x64/bin/codex'))


class FixR2Probe(unittest.TestCase):
    """Round 2 item 6: CS4 flags any descendant whose argv holds `devbox-probe` (the probe's $0, command_path.rs:181) or an
    agent CLI `--version`; `bash -i -c` stays flagged too (matrix CS4 (a))."""

    def test_both_shapes(self):
        script = 'printf "\\036%s\\036" "$(command -v -- "$1")"'
        flagged = lambda *argv: wsl_daemon.flagged([w.encode() for w in argv])
        self.assertEqual(flagged('bash', '-lic', script, 'devbox-probe', 'codex'), 'devbox-probe')  # probe_command's argv
        self.assertEqual(flagged('/usr/bin/sh', '-c', script, 'devbox-probe', 'pi'), 'devbox-probe')  # no bash -ic at all
        self.assertEqual(flagged('/home/u/.local/bin/codex', '--version'), 'codex --version')
        self.assertEqual(flagged('bash', '-i', '-c', 'claude --version'), 'bash -ic')
        for argv in (('bash', '-lc', 'true'), ('tmux', '-V'), ('/usr/bin/git', 'version'), ('bash', '-c', 'echo devbox-probes')):
            self.assertIsNone(flagged(*argv), argv)


def daemon_error(code, detail):
    """The real owned-RPC exception (wsl-owned.py RpcError, raised in-process by rpc_owned) for the daemon's wire error
    {"code", "detail"} (crates/protocol/src/error.rs:13, fixtures/envelopes.json:13): CL3 (a) reads `.error.detail.path`."""
    return wsl_owned.RpcError({'code': code, 'detail': detail})


class FakeClose(FakeAgents):
    """Round 2 items 7 and 8: the product's designed close paths (crates/agents/src/actions.rs:402-540, S7-13 Fix D). The
    squash merge runs the automatic cleanup (cleanup_after_close): a passing teardown removes the folder and branch, a
    failing one leaves cleanupError teardown_failed and a candidate, an initialized submodule is refused with
    worktree_has_submodules and kept. A discard whose teardown fails is refused with teardown_failed; the task stays open."""

    def __init__(self, root):
        super().__init__(root)
        self.branches, self.registered = set(), set()

    def project_dir(self, project_id):
        return self.root / ('P-SUB' if project_id == 'id-p-sub' else 'P-CL')

    def failing(self, project_id='id-p-cl'):
        toml = self.project_dir(project_id) / '.devbox' / 'devbox.toml'
        return toml.exists() and 'exit 3' in toml.read_text()

    def agents_create(self, params):
        agent_id = 'a%d' % (len(self.tasks) + 1)
        tree = self.root / (self.project_dir(params['projectId']).name + '-agents') / params['branch']
        self.tasks[agent_id] = {'id': agent_id, 'projectId': params['projectId'], 'state': 'preparing', 'worktreePath': str(tree),
                                'branch': 'agent/' + params['branch'], 'polls': 0, 'cleanupError': None, 'closedHow': None}
        self.branches.add('agent/' + params['branch'])
        self.registered.add(str(tree))
        return self.view(agent_id)

    def teardown(self, task):
        if (self.project_dir(task['projectId']) / '.devbox' / 'devbox.toml').exists():
            (Path(task['worktreePath']) / 's7-teardown.txt').write_text('s7\n')
        return not self.failing(task['projectId'])

    def remove(self, task):
        shutil.rmtree(task['worktreePath'])
        self.registered.discard(task['worktreePath'])
        self.branches.discard(task['branch'])

    def agents_discard_check(self, params):
        task = self.tasks[params['id']]
        return {'headOid': 'h', 'unmergedCommits': 0, 'fingerprint': 'f', 'hasSubmodules': (Path(task['worktreePath']) / 'sub' / '.git').exists()}

    def agents_merge(self, params):
        task = self.tasks[params['id']]
        task.update(state='closed', closedHow='merged')
        if (Path(task['worktreePath']) / 'sub' / '.git').exists():
            task['cleanupError'] = json.dumps({'code': 'agents.worktree_has_submodules', 'detail': {'path': task['worktreePath']}})
        elif not self.teardown(task):
            task['cleanupError'] = json.dumps({'code': 'agents.teardown_failed', 'detail': {'index': 1, 'exit_code': 3}})
        else:
            self.remove(task)
        return self.view(params['id'])

    def agents_discard(self, params):
        task = self.tasks[params['id']]
        if not self.teardown(task):
            raise daemon_error('agents.teardown_failed', {'index': 1, 'exit_code': 3})
        task.update(state='closed', closedHow='discarded')
        self.remove(task)
        return self.view(params['id'])

    def agents_cleanup_candidates(self, params):
        closed = [t for t in self.tasks.values() if t['projectId'] == params['projectId'] and t['state'] == 'closed' and Path(t['worktreePath']).exists()]
        held = [t for t in closed if 'worktree_has_submodules' in (t['cleanupError'] or '')]
        free = [t for t in closed if t not in held]
        trees, branches = [t['worktreePath'] for t in free], [t['branch'] for t in free if t['branch'] in self.branches]
        return {'worktrees': trees, 'branches': branches, 'offBase': trees + branches, 'submodules': [t['worktreePath'] for t in held]}

    def agents_cleanup(self, params):
        for tree in params['worktrees']:
            task = next(t for t in self.tasks.values() if t['worktreePath'] == tree)
            if 'worktree_has_submodules' in (task['cleanupError'] or ''):
                raise daemon_error('agents.worktree_has_submodules', {'path': tree})
            if not self.teardown(task):
                raise daemon_error('agents.teardown_failed', {'index': 1, 'exit_code': 3})
            self.remove(task)
        return {}

    def terminal_create(self, params):
        return {'id': 't-cl3'}

    def terminal_close(self, params):
        return {}

    def git(self, path, args):
        """git_owned in the test: the fake's own branch and worktree registry; `submodule update --init` initializes."""
        if args[:2] == ('branch', '--list'):
            return completed(args, args[2] + '\n' if args[2] in self.branches else '')
        if args[:2] == ('worktree', 'list'):
            return completed(args, ''.join('worktree %s\n\n' % t for t in sorted(self.registered) if Path(t).parent.name == Path(path).name + '-agents'))
        if 'submodule' in args and 'update' in args:
            (Path(path) / 'sub').mkdir()
            (Path(path) / 'sub' / '.git').write_text('gitdir: x\n')
        return completed(args)


class FixR2ClRows(unittest.TestCase):
    """Round 2 items 7 and 8 on the designed paths, with one preview right before each action as round 1 pinned."""
    ACTIONS, READS, previewed = FixClRows.ACTIONS, FixClRows.READS, FixClRows.previewed

    def run_rows(self, name, pane=None):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = self.root = Path(tmp.name)
        for project in ('P-CL', 'P-SUB'):
            (root / project).mkdir()
        (root / 'P-CL' / '.devbox').mkdir()
        (root / 'P-CL' / '.devbox' / 'devbox.toml').write_text(fixture.P_CL_TOML)
        m = {'nonce': NONCE, 'projectPaths': {'P-CL': str(root / 'P-CL'), 'P-SUB': str(root / 'P-SUB'), 'sub-src': str(root / 'sub-src')},
             'projectIdByName': {'P-CL': 'id-p-cl', 'P-SUB': 'id-p-sub'}}
        daemon = FakeClose(root)
        rows = daemon_rows.DaemonRows(daemon, lambda: m, lambda *a, **k: None, root, 'wsl-fresh', pane=pane, clock=FakeClock())
        with patch.object(daemon_rows, 'require_lock', NO_LOCK), patch.object(daemon_rows, 'git_owned', lambda path, *args, **kw: daemon.git(path, args)):
            getattr(rows, name)()
        return daemon, json.loads((root / 'cases' / (name.upper() + '.json')).read_text()), root

    def test_cl1_designed_paths(self):
        daemon, row, root = self.run_rows('cl1')
        tree = lambda n: str(root / 'P-CL-agents' / ('s7-%s-%d' % (NONCE, n)))
        ctx = common.trust_context
        self.assertEqual(self.previewed(daemon), [
            ctx('id-p-cl', 'create', worktree=True), ctx('id-p-cl', 'merge', agent_id='a1'),  # (a)
            ctx('id-p-cl', 'create', worktree=True), ctx('id-p-cl', 'discard', agent_id='a2'),  # (d)
            ctx('id-p-cl', 'create', worktree=True), ctx('id-p-cl', 'merge', agent_id='a3'),  # (c)
            ctx('id-p-cl', 'create', worktree=True), ctx('id-p-cl', 'discard', agent_id='a4'),  # record only: refused
            ctx('id-p-cl', 'discard', agent_id='a4'),  # after the fix, so nothing stays open
            ctx('id-p-cl', 'cleanup', worktrees=[tree(502)])])  # (b): exactly (c)'s folder
        self.assertEqual((row['status'], row['parts']), ('PASS', {'a': True, 'b': True, 'c': True, 'd': True}))
        self.assertEqual(row['c']['cleanupError'], json.dumps({'code': 'agents.teardown_failed', 'detail': {'index': 1, 'exit_code': 3}}))
        record = row['failingDiscard']
        self.assertEqual((record['discard']['code'], record['stateAfter10s'], record['afterFix']['state']), ('agents.teardown_failed', 'running', 'closed'))
        self.assertEqual({t['state'] for t in daemon.tasks.values()}, {'closed'})
        self.assertNotIn('exit 3', (root / 'P-CL' / '.devbox' / 'devbox.toml').read_text())  # left passing for CL2 and the cleanup
        self.assertEqual(daemon.registered, set())

    def test_cl1_restores_the_passing_teardown_on_error(self):
        original = FakeAgents.agents_review

        def broken(daemon, params):
            if daemon.failing():
                raise RuntimeError('review broke')
            return original(daemon, params)
        with patch.object(FakeClose, 'agents_review', broken):
            with self.assertRaises(RuntimeError):
                self.run_rows('cl1')
        self.assertNotIn('exit 3', (self.root / 'P-CL' / '.devbox' / 'devbox.toml').read_text())  # a mid-row error leaves it passing

    def test_cl3_keeps_its_folder_for_the_refusal(self):
        typed = []
        holder = {}

        def enter(tid, key, rule=None, project=None):
            typed.append(key)
            holder['daemon'].registered.discard(holder['tree'])
            shutil.rmtree(holder['tree'])
        pane = Fake(begin=lambda case: typed.append(case), type_text=lambda tid, text, cl3_folder=None: typed.append(text), key=enter)
        original = FakeClose.agents_create

        def create(self, params):
            reply = original(self, params)
            holder.update(daemon=self, tree=reply['worktreePath'])
            return reply
        with patch.object(FakeClose, 'agents_create', create):
            daemon, row, root = self.run_rows('cl3', pane=pane)
        folder = str(root / 'P-SUB-agents' / ('s7-%s-500' % NONCE))
        self.assertEqual(typed, ['CL3', "git worktree remove --force '%s'" % folder, 'Enter'])
        self.assertEqual(json.loads(row['closed']['cleanupError'])['code'], 'agents.worktree_has_submodules')
        self.assertEqual(row['candidates']['submodules'], [folder])
        self.assertEqual(row['cleanup']['code'], 'agents.worktree_has_submodules')
        self.assertIn("git worktree remove --force '%s'" % folder, row['cleanup']['appMessage'])
        self.assertEqual(row['shell'], {'before': {'folderExists': True, 'listed': True}, 'listedAfter': False, 'folderExists': False})
        self.assertEqual((row['status'], row['parts']), ('NOT_RUN', {'a': True, 'b': True, 'c': True}))
        methods = [c[0] for c in daemon.calls]
        self.assertNotIn('agents.discard', methods)
        self.assertLess(methods.index('agents.discard_check'), methods.index('agents.merge'))

    def test_cl3_needs_the_folder_before_the_command(self):
        typed = []
        pane = Fake(begin=lambda case: typed.append(case), type_text=lambda *a, **k: typed.append('text'), key=lambda *a, **k: typed.append('key'))
        original = FakeClose.agents_merge

        def merge_and_remove(self, params):
            reply = original(self, params)
            self.remove(self.tasks[params['id']])  # as if the folder were already gone (run 2's discard)
            return reply
        with patch.object(FakeClose, 'agents_merge', merge_and_remove):
            daemon, row, root = self.run_rows('cl3', pane=pane)
        self.assertEqual(typed, [])  # nothing typed on a folder that is not there
        self.assertEqual(row['status'], 'FAIL')
        self.assertFalse(row['parts']['a'])
        self.assertFalse(row['parts']['b'])


class FixR3OutputBound(unittest.TestCase):
    """Round 3 item 2 (R3-M2): codex's output-detected waits, H2X-RESUME (b), H2X-SCREENS and P1X (a), pass the same
    bound as the folder-trust rows; claude's hook-detected permission prompt (approval_row: P1, U1 추가) keeps 2 s."""

    def test_each_caller_passes_its_bound(self):
        calls = {}

        def visit(node, function):
            for child in ast.iter_child_nodes(node):
                name = child.name if isinstance(child, ast.FunctionDef) else function
                if isinstance(child, ast.Call) and getattr(child.func, 'id', None) == 'waiting_checks':
                    case = child.args[1].value if isinstance(child.args[1], ast.Constant) else name
                    calls[case] = {k.arg: ast.unparse(k.value) for k in child.keywords}
                visit(child, name)
        visit(ast.parse((BASE / 'agents_live.py').read_text()), None)
        output = "TIMEOUTS['trustAttention']"
        self.assertEqual(calls, {'trust_row': {'bound': output, 'since': "seen['screen']"}, 'H2X-RESUME': {'bound': output},
                                 'H2X-SCREENS': {'bound': output}, 'P1X': {'bound': output},
                                 'approval_row': {}, 'H1X-SHELL': {}, 'H2X-FEEDBACK': {}, 'H3X-SHELL': {}})
        # No bound is the hook-detected 2 s, from the call.
        self.assertIsNone(inspect.signature(agents_live.waiting_checks).parameters['bound'].default)
        self.assertEqual((agents_live.TIMEOUTS['attention'], agents_live.TIMEOUTS['trustAttention']), (2, 7.0))


class FixR3Mask(unittest.TestCase):
    """Round 3 item 3 (R3-M1): key-shaped tokens are masked inside a line. The whole-line rule is unchanged, the phrase
    "API key" stays readable (round 2 item 1 needs it), and OIDs, sha256 hex, ids and fixture paths stay as they are."""
    KEYS = {'sk': 'sk-ant-api03-' + 'Ab9_x-' * 8, 'proj': 'sk-proj-' + 'x1Y2z3' * 4, 'ghp': 'ghp_' + 'A1b2' * 9, 'gho': 'gho_' + 'c' * 20,
            'ghu': 'ghu_' + 'D4' * 12, 'ghs': 'ghs_' + 'e5' * 15, 'ghr': 'ghr_' + 'F6' * 11, 'pat': 'github_pat_' + '11ABCDEFG0_' * 4,
            'slack': 'xoxb-' + '1234567890-abcdefghij', 'aws': 'AKIA' + 'IOSFODNN7EXAMPLE', 'google': 'AIza' + 'SyD-' + 'a1_B' * 7 + 'c2d',
            'uuid': '123e4567-e89b-12d3-a456-426614174000'}

    def test_each_shape_is_masked(self):
        for name, key in self.KEYS.items():
            for line, masked in (('export ANTHROPIC_AUTH=%s' % key, 'export ANTHROPIC_AUTH=[masked]'), ('using "%s" now' % key, 'using "[masked]" now'),
                                 ('(%s)' % key, '([masked])')):
                self.assertEqual(common.redact([line]), [masked], name)
                self.assertNotIn(key, common.redact_text(line) + json.dumps(common.redact_json({'auth': key})), name)
        self.assertEqual(len(self.KEYS['google']), 39)
        for line, masked in (('ssh key=hunter2 rest', 'ssh key=[masked] rest'), ('the key: v4lue', 'the key: [masked]'),
                             ('AWS_ACCESS_KEY_ID: ' + self.KEYS['aws'], 'AWS_ACCESS_KEY_ID: [masked]')):
            self.assertEqual(common.redact([line]), [masked])
        # The value after token, secret or password is masked too; redact() still blanks those lines whole, as before.
        for word in ('key', 'token', 'secret', 'password'):
            self.assertEqual(common.mask_line('my %s = abc123 x' % word), 'my %s = [masked] x' % word)
            self.assertEqual(common.mask_line('my %s:abc123' % word), 'my %s:[masked]' % word)
        self.assertEqual(common.redact(['session token: abc', 'Bearer x', 'sk-abcdefgh1', 'mail me@example.com']),
                         ['[redacted]', '[redacted]', '[redacted]', 'mail [email]'])

    def test_api_key_phrase_stays_readable(self):
        for line in (' Your API key is missing ', 'Enter your API key:', '  Paste your API key below', 'Set an API key in settings'):
            self.assertEqual(common.redact([line]), [line])
        self.assertEqual(common.redact(['API key: ' + self.KEYS['sk']]), ['API key: [masked]'])
        self.assertEqual(agents_live.bounded(['  API key: ' + self.KEYS['proj'] + ' (saved)']), ['  API key: [masked] (saved)'])

    def test_ids_hashes_and_fixture_paths_stay(self):
        oid, digest = '0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c', hashlib.sha256(b's7').hexdigest()
        hexid = '0123456789abcdef0123456789abcdef'
        lines = ['commit ' + oid, '"headOid": "%s",' % oid, 'sha256 ' + digest, '"installerSha256": "%s"' % digest, 'agent a' + hexid,
                 'terminal t' + hexid, 'project ' + hexid, '/tmp/devbox-s7-fixture-0123abcd-89abcdef/P-SUB-agents/s7-0123456789ab-500',
                 'runs/0123abcd-89abcdef/cases/CL3.json', 'agent/s7-0123456789ab-501',
                 'devbox-accept-agent-a' + hexid + '.scope', '"unit": "devbox-accept-run-%s.service"' % hexid]
        self.assertEqual(common.redact(lines), lines)
        # The mask needs a token boundary before `sk-`; the unchanged whole-line rule (`sk-[a-z0-9]{8}`) still blanks it.
        self.assertEqual(common.mask_line('task-0123456789abcdef0123 is done'), 'task-0123456789abcdef0123 is done')
        self.assertEqual(common.redact(['task-0123456789abcdef0123 is done']), ['[redacted]'])


class FixR3Readme(unittest.TestCase):
    """Round 3 item 4 (R3-M3): the README names the new candidate's run, not run 2's."""

    def test_no_stale_candidate_run(self):
        text = (BASE / 'README.md').read_text()
        self.assertNotIn('41377466587', text)
        self.assertIn('CANDRUN=41423905881', text)


# --- Fix round 4 (Ruling 88) -----------------------------------------------------------------------
# Run 3's K1X-SEND stop capture (runs/40d97a83-a4e9f78c/cases/K1X-SEND.json), verbatim where its stop evidence kept it:
# the first three rows (stop.matches[0].context) and the last 15 (stop.lines). pi's update banner had lost its first
# line above the capture; RUN3_PI_ABOVE is that line as the case's passiveNotices kept it. The rows between were not
# recorded (matchCount 1: none of them matched `update`), nor was their number. RUN3_PI fills the 60-line window with
# blank rows; run 3's kept captures (K1X-SEND, H1X-TRUST, H2X-TRUST) are all 50 rows, the pane with no history above.
RUN3_PI_ABOVE = [' Update Available'.ljust(50)]
RUN3_PI_TOP = [' New version 1.5.0 is available. Run pi update'.ljust(50), ' Changelog: https://pi.dev/changelog'.ljust(50), '─' * 200]
RUN3_ERROR = ' Error: opencode API error (400): {"type":"server_error","message":"Upstream request failed: Model is unavailable."}'.ljust(200)
RUN3_PI_TAIL = [RUN3_ERROR, '', RUN3_ERROR, '', RUN3_ERROR, '', ' ⠋ Retrying (3/3) in 8s... (escape to cancel)'.ljust(50), '',
                ' Muse Spark 1.3 Free | think:med | dir s7-cc0a73ba411e-2 | ⎇ agent/s7-cc0a73ba411e-2 ?1 | ◫ 31k/1.0M (3.0%) AC | LSP Active: marksman  ·   '
                'bg ⬆ v2.6.10 /bg-update ', '─' * 200, ' ' * 50, '─' * 200, ' pi-lens', ' ↳ Reply with OK and stop.', '']
RUN3_PI = RUN3_PI_TOP + [''] * (agents_live.CAPTURE_LINES - len(RUN3_PI_TOP) - len(RUN3_PI_TAIL)) + RUN3_PI_TAIL


def tmux_out(lines):
    """capture-pane's output: every line ends with a newline, so an empty last row stays a row."""
    return '\n'.join(lines) + '\n'


class FixR4PiCutBanner(unittest.TestCase):
    """Round 4 item 1 (Rulings 87, 88): pi's update banner cut at the capture's top edge. Rows 0 and 1 that are exactly
    the banner's last two lines, stripped as pi_banners strips them, are the banner: when every `update` hit falls inside
    them or inside whole banners, pi does not stop, and the notice is kept like the whole banner, with "cut": true.
    Nothing else is widened."""

    def test_run3_capture_is_not_a_stop(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        m = {'nonce': NONCE, 'terminalIds': ['t1'], 'agents': {}}
        pane, tmux = screen_pane('', m)
        live = agents_live.Live(lambda method, params: {}, lambda: m, lambda *a, **k: None, Path(tmp.name), answers(), NONCE, pane, None,
                                clock=FakeClock(), home=tmp.name)
        pane.begin('K1X-SEND')
        tmux.screen = tmux_out(RUN3_PI_ABOVE + RUN3_PI[:-1])  # an earlier capture: the whole banner in the window's first rows
        live.capture('K1X-SEND', 't1', 'pi')
        tmux.screen = tmux_out(RUN3_PI_ABOVE + RUN3_PI)  # one row more: the window's last 60 rows lose the banner's first line
        self.assertEqual(live.capture('K1X-SEND', 't1', 'pi'), RUN3_PI)
        self.assertIsNone(pane.stopped)
        live.finish('K1X-SEND', 'PASS')
        row = json.loads((Path(tmp.name) / 'cases' / 'K1X-SEND.json').read_text())
        self.assertEqual(row['passiveNotices'], [{'lines': agents_live.redact(RUN3_PI_ABOVE + RUN3_PI_TOP[:2]), 'seen': 1},
                                                 {'lines': agents_live.redact(RUN3_PI_TOP[:2]), 'seen': 1, 'cut': True}])

    def test_a_short_capture_is_cut_at_its_top_too(self):
        """Run 3's captures are 50 rows: the pane's own top cuts as the 60-line window's does, so the rule has no length."""
        lines = RUN3_PI_TOP + [''] * (50 - len(RUN3_PI_TOP) - len(RUN3_PI_TAIL)) + RUN3_PI_TAIL
        pane, _ = screen_pane(tmux_out(lines))
        pane.begin('K1X-SEND')
        self.assertEqual(pane.capture('t1', 'pi'), lines)
        self.assertEqual((pane.stopped, [(n['lines'], n.get('cut')) for n in pane.notices]), (None, [(agents_live.redact(RUN3_PI_TOP[:2]), True)]))

    def test_other_patterns_are_still_checked(self):
        pane, _ = screen_pane(tmux_out(RUN3_PI_ABOVE + RUN3_PI[:-1] + ['  /login to continue']))
        pane.begin('K1X-SEND')
        with self.assertRaises(agents_live.StopPrompt) as caught:
            pane.capture('t1', 'pi')
        self.assertEqual((caught.exception.pattern, [m['index'] for m in caught.exception.matches]), ('login', [59]))
        self.assertEqual([(n['lines'], n.get('cut')) for n in pane.notices], [(agents_live.redact(RUN3_PI_TOP[:2]), True)])

    def test_the_banners_last_line_alone_is_not_a_notice(self):
        pane, _ = screen_pane(tmux_out(RUN3_PI_TOP[1:] + RUN3_PI_TAIL))  # it matches no pattern
        pane.begin('K1X-SEND')
        self.assertEqual(pane.capture('t1', 'pi'), RUN3_PI_TOP[1:] + RUN3_PI_TAIL)
        self.assertEqual((pane.stopped, pane.notices), (None, []))

    def test_everything_else_still_stops(self):
        bottom = RUN3_PI_TAIL[:-1] + RUN3_PI_ABOVE + RUN3_PI_TOP[:1]  # the banner's first two lines at the bottom, which never cuts
        cases = [(RUN3_PI_ABOVE + RUN3_PI[:-1] + ['Update now? (y/n)'], 'pi', [0, 59]),  # the cut banner and another update line
                 (['', '', ''] + RUN3_PI_TOP + RUN3_PI_TAIL, 'pi', [3]),  # its last two lines, but not at row 0
                 (RUN3_PI_ABOVE + RUN3_PI_TAIL, 'pi', [0]),  # a lone `Update Available`
                 (RUN3_PI_ABOVE + RUN3_PI_TOP[1:] + RUN3_PI_TAIL, 'pi', [0]),  # its first and last lines
                 (RUN3_PI_TOP[:1] + RUN3_PI_TOP[2:] + RUN3_PI_TAIL, 'pi', [0]),  # its second line alone at row 0
                 ([RUN3_PI_TOP[0].replace('1.5.0', 'v1.5.0')] + RUN3_PI_TOP[1:] + RUN3_PI_TAIL, 'pi', [0]),  # not exactly its line
                 (bottom, 'pi', [len(bottom) - 2, len(bottom) - 1])]
        cases += [(RUN3_PI_ABOVE + RUN3_PI, tool, [0]) for tool in ('claude', 'codex', 'opencode', None)]  # the cut banner, pi only
        for lines, tool, indexes in cases:
            pane, _ = screen_pane(tmux_out(lines))
            pane.begin('K1X-SEND')
            with self.assertRaises(agents_live.StopPrompt, msg=(tool, lines[0])) as caught:
                pane.capture('t1', tool)
            self.assertEqual((caught.exception.pattern, [m['index'] for m in caught.exception.matches]), ('update', indexes), (tool, lines[0]))
            self.assertEqual(pane.notices, [], (tool, lines[0]))


class FixR4AbsentRoot(unittest.TestCase):
    """Round 4 item 3 (review 2 §3.3, N1): a WSL restart empties the tmpfs /tmp, and the fixture root with it. The owned
    recovery (README §5: wsl-owned.py seal, reseal and cleanup) then goes on when lstat proves the root absent. It skips
    only the root's own steps and removes every other owned resource by its manifest and seal rows, with "fixtureRoot":
    "absent". A root that exists but is not the owned one still refuses, and the run itself is unchanged."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)
        parent = patch.object(fixture, 'FIXTURE_PARENT', self.tmp)
        parent.start()
        self.addCleanup(parent.stop)
        self.root = fixture.fixture_root(TOKEN)
        self.unit = self.tmp / 'home/.config/systemd/user'
        self.templates = dict(zip(wsl_owned.INSTANCE_UNITS, (self.unit / name for name in ('devbox-accept@.service', 'devbox-accept@.socket',
                                                                                             'devbox-accept-tmux@.service'))))
        self.config, self.share = self.tmp / 'home/.config/devbox-accept', self.tmp / 'home/.local/share/devbox-accept'
        self.data, self.socket = self.tmp / 'home/.local/share/devbox/accept', self.tmp / 'run/devbox-accept.sock'
        self.dropin = self.unit / 'devbox-accept@accept.service.d'
        self.wants = self.unit / 'default.target.wants/devbox-accept@accept.service'
        # The accept paths in fixture.accept_paths order. The owned tmux socket lived in /tmp, so the restart took it.
        self.paths = [self.config, self.share, self.data, self.socket, *self.templates.values(), self.dropin, self.tmp / 'tmux-1000/devbox-accept',
                      self.wants]
        self.path = self.tmp / 'runs/r1' / ('ownership-' + TOKEN + '.json')
        self.calls, self.stopped = [], set()

    def interrupted(self, flow='wsl-fresh'):
        """A run killed after setup and before its seal: the stage's owned folders and files, the install's folder, setup's
        units, a wants link and the socket. The manifest is the stage's: its sealedPaths holds only the stage's folders."""
        marker = json.dumps({'token': TOKEN, 'sourceSha': C_SHA})
        fixture.create_root(self.root, TOKEN, C_SHA)
        (self.root / 'P-CL').mkdir()
        vault = self.root / ('custom-vault-' + TOKEN[6:])
        vault.mkdir(mode=0o700)
        for folder in (self.config, self.dropin):
            folder.mkdir(mode=0o700, parents=True)
            (folder / fixture.MARKER).write_text(marker)
        files = {vault / ('s7-vault-' + TOKEN + '.md'): TOKEN + '\n' + C_SHA + '\n', self.config / 'config.toml': '[pc]\nname = "s7-accept-fixture"\n',
                 self.dropin / '90-s7-owned-resources.conf': '[Service]\nNice=10\n'}
        for path, text in files.items():
            path.write_text(text)
        (self.share / 'bin').mkdir(parents=True)
        (self.share / 'bin' / ('devbox-' + 'f' * 64)).write_bytes(b'linux artifact')
        for template in self.templates.values():
            template.write_bytes(wsl_owned.SETUP_MARKER + b'[Unit]\n')
        self.wants.parent.mkdir()
        self.wants.symlink_to(self.templates['devbox-accept@accept.service'])
        self.socket.parent.mkdir(exist_ok=True)
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(self.socket))
        created = [{'path': str(path), 'sha256': hashlib.sha256(text.encode()).hexdigest()} for path, text in files.items()]
        created.append({'path': str(self.tmp / ('devbox-s7-p1x-' + TOKEN[6:])), 'sha256': hashlib.sha256(b'').hexdigest(), 'optional': True})
        self.m = {'channel': 'accept', 'token': TOKEN, 'nonce': TOKEN[6:], 'sourceSha': C_SHA, 'flow': flow, 'fixtureRoot': str(self.root),
                  'projectPaths': {'P-CL': str(self.root / 'P-CL')}, 'absentBefore': [str(p) for p in self.paths],
                  'absentParentsBefore': [str(self.wants.parent)], 'createdFiles': created, 'agentIds': ['a1'], 'runIds': [], 'terminalIds': [],
                  'sealedPaths': [{'path': str(folder), 'kind': 'directory'} for folder in (self.config, self.dropin)],  # fixture.stage owned_dir
                  'mcpProcesses': [], 'socketPath': str(self.tmp / 'no.sock')}
        self.path.parent.mkdir(parents=True, exist_ok=True)
        common.dump(self.path, self.m)

    @contextlib.contextmanager
    def owned_env(self, uid=None):
        """The lock, systemd and the accept allowlist, faked over the temp tree. The instance units run until stopped."""
        def run(argv, **kw):
            self.calls.append(list(argv))
            if list(argv[:3]) == ['systemctl', '--user', 'stop']:
                self.stopped.add(argv[3])
            return completed(argv)

        def state(name):
            if name not in self.templates:
                return {'LoadState': 'not-found', 'ActiveState': 'inactive'}
            return {'LoadState': 'loaded', 'ActiveState': 'inactive' if name in self.stopped else 'active', 'FragmentPath': str(self.templates[name])}
        with contextlib.ExitStack() as stack:
            for item in (patch.object(wsl_owned, 'require_lock', NO_LOCK), patch.object(wsl_owned, 'UNIT', self.unit),
                         patch.object(wsl_owned, 'accept_paths', lambda: list(self.paths)),
                         patch.object(wsl_owned, 'parent_candidates', lambda: [self.wants.parent]), patch.object(wsl_owned, 'unit_state', state),
                         patch.object(wsl_owned, 'accept_runtime_units', lambda: []), patch.object(wsl_owned.subprocess, 'run', run)):
                stack.enter_context(item)
            if uid is not None:
                stack.enter_context(patch.object(fixture.os, 'getuid', lambda: uid))
            yield

    def main(self, action, uid=None):
        """`wsl-owned.py <action> --ownership <manifest>`, as README §5 runs it."""
        out = io.StringIO()
        with self.owned_env(uid), patch.object(sys, 'argv', ['wsl-owned.py', action, '--ownership', str(self.path)]), contextlib.redirect_stdout(out):
            wsl_owned.main()
        return json.loads(out.getvalue())

    def left(self):
        return [str(p) for p in self.paths if p.exists() or p.is_symlink()]

    def test_seal_and_cleanup_go_on_without_the_root(self):
        self.interrupted()
        shutil.rmtree(self.root)  # the restart: the tmpfs /tmp starts empty
        sealed = self.main('seal')
        self.assertEqual((sealed['status'], sealed['fixtureRoot']), ('PASS', 'absent'))
        self.assertEqual(sorted(row['kind'] for row in sealed['sealedPaths']), ['directory'] * 3 + ['file'] * 3 + ['socket', 'symlink'])
        disk = json.loads(self.path.read_text())
        self.assertEqual({k: v for k, v in disk.items() if k != 'sealedPaths'}, {k: v for k, v in self.m.items() if k != 'sealedPaths'})
        self.assertEqual(self.main('cleanup'), {'status': 'PASS', 'productFailures': [], 'fixtureRoot': 'absent'})
        self.assertEqual(self.left(), [])
        self.assertFalse(self.wants.parent.exists())  # the recorded absent parent, now empty
        self.assertEqual(sorted(c[3] for c in self.calls if c[:3] == ['systemctl', '--user', 'stop']), sorted(wsl_owned.INSTANCE_UNITS))
        self.assertIn(['systemctl', '--user', 'daemon-reload'], self.calls)
        self.assertEqual([c for c in self.calls if c[0] == 'git'], [])  # no worktree step: the projects went with the root
        self.assertFalse(self.root.exists() or self.root.is_symlink())
        steps = json.loads((self.path.parent / 'cleanup-product-steps.json').read_text())
        self.assertEqual(steps['product'], [{'action': 'product', 'status': 'NOT_RUN', 'reason': 'fixture root absent'}])
        row = json.loads((self.path.parent / 'fixture-cleanup.json').read_text())
        self.assertEqual((row['status'], row['root'], row['fixtureRoot'], row['parents']),
                         ('PASS', str(self.root), 'absent', [{'path': str(self.wants.parent), 'state': 'removed'}]))

    def test_reseal_then_cleanup_after_an_interrupted_upgrade(self):
        """README §5's reseal case (review 5, R5-M1), here after the restart: sealed after rc.1, killed after rc.2's setup."""
        self.interrupted(flow='wsl-upgrade')
        self.assertNotIn('fixtureRoot', self.main('seal'))  # the run's first seal, while its root was there
        service = self.templates['devbox-accept@accept.service']
        service.write_bytes(wsl_owned.SETUP_MARKER + b'[Unit]\nDescription=rc.2\n')  # rc.2's setup rewrote its unit
        (self.data / 'agents').mkdir(parents=True)  # and the rc.2 daemon opened its data folder
        shutil.rmtree(self.root)
        with self.assertRaisesRegex(ValueError, 'Unit file ownership changed'):  # cleanup alone stops at the changed unit
            self.main('cleanup')
        self.assertNotIn('devbox-accept@accept.service', self.stopped)
        resealed = self.main('reseal')
        self.assertEqual((resealed['status'], resealed['fixtureRoot']), ('PASS', 'absent'))
        self.assertEqual([(c['path'], c.get('new', False)) for c in resealed['resealed']], [(str(self.data), True), (str(service), False)])
        self.assertEqual(self.main('cleanup'), {'status': 'PASS', 'productFailures': [], 'fixtureRoot': 'absent'})
        self.assertEqual(self.left(), [])

    def test_a_root_that_is_there_must_be_the_owned_one(self):
        self.interrupted()
        other = self.tmp / 'elsewhere'
        other.mkdir(mode=0o700)
        (other / fixture.MARKER).write_text(json.dumps({'token': TOKEN, 'sourceSha': C_SHA}))

        def owned(mode=0o700, marker=None):
            self.root.mkdir()
            self.root.chmod(mode)
            if marker is not None:
                (self.root / fixture.MARKER).write_text(json.dumps(marker))
        mine = {'token': TOKEN, 'sourceSha': C_SHA}
        damages = [('a file', lambda: self.root.write_text('x'), None), ('a symlink', lambda: self.root.symlink_to(other), None),
                   ('a dangling symlink', lambda: self.root.symlink_to(self.tmp / 'gone'), None), ('mode 0755', lambda: owned(0o755, mine), None),
                   ('not ours', lambda: owned(0o700, mine), os.getuid() + 1), ('no marker', owned, None),
                   ('another token', lambda: owned(0o700, dict(mine, token='owned-' + 'e' * 12)), None),
                   ('another source', lambda: owned(0o700, dict(mine, sourceSha=B_SHA)), None)]
        manifest, present = self.path.read_bytes(), self.left()
        for name, damage, uid in damages:
            if self.root.is_symlink() or self.root.is_file():
                self.root.unlink()
            elif self.root.exists():
                self.root.chmod(0o700)
                shutil.rmtree(self.root)
            damage()
            for action in ('seal', 'reseal', 'cleanup'):
                with self.assertRaises((ValueError, OSError), msg=(name, action)):
                    self.main(action, uid)
                self.assertEqual(self.calls, [], (name, action))  # refused before any process or removal
                self.assertEqual((self.left(), self.path.read_bytes()), (present, manifest), (name, action))

    def test_the_run_still_needs_the_root(self):
        self.interrupted()
        shutil.rmtree(self.root)
        present = self.left()
        for action in ('instance', 'rpc', 'restart-tmux'):
            with self.assertRaises(FileNotFoundError, msg=action):
                self.main(action)
        with self.owned_env():
            # The driver's own teardown cleanup, every RPC and load_owned keep requiring the root.
            for call in (lambda: wsl_owned.cleanup(self.path, self.m), lambda: wsl_owned.rpc_owned(self.path, 'system.info'),
                         lambda: wsl_owned.load_owned(self.path)):
                with self.assertRaises(FileNotFoundError):
                    call()
        self.assertEqual((self.calls, self.left()), ([], present))

    def test_a_present_root_behaves_as_before(self):
        for by in ('README §5', 'driver teardown'):
            self.interrupted()
            self.calls, self.stopped = [], set()
            self.assertNotIn('fixtureRoot', self.main('seal'))
            if by == 'README §5':
                result = self.main('cleanup')
            else:
                with self.owned_env():
                    result = wsl_owned.cleanup(self.path, json.loads(self.path.read_text()))
            self.assertEqual(result, {'status': 'PASS', 'productFailures': []}, by)
            self.assertEqual((self.left(), self.root.exists()), ([], False), by)
            steps = json.loads((self.path.parent / 'cleanup-product-steps.json').read_text())
            self.assertEqual(steps['product'], [{'action': 'product', 'status': 'NOT_RUN', 'reason': 'socket absent'}], by)
            self.assertEqual(sorted(json.loads((self.path.parent / 'fixture-cleanup.json').read_text())),
                             ['parents', 'productFailures', 'root', 'status', 'units'], by)


class FixR4U1Record(unittest.TestCase):
    """Round 4 item 4 (review 5, Note A): U1 (d) is record only (s7-14-matrix.md:193). An rc.1 MCP tools/list or
    devbox_agents call that raises or times out is kept, redacted and bounded, as rc1Mcp.error, and (d) is not run: it
    never fails U1 and never passes (d). U1 concludes from (a)-(c), on the driver's own guarded/classify path."""
    KEY = 'sk-ant-api03-' + 'Ab9_x-' * 8

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.dir = Path(tmp.name)
        self.rows = {}

    def record(self, case, status, files, reason=None, **extra):
        self.rows.setdefault(case, (status, reason))

    def rc1(self, fails=None):
        """The rc.1 MCP client after the upgrade. `fails` names the call that raises: TimeoutError is mcp_stdio's 10 s
        deadline, BrokenPipeError a process that is gone."""
        def tools_list():
            if fails == 'list':
                raise TimeoutError('MCP reply deadline: tools/list')
            return {'jsonrpc': '2.0', 'id': 2, 'result': {'tools': [{'name': 'devbox_agents'}, {'name': 'devbox_agent_send'}]}}

        def call(name, arguments):
            if fails == 'call':
                raise BrokenPipeError('MCP stdin closed: ' + 'x' * 400 + ' ' + self.KEY)
            return {'jsonrpc': '2.0', 'id': 3, 'result': {'content': [{'type': 'text', 'text': '[{"profile": "opencode"}]'}]}}
        return Fake(pairs=[{'request': 'tools/list', 'response': '(no reply within 10 s)'}], tools_list=tools_list, call=call)

    def u1(self, state, task=None):
        """u1_rows as upgrade_rows runs it: inside Session.guarded, whose classify() made an escaped error a U1 FAIL."""
        self.rows.clear()
        m = {'nonce': NONCE, 'terminalIds': [], 'agents': {'a1': {'terminalIds': ['t2']}}}
        clock = FakeClock()
        pane = agents_live.Pane(lambda: m, answers(), NONCE, run=FakeTmux(), clock=clock)
        live = agents_live.Live(lambda method, params: dict(task or {}), lambda: m, self.record, self.dir, answers(), NONCE, pane, None,
                                clock=clock, home=self.dir, windows_half=evidence.reason('R-IOP½'))
        stub = Fake(cases=['U1', 'U1X'], evidence=self.dir, journal_since=lambda t0: [], case_result=self.record,
                    pending=lambda case: case not in self.rows)
        with patch.object(agents_live, 'require_lock', NO_LOCK):
            driver.Session.guarded(stub, ['U1', 'U1X'], lambda selected: agents_live.u1_rows(live, state, selected, None))
        return self.dir / 'cases' / 'U1.json'

    def test_an_mcp_error_is_the_d_record(self):
        for fails in ('list', 'call'):
            path = self.u1({'claude': None, 'codex': None, 'rc1Mcp': self.rc1(fails)})
            self.assertEqual((self.rows['U1'][0], self.rows['U1X'][0]), ('NOT_RUN', 'NOT_RUN'), fails)  # never FAIL by itself
            self.assertIn('rc1Mcp.error', self.rows['U1'][1])
            row = json.loads(path.read_text())
            self.assertEqual((row['parts'], row['partial']), ({'a': None, 'b': None, 'c': None, 'd': None}, []), fails)  # never a PASS of (d)
            error = row['rc1Mcp']['error']
            if fails == 'list':
                self.assertEqual(row['rc1Mcp'], {'error': 'TimeoutError: MCP reply deadline: tools/list'})
            else:
                self.assertTrue(error.startswith('BrokenPipeError: x') and error.endswith(' [masked]'), error)
                self.assertLessEqual(len(error), len('BrokenPipeError: ') + 300)
            self.assertNotIn(self.KEY, path.read_text())
            self.assertFalse((self.dir / 'cases' / 'U1-error.json').exists())
            self.assertEqual(json.loads((self.dir / 'cases' / 'U1-mcp.json').read_text()), [{'request': 'tools/list', 'response': '(no reply within 10 s)'}])

    def test_u1_keeps_its_verdict_from_a_to_c(self):
        task = {'id': 'a1', 'terminalId': 't2', 'sessionId': 's-after', 'state': 'idle'}  # the rc.1 codex task lost its session
        path = self.u1({'claude': None, 'codex': {'id': 'a1', 'terminalId': 't2', 'sessionId': 's-before'}, 'rc1Mcp': self.rc1('list')}, task)
        self.assertEqual(self.rows['U1'], ('FAIL', 'Failed parts: a'))
        row = json.loads(path.read_text())
        self.assertEqual((row['parts'], row['rc1Mcp']), ({'a': False, 'b': None, 'c': None, 'd': None},
                                                         {'error': 'TimeoutError: MCP reply deadline: tools/list'}))

    def test_replies_are_still_recorded(self):
        path = self.u1({'claude': None, 'codex': None, 'rc1Mcp': self.rc1()})
        row = json.loads(path.read_text())
        self.assertEqual((self.rows['U1'][0], row['parts']['d'], row['partial']), ('NOT_RUN', True, ['d']))
        self.assertEqual((sorted(row['rc1Mcp']), row['rc1Mcp']['mentionsOpencode'], row['rc1Mcp']['mentionsPi']),
                         (['lines', 'mentionsOpencode', 'mentionsPi'], True, False))


class FixR4Readme(unittest.TestCase):
    """Round 4 items 1-3 in the README: the cut banner (Ruling 87), §5's reseal case (review 5, R5-M1, the C6 block) and
    the recovery after a WSL restart (N1)."""

    def test_notice_and_recovery_text(self):
        text = (BASE / 'README.md').read_text()
        self.assertIn('"cut": true', text)
        section = text[text.index('## 5. If a run is interrupted'):text.index('## 6. ')]
        reseal = section.index('python3 -B $H/wsl-owned.py reseal --ownership $S7/runs/<run>/ownership-owned-<token>.json')
        self.assertLess(section.index('`resealed`'), reseal)
        self.assertIn('python3 -B $H/wsl-owned.py cleanup --ownership $S7/runs/<run>/ownership-owned-<token>.json', section[reseal:])
        self.assertIn('"fixtureRoot": "absent"', section)


# --- S8-5 fix round 1 (Ruling 147): W1-AB's closure signal (triage D2) and the seal after the instance proof (D5) ----------
def make_attempts(db, rows, wal=False):
    """A sqlite DB with the product's runtime_windows_attempts table (windows_copy_store.rs:15-18) and (nonce, phase) rows;
    the open writer is returned. wal: the product's journal mode (core db.rs:269), its writer kept open as the daemon does."""
    connection = sqlite3.connect(db)
    if wal:
        connection.execute('PRAGMA journal_mode=WAL')
    connection.execute('CREATE TABLE runtime_windows_attempts (nonce TEXT PRIMARY KEY, launch_id TEXT NOT NULL, attempt_id TEXT NOT NULL, '
                       'phase TEXT NOT NULL, body TEXT NOT NULL, UNIQUE(launch_id,attempt_id))')
    set_phases(connection, rows)
    return connection


def set_phases(connection, rows):
    connection.executemany('INSERT OR REPLACE INTO runtime_windows_attempts VALUES (?, ?, ?, ?, ?)', [(n, 'l-' + n, 'a-' + n, p, '{}') for n, p in rows])
    connection.commit()


class DrivenClock(FakeClock):
    """W1.ab_g on fakes: sleep plays the scripted product's due events, then samples every registered watcher in order."""

    def __init__(self):
        super().__init__()
        self.events, self.watchers = [], []

    def sleep(self, seconds):
        self.now += seconds
        due = sorted((e for e in self.events if e[0] <= self.now), key=lambda e: e[0])
        self.events = [e for e in self.events if e[0] > self.now]
        for _, play in due:
            play()
        for watcher in self.watchers:
            if not watcher.stop.is_set():
                watcher.sample()


class Driven:
    """A watcher that only DrivenClock.sleep samples: its own thread returns at once, so no real cadence races the test."""

    def __init__(self, *args):
        super().__init__(*args)
        self.clock.watchers.append(self)

    def _loop(self):
        pass


class FixS85Round1(unittest.TestCase):
    """Ruling 147. D2: a committed attempt closes in the owned accept DB (store::observed_closed), not by <nonce>.closed.json,
    which only revoke_attempt writes (a deny-execution marker); every control file is this run's nonce's. D5: the seal runs
    after the instance proof, when the app's setup has written the unit files."""
    START = 2.0  # start_ref of the (가) timelines; window_end = START + ORPHAN_SECONDS - 1
    FILES = ('holder-ready', 'assigned.json', 'emptied.json', 'closed.json')

    @staticmethod
    def failed(verdict):
        return {name for name, ok in verdict.items() if name != 'pass' and not ok}

    def test_control_watch_filters_by_nonce(self):
        with tempfile.TemporaryDirectory() as tmp:
            for nonce, run in (('aaaa', 'r1'), ('bbbb', 'r2')):
                (Path(tmp) / (nonce + '.assigned.json')).write_text(json.dumps({'pid': 1, 'creationTime': '2', 'runId': run}))
            (Path(tmp) / 'cccc.assigned.json').write_text(json.dumps({'pid': 3, 'creationTime': '4'}))  # no runId
            (Path(tmp) / 'aaaa.emptied.json').write_text('{}')
            watch = w1_windows.ControlWatch(tmp, FakeClock())
            watch.stop.set()
            watch.sample()
            self.assertEqual((watch.nonce_for('r1'), watch.nonce_for('r2'), watch.nonce_for('zz'), watch.nonce_for(None)), ('aaaa', 'bbbb', None, None))
            self.assertIsNone(watch.first('emptied.json', 'bbbb'))
            self.assertIsNotNone(watch.first('emptied.json', 'aaaa'))
            self.assertIsNotNone(watch.first('emptied.json'))  # no nonce: any run's file, as W1-H and W1-F still read it
            self.assertIsNone(w1_windows.first_of(watch, 'emptied.json', None))  # W1-AB never falls back to another run's file
            self.assertIsNotNone(w1_windows.first_of(watch, 'emptied.json', 'aaaa'))

    def test_read_phase_is_read_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            db = Path(tmp) / 'a b#?c' / 'devbox.db'  # characters the file: URI must quote
            db.parent.mkdir()
            with contextlib.closing(make_attempts(db, [('n1', 'committed')], wal=True)):
                content = lambda: (db.read_bytes(), Path(str(db) + '-wal').read_bytes())
                before, opened, statements = content(), [], []
                real = sqlite3.connect

                def connect(*args, **kwargs):
                    opened.append((args, kwargs))
                    connection = real(*args, **kwargs)
                    connection.set_trace_callback(statements.append)
                    return connection
                with patch.object(w1_windows.sqlite3, 'connect', connect):
                    self.assertEqual(w1_windows.read_phase(db, 'n1'), 'committed')
                    self.assertIsNone(w1_windows.read_phase(db, 'zz'))
                self.assertEqual(opened, [(('file:' + urllib.parse.quote(str(db)) + '?mode=ro',), {'uri': True, 'timeout': 1})] * 2)
                self.assertEqual(statements, ["SELECT phase FROM runtime_windows_attempts WHERE nonce = 'n1'",
                                              "SELECT phase FROM runtime_windows_attempts WHERE nonce = 'zz'"])  # no PRAGMA, no write
                with contextlib.closing(w1_windows.open_ro(db)) as connection:
                    with self.assertRaises(sqlite3.OperationalError):
                        connection.execute("INSERT INTO runtime_windows_attempts VALUES ('n2', 'l', 'a', 'closed', '{}')")
                self.assertEqual(content(), before)  # the DB and its WAL are unchanged

    def test_accept_db_requires_the_owned_folder(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp) / 'home'
            folder = home / '.local/share/devbox/accept'
            owned = {'absentBefore': [str(home / '.config/devbox-accept'), str(folder)]}
            with self.assertRaisesRegex(ValueError, 'not a real directory'):
                w1_windows.accept_db(owned, home)
            folder.mkdir(parents=True)
            with self.assertRaisesRegex(ValueError, 'not a regular file'):
                w1_windows.accept_db(owned, home)  # the DB is missing
            (folder / 'devbox.db').write_bytes(b'')
            self.assertEqual(w1_windows.accept_db(owned, home), folder / 'devbox.db')
            for m in ({'absentBefore': [str(home / '.local/share/devbox')]}, {}, {'absentBefore': str(folder)}):
                with self.assertRaisesRegex(ValueError, 'not proved absent'):
                    w1_windows.accept_db(m, home)
            with self.assertRaisesRegex(ValueError, re.escape(str(Path.home() / '.local/share/devbox/accept'))):
                w1_windows.accept_db({'absentBefore': []})  # the default home; refused before anything is read
            (Path(tmp) / 'other.db').write_bytes(b'')
            (folder / 'devbox.db').unlink()
            (folder / 'devbox.db').symlink_to(Path(tmp) / 'other.db')
            with self.assertRaisesRegex(ValueError, 'not a regular file'):
                w1_windows.accept_db(owned, home)
            (Path(tmp) / 'real').mkdir()
            (Path(tmp) / 'real' / 'devbox.db').write_bytes(b'')
            shutil.rmtree(folder)
            folder.symlink_to(Path(tmp) / 'real')
            with self.assertRaisesRegex(ValueError, 'not a real directory'):
                w1_windows.accept_db(owned, home)

    def committed(self, phase='committed', step=0.25, until=21.0):
        return [{'t': self.START + step * i, 'phase': phase} for i in range(int(until / step) + 1)]

    def ga(self, states=None, phases=None, start_ref=START, emptied=START + 21.2, deny=None, final=None):
        """The passing (가) timeline unless overridden: starting at 0, running to 40; committed every 0.25 s from start_ref to
        start_ref + 21, then closed; the receipt at start_ref + 21.2; no deny marker; exit 0."""
        if states is None:
            states = [{'t': 0.0, 'state': 'starting'}] + [{'t': 0.5 * i, 'state': 'running'} for i in range(1, 81)]
        if phases is None:
            phases = self.committed() + [{'t': self.START + 21.25, 'phase': 'closed'}]
        return w1_windows.ga_verdict(states, phases, start_ref, emptied, deny, final or {'state': 'exited', 'exit': {'code': 0}})

    def test_ga_verdict(self):
        self.assertEqual(self.ga(), {'keptRun': True, 'keptReader': True, 'keptAtWindowEnd': True, 'receipt': True, 'closedAfter': True,
                                     'noDenyMarker': True, 'exitedZero': True, 'pass': True})
        closed = {'t': self.START + 21.25, 'phase': 'closed'}
        early = [dict(r, phase='closed') if r['t'] >= self.START + 10 else r for r in self.committed()] + [closed]
        rows = lambda n: [{'t': self.START + i, 'phase': 'committed'} for i in range(n)]  # one a second, all at or before window_end
        exits = [{'t': 0.0, 'state': 'starting'}] + [{'t': 0.5 * i, 'state': 'running' if 0.5 * i <= self.START + 5 else 'exited'} for i in range(1, 81)]
        late = {'t': self.START + 19.25, 'phase': 'committed'}  # after window_end: not one of the `need` rows
        # Round 2 (F1): keptAtWindowEnd also fails, by definition, where no kept row reaches window_end before the first closed:
        # the early closed, the 19-row cases (their rows stop at start_ref + 18) and a None start_ref.
        cases = [(self.ga(phases=early), {'keptReader', 'keptAtWindowEnd', 'closedAfter'}),  # an early closed is also the first closed
                 (self.ga(phases=self.committed('assigned') + [closed]), {'keptReader'}),  # never committed
                 (self.ga(phases=rows(19) + [closed]), {'keptReader', 'keptAtWindowEnd'}),  # fewer than need
                 (self.ga(phases=rows(19) + [late, closed]), {'keptReader'}),  # fewer than need, alone
                 (self.ga(phases=rows(19) + [{'t': self.START, 'error': 'OperationalError'}] * 40 + [closed]),
                  {'keptReader', 'keptAtWindowEnd'}),  # errors do not count
                 (self.ga(phases=[{'t': self.START, 'phase': None}] + self.committed() + [closed]), {'keptReader'}),  # no DB row yet: fail closed
                 (self.ga(states=exits), {'keptRun'}),  # exited at start_ref + 5
                 (self.ga(phases=self.committed() + [{'t': self.START + 50.25, 'phase': 'closed'}]), {'closedAfter'}),  # after start_ref + 20 + 30
                 (self.ga(deny=self.START + 1), {'noDenyMarker'}),
                 (self.ga(final={'state': 'exited', 'exit': {'code': 7}}), {'exitedZero'}),
                 (self.ga(emptied=None), {'receipt'}),
                 (self.ga(start_ref=None), {'keptRun', 'keptReader', 'keptAtWindowEnd', 'closedAfter'})]  # every timed check
        for verdict, expected in cases:
            self.assertEqual((verdict['pass'], self.failed(verdict)), (False, expected), verdict)
        self.assertTrue(self.ga(phases=rows(20) + [closed])['pass'])  # need rows exactly, the last at window_end
        self.assertTrue(self.ga(phases=self.committed() + [{'t': self.START + 50, 'phase': 'closed'}])['pass'])  # closed at the bound

    def test_ga_verdict_needs_a_kept_row_at_window_end(self):
        """Review r2 F1: a reader that closed while the orphan ran, with every read from the close to window_end errored or
        missing, must not pass (가); one error-free kept row at or after window_end, before the first closed, is enough."""
        states = [{'t': round(0.2 * i, 3), 'state': 'running'} for i in range(115)]  # running until 22.8
        early = [{'t': self.START + 0.25 * i, 'phase': 'committed'} for i in range(37)]  # 2.0 to 11.0
        errors = [{'t': 11.25 + 0.25 * i, 'error': 'OperationalError'} for i in range(40)]  # 11.25 to 21.0 (window_end)
        closed = [{'t': 21.25 + 0.25 * i, 'phase': 'closed'} for i in range(8)]  # from 21.25
        stray = closed[:1] + [{'t': 21.5, 'phase': 'committed'}] + closed[1:]  # a kept row after the first closed does not count
        for phases in (early + errors + closed, early + closed, early + errors + stray):  # the reviewer's two timelines, and the order rule
            verdict = self.ga(states=states, phases=phases)
            self.assertEqual((verdict['pass'], self.failed(verdict)), (False, {'keptAtWindowEnd'}), verdict)
        masked = w1_windows.phase_evidence(early + errors + closed)
        self.assertEqual((masked['lastKeptBeforeClose'], masked['phaseErrors'], masked['phaseErrorTimes']), (11.0, 40, [r['t'] for r in errors]))
        good = [{'t': self.START + 0.25 * i, 'phase': 'committed'} for i in range(75)]  # 2.0 to 20.5
        error = lambda t: {'t': t, 'error': 'OperationalError'}
        for around in ([error(20.75), error(21.0), {'t': 21.25, 'phase': 'committed'}, error(21.5), error(21.75)] + closed[3:],  # after window_end
                       [error(20.75), {'t': 21.0, 'phase': 'committed'}, error(21.25), error(21.5)] + closed[2:]):  # at window_end
            verdict = self.ga(states=states, phases=good + around)
            self.assertTrue(verdict['pass'], verdict)

    def test_na_verdict(self):
        phases, exited = [{'t': 1.0, 'phase': 'committed'}, {'t': 2.0, 'phase': 'closed'}], {'state': 'exited', 'exit': {'code': 0}}
        self.assertEqual(w1_windows.na_verdict(phases, 1.5, None, exited), {'receipt': True, 'closed': True, 'noDenyMarker': True, 'exitedZero': True,
                                                                           'pass': True})
        for verdict, expected in ((w1_windows.na_verdict(phases[:1], 1.5, None, exited), {'closed'}),
                                  (w1_windows.na_verdict(phases, 1.5, 0.5, exited), {'noDenyMarker'}),
                                  (w1_windows.na_verdict(phases, 1.5, None, {'state': 'exited', 'exit': {'code': 7}}), {'exitedZero'}),
                                  (w1_windows.na_verdict(phases, None, None, exited), {'receipt'})):
            self.assertEqual((verdict['pass'], self.failed(verdict)), (False, expected), verdict)

    def test_phase_watch_records_after_the_nonce_and_errors(self):
        clock, state, first = FakeClock(), {'nonce': None}, threading.Event()
        outcomes = iter(['committed', OSError('locked'), 'closed'])

        def nonce_of():
            value = state['nonce']  # read before the event, so the thread's first sample keeps its None
            first.set()
            return value

        def read(nonce):
            self.assertEqual(nonce, 'n1')
            value = next(outcomes)
            if isinstance(value, Exception):
                raise value
            return value
        watch = w1_windows.PhaseWatch(nonce_of, read, clock, interval=3600)
        self.assertTrue(first.wait(5))  # its thread sampled once with no nonce, then waits out the interval
        state['nonce'] = 'n1'
        for now in (1001.0, 1002.0, 1003.0):
            clock.now = now
            watch.sample()
        self.assertTrue(watch.saw('committed') and watch.saw('closed') and not watch.saw('revoked'))
        rows = watch.result()
        self.assertTrue(watch.stop.is_set())
        self.assertEqual(rows, [{'t': 1001.0, 'phase': 'committed'}, {'t': 1002.0, 'error': 'OSError'}, {'t': 1003.0, 'phase': 'closed'}])
        self.assertEqual(w1_windows.phase_evidence([{'t': 1, 'phase': None}] + rows), {
            'phaseSamples': 4, 'phaseErrors': 1, 'phaseTransitions': [{'t': 1, 'phase': None}, rows[0], rows[2]],
            'lastKeptBeforeClose': 1001.0, 'phaseErrorTimes': [1002.0]})
        many = w1_windows.phase_evidence([{'t': float(i), 'error': 'OperationalError'} for i in range(150)])
        self.assertEqual((many['phaseErrors'], many['phaseErrorTimes'], many['lastKeptBeforeClose']), (150, [float(i) for i in range(100)], None))
        # Its own thread: no row before the nonce, one error row, and the loop goes on to the closed phase.
        nonces, errors = iter([None]), iter([OSError('locked')])

        def read_live(nonce):
            for error in errors:
                raise error
            return 'closed'
        live = w1_windows.PhaseWatch(lambda: next(nonces, 'n1'), read_live, interval=0.01)
        deadline = time.monotonic() + 5
        while not live.saw('closed') and time.monotonic() < deadline:
            time.sleep(0.01)
        rows = live.result()
        self.assertEqual((rows[0].get('error'), sum('error' in r for r in rows), live.saw('closed')), ('OSError', 1, True))

    def test_seal_follows_the_instance_proof(self):
        """A stub Session (no source-order shortcut): the launch, the phase, the instance proof, then the seal or reseal."""
        with tempfile.TemporaryDirectory() as tmp:
            stderr = Path(tmp) / 'stderr.log'
            stderr.write_text('first-data-painted run=x elapsed_ms=6500 visible=true\n')
            for reseal in (False, True):
                order = []
                stub = Fake(ownership=Path(tmp) / 'ownership.json', evidence=Path(tmp), launched=False, load=lambda: {},
                            windows=lambda path: 'C:\\owned\\ownership.json', observed=lambda name, operation, needs=(): operation(),
                            native=lambda script, timeout=200, **params: order.append('launch') or {'stderr': 'C:\\owned\\stderr.log'},
                            set_phase=lambda side: order.append('phase'), instance=lambda side: order.append('instance') or {'status': 'PASS'})
                with patch.object(driver, 'command', lambda argv, **kwargs: Fake(stdout=str(stderr) + '\n')), \
                        patch.object(driver.wsl_owned, 'seal', lambda *args: order.append('seal') or {}), \
                        patch.object(driver.wsl_owned, 'reseal', lambda *args: order.append('reseal') or {}):
                    proof, sc2 = driver.Session.launch_and_prove(stub, 'candidate', reseal=reseal)
                self.assertEqual(order, ['launch', 'phase', 'instance', 'reseal' if reseal else 'seal'])
                self.assertEqual((proof, len(sc2['firstDataPainted']), stub.launched), ({'status': 'PASS'}, 1, True))

    def ab_g(self, extra_a=(), extra_b=()):
        """W1.ab_g on a scripted product. The shared control dir already holds another run's files (zzzz), deny marker included."""
        tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, str(tmp))
        control, db = tmp / 'control', tmp / 'devbox.db'
        control.mkdir()
        writer = make_attempts(db, [('zzzz', 'closed')], wal=True)
        self.addCleanup(writer.close)
        for kind in self.FILES:
            (control / ('zzzz.' + kind)).write_text(json.dumps({'pid': 9, 'creationTime': '9', 'runId': 'r0'}))
        # run id: (nonce, seconds it stays live, (seconds after its first runtime.get, a control file or a DB phase)...)
        script = {'r1': ('aaaa', 22.5, ((1.5, 'holder-ready'), (2.0, 'assigned.json'), (2.0, 'assigned'), (2.5, 'committed'), (22.7, 'emptied.json'),
                                        (23.0, 'closed')) + tuple(extra_a)),
                  'r2': ('bbbb', 1.2, ((0.4, 'holder-ready'), (0.4, 'assigned.json'), (0.4, 'committed'), (1.6, 'emptied.json'), (1.9, 'closed'))
                         + tuple(extra_b))}
        clock, seen, calls, records = DrivenClock(), {}, [], []

        def play(run_id, what):
            nonce = script[run_id][0]
            if what in self.FILES:
                (control / (nonce + '.' + what)).write_text(json.dumps({'pid': 1, 'creationTime': '2', 'runId': run_id}))
            else:
                set_phases(writer, [(nonce, what)])

        def rpc(method, params):
            calls.append(method)
            if method == 'runtime.runs':
                return {'items': [{'id': 'r1', 'defId': 's7w1a'}, {'id': 'r2', 'defId': 's7w1b'}]}
            if method != 'runtime.get':
                return {}
            run_id = params['runId']
            if run_id not in seen:
                seen[run_id] = clock.now
                clock.events += [(clock.now + at, lambda what=what: play(run_id, what)) for at, what in script[run_id][2]]
            t = clock.now - seen[run_id]
            state = 'starting' if t < 0.5 and run_id == 'r1' else 'running' if t < script[run_id][1] else 'exited'
            return {'run': {'id': run_id, 'state': state, 'exit': {'code': 0} if state == 'exited' else None}}
        w1 = w1_windows.W1(rpc, lambda: {'windowsRoot': 'C:\\dev\\devbox-s7-accept-test'}, lambda *args, **kwargs: records.append(args[:2] + args[3:4]),
                           tmp, None, answers(), 'm' * 20, None, clock=clock)
        w1.project_id = 'pw'
        with patch.object(w1_windows, 'accept_db', lambda m: calls.append('accept_db') or db), \
                patch.object(w1_windows, 'control_dir', lambda run, root: control), \
                patch.object(w1_windows, 'ControlWatch', type('DrivenControl', (Driven, w1_windows.ControlWatch), {})), \
                patch.object(w1_windows, 'PhaseWatch', type('DrivenPhases', (Driven, w1_windows.PhaseWatch), {})):
            w1.ab_g({'W1-AB', 'W1-G'})
        self.assertEqual(calls[0], 'accept_db')  # proved owned before any run starts
        cases = {name: json.loads((tmp / 'cases' / (name + '.json')).read_text()) for name in ('W1-AB', 'W1-G')}
        return {case: (status, reason) for case, status, reason in records}, cases['W1-AB'], cases['W1-G']

    def test_ab_g_judges_only_its_own_nonce(self):
        records, row, g = self.ab_g()
        self.assertEqual(records, {'W1-AB': ('PASS', None), 'W1-G': ('PASS', None)})
        self.assertEqual((row['ga']['nonce'], row['na']['nonce'], row['parts']), ('aaaa', 'bbbb', {'ga': True, 'na': True}))
        self.assertEqual(row['ga']['whileRunning'][0], {'t': 1000.0, 'state': 'starting'})  # the pre-start sample is live
        self.assertEqual([r['phase'] for r in row['ga']['phaseTransitions']], ['assigned', 'committed', 'closed'])
        self.assertEqual((row['ga']['phaseErrors'], row['ga']['phaseSamples'] >= 20, row['ga']['verdict']['pass']), (0, True, True))
        self.assertTrue({'runId', 'whileRunning', 'final', 'states', 'control', 'nonce', 'verdict', 'phaseSamples', 'phaseErrors',
                         'phaseTransitions', 'lastKeptBeforeClose', 'phaseErrorTimes'} <= set(row['ga']))
        self.assertTrue({'lastKeptBeforeClose', 'phaseErrorTimes'} <= set(row['na']))
        # window_end = holder-ready (1001.5) + 19; the last kept row before the close is after it, and no read errored
        self.assertEqual((row['ga']['lastKeptBeforeClose'] >= 1020.5, row['ga']['phaseErrorTimes'], row['ga']['verdict']['keptAtWindowEnd']),
                         (True, [], True))
        self.assertEqual(g['assignedAfterSendSeconds'], 2.0)  # this nonce's assigned.json, not zzzz's (first seen at 0.5 s)
        records, row, _ = self.ab_g(extra_a=[(10.0, 'closed')], extra_b=[(1.0, 'closed.json')])
        self.assertEqual(records['W1-AB'], ('FAIL', 'Failed parts: ga (keptReader,keptAtWindowEnd,closedAfter),na (noDenyMarker)'))
        self.assertEqual(row['parts'], {'ga': False, 'na': False})

    def test_changed_helpers_pass_the_validate_scans(self):
        """validate.py (run at the start of the Windows flow) scans the helpers' raw text; the two changed helpers stay clean."""
        sources = {name: text for name, text in validate.helpers().items() if name in ('w1_windows.py', 'driver.py')}
        self.assertEqual(sorted(sources), ['driver.py', 'w1_windows.py'])
        self.assertEqual(validate.scan_forbidden(sources) + validate.scan_gh(sources) + validate.scan_tool_paths(sources), [])


class FixS85Round2(unittest.TestCase):
    """Ruling 153. D7: claude's trust screen redraws after Down; D8: a codex slash command needs a settle before Enter and its screen
    predicates ignore the composer line; D9: SN2's reason follows the answer."""

    class Pane:
        """A fake pane: `shots` is a function of (number of captures so far, events so far) -> lines."""

        def __init__(self, clock, shots):
            self.clock, self.shots, self.events, self.captures = clock, shots, [], 0

        def capture(self, tid, tool=None):
            self.captures += 1
            lines = self.shots(self.captures, self.events)
            self.events.append(('capture', self.clock.now, list(lines)))
            return lines

        def key(self, tid, key, rule=None, project=None):
            self.events.append(('key', self.clock.now, key, rule))

        def type_text(self, tid, text, cl3_folder=None):
            self.events.append(('type', self.clock.now, text))

    def live(self, shots, **values):
        clock = FakeClock()
        pane = self.Pane(clock, shots)
        live = object.__new__(agents_live.Live)
        live.pane, live.clock, live.data, live.answers = pane, clock, {}, answers(**values)
        return live, pane

    NO = ['Do you trust the files in this folder?', '❯ No, exit', '  Yes, I trust this folder']
    YES = ['Do you trust the files in this folder?', '  No, exit', '❯ Yes, I trust this folder']

    def test_claude_trust_waits_for_the_redraw(self):
        def shots(n, events):
            downs = sum(1 for e in events if e[0] == 'key' and e[2] == 'Down')
            return self.YES if downs and n >= 4 else self.NO
        live, pane = self.live(shots)
        agents_live.accept_trust(live, 'H1', 'claude', 't2', 'P-TOOLS')
        keys = [e for e in pane.events if e[0] == 'key']
        self.assertEqual([(k[2], k[3]) for k in keys], [('Down', None), ('Enter', 'trust-claude')])
        enter = pane.events.index(keys[1])
        self.assertEqual(pane.events[enter - 1][0], 'capture')
        self.assertIn('❯ Yes, I trust this folder', pane.events[enter - 1][2])

    def test_claude_trust_refuses_without_the_selection(self):
        live, pane = self.live(lambda n, events: self.NO)
        with self.assertRaisesRegex(agents_live.Refused, 'not selected within 3 s'):
            agents_live.accept_trust(live, 'H1', 'claude', 't2', 'P-TOOLS')
        keys = [e[2] for e in pane.events if e[0] == 'key']
        self.assertEqual(keys, ['Down'])
        self.assertGreaterEqual(live.clock.now - 1000.0, agents_live.TIMEOUTS['trustSelect'])

    def test_command_waits_before_enter(self):
        live, pane = self.live(lambda n, events: ['› /quit'] if any(e[0] == 'type' for e in pane.events) and not any(e[0] == 'key' for e in pane.events) else ['idle'])
        self.assertTrue(live.command('H1', 't2', '/quit', 'claude'))
        kinds = [e[0] for e in pane.events]
        typed, enter = kinds.index('type'), kinds.index('key')
        between = pane.events[typed + 1:enter]
        self.assertEqual(pane.events[enter][3], 'command')
        self.assertEqual(between[0][0], 'capture')
        self.assertIn('› /quit', between[0][2])
        self.assertEqual(between[-1][0], 'capture')
        self.assertGreaterEqual(pane.events[enter][1] - between[0][1], agents_live.COMMAND_SETTLE)
        self.assertGreaterEqual(pane.events[enter][1] - between[-1][1], 0)
        self.assertGreater(between[-1][1], between[0][1])

    def test_command_without_echo_presses_no_enter(self):
        live, pane = self.live(lambda n, events: ['idle'])
        self.assertFalse(live.command('H1', 't2', '/quit', 'codex'))
        self.assertEqual([e[0] for e in pane.events if e[0] in ('key',)], [])
        self.assertEqual(live.data['H1']['noEcho'], ['/quit'])
        self.assertNotIn('leftover', live.data['H1'])

    def test_command_clears_a_codex_leftover(self):
        stuck, pane = self.live(lambda n, events: ['› /quit'])
        self.assertFalse(stuck.command('H1', 't2', '/quit', 'codex'))
        self.assertEqual([e[2] for e in pane.events if e[0] == 'key'], ['Enter', 'C-u'])
        self.assertEqual(stuck.data['H1']['leftover'], ['/quit'])
        clear, pane = self.live(lambda n, events: ['› /quit'] if not any(e[0] == 'key' for e in events) else ['› '])
        self.assertTrue(clear.command('H1', 't2', '/quit', 'codex'))
        self.assertEqual([e[2] for e in pane.events if e[0] == 'key'], ['Enter'])
        self.assertNotIn('leftover', clear.data['H1'])

    def test_screen_text_ignores_the_codex_composer(self):
        rename = dict((name, pattern) for name, _, pattern in agents_live.CODEX_SCREENS)['rename']
        self.assertEqual(agents_live.screen_text(['› /feedback', '  hint']), '  hint')
        self.assertIsNone(re.search(r'(?i)feedback', agents_live.screen_text(['› /feedback'])))
        self.assertIsNone(rename.search(agents_live.screen_text(['› /rename'])))
        self.assertTrue(re.search(r'(?i)feedback|Tell us more', agents_live.screen_text(['Tell us more about it'])))
        self.assertIsNone(re.search(r'(?i)goal', agents_live.screen_text(['› /goal Reply with OK'][-12:])))

    def test_sn2_reason_follows_the_answer(self):
        def finish(**values):
            live, _ = self.live(lambda n, events: [], **values)
            done = []
            live.finish = lambda case, status, reason=None, partial=None, **k: done.append((status, reason))
            live.sn2 = {'opencode': [{'result': 'accepted'}], 'pi': [{'result': 'accepted'}]}
            agents_live.record_sn2(live)
            return done[0][1]
        true = finish(Q1=True, Q2=True)
        self.assertNotIn(agents_live.live_reason('R-Q1'), true)
        self.assertIn('claude part not recorded: the claude block ended before SN2', true)
        self.assertIn('codex part not recorded: the codex block ended before SN2', true)
        false = finish(Q1=False, Q2=False)
        self.assertIn(agents_live.live_reason('R-Q1'), false)
        self.assertIn(agents_live.live_reason('R-Q2'), false)


# --- Fix round 3 (Ruling 154) ----------------------------------------------------------------------
# Run 70b1b4f6-8ad0ac09's H1 stop evidence, verbatim: the matches[0] context lines, then the stop screen (claude 2.1.318 ready).
RUN_8AD0_CLAUDE = ['  code.claude.com/docs/en/changelog for details',
    '',
    '⚠ Your login expires in 2 days · run /login to renew',
    '',
    '  Get to finished work sooner with Opus 5.5. Switch anytime with /model.',
    '',
    '',
    '',
    '',
    '',
    '',
    '',
    '',
    '· Befuddling… (0s)',
    '                                                                                                                                                                                    ◐ medium · /effort',
    '────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────',
    '❯\xa0',
    '────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────',
    '',
    '  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents']
LOGIN_WARNING = '⚠ Your login expires in 2 days · run /login to renew'


class FixS85Round3(unittest.TestCase):
    """Ruling 154, D10: claude's login-expiry warning, exactly that line, is a passive notice; other login text still stops."""

    def test_claude_login_expiry_warning_is_a_notice(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        m = {'nonce': NONCE, 'terminalIds': ['t1'], 'agents': {}}
        pane, _ = screen_pane(RUN_8AD0_CLAUDE, m)
        live = agents_live.Live(lambda method, params: {}, lambda: m, lambda *a, **k: None, Path(tmp.name), answers(), NONCE, pane, None,
                                clock=FakeClock(), home=tmp.name)
        self.assertIn(LOGIN_WARNING, RUN_8AD0_CLAUDE)
        self.assertTrue(any(line.startswith('❯') for line in RUN_8AD0_CLAUDE))
        pane.begin('H1')
        live.capture('H1', 't1', 'claude')
        live.capture('H1', 't1', 'claude')
        self.assertIsNone(pane.stopped)
        live.finish('H1', 'PASS')
        row = json.loads((Path(tmp.name) / 'cases' / 'H1.json').read_text())
        self.assertEqual(row['passiveNotices'], [{'lines': agents_live.redact([LOGIN_WARNING]), 'seen': 2}])

    def test_other_login_text_still_stops(self):
        def swap(new):
            return [new if line == LOGIN_WARNING else line for line in RUN_8AD0_CLAUDE]
        cases = [(RUN_8AD0_CLAUDE + ['  Please run /login'], 'claude'),
                 (swap('⚠ Your login expires today · run /login to renew'), 'claude'),
                 (swap('⚠ Your login expires in 3 hours · run /login to renew'), 'claude'),
                 (swap(LOGIN_WARNING + ' now'), 'claude'),
                 (swap(LOGIN_WARNING[2:]), 'claude'),
                 (swap('Your login has expired · run /login to sign in again'), 'claude'),
                 (RUN_8AD0_CLAUDE, 'codex'), (RUN_8AD0_CLAUDE, 'opencode'), (RUN_8AD0_CLAUDE, 'pi')]
        for lines, tool in cases:
            pane, _ = screen_pane(lines)
            with self.assertRaises(agents_live.StopPrompt, msg=(tool, lines)) as caught:
                pane.capture('t1', tool)
            self.assertEqual(caught.exception.pattern, 'login', (tool, lines))

    def test_both_notices_are_named_in_the_readme(self):
        text = (BASE / 'README.md').read_text()
        self.assertIn('⚠ Your login expires in <N> days · run /login to renew', text)
        self.assertIn('New version <x.y.z> is available. Run pi update', text)


if __name__ == '__main__':
    unittest.main()
