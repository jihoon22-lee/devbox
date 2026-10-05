#!/usr/bin/env python3
"""Regression tests for authenticated, input-equivalent CI result reuse."""
import importlib.util
import sys
import unittest
import io
import re
import subprocess
from urllib.parse import parse_qs, urlsplit
from contextlib import redirect_stdout
from types import SimpleNamespace
from unittest.mock import patch
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("ci_reuse", Path(__file__).with_name("ci-reuse.py"))
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class ReuseTests(unittest.TestCase):
    def test_only_successful_same_repository_runs_are_trusted(self):
        run = {"status": "completed", "conclusion": "success", "event": "pull_request", "path": ".github/workflows/ci.yml", "repository": {"full_name": "owner/repo"}, "head_repository": {"full_name": "owner/repo"}}
        self.assertTrue(MODULE.trusted_run(run, "owner/repo"))
        for key, value in (("conclusion", "failure"), ("conclusion", "skipped"), ("event", "pull_request_target"), ("status", "in_progress")):
            self.assertFalse(MODULE.trusted_run(dict(run, **{key: value}), "owner/repo"))
        self.assertFalse(MODULE.trusted_run(dict(run, head_repository={"full_name": "fork/repo"}), "owner/repo"))

    def test_skipped_compiler_step_is_not_evidence(self):
        job = {"conclusion": "success", "steps": [{"name": name, "conclusion": "success"} for name in MODULE.REQUIRED["rust-windows"]]}
        self.assertTrue(MODULE.successful_compilers(job, "rust-windows"))
        job["steps"][0]["conclusion"] = "skipped"
        self.assertFalse(MODULE.successful_compilers(job, "rust-windows"))

    def test_scope_coverage_never_expands_scoped_or_skipped_runs(self):
        universe = {"a", "b", "c"}
        self.assertEqual(MODULE.covered_packages("packages", "a,b", universe), {"a", "b"})
        self.assertEqual(MODULE.covered_packages("all", "", universe), universe)
        for scope, packages in (("none", ""), ("packages", ""), ("packages", "unknown")):
            with self.assertRaises(ValueError):
                MODULE.covered_packages(scope, packages, universe)
        self.assertEqual(MODULE.unchanged_coverage({"a", "b"}, {"b"}), {"a"})

    def test_checkout_and_scope_come_from_unambiguous_runner_logs(self):
        sha = "a" * 40
        log = f"2026-10-05T01:00:00Z [command]git log -1 --format=%H\n2026-10-05T01:00:01Z {sha}\n2026-10-05T01:00:02Z   RUST_SCOPE: packages\n2026-10-05T01:00:03Z   RUST_PACKAGES: a,b\n"
        self.assertEqual(MODULE.log_evidence(log, "rust"), (sha, "packages", "a,b"))
        with self.assertRaises(ValueError):
            MODULE.log_evidence(log + "2026-10-05T01:00:04Z   RUST_SCOPE: all\n", "rust")
        with self.assertRaises(ValueError):
            MODULE.log_evidence(log.replace("log -1 --format=%H", "echo"), "rust")

    def test_workflow_orchestration_changes_do_not_hide_command_changes(self):
        text = (Path(__file__).parents[1] / "workflows/ci.yml").read_text()
        signature = MODULE.compiler_contract(text, "rust")
        self.assertEqual(signature, MODULE.compiler_contract(text.replace("cancel-in-progress: true", "cancel-in-progress: false"), "rust"))
        self.assertNotEqual(signature, MODULE.compiler_contract(text.replace("rust-toolchain@1.98.1", "rust-toolchain@1.98.2"), "rust"))
        self.assertNotEqual(signature, MODULE.compiler_contract(text.replace('run-rust-scope.sh check', 'run-rust-scope.sh test'), "rust"))
        self.assertNotEqual(signature, MODULE.compiler_contract(text.replace('CARGO_BUILD_JOBS: 2', 'CARGO_BUILD_JOBS: 3'), "rust-windows"))

    def test_source_locks_and_compile_drivers_are_inputs(self):
        for path in ("Cargo.lock", "Cargo.toml", "pnpm-lock.yaml", ".cargo/config.toml", "apps/devbox-workspace/src-tauri/src/main.rs", ".github/fixtures/workspace-lsp-server.mjs", ".github/scripts/run-rust-scope.sh"):
            self.assertTrue(MODULE.compiler_input(path, "rust"), path)
        self.assertFalse(MODULE.compiler_input(".github/scripts/windows-suite-receiver-access.ps1", "rust"))
        self.assertFalse(MODULE.compiler_input("docs/release-policy.md", "rust"))
        self.assertTrue(MODULE.compiler_input("packages/editor/src/index.ts", "frontend"))
        self.assertFalse(MODULE.compiler_input("apps/devbox-workspace/src-tauri/src/main.rs", "frontend"))

    def test_identical_inputs_and_affected_closure_coverage(self):
        universe = {"library", "consumer", "unrelated"}
        resolver = SimpleNamespace(resolve_paths=lambda paths: SimpleNamespace(rust_scope="packages", rust_packages=["library", "consumer"]))
        self.assertEqual(MODULE.affected_packages([], "rust", universe, resolver), set())
        affected = MODULE.affected_packages(["crates/library/src/lib.rs"], "rust", universe, resolver)
        self.assertEqual(MODULE.unchanged_coverage(universe, affected), {"unrelated"})
        for changed in ("Cargo.lock", "crates/library/Cargo.toml", ".github/scripts/run-rust-scope.sh", ".github/scripts/product-route-graph.mjs"):
            self.assertEqual(MODULE.affected_packages([changed], "rust", universe, resolver), universe)
        self.assertEqual(MODULE.affected_packages([".github/scripts/windows-suite-receiver-access.ps1"], "rust", universe, resolver), set())

    def test_schedule_never_queries_or_reuses_prior_results(self):
        with patch.dict(MODULE.os.environ, {"GITHUB_EVENT_NAME": "schedule"}), patch.object(MODULE, "api") as api:
            results, _ = MODULE.resolve("owner/repo")
        self.assertFalse(any(results.values()))
        api.assert_not_called()

    def test_api_error_stops_before_expensive_checks(self):
        with patch.dict(MODULE.os.environ, {"GITHUB_REPOSITORY": "owner/repo"}, clear=True), patch.object(MODULE, "resolve", side_effect=ValueError("API unavailable")), redirect_stdout(io.StringIO()) as output:
            self.assertEqual(MODULE.main(), 2)
        self.assertIn("Compiler verification stopped", output.getvalue())
        self.assertIn("API unavailable", output.getvalue())
        for name in ("frontend", "rust", "rust_windows"):
            self.assertIn(name + "_reuse=false", output.getvalue())

    def test_prior_checkout_must_match_run_or_its_actual_merge_parent(self):
        run = {"head_sha": "head", "event": "pull_request"}
        with patch.object(MODULE, "git", return_value="base head"):
            self.assertTrue(MODULE.checkout_matches_run("merge", run))
        with patch.object(MODULE, "git", return_value="base unrelated"):
            self.assertFalse(MODULE.checkout_matches_run("merge", run))
        self.assertFalse(MODULE.checkout_matches_run("head", run))
        self.assertTrue(MODULE.checkout_matches_run("head", dict(run, event="workflow_dispatch")))

    def test_workflow_gates_retain_fresh_policy_and_read_only_evidence_permissions(self):
        import yaml
        workflow = yaml.safe_load((Path(__file__).parents[1] / "workflows/ci.yml").read_text())
        self.assertEqual(workflow["jobs"]["scope"]["permissions"], {"actions": "read", "contents": "read"})
        self.assertNotIn("reuse", str(workflow["jobs"]["dependencies"]))
        for gate in MODULE.GATES:
            job = workflow["jobs"][gate]
            self.assertEqual(job["name"], MODULE.GATES[gate])
            for step in job["steps"]:
                if step.get("name") in MODULE.REQUIRED[gate]:
                    self.assertIn(gate.replace("-", "_") + "_reuse != 'true'", step["if"])
                if gate == "frontend" and step.get("name") in MODULE.FRESH_FRONTEND:
                    self.assertNotIn("reuse", step.get("if", ""))

    def test_compiler_driver_registry_covers_direct_and_transitive_imports(self):
        # Changes adding a new build/test script dependency must extend the
        # invalidation boundary before CI may reuse that compiler's results.
        root = Path(__file__).resolve().parents[2]
        files = subprocess.check_output(["git", "ls-files", "apps", "packages", "crates"], cwd=root, text=True).splitlines()
        referenced = set()
        for name in files:
            if Path(name).suffix not in {".rs", ".ts", ".tsx", ".mjs", ".js", ".json", ".toml", ".sh"}:
                continue
            text = (root / name).read_text(errors="replace")
            referenced.update(re.findall(r"\.github/scripts/[a-zA-Z0-9_.-]+", text))
        self.assertLessEqual(referenced, MODULE.DRIVERS)
        for name in MODULE.DRIVERS:
            if not name.endswith(".mjs"):
                continue
            text = (root / name).read_text()
            imports = re.findall(r"(?:from\s+|import\s*\()['\"](\./[^'\"]+)['\"]", text)
            self.assertLessEqual({str(Path(name).parent / value) for value in imports}, MODULE.DRIVERS, name)

    def test_lookup_diagnostics_redact_credentials_and_url_queries(self):
        with patch.dict(MODULE.os.environ, {"GH_TOKEN": "synthetic-auth-secret"}):
            message = MODULE.safe_diagnostic("fatal synthetic-auth-secret HTTP 403 https://example.test/path?sig=confidential\nnext")
        self.assertNotIn("synthetic-auth-secret", message)
        self.assertNotIn("confidential", message)
        self.assertIn("HTTP 403", message)

    def test_command_failure_exposes_stage_and_safe_stderr(self):
        failure = SimpleNamespace(returncode=1, stderr="gh: Resource not accessible by integration (HTTP 403)", stdout="")
        with patch.object(MODULE.subprocess, "run", return_value=failure):
            with self.assertRaisesRegex(ValueError, "HTTP 403"):
                MODULE.command("gh", "api", "repos/owner/repo/actions/jobs/123/logs")
        self.assertIn("actions/jobs/123/logs", MODULE.LOOKUP_STAGE)

    def test_malformed_historical_receipt_does_not_discard_later_valid_receipt(self):
        rejected = []
        self.assertIsNone(MODULE.historical_receipt("malformed", "rust", 12, rejected))
        self.assertEqual(len(rejected), 1)
        sha = "b" * 40
        valid = f"2026-10-05T00:00:00Z [command]git log -1 --format=%H\n2026-10-05T00:00:01Z {sha}\n2026-10-05T00:00:02Z   RUST_SCOPE: all\n2026-10-05T00:00:03Z   RUST_PACKAGES: \n"
        self.assertEqual(MODULE.historical_receipt(valid, "rust", 13, rejected), (sha, "all", ""))

    def test_discovery_uses_verified_numeric_identity_and_explicit_first_page(self):
        recent = MODULE.datetime.now(MODULE.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        record = {"id": 456, "created_at": recent}
        replies = [{"id": 123, "path": ".github/workflows/ci.yml", "state": "active"}, {"workflow_runs": [record]}]
        with patch.object(MODULE, "api", side_effect=replies) as api:
            self.assertEqual(MODULE.prior_runs("owner/repo"), (123, [record]))
        endpoint = api.call_args_list[1].args[0]
        self.assertTrue(endpoint.startswith("repos/owner/repo/actions/workflows/123/runs?"))
        query = parse_qs(urlsplit(endpoint).query)
        self.assertEqual(query["page"], ["1"])
        self.assertEqual(query["per_page"], ["30"])
        self.assertTrue(query["created"][0].startswith(">="))
        with patch.object(MODULE, "api", return_value={"id": 123, "path": ".github/workflows/other.yml", "state": "active"}):
            with self.assertRaises(MODULE.LookupFailure):
                MODULE.prior_runs("owner/repo")

    def test_old_malformed_receipt_does_not_block_changed_package_checks(self):
        sha = "b" * 40
        log = f"2026-10-05T00:00:00Z [command]git log -1 --format=%H\n2026-10-05T00:00:01Z {sha}\n2026-10-05T00:00:02Z   RUST_SCOPE: all\n2026-10-05T00:00:03Z   RUST_PACKAGES: \n"
        run = {"workflow_id": 123, "status": "completed", "conclusion": "success", "event": "workflow_dispatch", "path": MODULE.WORKFLOW, "repository": {"full_name": "owner/repo"}, "head_repository": {"full_name": "owner/repo"}, "head_sha": sha}
        runs = [dict(run, id=1), dict(run, id=2)]
        def api(endpoint, **kwargs):
            if "/runs/" in endpoint:
                number = 1 if "/runs/1/" in endpoint else 2
                return {"jobs": [{"id": number, "name": MODULE.GATES["rust-windows"], "conclusion": "success", "steps": [{"name": name, "conclusion": "success"} for name in MODULE.REQUIRED["rust-windows"]]}]}
            return "malformed old log" if "/jobs/1/" in endpoint else log
        workflow = (MODULE.ROOT / MODULE.WORKFLOW).read_text()
        def git(*args):
            if args[0] == "diff":
                return "crates/changed/src/lib.rs"
            return workflow if args[0] == "show" else "a" * 40
        scope = SimpleNamespace(
            load_frontend_graph=lambda: SimpleNamespace(nodes={}),
            load_rust_graph=lambda: SimpleNamespace(nodes={"changed": object()}),
            resolve_paths=lambda paths: SimpleNamespace(rust_scope="packages", rust_packages=["changed"]),
        )
        with patch.dict(MODULE.os.environ, {"GITHUB_EVENT_NAME": "pull_request", "FRONTEND_SCOPE": "none", "RUST_SCOPE": "all"}, clear=True), patch.object(MODULE, "load_scope", return_value=scope), patch.object(MODULE, "prior_runs", return_value=(123, runs)), patch.object(MODULE, "api", side_effect=api), patch.object(MODULE, "git", side_effect=git), patch.object(MODULE, "ensure_commit"):
            results, evidence = MODULE.resolve("owner/repo")
        self.assertFalse(results["rust-windows"])
        self.assertTrue(any("Historical receipt not used" in line for line in evidence))
        self.assertTrue(any("Rust (Windows): normal execution (1 package(s)" in line for line in evidence))

    def test_new_cli_raw_logs_require_detected_escape_flag_and_never_print_payload(self):
        MODULE.api_raw_flags.cache_clear()
        payload = "raw secret log with \x1b[31mcolor\x1b[0m"
        def cli(args, **kwargs):
            if args == ("gh", "api", "--help"):
                return SimpleNamespace(returncode=0, stderr="", stdout="--allow-escape-sequences")
            allowed = "--allow-escape-sequences" in args
            return SimpleNamespace(returncode=0 if allowed else 1, stderr="response contains terminal escape sequences", stdout=payload if allowed else "")
        with patch.object(MODULE.subprocess, "run", side_effect=cli) as cli_mock, redirect_stdout(io.StringIO()) as output:
            self.assertEqual(MODULE.api("repos/owner/repo/actions/jobs/1/logs", raw=True), payload)
        self.assertNotIn("raw secret log", output.getvalue())
        self.assertIn("--allow-escape-sequences", cli_mock.call_args.args[0])
        MODULE.api_raw_flags.cache_clear()

    def test_old_cli_does_not_receive_unsupported_escape_flag(self):
        MODULE.api_raw_flags.cache_clear()
        with patch.object(MODULE, "command", side_effect=["older help", "raw log"]) as command:
            self.assertEqual(MODULE.api("repos/owner/repo/actions/jobs/1/logs", raw=True), "raw log")
        self.assertEqual(command.call_args.args, ("gh", "api", "repos/owner/repo/actions/jobs/1/logs"))
        MODULE.api_raw_flags.cache_clear()

    def test_colored_checkout_logs_parse_after_terminal_controls_removed(self):
        sha = "a" * 40
        log = f"2026-10-05T00:00:00Z \x1b[36m[command]git log -1 --format=%H\x1b[0m\n2026-10-05T00:00:01Z \x1b[32m{sha}\x1b[0m\n2026-10-05T00:00:02Z   RUST_SCOPE: all\n2026-10-05T00:00:03Z   RUST_PACKAGES: \n"
        self.assertEqual(MODULE.log_evidence(log, "rust"), (sha, "all", ""))

    def test_discovery_rejects_server_response_violating_recent_time_filter(self):
        replies = [{"id": 123, "path": MODULE.WORKFLOW, "state": "active"}, {"workflow_runs": [{"id": 456, "created_at": "2020-01-01T00:00:00Z"}]}]
        with patch.object(MODULE, "api", side_effect=replies):
            with self.assertRaisesRegex(MODULE.LookupFailure, "recent"):
                MODULE.prior_runs("owner/repo")


if __name__ == "__main__":
    unittest.main()
