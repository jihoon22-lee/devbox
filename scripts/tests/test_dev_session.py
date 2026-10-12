import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

SCRIPT = Path(__file__).resolve().parents[1] / 'dev-session.py'
spec = importlib.util.spec_from_file_location('dev_session', SCRIPT)
session = importlib.util.module_from_spec(spec)
spec.loader.exec_module(session)


class OwnedDevelopmentSession(unittest.TestCase):
    def test_runtime_directory_matches_core_absolute_xdg_rule(self):
        with patch.object(session.os, 'getuid', return_value=123):
            for value in [None, '', 'relative/run']:
                env = {} if value is None else {'XDG_RUNTIME_DIR': value}
                self.assertEqual(session.runtime_directory(env), Path('/run/user/123'))
            self.assertEqual(session.runtime_directory({'XDG_RUNTIME_DIR': '/owned/run'}), Path('/owned/run'))

    def test_partial_gateway_failure_reaps_only_owned_daemon(self):
        child = Mock(pid=321)
        child.poll.return_value = None
        with patch.object(session.signal, 'signal') as handlers, \
                patch.object(session.subprocess, 'check_output', return_value=json.dumps({'target_directory': '/fixture/target'}).encode()), \
                patch.object(session.subprocess, 'Popen', side_effect=[child, RuntimeError('gateway spawn failed')]) as spawn, \
                patch.object(session, 'hello', side_effect=[OSError('absent'), None]), \
                patch.object(session.os, 'killpg') as kill:
            with self.assertRaisesRegex(RuntimeError, 'gateway spawn failed'):
                session.main()
            self.assertEqual(handlers.call_count, 2)
            self.assertEqual(spawn.call_count, 2)
            self.assertTrue(spawn.call_args_list[0].kwargs['start_new_session'])
            kill.assert_called_once_with(child.pid, session.signal.SIGTERM)
            child.wait.assert_called_once_with(timeout=5)

    def test_existing_compatible_daemon_is_never_owned_or_stopped(self):
        with patch.object(session.signal, 'signal'), \
                patch.object(session.subprocess, 'check_output', return_value=json.dumps({'target_directory': '/fixture/target'}).encode()), \
                patch.object(session.subprocess, 'Popen', side_effect=RuntimeError('gateway spawn failed')) as spawn, \
                patch.object(session, 'hello'), \
                patch.object(session.os, 'killpg') as kill:
            with self.assertRaisesRegex(RuntimeError, 'gateway spawn failed'):
                session.main()
            self.assertEqual(spawn.call_args.args[0][1], 'dev-gateway')
            kill.assert_not_called()


if __name__ == '__main__':
    unittest.main()
