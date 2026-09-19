#!/usr/bin/env python3
"""Synthetic assembly ownership regressions; no app or NSIS compiler is run."""
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("suite_installer", Path(__file__).with_name("build-suite-installer.py"))
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class SuiteInstallerInputs(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="devbox-suite-input-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.payload = self.root / "payload"
        self.payload.mkdir()
        (self.payload / "THIRD_PARTY_NOTICES.md").write_bytes(b"synthetic notices")
        for product in installer.package.PRODUCTS:
            root = self.payload / product
            root.mkdir()
            (root / f"devbox-{product}.exe").write_bytes(f"synthetic {product}".encode())
        wsl = self.payload / "workspace/resources/wsl"
        wsl.mkdir(parents=True)
        (wsl / "manifest.json").write_bytes(b"{}")
        (wsl / "devbox-workspace-wsl").write_bytes(b"synthetic wsl helper")
        suite = self.payload / "control-center/resources/suite"
        suite.mkdir(parents=True)
        self.bootstrap = suite / "devbox-suite-bootstrap.exe"
        self.bootstrap.write_bytes(b"synthetic bootstrap")
        self.staging = self.root / "stage"
        installer.package.portables(self.payload, self.staging, "0.8.0", "a" * 40)

    def rewrite(self, edit):
        path = self.staging / "suite-payload.json"
        value = json.loads(path.read_text())
        edit(value)
        path.write_text(json.dumps(value))

    def rejected(self):
        with self.assertRaises((ValueError, StopIteration)):
            installer.prepare(self.staging, self.bootstrap)
        self.assertFalse((self.staging / "suite-setup.nsi").exists())
        self.assertFalse((self.staging / "Devbox_0.8.0_x64-setup.exe").exists())

    def test_review_source_is_created_without_running_or_publishing_a_setup(self):
        script, output, inputs = installer.prepare(self.staging, self.bootstrap)
        self.assertTrue(script.is_file())
        self.assertFalse(output.exists())
        self.assertEqual(len(inputs), 6)  # Private payload + four exact ZIPs + helper.
        for path, expected in inputs:
            self.assertEqual(installer.package.asset(path, expected["name"]), expected)
        with self.assertRaises(ValueError):
            installer.prepare(self.staging, self.bootstrap)

    def test_changed_helper_cannot_be_embedded(self):
        self.bootstrap.write_bytes(b"unreviewed replacement bootstrap")
        self.rejected()

    def test_changed_zip_cannot_be_embedded(self):
        (self.staging / "devbox-workspace_0.8.0_x64.zip").write_bytes(b"unreviewed archive")
        self.rejected()

    def test_manifest_cannot_select_a_parent_file(self):
        self.rewrite(lambda value: value["products"][0]["portable"].update(name="../outside.zip"))
        self.rejected()

    def test_mixed_product_versions_cannot_be_embedded(self):
        self.rewrite(lambda value: value["products"][0].update(version="0.9.0"))
        self.rejected()

    def test_existing_setup_is_never_overwritten(self):
        output = self.staging / "Devbox_0.8.0_x64-setup.exe"
        output.write_bytes(b"previous candidate")
        with self.assertRaises(ValueError):
            installer.prepare(self.staging, self.bootstrap)
        self.assertEqual(output.read_bytes(), b"previous candidate")


if __name__ == "__main__":
    unittest.main()
