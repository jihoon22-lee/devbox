#!/usr/bin/env python3
"""Resolve affected pnpm and Cargo workspace members for local and CI checks."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from collections.abc import Iterable
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
FRONTEND_FIELDS = ("dependencies", "devDependencies", "peerDependencies", "optionalDependencies")
SCOPE_DRIVER_PATHS = {
    ".github/scripts/ci-scope.sh",
    ".github/scripts/resolve-ci-scope.py",
    ".github/scripts/run-frontend-scope.sh",
    ".github/scripts/run-rust-scope.sh",
    ".github/scripts/test-ci-scope.py",
    ".github/scripts/test-ci-scope-runners.py",
    ".github/scripts/verify-affected.sh",
    ".github/workflows/ci.yml",
}
FRONTEND_DRIVER_PATHS = {
    ".github/scripts/check-frontend-bundles.mjs",
    ".github/scripts/frontend-bundle-budgets.json",
    ".github/scripts/test-check-frontend-bundles.mjs",
}
# catalog.json is imported directly rather than through a package manifest, so
# these virtual build edges complement the dependency graph. The regression
# test deliberately locks the current consumers to this set.
CATALOG_FRONTEND_CONSUMERS = {
    "devbox-launcher",
    "devbox-manager",
    "everything-plus",
    "repo-manager",
}
CATALOG_RUST_CONSUMERS = {
    "catalog",
    "devbox-launcher",
    "devbox-manager",
    "launch",
    "log-lens",
}


class ScopeError(RuntimeError):
    """Raised when the workspace or Git diff cannot be resolved safely."""


@dataclass(frozen=True)
class WorkspaceNode:
    name: str
    directory: str
    kind: str


@dataclass
class WorkspaceGraph:
    nodes: dict[str, WorkspaceNode]
    by_directory: dict[str, str]
    reverse: dict[str, set[str]]

    def closure(self, seeds: Iterable[str]) -> set[str]:
        affected = set(seeds)
        pending = list(affected)
        while pending:
            dependency = pending.pop()
            for consumer in self.reverse.get(dependency, set()):
                if consumer not in affected:
                    affected.add(consumer)
                    pending.append(consumer)
        return affected


@dataclass
class ScopeResult:
    frontend_scope: str
    frontend_packages: list[str]
    frontend_apps: list[str]
    rust_scope: str
    rust_packages: list[str]
    dependency_scope: str
    changed_count: int
    reasons: list[str] = field(default_factory=list)

    def outputs(self) -> dict[str, str]:
        unique_reasons = list(dict.fromkeys(self.reasons))
        scope_reason = "; ".join(unique_reasons)
        if len(scope_reason) > 2048:
            scope_reason = f"{scope_reason[:2045]}..."
        return {
            "frontend_scope": self.frontend_scope,
            "frontend_packages": ",".join(self.frontend_packages),
            "frontend_apps": ",".join(self.frontend_apps),
            "rust_scope": self.rust_scope,
            "rust_packages": ",".join(self.rust_packages),
            "dependency_scope": self.dependency_scope,
            "changed_count": str(self.changed_count),
            "scope_reason": scope_reason,
        }


def _read_json(path: Path, root: Path) -> dict:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ScopeError(f"cannot read workspace manifest {path.relative_to(root)}") from error
    if not isinstance(value, dict):
        raise ScopeError(f"workspace manifest is not an object: {path.relative_to(root)}")
    return value


def load_frontend_graph(root: Path = ROOT) -> WorkspaceGraph:
    manifests = sorted(root.glob("apps/*/package.json")) + sorted(root.glob("packages/*/package.json"))
    nodes: dict[str, WorkspaceNode] = {}
    manifest_data: dict[str, dict] = {}
    by_directory: dict[str, str] = {}

    for manifest in manifests:
        data = _read_json(manifest, root)
        name = data.get("name")
        if not isinstance(name, str) or not name or name in nodes:
            raise ScopeError(f"invalid or duplicate frontend package name in {manifest.relative_to(root)}")
        directory = manifest.parent.relative_to(root).as_posix()
        kind = "app" if directory.startswith("apps/") else "package"
        nodes[name] = WorkspaceNode(name=name, directory=directory, kind=kind)
        manifest_data[name] = data
        by_directory[directory] = name

    reverse = {name: set() for name in nodes}
    for consumer, data in manifest_data.items():
        for field_name in FRONTEND_FIELDS:
            dependencies = data.get(field_name, {})
            if not isinstance(dependencies, dict):
                raise ScopeError(f"{field_name} must be an object in {nodes[consumer].directory}/package.json")
            for dependency in dependencies:
                if dependency in nodes:
                    reverse[dependency].add(consumer)

    return WorkspaceGraph(nodes=nodes, by_directory=by_directory, reverse=reverse)


def load_rust_graph(root: Path = ROOT) -> WorkspaceGraph:
    metadata = subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--offline",
            "--no-deps",
        ],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if metadata.returncode != 0:
        raise ScopeError(metadata.stderr.strip() or "cargo metadata failed")
    try:
        document = json.loads(metadata.stdout)
        packages = document["packages"]
        workspace_members = set(document["workspace_members"])
    except (json.JSONDecodeError, KeyError, TypeError) as error:
        raise ScopeError("cargo metadata returned an invalid workspace graph") from error

    nodes: dict[str, WorkspaceNode] = {}
    package_data: dict[str, dict] = {}
    by_directory: dict[str, str] = {}
    resolved_directories: dict[Path, str] = {}
    for data in packages:
        if not isinstance(data, dict) or data.get("id") not in workspace_members:
            continue
        name = data.get("name")
        manifest_value = data.get("manifest_path")
        if not isinstance(manifest_value, str):
            raise ScopeError("cargo metadata package is missing its manifest path")
        manifest = Path(manifest_value)
        if not isinstance(name, str) or not name or name in nodes:
            raise ScopeError(f"invalid or duplicate Cargo package name in {manifest.relative_to(root)}")
        directory = manifest.parent.relative_to(root).as_posix()
        kind = "app" if directory.startswith("apps/") else "crate"
        nodes[name] = WorkspaceNode(name=name, directory=directory, kind=kind)
        package_data[name] = data
        by_directory[directory] = name
        resolved_directories[manifest.parent.resolve()] = name

    reverse = {name: set() for name in nodes}
    for consumer, data in package_data.items():
        dependencies = data.get("dependencies", [])
        if not isinstance(dependencies, list):
            raise ScopeError(f"cargo metadata dependencies are invalid for {consumer}")
        for declaration in dependencies:
            if not isinstance(declaration, dict):
                continue
            dependency_path = declaration.get("path")
            if not isinstance(dependency_path, str):
                continue
            dependency = resolved_directories.get(Path(dependency_path).resolve())
            if dependency is not None:
                reverse[dependency].add(consumer)

    return WorkspaceGraph(nodes=nodes, by_directory=by_directory, reverse=reverse)


def _is_documentation(path: str) -> bool:
    name = PurePosixPath(path).name
    return (
        path.startswith(("docs/", "workthrough/"))
        or name.endswith(".md")
        or name in {"README", "LICENSE"}
        or path == ".gitignore"
    )


def _normalized_paths(paths: Iterable[str]) -> list[str]:
    normalized = set()
    for raw_path in paths:
        if raw_path != raw_path.strip():
            raise ScopeError("changed path has leading or trailing whitespace")
        if "\\" in raw_path:
            raise ScopeError("changed path contains an unsupported path separator")
        path = raw_path
        while path.startswith("./"):
            path = path[2:]
        if not path:
            continue
        if any(ord(character) < 32 or ord(character) == 127 for character in path):
            raise ScopeError("changed path contains a control character")
        portable = PurePosixPath(path)
        if portable.is_absolute() or ".." in portable.parts:
            raise ScopeError("changed path escapes the workspace")
        normalized.add(path)
    return sorted(normalized)


def resolve_paths(paths: Iterable[str], root: Path = ROOT, *, empty_is_all: bool = False) -> ScopeResult:
    changed_paths = _normalized_paths(paths)
    frontend = load_frontend_graph(root)
    rust = load_rust_graph(root)

    frontend_seeds: set[str] = set()
    rust_seeds: set[str] = set()
    frontend_manifest_seeds: set[str] = set()
    rust_manifest_seeds: set[str] = set()
    frontend_all = False
    rust_all = False
    dependency_required = False
    pnpm_lock_changed = False
    cargo_lock_changed = False
    reasons: list[str] = []

    if not changed_paths and empty_is_all:
        return ScopeResult(
            frontend_scope="all",
            frontend_packages=[],
            frontend_apps=[],
            rust_scope="all",
            rust_packages=[],
            dependency_scope="all",
            changed_count=0,
            reasons=["explicit repository-wide audit"],
        )

    for path in changed_paths:
        if path == "THIRD_PARTY_NOTICES.md":
            dependency_required = True
            continue
        if _is_documentation(path):
            continue

        if path in SCOPE_DRIVER_PATHS:
            frontend_all = True
            rust_all = True
            dependency_required = True
            reasons.append(f"verification driver changed: {path}")
            continue
        if path in FRONTEND_DRIVER_PATHS:
            frontend_all = True
            dependency_required = True
            reasons.append(f"frontend verification contract changed: {path}")
            continue
        if path.startswith((".github/scripts/", ".github/workflows/")):
            dependency_required = True
            continue
        if path in {".github/dependency-policy.json", "deny.toml"}:
            dependency_required = True
            continue

        if path == "package.json":
            frontend_all = True
            dependency_required = True
            reasons.append("root frontend manifest changed")
            continue
        if path == "pnpm-lock.yaml":
            pnpm_lock_changed = True
            dependency_required = True
            continue
        if path == "pnpm-workspace.yaml" or path.startswith(("tsconfig", "vitest")):
            frontend_all = True
            dependency_required = True
            reasons.append(f"frontend workspace configuration changed: {path}")
            continue

        if path == "Cargo.toml":
            rust_all = True
            dependency_required = True
            reasons.append("root Cargo workspace manifest changed")
            continue
        if path == "Cargo.lock":
            cargo_lock_changed = True
            dependency_required = True
            continue
        if path.startswith((".cargo/", "rust-toolchain")):
            rust_all = True
            dependency_required = True
            reasons.append(f"Rust workspace configuration changed: {path}")
            continue

        if path == "apps/catalog.json":
            for app_name in CATALOG_FRONTEND_CONSUMERS:
                node_name = frontend.by_directory.get(f"apps/{app_name}")
                if node_name is None:
                    frontend_all = True
                    break
                frontend_seeds.add(node_name)
            for node_name in CATALOG_RUST_CONSUMERS:
                if node_name not in rust.nodes:
                    rust_all = True
                    break
                rust_seeds.add(node_name)
            reasons.append("catalog consumers selected")
            continue

        parts = PurePosixPath(path).parts
        if len(parts) >= 2 and parts[0] == "packages":
            directory = "/".join(parts[:2])
            node_name = frontend.by_directory.get(directory)
            if node_name is None:
                frontend_all = True
                reasons.append(f"unknown or deleted frontend package: {directory}")
            else:
                frontend_seeds.add(node_name)
                if path == f"{directory}/package.json":
                    frontend_manifest_seeds.add(node_name)
                    dependency_required = True
            continue

        if len(parts) >= 2 and parts[0] == "crates":
            directory = "/".join(parts[:2])
            node_name = rust.by_directory.get(directory)
            if node_name is None:
                rust_all = True
                reasons.append(f"unknown or deleted Rust crate: {directory}")
            else:
                rust_seeds.add(node_name)
                if path == f"{directory}/Cargo.toml":
                    rust_manifest_seeds.add(node_name)
                    dependency_required = True
            continue

        if len(parts) >= 2 and parts[0] == "apps":
            app_directory = "/".join(parts[:2])
            if len(parts) >= 3 and parts[2] == "src-tauri":
                rust_directory = f"{app_directory}/src-tauri"
                node_name = rust.by_directory.get(rust_directory)
                if node_name is None:
                    rust_all = True
                    reasons.append(f"unknown or deleted Rust app package: {rust_directory}")
                else:
                    rust_seeds.add(node_name)
                    if path == f"{rust_directory}/Cargo.toml":
                        rust_manifest_seeds.add(node_name)
                        dependency_required = True
            else:
                node_name = frontend.by_directory.get(app_directory)
                if node_name is None:
                    frontend_all = True
                    reasons.append(f"unknown or deleted frontend app: {app_directory}")
                else:
                    frontend_seeds.add(node_name)
                    if path == f"{app_directory}/package.json":
                        frontend_manifest_seeds.add(node_name)
                        dependency_required = True
            continue

        frontend_all = True
        rust_all = True
        reasons.append(f"unclassified workspace path: {path}")

    if pnpm_lock_changed:
        if frontend_manifest_seeds:
            reasons.append("pnpm lockfile paired with changed workspace manifest")
        elif not frontend_all:
            frontend_all = True
            reasons.append("pnpm lockfile changed without a workspace manifest")
    if cargo_lock_changed:
        if rust_manifest_seeds:
            reasons.append("Cargo lockfile paired with changed workspace manifest")
        elif not rust_all:
            rust_all = True
            reasons.append("Cargo lockfile changed without a workspace manifest")

    if frontend_all:
        frontend_scope = "all"
        frontend_packages: list[str] = []
        frontend_apps: list[str] = []
    else:
        affected_frontend = frontend.closure(frontend_seeds)
        frontend_packages = sorted(frontend.nodes[name].directory for name in affected_frontend)
        frontend_apps = sorted(
            frontend.nodes[name].directory.split("/", 1)[1]
            for name in affected_frontend
            if frontend.nodes[name].kind == "app"
        )
        frontend_scope = "apps" if frontend_packages else "none"

    if rust_all:
        rust_scope = "all"
        rust_packages: list[str] = []
    else:
        rust_packages = sorted(rust.closure(rust_seeds))
        rust_scope = "packages" if rust_packages else "none"

    if not reasons:
        if not changed_paths:
            reasons.append("no local changes detected")
        elif frontend_scope == "none" and rust_scope == "none" and not dependency_required:
            reasons.append("documentation-only changes")
        elif frontend_scope == "none" and rust_scope == "none" and dependency_required:
            reasons.append("dependency policy inputs selected")
        else:
            reasons.append("affected dependency closures selected")

    return ScopeResult(
        frontend_scope=frontend_scope,
        frontend_packages=frontend_packages,
        frontend_apps=frontend_apps,
        rust_scope=rust_scope,
        rust_packages=rust_packages,
        dependency_scope="all" if dependency_required else "none",
        changed_count=len(changed_paths),
        reasons=reasons,
    )


def _git(root: Path, *arguments: str, allow_failure: bool = False) -> str:
    result = subprocess.run(
        ["git", *arguments], cwd=root, check=False, capture_output=True, text=True
    )
    if result.returncode != 0 and not allow_failure:
        raise ScopeError(result.stderr.strip() or "git command failed")
    return result.stdout.strip() if result.returncode == 0 else ""


def _git_paths(root: Path, *arguments: str) -> list[str]:
    result = subprocess.run(
        ["git", *arguments], cwd=root, check=False, capture_output=True
    )
    if result.returncode != 0:
        message = result.stderr.decode("utf-8", errors="replace").strip() or "git command failed"
        raise ScopeError(message)
    try:
        output = result.stdout.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ScopeError("changed path is not valid UTF-8") from error
    return [path for path in output.split("\0") if path]


def commit_paths(base_sha: str, head_sha: str, root: Path = ROOT) -> list[str]:
    if base_sha and set(base_sha) == {"0"}:
        base_sha = f"{head_sha}^"
    merge_base = _git(root, "merge-base", base_sha, head_sha, allow_failure=True) or base_sha
    return _git_paths(
        root, "diff", "--name-only", "-z", "--diff-filter=ACDMRTUXB", merge_base, head_sha, "--"
    )


def _default_local_base(root: Path) -> str:
    for candidate in ("origin/main", "main"):
        if _git(root, "rev-parse", "--verify", candidate, allow_failure=True):
            return candidate
    return "HEAD^"


def working_tree_paths(base: str | None = None, root: Path = ROOT) -> list[str]:
    comparison_base = base or os.environ.get("AFFECTED_BASE") or _default_local_base(root)
    merge_base = _git(root, "merge-base", comparison_base, "HEAD", allow_failure=True) or comparison_base
    paths: list[str] = []
    for arguments in (
        ("diff", "--name-only", "-z", "--diff-filter=ACDMRTUXB", merge_base, "HEAD", "--"),
        ("diff", "--name-only", "-z", "--diff-filter=ACDMRTUXB", "--"),
        ("diff", "--cached", "--name-only", "-z", "--diff-filter=ACDMRTUXB", "--"),
        ("ls-files", "--others", "--exclude-standard", "-z"),
    ):
        paths.extend(_git_paths(root, *arguments))
    return _normalized_paths(paths)


def write_outputs(result: ScopeResult) -> None:
    destination = os.environ.get("GITHUB_OUTPUT")
    serialized = "".join(f"{key}={value}\n" for key, value in result.outputs().items())
    if destination:
        with Path(destination).open("a", encoding="utf-8") as output:
            output.write(serialized)
    else:
        sys.stdout.write(serialized)


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--working-tree", action="store_true", help="include branch and uncommitted changes")
    mode.add_argument("--all", action="store_true", help="select the complete workspace")
    parser.add_argument("base", nargs="?", help="base SHA/ref (or local comparison ref)")
    parser.add_argument("head", nargs="?", help="head SHA for CI commit mode")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv or sys.argv[1:])
    try:
        if args.all:
            result = resolve_paths([], empty_is_all=True)
        elif args.working_tree:
            if args.head is not None:
                raise ScopeError("working-tree mode accepts at most one base ref")
            result = resolve_paths(working_tree_paths(args.base), empty_is_all=False)
        else:
            if args.base is None or args.head is None:
                raise ScopeError("base and head SHAs are required in CI mode")
            result = resolve_paths(commit_paths(args.base, args.head), empty_is_all=True)
        write_outputs(result)
        return 0
    except ScopeError as error:
        print(f"scope resolution failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
