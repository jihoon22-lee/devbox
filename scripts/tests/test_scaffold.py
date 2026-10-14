import os
import textwrap
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ScaffoldScripts(unittest.TestCase):
    def test_banned_words_detects_string_even_with_router_identifier(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            (root / 'app/src').mkdir(parents=True)
            script = root / 'scripts/banned-words.sh'
            script.write_bytes((ROOT / 'scripts/banned-words.sh').read_bytes())
            source = root / 'app/src/View.tsx'
            source.write_text('const routerLabel = "Workbench";\n')
            self.assertEqual(subprocess.run(['bash', str(script)], capture_output=True).returncode, 1)
            source.write_text('import { createRoute } from "@tanstack/react-router";\nconst label = "연결";\n')
            self.assertEqual(subprocess.run(['bash', str(script)], capture_output=True).returncode, 0)
            (root / 'app/src/View.test.tsx').write_text('const label = "Workbench";\n')
            self.assertEqual(subprocess.run(['bash', str(script)], capture_output=True).returncode, 0)
            for text, expected in [
                ('const identity = `${target}:${generation}:${state}`;', 0),
                ('const label = `generation ${count}`;', 1),
                ('const label = `${"generation"}`;', 1),
                ('const label = "it\\\"s generation";', 1),
                ('const label = "${generation}";', 1),
                (r'const label = `\${generation}`;', 1),
                (r'const identity = `\\${generation}`;', 0),
            ]:
                with self.subTest(text=text):
                    source.write_text(text + '\n')
                    self.assertEqual(subprocess.run(['bash', str(script)], capture_output=True).returncode, expected)

    def test_sweep_never_removes_external_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory) / 'shared-cache'
            cache.mkdir()
            artifact = cache / 'artifact'
            artifact.write_text('preserve')
            result = subprocess.run(['bash', str(ROOT / 'scripts/sweep.sh'), str(cache)], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(artifact.read_text(), 'preserve')
            result = subprocess.run(['bash', str(ROOT / 'scripts/sweep.sh')], capture_output=True)
            self.assertEqual(result.returncode, 0)
            self.assertEqual(artifact.read_text(), 'preserve')


class WorkflowScope(unittest.TestCase):
    def run_scope(self, root, base, event="pull_request", path=None):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        block = workflow.split("        run: |\n", 1)[1].split("  rust:\n", 1)[0]
        windows = (ROOT / ".github/workflows/windows.yml").read_text()
        windows_block = windows.split("        run: |\n", 1)[1].split("  linux-bin:\n", 1)[0]
        self.assertEqual(block, windows_block, "Both required workflow scope gates must match")
        script = textwrap.dedent(block)
        output = root / "output"
        output.unlink(missing_ok=True)
        env = dict(os.environ, BASE=base, EVENT=event, RUNNER_TEMP=str(root), GITHUB_OUTPUT=str(output))
        if path is not None:
            env["PATH"] = str(path) + os.pathsep + env["PATH"]
        result = subprocess.run(["bash", "-eo", "pipefail", "-c", script], cwd=root, env=env, capture_output=True)
        return result, output.read_text() if output.exists() else ""

    def git(self, root, *args):
        return subprocess.check_output(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", *args], cwd=root).decode().strip()

    def test_rename_classification_uses_actual_workflow(self):
        for source, target, expected in [("scripts/check.sh", "docs/check.md", "true"), ("docs/old.md", "docs/new.md", "false")]:
            with self.subTest(source=source), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.git(root, "init", "-q")
                old = root / source
                old.parent.mkdir(parents=True)
                old.write_text("fixture\n")
                self.git(root, "add", ".")
                self.git(root, "commit", "-qm", "fixture")
                base = self.git(root, "rev-parse", "HEAD")
                new = root / target
                new.parent.mkdir(parents=True, exist_ok=True)
                old.rename(new)
                self.git(root, "add", "-A")
                self.git(root, "commit", "-qm", "rename")
                result, output = self.run_scope(root, base)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, f"code={expected}\n")

    def test_bad_baseline_and_grep_errors_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.git(root, "init", "-q")
            (root / "README.md").write_text("fixture\n")
            self.git(root, "add", ".")
            self.git(root, "commit", "-qm", "fixture")
            base = self.git(root, "rev-parse", "HEAD")
            for invalid in ["", "bad-base", "a" * 40]:
                result, output = self.run_scope(root, invalid)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, "")
            for event, initial in [("workflow_dispatch", ""), ("push", "0" * 40)]:
                result, output = self.run_scope(root, initial, event)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(output, "code=true\n")
            fake_bin = root / "bin"
            fake_bin.mkdir()
            grep = fake_bin / "grep"
            grep.write_text("#!/usr/bin/env bash\nexit 2\n")
            grep.chmod(0o700)
            result, output = self.run_scope(root, base, path=fake_bin)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(output, "")


if __name__ == '__main__':
    unittest.main()
