#!/usr/bin/env python3
"""Unit tests for deterministic Windows package shard planning."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github/scripts/plan-windows-package-shards.py"
SPEC = importlib.util.spec_from_file_location("plan_windows_package_shards", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class PlanWindowsPackageShardsTests(unittest.TestCase):
    def test_current_catalog_is_covered_once_by_three_balanced_shards(self) -> None:
        catalog = json.loads((ROOT / "apps/catalog.json").read_text(encoding="utf-8"))
        app_ids = MODULE.release_app_ids(catalog)

        matrix = MODULE.build_matrix(app_ids, 3)
        shards = matrix["include"]
        planned = [app_id for shard in shards for app_id in shard["apps"].split(",")]

        self.assertEqual([shard["shard"] for shard in shards], ["01", "02", "03"])
        self.assertEqual([shard["app_count"] for shard in shards], [5, 5, 5])
        self.assertCountEqual(planned, app_ids)
        self.assertEqual(len(planned), len(set(planned)))

    def test_round_robin_plan_is_deterministic_and_bounded(self) -> None:
        app_ids = [f"app-{index:02d}" for index in range(7)]
        expected = MODULE.build_matrix(app_ids, 3)

        self.assertEqual(MODULE.build_matrix(app_ids, 3), expected)
        self.assertEqual(
            [shard["apps"] for shard in expected["include"]],
            ["app-00,app-03,app-06", "app-01,app-04", "app-02,app-05"],
        )
        for invalid_count in (0, 1, 5, 100):
            with (
                self.subTest(shards=invalid_count),
                self.assertRaises(MODULE.ShardPlanError),
            ):
                MODULE.build_matrix(app_ids, invalid_count)

    def test_catalog_rejects_wrong_count_duplicates_and_unsafe_ids(self) -> None:
        valid_entries = [
            {"id": f"app-{index:02d}", "release": True}
            for index in range(MODULE.EXPECTED_RELEASE_APPS)
        ]
        variants = (
            {"apps": valid_entries[:-1]},
            {"apps": [*valid_entries[:-1], valid_entries[0]]},
            {"apps": [*valid_entries[:-1], {"id": "../escape", "release": True}]},
            {"apps": [*valid_entries[:-1], {"id": "last", "release": "true"}]},
        )
        for catalog in variants:
            with (
                self.subTest(catalog=catalog),
                self.assertRaises(MODULE.ShardPlanError),
            ):
                MODULE.release_app_ids(catalog)

        with self.assertRaises(MODULE.ShardPlanError):
            MODULE.build_matrix(["safe", 42], 2)

    def test_cli_writes_a_single_line_matrix_for_github(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = pathlib.Path(temporary) / "github-output"
            result = MODULE.main(
                [
                    "--catalog",
                    str(ROOT / "apps/catalog.json"),
                    "--shards",
                    "3",
                    "--github-output",
                    str(output),
                ]
            )

            self.assertEqual(result, 0)
            lines = output.read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(lines), 3)
            self.assertEqual(
                json.loads(lines[0].removeprefix("matrix="))["include"][0]["shard"],
                "01",
            )
            self.assertEqual(lines[1:], ["shard_count=3", "app_count=15"])


if __name__ == "__main__":
    unittest.main()
