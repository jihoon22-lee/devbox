"""The acceptance run measures the doctor its own app source actually starts."""
import unittest

import wsl_daemon


class LaunchDoctor(unittest.TestCase):
    def test_current_and_previous_bound_deployments(self):
        core = 'pub const STARTUP_DOCTOR_ARGS: [&str; 3] = ["doctor", "--json", "--launch-check"];'
        platform = "doctor.args(args).args(STARTUP_DOCTOR_ARGS);"
        self.assertEqual(wsl_daemon.doctor_argv(core, platform), ['doctor', '--json', '--launch-check'])
        old = core.replace('--launch-check', '--startup')
        self.assertEqual(wsl_daemon.doctor_argv(old, platform), ['doctor', '--json', '--startup'])
        rc1 = 'doctor.args(["--exec", "sh", "doctor", "--json", ]);'
        self.assertEqual(wsl_daemon.doctor_argv('', rc1), ['doctor', '--json'])
        with self.assertRaises(ValueError):
            wsl_daemon.doctor_argv(core.replace('--launch-check', '--unknown'), platform)
        with self.assertRaises(ValueError):
            wsl_daemon.doctor_argv(core, 'command.args(["doctor","--json"]);')
