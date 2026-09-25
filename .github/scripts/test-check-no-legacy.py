#!/usr/bin/env python3
import importlib.util, pathlib, tempfile, unittest

SPEC = importlib.util.spec_from_file_location("guard", pathlib.Path(__file__).with_name("check-no-legacy.py"))
guard = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(guard)

class GuardTest(unittest.TestCase):
    def test_flags_forbidden_names_and_ignores_other_files(self):
        with tempfile.TemporaryDirectory() as root:
            base = pathlib.Path(root)
            (base / "a.rs").write_text('fn x() { legacy_cleanup::run(); }\n')
            (base / "b.md").write_text("legacy_cleanup in docs is fine\n")
            (base / "c.ts").write_text("export const ok = 1;\n")
            (base / "d.rs").write_text("use life_log_lib::component;\n")
            original = guard.ROOT
            guard.ROOT = base
            try:
                hits = guard.scan(["."])
            finally:
                guard.ROOT = original
            self.assertEqual(len(hits), 2)
            self.assertEqual({hit.split(":")[0] for hit in hits}, {"a.rs", "d.rs"})

if __name__ == "__main__":
    unittest.main()
