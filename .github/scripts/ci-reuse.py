#!/usr/bin/env python3
"""Reuse only actual successful CI compiler work with unchanged Git inputs.

GitHub job metadata and authenticated logs are the receipts, including for runs
that predate this resolver. A successful job whose compiler steps were skipped
is never evidence. Missing coverage runs checks; lookup faults stop the gate.
"""
from __future__ import annotations

import copy
import importlib.util
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ".github/workflows/ci.yml"
GATES = {"frontend": "Frontend (pnpm)", "rust": "Rust (Cargo workspace)", "rust-windows": "Rust (Windows)"}
REQUIRED = {
    "frontend": {"Build frontends", "Check frontend bundle budgets", "Test frontends", "Check TypeScript packages not covered by build"},
    "rust": {"Check", "Clippy", "Format", "Test", "Check generated TypeScript bindings"},
    "rust-windows": {"Check", "Clippy", "Test"},
}
FRESH_FRONTEND = {"Check format and hook rules", "Check component size limits", "Check frontend input and fixture contracts"}
DRIVERS = {
    ".github/scripts/ci-scope.sh", ".github/scripts/resolve-ci-scope.py",
    ".github/scripts/run-rust-scope.sh", ".github/scripts/run-frontend-scope.sh",
    ".github/scripts/check-generated-bindings.sh", ".github/scripts/check-frontend-bundles.mjs",
    ".github/scripts/frontend-bundle-budgets.json", ".github/scripts/check-api-studio-routes.mjs",
    ".github/scripts/check-knowledge-routes.mjs", ".github/scripts/workspace-wsl-artifact.mjs",
    ".github/scripts/windows-packaged-smoke.mjs",
    ".github/scripts/windows-process-identity.mjs",
    ".github/scripts/product-foundation-performance.mjs",
    ".github/scripts/product-route-graph.mjs",
}
MAX_RUNS = 30
MAX_LOG_BYTES = 32 * 1024 * 1024
LOOKUP_STAGE = "initialization"


class LookupFailure(ValueError):
    """Operational failure; never silently replace it with compiler work."""


def safe_diagnostic(value: str) -> str:
    for key, secret in os.environ.items():
        if len(secret) >= 6 and re.search(r"TOKEN|SECRET|PASSWORD|PRIVATE_KEY|API_KEY", key, re.I):
            value = value.replace(secret, "[REDACTED]")
    value = re.sub(r"(https?://[^\s?]+)\?[^\s]+", r"\1?[REDACTED]", value)
    value = re.sub(r"(?i)(bearer|authorization:)\s+\S+", r"\1 [REDACTED]", value)
    return " ".join(value.split())[:800]


def command(*args: str) -> str:
    global LOOKUP_STAGE
    LOOKUP_STAGE = safe_diagnostic(" ".join(args))
    print(f"Evidence lookup: {LOOKUP_STAGE}", flush=True)
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True, timeout=55, check=False)
    if result.returncode:
        raise LookupFailure(f"command exit {result.returncode}: {safe_diagnostic(result.stderr)}")
    return result.stdout


def git(*args: str) -> str:
    return command("git", *args).strip()


def api(path: str, *, raw: bool = False):
    value = command("gh", "api", path)
    if len(value.encode()) > MAX_LOG_BYTES:
        raise ValueError("verification evidence exceeds size limit")
    return value if raw else json.loads(value)


def trusted_run(run: dict, repository: str) -> bool:
    return (
        run.get("status") == "completed" and run.get("conclusion") == "success"
        and run.get("event") in {"pull_request", "workflow_dispatch", "schedule", "push"}
        and run.get("path") == WORKFLOW
        and run.get("repository", {}).get("full_name") == repository
        and run.get("head_repository", {}).get("full_name") == repository
    )


def successful_compilers(job: dict, gate: str) -> bool:
    steps = {step.get("name"): step.get("conclusion") for step in job.get("steps", [])}
    return job.get("conclusion") == "success" and all(steps.get(name) == "success" for name in REQUIRED[gate])


def covered_packages(scope: str, packages: str, universe: set[str]) -> set[str]:
    if scope == "all":
        return set(universe)
    selected = set(packages.split(","))
    if scope not in {"packages", "apps"} or not selected or "" in selected or not selected <= universe:
        raise ValueError("prior compiler scope is absent, skipped, or unknown")
    return selected


def unchanged_coverage(covered: set[str], affected: set[str]) -> set[str]:
    return covered - affected


def historical_receipt(log: str, family: str, job_id: int, rejected: list[str]):
    try:
        return log_evidence(log, family)
    except ValueError as error:
        rejected.append(f"job {job_id}: {safe_diagnostic(str(error))}")
        print(f"Historical receipt rejected: {rejected[-1]}", flush=True)
        return None


def log_evidence(log: str, family: str) -> tuple[str, str, str]:
    # Read runner-emitted checkout and environment records; ambiguous values are
    # rejected. Do not use run.head_sha: PR checkout normally builds a merge SHA.
    lines = [re.sub(r"^\d{4}-\d\d-\d\dT\S+\s", "", line).rstrip("\r") for line in log.splitlines()]
    commits = {lines[index + 1].strip() for index, line in enumerate(lines[:-1]) if "[command]" in line and line.endswith("log -1 --format=%H")}
    prefix = family.upper()
    scopes = {line.removeprefix(f"  {prefix}_SCOPE: ") for line in lines if line.startswith(f"  {prefix}_SCOPE: ")}
    packages = {line.removeprefix(f"  {prefix}_PACKAGES:").strip() for line in lines if line.startswith(f"  {prefix}_PACKAGES:")}
    if len(commits) != 1 or not re.fullmatch(r"[a-f0-9]{40}", next(iter(commits), "")) or len(scopes) != 1 or len(packages) != 1:
        raise ValueError("checkout or compiler scope evidence is ambiguous")
    return next(iter(commits)), next(iter(scopes)), next(iter(packages))


def compiler_contract(text: str, gate: str) -> str:
    # The workflow supplies a pinned PyYAML parser when absent. If unavailable
    # outside CI, fall back to checks rather than a permissive YAML parser.
    import yaml
    workflow = yaml.safe_load(text)
    job = copy.deepcopy(workflow["jobs"][gate])
    token = gate.replace("-", "_")
    reuse_condition = f"needs.scope.outputs.{token}_reuse != 'true'"
    kept = []
    for step in job["steps"]:
        name = step.get("name", "")
        if name in {"Require successful scope detection", "Report reused compiler evidence"} or (gate == "frontend" and name in FRESH_FRONTEND):
            continue
        if "if" in step:
            condition = step["if"]
            condition = condition.replace(f" && {reuse_condition}", "")
            if condition == "${{ " + reuse_condition + " }}":
                step.pop("if")
            else:
                step["if"] = condition
        kept.append(step)
    # Job scheduling and the new proof-reporting step do not alter compilation.
    # Everything inside actual compiler/setup steps remains significant, as do
    # runner, environment, defaults, containers, services and matrix settings.
    for key in ("name", "needs", "if", "timeout-minutes", "outputs"):
        job.pop(key, None)
    job["steps"] = kept
    return json.dumps({"job": job, "env": workflow.get("env"), "defaults": workflow.get("defaults")}, sort_keys=True)


def compiler_input(path: str, gate: str) -> bool:
    if path == WORKFLOW or path.startswith("docs/") or (path.endswith(".md") and path != "THIRD_PARTY_NOTICES.md"):
        return False
    if path.startswith(".github/scripts/"):
        return path in DRIVERS
    if path.startswith(".github/workflows/"):
        return False
    if path.startswith(".github/"):
        return True
    if path in {"AGENTS.md", ".gitignore", ".git-blame-ignore-revs", "LICENSE"}:
        return False
    if gate == "frontend":
        if path.startswith("crates/") or "/src-tauri/" in path or path.startswith("apps/devbox-agent/"):
            return False
    elif path.startswith("packages/"):
        return "/generated/" in path or path.startswith("packages/product-shell/fixtures/") or path.endswith("package.json")
    elif path.startswith("apps/") and len(Path(path).parts) > 2 and "/src-tauri/" not in path and not path.startswith("apps/devbox-agent/"):
        return path.endswith("package.json")
    return True


def load_scope():
    spec = importlib.util.spec_from_file_location("ci_reuse_scope", ROOT / ".github/scripts/resolve-ci-scope.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def global_input(path: str) -> bool:
    # Locks/manifests may change resolution or feature unification beyond a
    # declared reverse dependency. Never reuse across such changes.
    return (
        path in DRIVERS or path.startswith(".github/")
        or path.endswith(("Cargo.toml", "package.json", "lock.yaml", "Cargo.lock"))
        or "/" not in path or path.startswith(".cargo/")
    )


def affected_packages(paths: list[str], gate: str, universe: set[str], scope_module) -> set[str]:
    relevant = [path for path in paths if compiler_input(path, gate)]
    if not relevant:
        return set()
    if any(global_input(path) for path in relevant):
        return set(universe)
    result = scope_module.resolve_paths(relevant)
    family = "frontend" if gate == "frontend" else "rust"
    if getattr(result, f"{family}_scope") == "all":
        return set(universe)
    return set(getattr(result, f"{family}_packages"))


def ensure_commit(sha: str) -> None:
    if not re.fullmatch(r"[a-f0-9]{40}", sha):
        raise ValueError("invalid evidence commit")
    try:
        git("cat-file", "-e", f"{sha}^{{commit}}")
    except ValueError:
        git("fetch", "--no-tags", "origin", sha)


def checkout_matches_run(sha: str, run: dict) -> bool:
    head = run.get("head_sha", "")
    if sha == head:
        return run.get("event") != "pull_request"
    parents = git("show", "-s", "--format=%P", sha).split()
    return run.get("event") == "pull_request" and len(parents) == 2 and parents[1] == head


def prior_runs(repository: str):
    workflow_info = api(f"repos/{repository}/actions/workflows/ci.yml")
    workflow_id = workflow_info.get("id")
    if type(workflow_id) is not int or workflow_info.get("path") != WORKFLOW or workflow_info.get("state") != "active":
        raise LookupFailure("CI workflow identity could not be verified")
    # Resolve the stable numeric identity first. The filename alias has returned
    # stale historical pages even while the numeric endpoint returns latest runs.
    runs = api(f"repos/{repository}/actions/workflows/{workflow_id}/runs?status=success&per_page={MAX_RUNS}&page=1")["workflow_runs"]
    return workflow_id, runs


def resolve(repository: str) -> tuple[dict[str, bool], list[str]]:
    results = {gate: False for gate in GATES}
    evidence = []
    rejected: list[str] = []
    if os.environ.get("GITHUB_EVENT_NAME") == "schedule":
        return results, ["Weekly scheduled audit: all selected compiler checks execute."]
    scope_module = load_scope()
    frontend = scope_module.load_frontend_graph()
    rust = scope_module.load_rust_graph()
    universes = {"frontend": {node.directory for node in frontend.nodes.values()}, "rust": set(rust.nodes), "rust-windows": set(rust.nodes)}
    requested = {}
    for gate in GATES:
        family = "FRONTEND" if gate == "frontend" else "RUST"
        scope = os.environ.get(f"{family}_SCOPE", "all")
        requested[gate] = set() if scope == "none" else covered_packages(scope, os.environ.get(f"{family}_PACKAGES", ""), universes[gate])
    outstanding = copy.deepcopy(requested)
    # Rust fmt and generated-binding consistency are global even in scoped
    # jobs. Require a real successful Linux job with all Rust inputs unchanged.
    linux_global = False
    current = git("rev-parse", "HEAD")
    workflow = (ROOT / WORKFLOW).read_text()
    signatures = {gate: compiler_contract(workflow, gate) for gate in GATES}
    workflow_id, runs = prior_runs(repository)
    for run in runs:
        if run.get("workflow_id") != workflow_id or not trusted_run(run, repository) or str(run.get("id")) == os.environ.get("GITHUB_RUN_ID"):
            continue
        jobs = api(f"repos/{repository}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")["jobs"]
        for gate, name in GATES.items():
            if not outstanding[gate] and (gate != "rust" or linux_global):
                continue
            matches = [job for job in jobs if job.get("name") == name]
            if len(matches) != 1 or not successful_compilers(matches[0], gate):
                continue
            job = matches[0]
            family = "frontend" if gate == "frontend" else "rust"
            log = api(f"repos/{repository}/actions/jobs/{job['id']}/logs", raw=True)
            receipt = historical_receipt(log, family, job["id"], rejected)
            if receipt is None:
                continue
            sha, prior_scope, prior_packages = receipt
            ensure_commit(sha)
            if not checkout_matches_run(sha, run):
                continue
            prior_workflow = git("show", f"{sha}:{WORKFLOW}")
            try:
                contract = compiler_contract(prior_workflow, gate)
                covered = covered_packages(prior_scope, prior_packages, universes[gate])
            except (ValueError, KeyError, TypeError) as error:
                rejected.append(f"job {job['id']}: prior contract/scope invalid ({type(error).__name__})")
                continue
            if contract != signatures[gate]:
                continue
            paths = git("diff", "--name-only", sha, current, "--").splitlines()
            # Reconstructing coverage with a modified resolver would be unsafe.
            if any(path in {".github/scripts/resolve-ci-scope.py", ".github/scripts/ci-scope.sh"} for path in paths):
                continue
            affected = affected_packages(paths, gate, universes[gate], scope_module)
            reusable = unchanged_coverage(covered, affected) & outstanding[gate]
            if gate == "rust" and not any(compiler_input(path, gate) for path in paths):
                linux_global = True
            if reusable:
                outstanding[gate] -= reusable
                evidence.append(f"{name}: {len(reusable)} unchanged package(s), actual compiler success [run {run['id']} / job {job['id']}](https://github.com/{repository}/actions/runs/{run['id']}/job/{job['id']}), checkout `{sha}`; coverage `{','.join(sorted(reusable))}`.")
        if all(not value for value in outstanding.values()) and (not requested["rust"] or linux_global):
            break
    if rejected and (any(outstanding.values()) or (requested["rust"] and not linux_global)):
        raise LookupFailure("Valid evidence remains incomplete after rejecting historical receipts: " + "; ".join(rejected[:3]))
    evidence.extend(f"Historical receipt not used: {reason}." for reason in rejected)
    for gate in GATES:
        results[gate] = bool(requested[gate]) and not outstanding[gate] and (gate != "rust" or linux_global)
        if not results[gate]:
            evidence.append(f"{GATES[gate]}: normal execution ({len(outstanding[gate])} package(s) lack reusable evidence" + (", global Rust evidence required" if gate == "rust" and not linux_global else "") + ").")
    return results, evidence


def main() -> int:
    results = {gate: False for gate in GATES}
    exit_code = 0
    try:
        repository = os.environ["GITHUB_REPOSITORY"]
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
            raise ValueError("invalid repository")
        results, evidence = resolve(repository)
    except Exception as error:
        # An operational lookup failure is not evidence of changed inputs.
        # Stop the scope gate before expensive jobs can start; do not silently
        # spend another complete build attempting to hide a resolver fault.
        exit_code = 2
        evidence = [f"Compiler verification stopped at `{safe_diagnostic(LOOKUP_STAGE)}`: {type(error).__name__}: {safe_diagnostic(str(error))}. No compiler rerun authorized by this lookup failure."]
    output = "".join(f"{gate.replace('-', '_')}_reuse={str(value).lower()}\n" for gate, value in results.items())
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
            stream.write(output)
    else:
        print(output, end="")
    summary = "### Compiler verification evidence\n\n" + "\n".join(f"- {line}" for line in evidence) + "\n"
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as stream:
            stream.write(summary)
    print(summary)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
