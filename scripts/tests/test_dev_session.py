import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import Mock, patch

SCRIPT = Path(__file__).resolve().parents[1] / 'dev-session.py'
spec = importlib.util.spec_from_file_location('dev_session', SCRIPT)
session = importlib.util.module_from_spec(spec)
spec.loader.exec_module(session)

class OwnedDevelopmentSession(unittest.TestCase):
    def test_vite_announcement_handles_colored_aggregated_output(self):
        colored = '\x1b[32m  ➜\x1b[39m  \x1b[1mLocal:\x1b[22m  \x1b[36mhttp://127.0.0.1:\x1b[1m1450\x1b[22m/\x1b[39m\n'
        for split in range(len(colored)):
            self.assertTrue(session.vite_announced(colored[:split] + colored[split:], 1450))
        for invalid in ['Local: http://localhost:1460/\n', 'Local: http://localhost:14500/\n',
                        'Local:\nhttp://localhost:1450/\n', 'Local: error\nNetwork: http://localhost:1450/\n',
                        'Local: http://foreign.example:1450/\n', 'Local: http://localhost:1450/extra\n',
                        'Local: http://localhost:1450']:
            self.assertFalse(session.vite_announced(invalid, 1450), invalid)

    def test_runtime_directory_matches_core_absolute_xdg_rule(self):
        with patch.object(session.os, 'getuid', return_value=123):
            for value in [None, '', 'relative/run']:
                env = {} if value is None else {'XDG_RUNTIME_DIR': value}
                self.assertEqual(session.runtime_directory(env), Path('/run/user/123'))
            self.assertEqual(session.runtime_directory({'XDG_RUNTIME_DIR': '/owned/run'}), Path('/owned/run'))

    def test_gateway_failure_cleans_only_a_newly_owned_daemon(self):
        for existing in [False, True]:
            with self.subTest(existing=existing):
                child = Mock(pid=321)
                child.poll.return_value = None
                failure = RuntimeError('gateway spawn failed')
                with patch.object(session.signal, 'signal') as handlers, \
                        patch.object(session.subprocess, 'check_output', return_value=json.dumps({'target_directory': '/fixture/target'}).encode()), \
                        patch.object(session.subprocess, 'Popen', side_effect=failure if existing else [child, failure]) as spawn, \
                        patch.object(session, 'hello', side_effect=None if existing else [OSError('absent'), None]), \
                        patch.object(session.os, 'killpg') as kill:
                    with self.assertRaisesRegex(RuntimeError, 'gateway spawn failed'):
                        session.main()
                    self.assertEqual([call.args[0] for call in handlers.call_args_list],
                                     [session.signal.SIGINT, session.signal.SIGTERM, session.signal.SIGUSR1])
                    self.assertEqual(spawn.call_count, 1 if existing else 2)
                    self.assertTrue(spawn.call_args_list[0].kwargs['start_new_session'])
                    if existing:
                        self.assertEqual(spawn.call_args.args[0][1], 'dev-gateway')
                        kill.assert_not_called()
                    else:
                        kill.assert_called_once_with(child.pid, session.signal.SIGTERM)
                        child.wait.assert_called_once_with(timeout=5)

    def test_restart_replaces_only_the_owned_daemon_and_announces_monotonic_times(self):
        old, new, gateway, vite = Mock(pid=11), Mock(pid=12), Mock(pid=13), Mock(pid=14)
        children = [old, gateway, vite]
        old.wait.side_effect = lambda **_: self.assertIn(old, children)
        probe = Mock()
        out = io.StringIO()
        with patch.object(session.os, 'killpg') as kill, patch.object(session.time, 'monotonic', side_effect=[10.0, 12.5]), \
                patch('sys.stdout', out):
            self.assertIs(session.restart(old, children, lambda: children.append(new) or new, probe), new)
        kill.assert_called_once_with(old.pid, session.signal.SIGTERM)
        old.wait.assert_called_once_with(timeout=5)
        probe.assert_called_once_with()
        self.assertEqual(children, [gateway, vite, new])
        gateway.wait.assert_not_called()
        vite.wait.assert_not_called()
        self.assertEqual(out.getvalue().splitlines(), ['Devbox: daemon restart requested monotonic=10.000',
                                                       'Devbox: daemon ready monotonic=12.500 elapsed_ms=2500'])

    def test_cleanup_signals_every_child_before_reaping_any(self):
        events = []
        children = [Mock(pid=pid) for pid in (21, 22, 23)]
        for child in children:
            child.wait.side_effect = lambda child=child, **_: events.append(('wait', child.pid))
        with patch.object(session.os, 'killpg', side_effect=lambda pid, sig: events.append(('kill', pid))):
            session.cleanup(children)
        self.assertEqual(events, [('kill', 23), ('kill', 22), ('kill', 21), ('wait', 23), ('wait', 22), ('wait', 21)])

    def test_restart_refuses_a_reused_daemon_without_signalling(self):
        gateway = Mock(pid=13)
        relaunch, probe, out = Mock(), Mock(), io.StringIO()
        with patch.object(session.os, 'killpg') as kill, patch('sys.stdout', out):
            self.assertIsNone(session.restart(None, [gateway], relaunch, probe))
        kill.assert_not_called()
        relaunch.assert_not_called()
        probe.assert_not_called()
        self.assertIn('restart refused', out.getvalue())

if __name__ == '__main__':
    unittest.main()
