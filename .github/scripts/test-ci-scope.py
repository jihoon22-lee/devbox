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
assert rust_only.rust_packages == ["devbox-workspace", "run-manager"]

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
assert editor.frontend_packages == [
    "apps/code-pad", "apps/devbox-knowledge", "apps/devbox-workspace", "apps/everything-plus", "apps/knowledge-base", "apps/life-log", "apps/log-lens", "apps/port-manager", "apps/repo-manager", "apps/run-manager",
    "apps/workbench", "apps/wsl-desktop", "packages/editor", "packages/knowledge-features", "packages/workspace-features",
]
assert editor.frontend_apps == ["code-pad", "devbox-knowledge", "devbox-workspace", "everything-plus", "knowledge-base", "life-log", "log-lens", "port-manager", "repo-manager", "run-manager", "workbench", "wsl-desktop"]
for feature in ["tasks", "runtime", "logs", "terminal"]:
    workspace_feature = resolve(f"packages/workspace-features/src/{feature}/api.ts")
    assert workspace_feature.frontend_apps == [
        "code-pad", "devbox-workspace", "log-lens", "port-manager", "repo-manager",
        "run-manager", "workbench", "wsl-desktop",
    ]
    assert workspace_feature.rust_scope == "none"
knowledge_features = resolve("packages/knowledge-features/src/notes/api.ts")
assert knowledge_features.frontend_apps == ["devbox-knowledge", "everything-plus", "knowledge-base", "life-log"]

openapi = resolve("packages/openapi/src/index.ts")
assert openapi.frontend_packages == [
    "apps/api-playground", "apps/devbox-api-studio", "apps/developer-toolbox",
    "apps/webhook-lab", "packages/api-studio-features", "packages/openapi",
]
api_features = resolve("packages/api-studio-features/src/requests/api.ts")
assert api_features.frontend_apps == ["api-playground", "devbox-api-studio", "developer-toolbox", "webhook-lab"]
api_protocols = resolve("crates/api-protocols/src/core/grpc.rs")
assert api_protocols.rust_packages == ["api-playground", "api-protocols", "devbox-api-studio"]
for crate, legacy in [("webhook-core", "webhook-lab"), ("transforms-core", "developer-toolbox")]:
    shared_domain = resolve(f"crates/{crate}/src/lib.rs")
    assert shared_domain.rust_packages == sorted([crate, legacy, "devbox-api-studio"])
migration = resolve("crates/data-migration/src/lib.rs")
assert migration.rust_packages == ["data-migration", "devbox-api-studio", "devbox-control-center", "devbox-knowledge", "devbox-workspace", "run-manager"]
for app in ["run-manager", "port-manager", "log-lens"]:
    native_runtime = resolve(f"apps/{app}/src-tauri/src/component.rs")
    assert native_runtime.rust_packages == sorted([app, "devbox-workspace"])
api_native = resolve("apps/api-playground/src-tauri/src/component.rs")
assert api_native.rust_packages == ["api-playground", "devbox-api-studio"]

a11y = resolve("packages/a11y/src/index.ts")
assert len(a11y.frontend_apps) == 19
assert "packages/a11y" in a11y.frontend_packages

process = resolve("crates/process/src/lib.rs")
assert process.rust_packages == ["devbox-workspace", "port-manager", "process"]

search = resolve("crates/search/src/lib.rs")
assert search.rust_packages == ["devbox-knowledge", "everything-plus", "knowledge-base", "search"]
for app in ["knowledge-base", "life-log", "everything-plus"]:
    native_component = resolve(f"apps/{app}/src-tauri/src/component.rs")
    assert native_component.rust_packages == sorted([app, "devbox-knowledge"])

secrets = resolve("crates/secrets/src/lib.rs")
assert secrets.rust_packages == [
    "api-playground", "devbox-api-studio", "devbox-control-center", "devbox-knowledge",
    "devbox-workspace", "knowledge-base", "product-contract", "product-shell-tauri", "run-manager",
    "secrets", "workbench", "workspace-wsl",
]

native_helper = resolve("apps/devbox-workspace/native/src/engine.rs")
assert native_helper.frontend_scope == "none"
assert native_helper.rust_packages == ["devbox-workspace", "workspace-wsl"]
helper_manifest = resolve("apps/devbox-workspace/native/Cargo.toml")
assert helper_manifest.dependency_scope == "all"
assert helper_manifest.rust_packages == native_helper.rust_packages
rust_graph = module.load_rust_graph(ROOT)
assert rust_graph.nodes["workspace-wsl"].kind == "crate"
wsl = resolve("crates/wsl/src/lib.rs")
assert len({node for node in wsl.rust_packages if rust_graph.nodes[node].kind == "app"}) == 19

catalog = resolve("apps/catalog.json")
assert catalog.frontend_apps == ["code-pad", "devbox-knowledge", "devbox-launcher", "devbox-manager", "devbox-workspace", "everything-plus", "knowledge-base", "life-log", "log-lens", "port-manager", "repo-manager", "run-manager", "workbench", "wsl-desktop"]
assert "packages/workspace-features" in catalog.frontend_packages
assert "packages/knowledge-features" in catalog.frontend_packages
assert "catalog" in catalog.rust_packages
assert "launch" in catalog.rust_packages
assert "code-pad" not in catalog.rust_packages

catalog_frontend_importers = {
    "/".join(source.relative_to(ROOT).parts[:2])
    for source in [*ROOT.glob("apps/*/src/**/*"), *ROOT.glob("packages/*/src/**/*")]
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


agent_metadata = resolve(module.AGENT_POLICY_PATH)
assert agent_metadata.frontend_scope == agent_metadata.rust_scope == "none"
assert agent_metadata.dependency_scope == "none"
for path in (".agents/skills/devbox-change/scripts/check.py", ".agents/skills/new/agents/openai.yaml"):
    unknown_agent = resolve(path)
    assert unknown_agent.frontend_scope == unknown_agent.rust_scope == "all"
for path in (".github/scripts/verify-resources.py", ".github/scripts/check-agent-metadata.py"):
    driver = resolve(path)
    assert driver.frontend_scope == driver.rust_scope == "all"

print("CI scope regression tests passed")

for path in ("apps/products.json", "packages/product-shell/fixtures/route-request.json"):
    products = resolve(path)
    assert products.frontend_scope == "apps"
    assert set(products.frontend_apps) == {"devbox-workspace", "devbox-api-studio", "devbox-knowledge", "devbox-control-center"}
    assert {"devbox-workspace", "devbox-api-studio", "devbox-knowledge", "devbox-control-center"} <= set(products.rust_packages)
parity = resolve("apps/v0.8-feature-parity.json")
assert parity.frontend_scope == parity.rust_scope == "all"
