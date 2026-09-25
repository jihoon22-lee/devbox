#!/usr/bin/env python3
"""Regression coverage for graph-aware local and CI scope resolution."""

from __future__ import annotations

import importlib.util
import re
import sys
import subprocess
import tempfile
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


frontend_only = resolve("apps/devbox-workspace/src/App.tsx")
assert frontend_only.frontend_scope == "apps"
assert frontend_only.frontend_packages == ["apps/devbox-workspace"]
assert frontend_only.frontend_apps == ["devbox-workspace"]
assert frontend_only.rust_scope == "none"
assert frontend_only.dependency_scope == "none"

rust_only = resolve("crates/runtime-engine/src/lib.rs")
assert rust_only.frontend_scope == "none"
assert rust_only.rust_scope == "packages"
assert rust_only.rust_packages == ["devbox-runtime-engine", "devbox-workspace"]

frontend_manifest_lock = resolve("apps/devbox-workspace/package.json", "pnpm-lock.yaml")
assert frontend_manifest_lock.frontend_scope == "apps"
assert frontend_manifest_lock.frontend_packages == ["apps/devbox-workspace"]
assert frontend_manifest_lock.dependency_scope == "all"

rust_manifest_lock = resolve("crates/installation-tools/Cargo.toml", "Cargo.lock")
assert rust_manifest_lock.rust_scope == "packages"
assert rust_manifest_lock.rust_packages == ["devbox-control-center", "devbox-installation-tools"]
assert rust_manifest_lock.dependency_scope == "all"

lock_only = resolve("Cargo.lock")
assert lock_only.rust_scope == "all"
assert lock_only.dependency_scope == "all"

pnpm_lock_only = resolve("pnpm-lock.yaml")
assert pnpm_lock_only.frontend_scope == "all"
assert pnpm_lock_only.dependency_scope == "all"

editor = resolve("packages/editor/src/index.ts")
assert editor.frontend_packages == ["apps/devbox-knowledge", "apps/devbox-workspace", "packages/editor", "packages/knowledge-features", "packages/workspace-features"]
assert editor.frontend_apps == ["devbox-knowledge", "devbox-workspace"]
for feature in ["tasks", "runtime", "logs", "terminal"]:
    workspace_feature = resolve(f"packages/workspace-features/src/{feature}/api.ts")
    assert workspace_feature.frontend_apps == ["devbox-workspace"]
    assert workspace_feature.rust_scope == "none"
knowledge_features = resolve("packages/knowledge-features/src/notes/api.ts")
assert knowledge_features.frontend_apps == ["devbox-knowledge"]
openapi = resolve("packages/openapi/src/index.ts")
assert openapi.frontend_packages == ["apps/devbox-api-studio", "packages/api-studio-features", "packages/openapi"]
assert resolve("packages/api-studio-features/src/requests/api.ts").frontend_apps == ["devbox-api-studio"]
assert resolve("crates/api-protocols/src/core/grpc.rs").rust_packages == ["api-protocols", "devbox-api-studio", "devbox-http-client-engine"]
for crate, engine in [("webhook-core", "devbox-webhook-host"), ("transforms-core", "devbox-toolbox-engine")]:
    assert resolve(f"crates/{crate}/src/lib.rs").rust_packages == sorted([crate, engine, "devbox-api-studio"])
assert resolve("crates/data-migration/src/lib.rs").rust_packages == ["data-migration", "devbox-api-studio", "devbox-control-center", "devbox-knowledge", "devbox-runtime-engine", "devbox-workspace"]
for engine in ["runtime-engine", "ports-engine", "logs-engine"]:
    assert resolve(f"crates/{engine}/src/component.rs").rust_packages == sorted(["devbox-"+engine, "devbox-workspace"])
assert resolve("crates/http-client-engine/src/component.rs").rust_packages == ["devbox-api-studio", "devbox-http-client-engine"]
a11y = resolve("packages/a11y/src/index.ts")
assert len(a11y.frontend_apps) == 4
assert "packages/a11y" in a11y.frontend_packages
assert resolve("crates/process/src/lib.rs").rust_packages == ["devbox-ports-engine", "devbox-workspace", "process"]
assert resolve("crates/search/src/lib.rs").rust_packages == ["devbox-content-index-engine", "devbox-knowledge", "devbox-knowledge-vault-engine", "search"]
for engine in ["knowledge-vault-engine", "activity-engine", "content-index-engine"]:
    assert resolve(f"crates/{engine}/src/component.rs").rust_packages == sorted(["devbox-"+engine, "devbox-knowledge"])
secrets = resolve("crates/secrets/src/lib.rs")
assert secrets.rust_packages == sorted(["devbox-installation-tools", "devbox-http-client-engine", "devbox-api-studio", "devbox-control-center", "devbox-knowledge", "devbox-workspace", "devbox-knowledge-vault-engine", "product-contract", "product-shell-tauri", "devbox-runtime-engine", "secrets", "devbox-projects-engine", "workspace-wsl", "suite-runtime"])

native_helper = resolve("apps/devbox-workspace/native/src/engine.rs")
assert native_helper.frontend_scope == "none"
assert native_helper.rust_packages == ["devbox-knowledge", "devbox-knowledge-vault-engine", "devbox-workspace", "workspace-wsl"]
helper_manifest = resolve("apps/devbox-workspace/native/Cargo.toml")
assert helper_manifest.dependency_scope == "all"
assert helper_manifest.rust_packages == native_helper.rust_packages
rust_graph = module.load_rust_graph(ROOT)
assert rust_graph.nodes["workspace-wsl"].kind == "crate"
wsl = resolve("crates/wsl/src/lib.rs")
assert len({node for node in wsl.rust_packages if rust_graph.nodes[node].kind == "app"}) == 4

catalog = resolve("apps/catalog.json")
assert catalog.frontend_apps == ["devbox-control-center", "devbox-knowledge", "devbox-workspace"]
assert "packages/workspace-features" in catalog.frontend_packages
assert "packages/knowledge-features" in catalog.frontend_packages
assert "catalog" in catalog.rust_packages
assert "launch" in catalog.rust_packages
assert "devbox-editor-engine" not in catalog.rust_packages

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

for unsafe_path in (" apps/devbox-workspace/src/App.tsx", "apps\\run-manager\\src\\App.tsx"):
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


for path in ("apps/products.json", "packages/product-shell/fixtures/route-request.json"):
    products = resolve(path)
    assert products.frontend_scope == "apps"
    assert set(products.frontend_apps) == {"devbox-workspace", "devbox-api-studio", "devbox-knowledge", "devbox-control-center"}
    assert {"devbox-workspace", "devbox-api-studio", "devbox-knowledge", "devbox-control-center"} <= set(products.rust_packages)
parity = resolve("apps/v0.8-feature-parity.json")
assert parity.frontend_scope == parity.rust_scope == "all"

# Discover every explicit include and propagate consumers through shared modules.
# Same-crate includes require no manual edge unless the including file itself
# is compiled by a different crate (suite.rs -> platform/mod.rs, for example).
def included_consumers(root, sources, owners):
    def owner(path):
        return next((name for directory, name in sorted(owners.items(), key=lambda item: -len(item[0]))
                     if path == directory or path.startswith(directory + "/")), None)

    edges = []
    consumers = {}
    for source in sources:
        name = source.relative_to(root).as_posix()
        native = owner(name)
        if native is None:
            continue
        consumers.setdefault(name, set()).add(native)
        for relative in re.findall(r'#\[\s*path\s*=\s*"([^"\n]+)"\s*\]', source.read_text()):
            target = (source.parent / relative).resolve().relative_to(root).as_posix()
            assert (root / target).is_file(), (name, target)
            edges.append((name, target))
            consumers.setdefault(target, set()).add(owner(target))
    changed = True
    while changed:
        changed = False
        for source, target in edges:
            previous = len(consumers[target])
            consumers[target].update(consumers[source])
            changed |= previous != len(consumers[target])
    return {target: values - {owner(target), None} for target, values in consumers.items()
            if values - {owner(target), None}}


def require_include_edges(discovered, registered):
    for path, consumers in discovered.items():
        assert consumers <= registered.get(path, set()), ("unregistered cross-crate include", path, consumers)


rust_files = subprocess.check_output(
    ["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", "*.rs"], cwd=ROOT, text=True
).splitlines()
owners = {node.directory: name for name, node in module.load_rust_graph(ROOT).nodes.items()}
included_sources = included_consumers(ROOT, [ROOT / name for name in rust_files if (ROOT / name).is_file()], owners)
require_include_edges(included_sources, module.RUST_SHARED_PLATFORM_CONSUMERS)
for path, consumers in included_sources.items():
    shared = resolve(path)
    assert consumers <= set(shared.rust_packages)
    assert shared.frontend_scope == "none"

# Inject a new cross-crate include outside the formerly scanned platform module.
# Test an isolated source tree so the repository never contains a broken fixture.
with tempfile.TemporaryDirectory(prefix="devbox-include-scope-") as temporary:
    root = Path(temporary).resolve()
    sources = {
        "apps/a/src-tauri/src/lib.rs": '#[path = "../../../b/src-tauri/src/shared.rs"]\nmod shared;',
        "apps/b/src-tauri/src/shared.rs": '#[path = "nested.rs"]\nmod nested;',
        "apps/b/src-tauri/src/nested.rs": "",
        "apps/a/src-tauri/src/local.rs": '#[path = "local_child.rs"]\nmod local_child;',
        "apps/a/src-tauri/src/local_child.rs": "",
    }
    for name, contents in sources.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
    discovered = included_consumers(root, [root / name for name in sources], {"apps/a/src-tauri": "a", "apps/b/src-tauri": "b"})
    assert discovered == {"apps/b/src-tauri/src/shared.rs": {"a"}, "apps/b/src-tauri/src/nested.rs": {"a"}}
    try:
        require_include_edges(discovered, {})
    except AssertionError:
        pass
    else:
        raise AssertionError("an unregistered include must fail")
    require_include_edges(discovered, discovered)
print("CI scope regression tests passed")

for path in module.RUST_SHARED_PLATFORM_CONSUMERS:
    if path.startswith("apps/devbox-control-center/src-tauri/src/") and len(module.RUST_SHARED_PLATFORM_CONSUMERS[path]) > 1:
        shared = resolve(path)
        assert {"devbox-workspace", "devbox-api-studio", "devbox-knowledge", "devbox-control-center"} <= set(shared.rust_packages)
        assert shared.frontend_scope == "none"

hotkey = resolve("apps/devbox-control-center/src-tauri/src/platform/hotkey.rs")
assert hotkey.rust_packages == ["devbox-control-center"]

# The frozen migration catalog must still reach native readers and browser fixtures.
legacy_catalog = resolve("apps/legacy-v0.7-catalog.json")
assert legacy_catalog.rust_packages == catalog.rust_packages
assert legacy_catalog.frontend_packages == catalog.frontend_packages

# Explicit crate edges replace source-inclusion exceptions for Suite ownership.
suite = resolve("crates/suite-runtime/src/lib.rs")
assert {"suite-runtime", "devbox-workspace", "devbox-api-studio", "devbox-knowledge", "devbox-control-center"} <= set(suite.rust_packages)
