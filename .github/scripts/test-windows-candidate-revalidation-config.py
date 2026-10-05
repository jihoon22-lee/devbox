"""Fail closed on revalidation wiring that can lose provenance or skip acceptance."""
import pathlib
import unittest

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


if __name__ == "__main__":
    unittest.main()
