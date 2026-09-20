#!/usr/bin/env python3
"""Keep the packaged Windows acceptance matrix aligned with release sources."""

from __future__ import annotations

import json
import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG_PATH = ROOT / ".github/scripts/legacy-v0.7-windows-packaged-smoke-config.json"
CATALOG_PATH = ROOT / "apps/legacy-v0.7-catalog.json"


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def source_text(directory: pathlib.Path, suffixes: set[str]) -> str:
    return "\n".join(
        path.read_text(encoding="utf-8", errors="ignore")
        for path in directory.rglob("*")
        if path.is_file() and path.suffix in suffixes
    )


def frontend_source(app_root: pathlib.Path, root: pathlib.Path = ROOT) -> str:
    """Include only declared production workspace consumers, not unrelated apps."""
    packages = {}
    for manifest in sorted((root / "packages").glob("*/package.json")):
        data = read_json(manifest)
        name = data.get("name")
        if isinstance(name, str):
            if name in packages:
                raise ValueError("duplicate workspace package")
            packages[name] = manifest.parent
    visited = set()
    sources = []

    def visit(directory: pathlib.Path) -> None:
        if directory in visited:
            return
        visited.add(directory)
        sources.append(source_text(directory / "src", {".ts", ".tsx", ".js", ".jsx", ".html"}))
        data = read_json(directory / "package.json")
        for field in ("dependencies", "peerDependencies", "optionalDependencies"):
            for name, version in data.get(field, {}).items():
                if isinstance(version, str) and version.startswith("workspace:") and name in packages:
                    visit(packages[name])

    visit(app_root)
    return "\n".join(sources)


def test_workspace_source_boundary() -> None:
    import tempfile
    with tempfile.TemporaryDirectory(prefix="devbox-ui-source-fixture-") as temporary:
        root = pathlib.Path(temporary)
        fixtures = {
            "apps/fixture": {"name": "fixture", "dependencies": {"@devbox/feature": "workspace:*"}, "devDependencies": {"@devbox/test-only": "workspace:*"}},
            "packages/feature": {"name": "@devbox/feature", "dependencies": {"@devbox/nested": "workspace:*"}},
            "packages/nested": {"name": "@devbox/nested", "dependencies": {"@devbox/feature": "workspace:*"}},
            "packages/test-only": {"name": "@devbox/test-only"},
            "packages/unrelated": {"name": "@devbox/unrelated"},
        }
        for directory, manifest in fixtures.items():
            path = root / directory
            (path / "src").mkdir(parents=True)
            (path / "package.json").write_text(json.dumps(manifest), encoding="utf-8")
            (path / "src/index.ts").write_text(directory, encoding="utf-8")
        source = frontend_source(root / "apps/fixture", root)
        assert "packages/feature" in source and "packages/nested" in source
        assert "packages/test-only" not in source and "packages/unrelated" not in source
        assert source.count("packages/feature") == 1, "cyclic declarations must terminate"


def check() -> list[str]:
    failures: list[str] = []
    config = read_json(CONFIG_PATH)
    catalog = read_json(CATALOG_PATH)
    if set(config) != {"schemaVersion", "apps"} or config.get("schemaVersion") != 1:
        failures.append("acceptance config envelope must be schema v1 with only apps")
        return failures

    configured = config.get("apps")
    if not isinstance(configured, list):
        failures.append("acceptance config apps must be an array")
        return failures

    released = {app["id"]: app for app in catalog["apps"] if app["release"]}
    configured_ids = [app.get("id") for app in configured]
    if len(configured_ids) != len(set(configured_ids)):
        failures.append("acceptance config app ids must be unique")
    if set(configured_ids) != set(released):
        failures.append("acceptance config app ids must equal the release catalog")
    isolated_knowledge_ids = [
        app.get("id") for app in configured if app.get("isolatedKnowledgeRoot") is True
    ]
    if isolated_knowledge_ids != ["knowledge-base"]:
        failures.append("only Knowledge Base must declare the isolated acceptance root")
    if any(
        "isolatedKnowledgeRoot" in app and app.get("isolatedKnowledgeRoot") is not True
        for app in configured
    ):
        failures.append("isolated Knowledge root declarations must be true")

    for app in configured:
        app_id = app.get("id")
        if app_id not in released:
            continue
        catalog_app = released[app_id]
        if app.get("identifier") != catalog_app.get("identifier"):
            failures.append(f"{app_id}: frozen identifier differs from legacy catalog")
        process_names = app.get("additionalProcessNames", [])
        if not process_names or len(process_names) != len(set(process_names)):
            failures.append(f"{app_id}: protected process names omit or duplicate the product image")
        if any(
            not isinstance(name, str)
            or re.fullmatch(r"[A-Za-z0-9 .+_-]+\.exe", name) is None
            or pathlib.PureWindowsPath(name).name != name
            for name in process_names
        ):
            failures.append(f"{app_id}: protected process name is unsafe")

        if not app.get("markers") or not app.get("probes"):
            failures.append(f"{app_id}: frozen native contract missing")

    # Current public product identities come from the four Tauri sources.
    live = read_json(ROOT / ".github/scripts/windows-packaged-smoke-config.json")
    assert live == read_json(ROOT / ".github/scripts/windows-installer-acceptance-config.json")
    assert live["schemaVersion"] == 2 and len(live["products"]) == 4
    for product in live["products"]:
        app_root = ROOT / f"apps/devbox-{product['id']}"
        tauri = read_json(app_root / "src-tauri/tauri.conf.json")
        package = read_json(app_root / "package.json")
        if product["version"] != package["version"] or product["version"] != live["suiteVersion"]:
            failures.append("Suite product version mismatch")
        if product["identifier"] != tauri["identifier"]:
            failures.append("Suite identifier mismatch")

    return failures


def main() -> int:
    test_workspace_source_boundary()
    failures = check()
    if failures:
        print("WINDOWS PACKAGED SMOKE CONFIG FAILED:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("WINDOWS PACKAGED SMOKE CONFIG OK: release catalog and 15 app contracts align")
    return 0


if __name__ == "__main__":
    sys.exit(main())
