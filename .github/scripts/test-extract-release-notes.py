#!/usr/bin/env python3
"""CLI tests for extract-release-notes.py."""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("extract-release-notes.py")


class ExtractReleaseNotesCliTests(unittest.TestCase):
    def run_cli(self, changelog: str, tag: str) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as directory:
            changelog_path = Path(directory) / "CHANGELOG.md"
            changelog_path.write_text(changelog, encoding="utf-8")
            return subprocess.run(
                [sys.executable, str(SCRIPT), str(changelog_path), tag],
                check=False,
                capture_output=True,
                text=True,
            )

    def test_existing_section_writes_exact_non_empty_body(self) -> None:
        result = self.run_cli(
            "## [v1.2.3] - 2026-01-01\n"
            "\n"
            "### Fixed\n"
            "\n"
            "- Preserve this line.\n"
            "\n"
            "## [v1.2.2] - 2025-12-01\n"
            "\n"
            "- Do not include this adjacent release.\n",
            "v1.2.3",
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout,
            "\n### Fixed\n\n- Preserve this line.\n\n",
        )
        self.assertEqual(result.stderr, "")

    def test_missing_tag_exits_nonzero_with_actionable_stderr(self) -> None:
        result = self.run_cli(
            "## [v1.2.3] - 2026-01-01\n\n- Existing release.\n",
            "v9.9.9",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("v9.9.9", result.stderr)
        self.assertIn("not found", result.stderr.lower())

    def test_whitespace_only_section_exits_nonzero_with_actionable_stderr(self) -> None:
        result = self.run_cli(
            "## [v1.2.3] - 2026-01-01\n"
            "\n"
            "   \n"
            "\t\n"
            "## [v1.2.2] - 2025-12-01\n"
            "\n"
            "- Existing release.\n",
            "v1.2.3",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("v1.2.3", result.stderr)
        self.assertIn("empty", result.stderr.lower())

    def test_exact_tag_selection_stops_at_adjacent_release_sections(self) -> None:
        result = self.run_cli(
            "## [v1.2.3-rc1] - 2026-01-01\n"
            "\n"
            "- RC content.\n"
            "\n"
            "## [v1.2.3] - 2026-01-02\n"
            "\n"
            "- Stable content.\n"
            "\n"
            "## [v1.2.2] - 2025-12-01\n"
            "\n"
            "- Older content.\n",
            "v1.2.3-rc1",
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "\n- RC content.\n\n")
        self.assertNotIn("Stable content", result.stdout)
        self.assertNotIn("Older content", result.stdout)
        self.assertEqual(result.stderr, "")


if __name__ == "__main__":
    unittest.main()
