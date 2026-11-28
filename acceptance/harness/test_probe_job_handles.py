"""G1/G2 job ownership against the candidate's native class, without starting Windows processes."""
import json
import re
import tempfile
import unittest
from contextlib import ExitStack, redirect_stdout
from io import StringIO
from pathlib import Path
from unittest.mock import Mock, patch

import common
import g1_live_member
import g2_probe_timing


class ProbeJobHandles(unittest.TestCase):
    def test_both_root_scripts_assign_the_created_handle_and_protect_its_lifetime(self):
        binding = common.BASE / 'source-binding.json'
        bound = json.loads(binding.read_bytes()).get('candidate') if binding.is_file() else None
        native = common.native_class_source(common.BASE, bound['sha'] if bound else 'HEAD')
        self.assertRegex(native, r'public static IntPtr CreateFresh\(string \w+\)')
        self.assertRegex(native, r'public static void AssociateFresh\(IntPtr \w+\)')

        nonce = 'a' * 32
        job = common.assigned_receipt(nonce, 1, '1', 'S-1-5-1', 1)['jobName']
        for case, template in [('G1', g1_live_member.ROOT), ('G2', g2_probe_timing.ROOT)]:
            with self.subTest(case=case):
                script = template.replace('JOB', job)
                setup, cleanup = script.split('} finally {', 1)
                protected = setup.split('try {', 1)[1]
                create = "$job=[DevboxObservation]::CreateFresh('%s')" % job
                associate = '[DevboxObservation]::AssociateFresh($job)'
                self.assertIn(create, protected)
                self.assertIn(associate, [line.strip() for line in protected.splitlines()])
                self.assertLess(protected.index(create), protected.index(associate))
                self.assertLess(protected.index(associate), protected.index('$listener.Start()'))
                self.assertRegex(cleanup, r'if\(\$job -ne \[IntPtr\]::Zero\)\s*\{\s*\[void\]\[DevboxObservation\]::CloseHandle\(\$job\)')
                self.assertLess(cleanup.index('::CloseHandle($job)'), cleanup.index('$listener.Stop()'))
                if case == 'G1':
                    self.assertLess(protected.index(associate), protected.index('[S7G1]::StartIsolated'))
                    self.assertRegex(protected, re.escape('[DevboxObservation]::CloseHandle($job)') + r';\s*\$job=\[IntPtr\]::Zero')

    def test_g1_releases_an_unannounced_child_before_closing_its_exact_process_handle(self):
        root = g1_live_member.ROOT
        self.assertIn("::new($false, 'ManualReset', 'EVENT'", root)
        self.assertLess(root.index("::new($false, 'ManualReset', 'EVENT'"), root.index('::StartIsolated'))
        self.assertLess(g1_live_member.CHILD.index("::OpenExisting('EVENT')"), g1_live_member.CHILD.index('$ready.Set()'))
        self.assertLess(root.index('::StartIsolated'), root.index('$ready.WaitOne('))
        self.assertIn("if($line -eq 'exit') { $preserveChild=$true; break }", root)
        cleanup = root.split('} finally {', 1)[1]
        self.assertLess(cleanup.index('$release.Set()'), cleanup.index('::WaitForExit($child'))
        self.assertLess(cleanup.index('::WaitForExit($child'), cleanup.index('::Release($child)'))
        self.assertFalse(g1_live_member.proved_startup_cleanup([]))
        self.assertFalse(g1_live_member.proved_startup_cleanup(['not JSON', '{"phase":"startup-cleanup","child":"unknown"}']))
        for child in ('never-started', 'exited'):
            self.assertTrue(g1_live_member.proved_startup_cleanup([json.dumps({'phase': 'startup-cleanup', 'child': child})]))

    def test_g1_reports_an_unannounced_child_as_retained_without_proof_of_exit(self):
        for outcome in ('exited', 'unknown', None):
            with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as directory, ExitStack() as ctx:
                root = Mock()
                report = {'phase': 'startup-cleanup', 'child': outcome}
                root.lines = [json.dumps(report)] if outcome else []
                root.stderr = []
                root.close.return_value = 1
                root.read_json.side_effect = RuntimeError('setup failed after child creation')
                runs = Path(directory) / 'runs'
                runs.mkdir()
                overrides = {'evidence_root': Path(directory), 'require_lock': 1, 'probe_script': ('mock', {}),
                             'native_class_source': '', 'LineProcess': root, 'ps_command': {}}
                for helper, value in overrides.items():
                    ctx.enter_context(patch.object(g1_live_member, helper, return_value=value))
                ctx.enter_context(patch('sys.argv', ['g1', '--worktree', directory, '--source-sha', 'a' * 40, '--out', str(runs)]))
                with redirect_stdout(StringIO()):
                    self.assertEqual(g1_live_member.main(), 1)
                g1_live_member.ps_command.assert_not_called()
                result = json.loads(next(runs.glob('g1-*/g1.json')).read_bytes())
                self.assertEqual(result['status'], 'FAIL')
                self.assertEqual(result['retainedResources'], [] if outcome == 'exited' else ['child'])

    def test_final_observation_requires_positive_root_and_child_exit_evidence(self):
        def released(states):
            return g1_live_member.all_owned_released({'observation': {'rawState': 'notFound'}, 'identities': states})

        self.assertTrue(released({'root': 'reused', 'child': 'exited:0'}))
        self.assertTrue(released({'root': 'exited:4294967295', 'child': 'absent'}))
        for state in ('alive', 'unavailable:5', 'exited:', 'exited:bad', 'exited:259', 'exited:4294967296'):
            with self.subTest(state=state):
                self.assertFalse(released({'root': 'absent', 'child': state}))
        self.assertFalse(released({'root': 'absent'}))
        self.assertFalse(released({}))

    def test_unavailable_or_missing_identity_is_retained_in_both_cleanup_paths(self):
        started = dict(phase='started', pid=101, creationTime='10', childPid=102, childCreationTime='11',
                       sid='S-1-5-1', sessionId=1, port=50000)
        for module, case in ((g1_live_member, 'g1'), (g2_probe_timing, 'g2')):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as tmp, ExitStack() as ctx:
                directory = Path(tmp)
                runs = directory / 'runs'
                runs.mkdir()
                root = Mock(lines=[], stderr=[])
                root.read_json.return_value = started
                root.close.return_value = 0
                overrides = {'evidence_root': directory, 'require_lock': 1, 'probe_script': ('mock', {}),
                             'native_class_source': '', 'LineProcess': root, 'identity_states': {'root': 'unavailable:5'}}
                if module is g1_live_member:
                    overrides['ps_command'] = {}
                for key, value in overrides.items():
                    ctx.enter_context(patch.object(module, key, return_value=value))
                ctx.enter_context(patch.object(module, 'run_probe', side_effect=RuntimeError('owned probe fixture')))
                stop = ctx.enter_context(patch.object(module, 'stop_exact'))
                ctx.enter_context(patch('sys.argv', [case, '--worktree', tmp, '--source-sha', 'a' * 40, '--out', str(runs)]))
                with redirect_stdout(StringIO()):
                    self.assertEqual(module.main(), 1)
                evidence = json.loads(next(runs.glob(f'{case}-*/{case}.json')).read_bytes())
                self.assertEqual(evidence['retainedResources'], ['root', 'child'] if case == 'g1' else ['root'])
                self.assertTrue(all(row['status'] == 'FAIL' for row in evidence['cleanup']))
                stop.assert_not_called()
