#!/usr/bin/env python3
"""Unit tests for cross-platform Windows package flattening."""

from __future__ import annotations

import importlib.util
import json
import pathlib
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github/scripts/flatten-windows-packages.py"
SPEC = importlib.util.spec_from_file_location("flatten_windows_packages", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def fixture(root: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    staging = root / "staging"
    staging.mkdir()
    catalog_apps = []
    manifest_apps = []
    for index in range(MODULE.EXPECTED_RELEASE_APPS):
        app_id = f"app-{index:02d}"
        portable_name = f"{app_id}.exe"
        installer_name = f"{app_id}_1.2.3_x64-setup.exe"
        portable = staging / app_id / "portable" / portable_name
        installer = staging / app_id / "installer" / installer_name
        portable.parent.mkdir(parents=True)
        installer.parent.mkdir(parents=True)
        portable.write_bytes(f"portable:{app_id}".encode())
        installer.write_bytes(f"installer:{app_id}".encode())
        catalog_apps.append({"id": app_id, "release": True})
        manifest_apps.append(
            {
                "id": app_id,
                "portable": {"name": portable_name},
                "installer": {"name": installer_name},
            }
        )

    catalog = root / "catalog.json"
    catalog.write_text(json.dumps({"apps": catalog_apps}), encoding="utf-8")
    (staging / "THIRD_PARTY_NOTICES.md").write_text("notices\n", encoding="utf-8")
    (staging / "release-manifest.json").write_text(
        json.dumps(
            {
                "apps": manifest_apps,
                "notices": {"name": "THIRD_PARTY_NOTICES.md"},
            }
        ),
        encoding="utf-8",
    )
    return staging, catalog


class FlattenWindowsPackagesTests(unittest.TestCase):
    def test_flattens_exact_complete_package_contract(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            staging, catalog = fixture(root)
            output = root / "assets"

            MODULE.flatten(staging, catalog, output)

            files = sorted(output.iterdir())
            self.assertEqual(len(files), MODULE.EXPECTED_FILES)
            self.assertTrue(
                all(path.is_file() and not path.is_symlink() for path in files)
            )
            self.assertIn(output / "app-00.exe", files)
            self.assertIn(output / "app-14_1.2.3_x64-setup.exe", files)

    def test_rejects_undeclared_staging_entry(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            staging, catalog = fixture(root)
            (staging / "unexpected.bin").write_bytes(b"unexpected")

            with self.assertRaisesRegex(MODULE.FlattenError, "missing or undeclared"):
                MODULE.flatten(staging, catalog, root / "assets")

    def test_rejects_linked_or_misnamed_package(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            staging, catalog = fixture(root)
            portable = staging / "app-00" / "portable" / "app-00.exe"
            outside = root / "outside.exe"
            outside.write_bytes(b"outside")
            portable.unlink()
            portable.symlink_to(outside)

            with self.assertRaisesRegex(MODULE.FlattenError, "non-empty regular file"):
                MODULE.flatten(staging, catalog, root / "assets")

            portable.unlink()
            (portable.parent / "wrong.exe").write_bytes(b"wrong")
            with self.assertRaisesRegex(
                MODULE.FlattenError, "staging contract mismatch"
            ):
                MODULE.flatten(staging, catalog, root / "assets-2")

    def test_rejects_manifest_catalog_identity_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            staging, catalog = fixture(root)
            manifest_path = staging / "release-manifest.json"
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            manifest["apps"].reverse()
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

            with self.assertRaisesRegex(MODULE.FlattenError, "differ from the catalog"):
                MODULE.flatten(staging, catalog, root / "assets")


if __name__ == "__main__":
    unittest.main()
