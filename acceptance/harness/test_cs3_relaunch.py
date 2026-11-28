"""CS3 compares two launches with like inputs and waits for evidence from the relaunched app."""
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

import daemon_rows


class RelaunchIdentity(unittest.TestCase):
    def run_case(self, *, native=True, new_identity=False, pid='101', first_data=True,
                 second_changes=False, missing_invocation=False, read_error=False):
        events = []
        identity = {'MainPID': '17', 'InvocationID': 'a' * 32, 'ActiveState': 'active'}
        if missing_invocation:
            identity['InvocationID'] = ''
        if read_error:
            identity['error'] = 'systemctl exit 1'
        setup_count = 0

        def setup(trigger):
            nonlocal setup_count
            setup_count += 1
            self.assertEqual(trigger, 'launch')
            events.append('setup')
            # Unlike the app, the harness captures a fixed PATH. Establishing it once may restart the unit.
            changed = native and setup_count == 1 or second_changes
            if changed:
                identity.update(MainPID=str(20 + setup_count), InvocationID=str(setup_count) * 32)
            return {'steps': [{'id': 'restart', 'ok': True, 'message': '' if changed else daemon_rows.SKIPPED}]}

        def sc2(record, clock):
            events.append('native-ready')
            if new_identity:
                identity.update(MainPID='18', InvocationID='b' * 32)
            return {'firstDataPainted': [{'run': pid}] if first_data else []}

        win = Mock() if native else None
        if win:
            win.stop_owned.return_value = {'stopped': True}
            win.launch.return_value = {'pid': 101}
            win.sc2.side_effect = sc2
        with tempfile.TemporaryDirectory() as tmp:
            rows = daemon_rows.DaemonRows(Mock(), Mock(), Mock(), tmp, 'fresh' if native else 'wsl-fresh',
                                          daemon=Mock(setup=setup), win=win)
            rows.info = Mock()
            with patch.object(daemon_rows, 'require_lock'), patch.object(daemon_rows, 'unit_ids', side_effect=lambda: dict(identity)):
                rows.cs3()
            result = json.loads((Path(tmp) / 'cases/CS3.json').read_text())
        return result, events

    def test_native_and_harness_launches_each_keep_their_own_baseline(self):
        row, events = self.run_case()
        self.assertEqual(row['status'], 'PASS')
        self.assertEqual(events, ['native-ready', 'setup', 'setup'])
        self.assertNotEqual(row['before']['MainPID'], row['beforeSetup']['MainPID'])
        self.assertEqual(row['beforeSetup'], row['after'])

    def test_a_native_restart_or_unproven_completion_cannot_pass(self):
        for options in ({'new_identity': True}, {'pid': '99'}, {'first_data': False}, {'missing_invocation': True},
                        {'read_error': True}):
            with self.subTest(options=options):
                row, events = self.run_case(**options)
                self.assertEqual(row['status'], 'FAIL')
                self.assertNotIn('setup', events)

    def test_restart_on_the_repeated_identical_setup_fails(self):
        row, _ = self.run_case(second_changes=True)
        self.assertEqual(row['status'], 'FAIL')
        self.assertFalse(row['parts']['a'])
        self.assertFalse(row['parts']['b'])

    def test_wsl_only_keeps_the_existing_single_launch_check(self):
        row, events = self.run_case(native=False)
        self.assertEqual(row['status'], 'NOT_RUN')
        self.assertEqual(row['parts'], {'a': True, 'b': True})
        self.assertEqual(events, ['setup'])
