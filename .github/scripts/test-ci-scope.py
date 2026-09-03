#!/usr/bin/env python3
"""Regression coverage for graph-aware local and CI scope resolution."""

from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github" / "scripts" / "resolve-ci-scope.py"
spec = importlib.util.spec_from_file_location("resolve_ci_scope", SCRIPT)
module = importlib.util.module_from_spec(spec)
assert spec.loader is not None
sys.modules[spec.name] = module
spec.loader.exec_module(module)


def resolve(*paths: str):
    return module.resolve_paths(paths, ROOT)


frontend_only = resolve("apps/run-manager/src/App.tsx")
assert frontend_only.frontend_scope == "apps"
assert frontend_only.frontend_packages == ["apps/run-manager"]
assert frontend_only.frontend_apps == ["run-manager"]
assert frontend_only.rust_scope == "none"
assert frontend_only.dependency_scope == "none"

rust_only = resolve("apps/run-manager/src-tauri/src/lib.rs")
assert rust_only.frontend_scope == "none"
assert rust_only.rust_scope == "packages"
assert rust_only.rust_packages == ["run-manager"]

frontend_manifest_lock = resolve("apps/wsl-desktop/package.json", "pnpm-lock.yaml")
assert frontend_manifest_lock.frontend_scope == "apps"
assert frontend_manifest_lock.frontend_packages == ["apps/wsl-desktop"]
assert frontend_manifest_lock.dependency_scope == "all"

rust_manifest_lock = resolve("apps/devbox-manager/src-tauri/Cargo.toml", "Cargo.lock")
assert rust_manifest_lock.rust_scope == "packages"
assert rust_manifest_lock.rust_packages == ["devbox-manager"]
assert rust_manifest_lock.dependency_scope == "all"

lock_only = resolve("Cargo.lock")
assert lock_only.rust_scope == "all"
assert lock_only.dependency_scope == "all"

pnpm_lock_only = resolve("pnpm-lock.yaml")
assert pnpm_lock_only.frontend_scope == "all"
assert pnpm_lock_only.dependency_scope == "all"

editor = resolve("packages/editor/src/index.ts")
assert editor.frontend_packages == ["apps/code-pad", "apps/knowledge-base", "packages/editor"]
assert editor.frontend_apps == ["code-pad", "knowledge-base"]

openapi = resolve("packages/openapi/src/index.ts")
assert openapi.frontend_packages == ["apps/api-playground", "apps/webhook-lab", "packages/openapi"]

a11y = resolve("packages/a11y/src/index.ts")
assert len(a11y.frontend_apps) == 15
assert "packages/a11y" in a11y.frontend_packages

process = resolve("crates/process/src/lib.rs")
assert process.rust_packages == ["port-manager", "process"]

search = resolve("crates/search/src/lib.rs")
assert search.rust_packages == ["everything-plus", "knowledge-base", "search"]

secrets = resolve("crates/secrets/src/lib.rs")
assert secrets.rust_packages == ["api-playground", "run-manager", "secrets", "workbench"]

rust_graph = module.load_rust_graph(ROOT)
wsl = resolve("crates/wsl/src/lib.rs")
assert len({node for node in wsl.rust_packages if rust_graph.nodes[node].kind == "app"}) == 15

catalog = resolve("apps/catalog.json")
assert catalog.frontend_apps == ["devbox-launcher", "devbox-manager", "everything-plus", "repo-manager"]
assert "catalog" in catalog.rust_packages
assert "launch" in catalog.rust_packages
assert "code-pad" not in catalog.rust_packages

catalog_frontend_importers = {
    source.relative_to(ROOT).parts[1]
    for source in ROOT.glob("apps/*/src/**/*")
    if source.is_file()
    and source.suffix in {".js", ".jsx", ".ts", ".tsx"}
    and "catalog.json" in source.read_text(encoding="utf-8")
}
assert catalog_frontend_importers == module.CATALOG_FRONTEND_CONSUMERS

catalog_include = re.compile(r"include_str!\s*\([^)]*catalog\.json")
catalog_rust_importers = {
    node.name
    for node in rust_graph.nodes.values()
    if any(
        catalog_include.search(source.read_text(encoding="utf-8"))
        for source in (ROOT / node.directory).rglob("*.rs")
    )
}
assert catalog_rust_importers == module.CATALOG_RUST_CONSUMERS

dependency_metadata = resolve("THIRD_PARTY_NOTICES.md")
assert dependency_metadata.frontend_scope == "none"
assert dependency_metadata.rust_scope == "none"
assert dependency_metadata.dependency_scope == "all"

docs = resolve("docs/development.md", "workthrough/example.md", "README.md")
assert docs.frontend_scope == "none"
assert docs.rust_scope == "none"
assert docs.dependency_scope == "none"
assert docs.reasons == ["documentation-only changes"]

driver = resolve(".github/scripts/ci-scope.sh")
assert driver.frontend_scope == "all"
assert driver.rust_scope == "all"
assert driver.dependency_scope == "all"

frontend_driver = resolve(".github/scripts/check-frontend-bundles.mjs")
assert frontend_driver.frontend_scope == "all"
assert frontend_driver.rust_scope == "none"
assert frontend_driver.dependency_scope == "all"

release_contract = resolve(".github/workflows/release.yml")
assert release_contract.frontend_scope == "none"
assert release_contract.rust_scope == "none"
assert release_contract.dependency_scope == "all"

unknown = resolve("unclassified.workspace")
assert unknown.frontend_scope == "all"
assert unknown.rust_scope == "all"

manual = module.resolve_paths([], ROOT, empty_is_all=True)
assert manual.frontend_scope == "all"
assert manual.rust_scope == "all"
assert manual.dependency_scope == "all"

local_clean = module.resolve_paths([], ROOT, empty_is_all=False)
assert local_clean.frontend_scope == "none"
assert local_clean.rust_scope == "none"
assert local_clean.dependency_scope == "none"

for unsafe_path in (" apps/run-manager/src/App.tsx", "apps\\run-manager\\src\\App.tsx"):
    try:
        resolve(unsafe_path)
    except module.ScopeError:
        pass
    else:
        raise AssertionError(f"unsafe path must fail closed: {unsafe_path!r}")

print("CI scope regression tests passed")
