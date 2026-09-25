#!/usr/bin/env python3
import importlib.util, pathlib, tempfile, textwrap, unittest

SPEC = importlib.util.spec_from_file_location("deps", pathlib.Path(__file__).with_name("check-workspace-deps.py"))
deps = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(deps)

class WorkspaceDepsTest(unittest.TestCase):
    def write(self, root, relative, text):
        path = pathlib.Path(root) / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text))

    def test_members_must_use_workspace_versions(self):
        with tempfile.TemporaryDirectory() as root:
            self.write(root, "Cargo.toml", """
                [workspace]
                members = ["a"]
                [workspace.dependencies]
                serde = "1"
            """)
            self.write(root, "a/Cargo.toml", """
                [package]
                name = "a"
                [dependencies]
                serde = "1.0.200"
                local = { path = "../local" }
            """)
            self.assertEqual(deps.violations(pathlib.Path(root)), ["a/Cargo.toml: serde must use workspace = true"])

    def test_workspace_usage_passes(self):
        with tempfile.TemporaryDirectory() as root:
            self.write(root, "Cargo.toml", """
                [workspace]
                members = ["a"]
                [workspace.dependencies]
                serde = "1"
            """)
            self.write(root, "a/Cargo.toml", """
                [package]
                name = "a"
                [dependencies]
                serde = { workspace = true }
            """)
            self.assertEqual(deps.violations(pathlib.Path(root)), [])

if __name__ == "__main__":
    unittest.main()
