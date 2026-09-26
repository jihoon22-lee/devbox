#!/usr/bin/env python3
"""Check hidden product ownership, build identity and evidenced parity coverage."""
import json
import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PRODUCTS = {"workspace", "api-studio", "knowledge", "control-center"}


def check(root=ROOT):
    catalog = json.loads((root / "apps/products.json").read_text())
    public = json.loads((root / "apps/catalog.json").read_text())
    assert catalog["schemaVersion"] == 3 and catalog["channel"] == "stable"
    assert {p["id"] for p in catalog["products"]} == PRODUCTS
    assert len(catalog["products"]) == 4
    assert {p["id"] for p in public["apps"]} == {"devbox-" + p for p in PRODUCTS}
    for product in catalog["products"]:
        app_id = "devbox-" + product["id"]
        entry = next(p for p in public["apps"] if p["id"] == app_id)
        assert entry["release"] is True and entry["managerVisible"] is True
        assert entry["identifier"] == product["identifier"]
        config = json.loads((root / entry["appDir"] / "src-tauri/tauri.conf.json").read_text())
        assert config["identifier"] == product["identifier"]
        cargo = tomllib.loads((root / entry["appDir"] / "src-tauri/Cargo.toml").read_text())
        package = json.loads((root / entry["appDir"] / "package.json").read_text())
        assert re.fullmatch(r"0\.8\.(?:0|[1-9][0-9]*)", cargo["package"]["version"])
        assert config["version"] == package["version"] == cargo["package"]["version"]
        host = (root / entry["appDir"] / "src-tauri/src/lib.rs").read_text()
        assert re.search(r'suite::plugin\(\s*"[a-z-]+"\s*,\s*env!\("CARGO_PKG_VERSION"\)', host), "Suite identity must use the product host version"

        features = [f for f in catalog["features"] if f["owner"] == product["id"]]
        assert product["defaultRoute"] in {f["route"] for f in features}
        assert len({f["route"] for f in features}) == len(features)
        capability = json.loads((root / entry["appDir"] / "src-tauri/capabilities/default.json").read_text())
        assert capability["windows"] == ["main"]
        assert "remote" not in capability
        expected_permissions = {"core:default", "product-shell:allow-describe", "product-shell:allow-route-status", "suite:allow-connection"}
        if product["id"] == "workspace":
            expected_permissions.update({"workspace:allow-runtime","workspace:allow-processes","workspace:allow-process-actions","workspace:allow-logs","workspace:allow-terminal","workspace:allow-problems","workspace:allow-commands","workspace:allow-files","workspace:allow-lsp","workspace:allow-source","workspace:allow-registry","workspace:allow-setup","workspace:allow-definitions","workspace:allow-dependencies"})
        if product["id"] == "api-studio":
            expected_permissions.update({"api-studio:allow-api", "api-studio:allow-webhooks", "api-studio:allow-transforms"})
        if product["id"] == "knowledge":
            expected_permissions.update({"knowledge:allow-activity", "knowledge:allow-notes", "knowledge:allow-search", "knowledge:allow-search-settings", "knowledge:allow-opener", "knowledge:allow-setup", "knowledge:allow-commands"})
        if product["id"] == "control-center":
            expected_permissions.update({"control-center:allow-tools", "control-center:allow-delivery", "commands:allow-command-search", "commands:allow-command-preview", "commands:allow-command-source", "commands:allow-command-cancel", "commands:allow-command-open", "commands:allow-command-status", "commands:allow-command-preferences", "commands:allow-command-shortcut", "commands:allow-command-trigger-shortcut"})
        assert set(capability["permissions"]) == expected_permissions
        capability_dir = root / entry["appDir"] / "src-tauri/capabilities"
        expected_files = {"default.json"}
        if product["id"] == "workspace":
            expected_files.add("terminal.json")
            terminal = json.loads((capability_dir / "terminal.json").read_text())
            assert terminal["windows"] == ["terminal-*"]
            assert "remote" not in terminal and not terminal.get("webviews")
            assert set(terminal["permissions"]) == {
                "core:default", "workspace:allow-terminal-describe",
                "workspace:allow-terminal-execute", "clipboard-manager:allow-read-text",
                "opener:allow-open-url",
            }
        assert {path.name for path in capability_dir.glob("*.json")} == expected_files
    for source in (root / "crates/suite-runtime/src").rglob("*.rs"):
        assert "CARGO_PKG_VERSION" not in source.read_text(), "Suite library metadata is not product identity"
    workflow = (root / ".github/workflows/product-foundation.yml").read_text()
    assert "'crates/suite-runtime/**'" in workflow, "Suite-only changes require native acceptance"
    print("Product foundation: four products with exact local capability boundaries")


if __name__ == "__main__":
    check()
