#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / f".github/scripts/{name}.py")
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


scaffold = module("scaffold-product-feature").scaffold
check = module("check-feature-fixtures").check


class FeatureScaffoldTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "apps").mkdir()
        (self.root / "apps/products.json").write_bytes((ROOT / "apps/products.json").read_bytes())
        (self.root / "apps/product-feature-fixtures.json").write_text('{"schemaVersion":1,"entries":[]}')

    def test_new_route_is_registered_with_fixture_document_and_revision(self):
        original = json.loads((self.root / "apps/products.json").read_text())
        self.assertEqual(scaffold(self.root, "workspace", "fixture-route", "Fixture"), "workspace.fixture-route")
        check(self.root)
        catalog = json.loads((self.root / "apps/products.json").read_text())
        self.assertEqual(catalog["catalogRevision"], original["catalogRevision"] + 1)
        self.assertEqual(catalog["features"][-1]["authority"], "shell-read")
        document = self.root / "docs/features/workspace.fixture-route.md"
        document.unlink()
        with self.assertRaises(AssertionError):
            check(self.root)

    def test_invalid_and_duplicate_inputs_do_not_change_existing_files(self):
        scaffold(self.root, "control-center", "migration", "이전 및 백업")
        def files():
            return {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        before = files()
        for owner, route, label in [("other", "new", "New"), ("workspace", "../escape", "New"),
                                     ("workspace", "new", ""), ("control-center", "migration", "이전 및 백업"),
                                     ("workspace", "overview", "Conflicting label")]:
            with self.assertRaises(ValueError):
                scaffold(self.root, owner, route, label)
            self.assertEqual(files(), before)
        check(self.root)


if __name__ == "__main__":
    unittest.main()
