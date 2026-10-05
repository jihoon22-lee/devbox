"""Fail closed on revalidation wiring that can lose provenance or skip acceptance."""
import pathlib
import unittest
import hashlib
import json
import os
import subprocess
import sys
import tempfile

import yaml


WORKFLOW = pathlib.Path(__file__).resolve().parents[1] / "workflows/windows-candidate-revalidation.yml"


class RevalidationWorkflowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = yaml.safe_load(WORKFLOW.read_text())
        cls.jobs = cls.document["jobs"]

    def commands(self, job):
        return "\n".join(step.get("run", "") for step in self.jobs[job]["steps"])

    def test_provenance_checks_have_history_and_explicit_tag(self):
        for job in ("plan", "seal", "publish"):
            checkout = next(step for step in self.jobs[job]["steps"] if step.get("uses", "").startswith("actions/checkout@"))
            self.assertEqual(checkout["with"]["fetch-depth"], 0, job)
        commands = self.commands("plan")
        calls = [line for line in commands.splitlines() if "prepare-candidate-revalidation.py" in line]
        self.assertEqual(len(calls), 2)
        for call in calls:
            for argument in ('--source "$BUILD_SOURCE"', '--fixture "$GITHUB_SHA"', '--build-run "$BUILD_RUN"', '--run "$GITHUB_RUN_ID"', '--tag "$TAG"'):
                self.assertIn(argument, call)
        self.assertIn("--resolve --github-output", calls[0])
        self.assertIn("--assets candidate/assets --output candidate/evidence/revalidation-proof.json", calls[1])

    def test_original_bytes_and_private_fixtures_use_exact_artifact_ids(self):
        plan_download = next(step for step in self.jobs["plan"]["steps"] if step.get("uses", "").startswith("actions/download-artifact@"))
        self.assertEqual(plan_download["with"]["artifact-ids"], "${{ steps.resolve.outputs.artifact_id }}")
        self.assertEqual(plan_download["with"]["run-id"], "${{ inputs.build_run }}")
        downloads = [step["with"] for step in self.jobs["installed"]["steps"] if step.get("uses", "").startswith("actions/download-artifact@")]
        for output in ("windows_lsp_artifact_id", "wsl_lsp_artifact_id"):
            item = next(item for item in downloads if output in item.get("artifact-ids", ""))
            self.assertEqual(item["run-id"], "${{ inputs.build_run }}")

    def test_acceptance_is_parallel_but_seal_requires_both(self):
        for job in ("installed", "migration"):
            self.assertEqual(self.jobs[job]["needs"], "plan")
            self.assertEqual(self.jobs[job]["runs-on"], "windows-2025")
        self.assertEqual(set(self.jobs["seal"]["needs"]), {"plan", "installed", "migration"})
        self.assertNotIn("if", self.jobs["seal"])
        self.assertIn("seal", self.jobs["publish"]["needs"])
        self.assertNotIn("if", self.jobs["publish"])
        self.assertIn("publish", self.jobs["public-smoke"]["needs"])

    def test_complete_installer_and_full_migration_are_not_scoped_away(self):
        installed = self.commands("installed")
        for script in ("windows-suite-user-flow.mjs", "windows-workspace-user-flows.mjs", "windows-api-user-flows.mjs", "windows-knowledge-user-flows.mjs", "windows-suite-integration.mjs", "windows-suite-layout.mjs", "windows-suite-delivery-user-flows.mjs", "windows-suite-legacy-upgrade-ui.mjs", "windows-suite-agent-user-flows.mjs"):
            self.assertIn(script, installed)
        self.assertIn("if ($failed.Count -gt 0)", installed)
        migration = self.commands("migration")
        self.assertIn("windows-suite-delivery.ps1 -Staging candidate/delivery", migration)
        self.assertNotIn("-RemainingOnly", migration)
        self.assertIn("cleanupFailures", self.commands("seal"))

    def test_seal_and_publication_preserve_source_and_proof(self):
        for job in ("seal", "publish"):
            for line in self.commands(job).splitlines():
                if "build-candidate-metadata.py" in line or "verify-downloaded-release.py" in line and "--artifact-kind candidate" in line:
                    self.assertIn('--commit "$BUILD_SOURCE"', line)
                    self.assertIn('--workflow-run "$GITHUB_RUN_ID"', line)
                    self.assertIn("--revalidation-proof", line)
        publish = self.commands("publish")
        self.assertIn("'object': os.environ['BUILD_SOURCE']", publish)
        self.assertIn("--verify-tag --draft", publish)
        self.assertLess(publish.index("Fresh draft bytes differ"), publish.index("--draft=false --latest"))
        self.assertEqual(self.jobs["public-smoke"]["env"]["DEVBOX_USER_FLOW_ASSETS"], "public-assets")
        self.assertIn("DEVBOX_USER_FLOW_REVALIDATION_PROOF", self.commands("public-smoke"))

    def test_no_build_or_retained_native_rerun_or_source_spoofing(self):
        commands = "\n".join(self.commands(job) for job in self.jobs)
        for forbidden in ("cargo build", "tauri build", "build-windows-packages", "assemble-suite-candidate", "windows-knowledge-lifecycle.mjs", "windows-api-native.mjs", "GITHUB_SHA=", "$env:GITHUB_SHA ="):
            self.assertNotIn(forbidden, commands)
        self.assertEqual(self.document["name"], "Windows candidate revalidation")
        self.assertFalse(self.document["concurrency"]["cancel-in-progress"])
        self.assertEqual(self.jobs["publish"]["permissions"]["contents"], "write")

    def render_notes(self, change=None):
        steps = self.jobs['publish']['steps']
        step = next(item for item in steps if item.get('name') == 'Generate final Korean notes from verified original assets')
        script = step['run'].split("python3 - <<'PYTHON'\n", 1)[1].split('\nPYTHON', 1)[0]
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            assets = root / 'candidate/assets'; assets.mkdir(parents=True)
            evidence = root / 'candidate/evidence'; evidence.mkdir()
            names = ['workspace.zip', 'api.zip', 'knowledge.zip', 'center.zip', 'setup.exe', 'notices.md']
            for name in names: (assets / name).write_bytes(name.encode())
            manifest = {'products': [{'portable': {'name': name}} for name in names[:4]], 'setup': {'name': names[4]}, 'notices': {'name': names[5]}}
            (assets / 'release-manifest.json').write_text(json.dumps(manifest))
            digests = {file.name: hashlib.sha256(file.read_bytes()).hexdigest() for file in assets.iterdir()}
            proof = dict(sourceSha='a'*40,fixtureSha='b'*40,buildRunId=11,revalidationRunId=12,repository='owner/repo',assetDigests=digests)
            (evidence / 'revalidation-proof.json').write_text(json.dumps(proof))
            notes = evidence / 'release-notes.md'
            notes.write_text('v0.8.1은 setup 직접 설치. 철회된 v0.9.0 데이터는 삭제하지 마세요.\n', encoding='utf-8')
            if change: change(assets, notes)
            env = {**os.environ,'BUILD_SOURCE':'a'*40,'BUILD_RUN':'11','GITHUB_SHA':'b'*40,'GITHUB_RUN_ID':'12','GITHUB_REPOSITORY':'owner/repo'}
            result = subprocess.run([sys.executable, '-c', script], cwd=root, env=env, capture_output=True, text=True)
            return result, notes.read_text(encoding='utf-8'), digests

    def test_release_notes_use_seven_actual_files_and_distinct_build_fixture_identity(self):
        result, notes, digests = self.render_notes()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('v0.8.1은 setup 직접 설치', notes)
        self.assertIn('데이터는 삭제하지 마세요', notes)
        rows = [line for line in notes.splitlines() if line.startswith('| `')]
        self.assertEqual(len(rows), 7)
        for name, digest in digests.items():
            self.assertTrue(any(f'`{name}`' in row and digest in row for row in rows))
        self.assertIn('a'*40, notes); self.assertIn('b'*40, notes)
        self.assertIn('/actions/runs/11', notes); self.assertIn('/actions/runs/12', notes)
        self.assertNotIn('PENDING', notes)
        self.assertNotIn('공개본 검사 PASS', notes)
        self.assertIn('공개 후 새로 다운로드', notes)
        publish = self.commands('publish')
        self.assertLess(publish.index('Release notes contain unfinished placeholders'), publish.index('git/tags'))
        self.assertIn('--notes-file candidate/evidence/release-notes.md', publish)

    def test_release_notes_reject_placeholders_missing_and_changed_assets(self):
        for change in (
            lambda assets, notes: notes.write_text('PENDING_CANDIDATE_RESULT'),
            lambda assets, notes: (assets / 'workspace.zip').unlink(),
            lambda assets, notes: (assets / 'workspace.zip').write_bytes(b'changed'),
        ):
            with self.subTest(change=change):
                result, _, _ = self.render_notes(change)
                self.assertNotEqual(result.returncode, 0)


if __name__ == "__main__":
    unittest.main()
