#!/usr/bin/env python3
"""Unit tests for the release input policy used by release.yml."""

from __future__ import annotations

import importlib.util
import pathlib
import subprocess
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github/scripts/validate-release-input.py"
SPEC = importlib.util.spec_from_file_location("validate_release_input", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class ValidateReleaseInputTests(unittest.TestCase):
    def test_stable_push_remains_a_latest_release(self) -> None:
        result = MODULE.validate_release_input("push", "v0.5.0", "false")
        self.assertEqual(result.tag, "v0.5.0")
        self.assertFalse(result.prerelease)
        self.assertTrue(result.make_latest)

    def test_stable_manual_dispatch_remains_supported(self) -> None:
        result = MODULE.validate_release_input(
            "workflow_dispatch", "v1.2.3", False
        )
        self.assertFalse(result.prerelease)
        self.assertTrue(result.make_latest)

    def test_explicitly_gated_manual_rc_is_supported(self) -> None:
        result = MODULE.validate_release_input(
            "workflow_dispatch", "v0.5.0-rc3", "true"
        )
        self.assertEqual(result.tag, "v0.5.0-rc3")
        self.assertTrue(result.prerelease)
        self.assertFalse(result.make_latest)

    def test_any_valid_exact_prerelease_can_be_gated_for_future_use(self) -> None:
        result = MODULE.validate_release_input(
            "workflow_dispatch", "v2.0.0-beta.1", True
        )
        self.assertTrue(result.prerelease)
        self.assertFalse(result.make_latest)

    def test_push_rc_is_rejected_before_remote_or_build_checks(self) -> None:
        with self.assertRaisesRegex(MODULE.ReleaseInputError, "rejected from push"):
            MODULE.validate_release_input("push", "v0.5.0-rc1", "false")

    def test_manual_rc_defaults_closed(self) -> None:
        with self.assertRaisesRegex(MODULE.ReleaseInputError, "allow_prerelease=true"):
            MODULE.validate_release_input(
                "workflow_dispatch", "v0.5.0-rc1", "false"
            )

    def test_malformed_or_ambiguous_tags_are_rejected(self) -> None:
        for tag in (
            "0.5.0",
            "v0.5",
            "v01.2.3",
            "v0.5.0-",
            "v0.5.0-rc..1",
            "v0.5.0-01",
            "v0.5.0+build.1",
            "v0.5.0\n",
        ):
            with self.subTest(tag=repr(tag)):
                with self.assertRaises(MODULE.ReleaseInputError):
                    MODULE.validate_release_input("workflow_dispatch", tag, False)

    def test_unknown_event_and_gate_value_fail_closed(self) -> None:
        with self.assertRaisesRegex(MODULE.ReleaseInputError, "unsupported release event"):
            MODULE.validate_release_input("schedule", "v0.5.0", False)
        with self.assertRaisesRegex(MODULE.ReleaseInputError, "exactly 'true' or 'false'"):
            MODULE.validate_release_input("workflow_dispatch", "v0.5.0-rc1", "TRUE")

    def test_cli_writes_only_validated_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = pathlib.Path(temporary) / "github-output"
            completed = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--event",
                    "workflow_dispatch",
                    "--tag",
                    "v0.5.0-rc3",
                    "--allow-prerelease",
                    "true",
                    "--github-output",
                    str(output),
                ],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertEqual(
                output.read_text(encoding="utf-8"),
                "tag=v0.5.0-rc3\nprerelease=true\nmake_latest=false\n",
            )

    def test_rejected_cli_input_does_not_write_job_outputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = pathlib.Path(temporary) / "github-output"
            completed = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT),
                    "--event",
                    "push",
                    "--tag",
                    "v0.5.0-rc1",
                    "--allow-prerelease",
                    "false",
                    "--github-output",
                    str(output),
                ],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(completed.returncode, 0)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
