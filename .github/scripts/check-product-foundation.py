#!/usr/bin/env python3
"""Check hidden product ownership, build identity and evidenced parity coverage."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PRODUCTS = {"workspace", "api-studio", "knowledge", "control-center"}


def has_verified_source(record):
    commit = record.get("verifiedSourceCommit")
    return (isinstance(commit, str) and len(commit) == 40
            and all(character in "0123456789abcdef" for character in commit)
            and bool(record.get("evidence")))


def check(root=ROOT):
    catalog = json.loads((root / "apps/products.json").read_text())
    legacy = json.loads((root / "apps/catalog.json").read_text())
    parity = json.loads((root / "apps/v0.8-feature-parity.json").read_text())
    data_inventory = json.loads((root / "apps/v0.8-data-inventory.json").read_text())
    assert catalog["schemaVersion"] == 3 and catalog["channel"] == "development"
    assert {p["id"] for p in catalog["products"]} == PRODUCTS
    assert len(catalog["products"]) == 4
    assert len([p for p in legacy["apps"] if p["release"]]) == 15
    assert {p["legacyApp"] for p in parity["apps"]} == {p["id"] for p in legacy["apps"] if p["release"]}
    for app in parity["apps"]:
        review = app["baselineReview"]
        assert review["implementedGroups"] and review["authorityBoundary"]
        assert review["newProductParity"] in {"pending-owner-implementation", "verified-product-internal"}
        if review["newProductParity"] == "verified-product-internal":
            features = [f for f in parity["features"] if f["legacyFeatureId"].startswith(app["legacyApp"] + ":")]
            assert features and all(has_verified_source(f) for f in features)
            assert all(f["status"] == "verified" or (
                f.get("producerVerified") is True
                and f.get("integrationOwnerIssue") in range(544, 551)
                and f["integrationOwnerIssue"] != f["ownerIssue"]
            ) for f in features), "unverified internal behavior cannot satisfy product acceptance"
        assert review["nativeRegisteredCommands"] == sum(
            f["kind"] == "native-command" and f["legacyFeatureId"].startswith(app["legacyApp"] + ":")
            for f in parity["features"])
        for evidence in review["evidence"]:
            assert (root / evidence).is_file()
        assert app["registration"]["singleInstance"] and app["registration"]["portable"]
        if mapping := app.get("implementationReview"):
            assert mapping["featureMappings"] == sum(f["legacyFeatureId"].startswith(app["legacyApp"] + ":") for f in parity["features"])
            assert (root / mapping["evidence"]).is_file()
    assert data_inventory["schemaVersion"] == 1
    assert data_inventory["baselineCommit"] == parity["baselineCommit"]
    assert data_inventory["baselineRelease"] == parity["baselineRelease"]
    assert {g["legacyApp"] for g in data_inventory["groups"]} == {p["legacyApp"] for p in parity["apps"]}
    for group in data_inventory["groups"]:
        assert group["classification"] in {"authoritative", "derived", "ephemeral", "mixed"}
        assert group["items"] and group["treatment"]
        assert group["importerStatus"] in {"pending", "verified"}
        if group["importerStatus"] == "verified":
            assert has_verified_source(group) and group.get("test")
            for file in [group.get("implementationPath"), group.get("schemaMapping"), *group["test"]]:
                assert isinstance(file, str) and not Path(file).is_absolute() and ".." not in Path(file).parts
                assert (root / file).is_file()
        path = Path(group["sourcePath"])
        assert not path.is_absolute() and ".." not in path.parts
        assert (root / path).is_file()
    for product in catalog["products"]:
        app_id = "devbox-" + product["id"]
        entry = next(p for p in legacy["apps"] if p["id"] == app_id)
        assert entry["release"] is False and entry["managerVisible"] is False
        assert entry["identifier"] == product["identifier"]
        config = json.loads((root / entry["appDir"] / "src-tauri/tauri.conf.json").read_text())
        assert config["identifier"] == product["identifier"]
        assert config["version"] == "0.8.0"
        assert product["identifier"] not in {p["identifier"] for p in parity["apps"]}
        features = [f for f in catalog["features"] if f["owner"] == product["id"]]
        assert product["defaultRoute"] in {f["route"] for f in features}
        assert len({f["route"] for f in features}) == len(features)
        capability = json.loads((root / entry["appDir"] / "src-tauri/capabilities/default.json").read_text())
        assert capability["windows"] == ["main"]
        assert "remote" not in capability
        expected_permissions = {"core:default", "product-shell:allow-describe", "product-shell:allow-route-status", "suite:allow-connection"}
        if product["id"] == "workspace":
            expected_permissions.add("workspace:allow-execute")
        if product["id"] == "api-studio":
            expected_permissions.add("api-studio:allow-execute")
        if product["id"] == "knowledge":
            expected_permissions.add("knowledge:allow-execute")
        if product["id"] == "control-center":
            expected_permissions.update({"commands:allow-command-search", "commands:allow-command-preview", "commands:allow-command-source", "commands:allow-command-cancel", "commands:allow-command-open", "commands:allow-command-status", "commands:allow-command-preferences", "commands:allow-command-shortcut", "commands:allow-command-trigger-shortcut"})
        assert set(capability["permissions"]) == expected_permissions
        capability_dir = root / entry["appDir"] / "src-tauri/capabilities"
        expected_files = {"default.json"}
        if product["id"] == "workspace":
            expected_files.update({"terminal.json", "terminal-export.json"})
            terminal = json.loads((capability_dir / "terminal.json").read_text())
            assert terminal["windows"] == ["terminal-*"]
            assert "remote" not in terminal and not terminal.get("webviews")
            assert set(terminal["permissions"]) == {
                "core:default", "workspace:allow-terminal-describe",
                "workspace:allow-terminal-execute", "clipboard-manager:allow-read-text",
                "opener:allow-open-url",
            }
            exporter = json.loads((capability_dir / "terminal-export.json").read_text())
            assert exporter["windows"] == ["legacy-terminal-export"]
            assert "remote" not in exporter and not exporter.get("webviews")
            assert exporter["permissions"] == ["workspace:allow-terminal-export-message"]
        if product["id"] == "api-studio":
            expected_files.add("legacy-export.json")
            exporter = json.loads((capability_dir / "legacy-export.json").read_text())
            assert exporter["windows"] == ["legacy-api-export"]
            assert "remote" not in exporter and not exporter.get("webviews")
            assert exporter["permissions"] == ["api-studio:allow-legacy-export-message"]
        assert {path.name for path in capability_dir.glob("*.json")} == expected_files
    ids = set()
    for feature in parity["features"]:
        assert feature["legacyFeatureId"] not in ids, feature["legacyFeatureId"]
        ids.add(feature["legacyFeatureId"])
        path = Path(feature["sourcePath"])
        assert not path.is_absolute() and ".." not in path.parts
        assert (root / path).is_file(), path
        assert feature["product"] in PRODUCTS
        assert any(f["owner"] == feature["product"] and f["route"] == feature["route"] for f in catalog["features"])
        assert feature["ownerIssue"] in range(544, 551)
        assert feature["status"] in {"pending", "verified"}
        if feature.get("implementationPath"):
            implementation = Path(feature["implementationPath"])
            assert not implementation.is_absolute() and ".." not in implementation.parts
            assert (root / implementation).is_file()
        if feature.get("test") is not None:
            assert isinstance(feature["test"], list) and feature["test"], "mapped tests must name actual files"
            for test in feature["test"]:
                test_path = Path(test)
                assert not test_path.is_absolute() and ".." not in test_path.parts
                assert (root / test_path).is_file(), test_path
        if feature["status"] == "verified":
            assert feature["test"] and feature.get("implementationPath") and has_verified_source(feature)
    print(f"Product foundation metadata: 4 hidden products, {len(ids)} parity entries; "
          f"{sum(f['status'] == 'pending' for f in parity['features'])} still pending. This is not feature parity acceptance.")


if __name__ == "__main__":
    check()
