#!/usr/bin/env python3
"""Check hidden product ownership, build identity and pending parity coverage."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PRODUCTS = {"workspace", "api-studio", "knowledge", "control-center"}


def check(root=ROOT):
    catalog = json.loads((root / "apps/products.json").read_text())
    legacy = json.loads((root / "apps/catalog.json").read_text())
    parity = json.loads((root / "apps/v0.8-feature-parity.json").read_text())
    assert catalog["schemaVersion"] == 3 and catalog["channel"] == "development"
    assert {p["id"] for p in catalog["products"]} == PRODUCTS
    assert len(catalog["products"]) == 4
    assert len([p for p in legacy["apps"] if p["release"]]) == 15
    assert {p["legacyApp"] for p in parity["apps"]} == {p["id"] for p in legacy["apps"] if p["release"]}
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
        assert set(capability["permissions"]) == {"core:default", "product-shell:allow-describe", "product-shell:allow-route-status"}
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
        if feature["status"] == "verified":
            assert feature["test"] and feature.get("implementationPath") and feature.get("evidence")
            assert (root / feature["implementationPath"]).is_file()
    print(f"Product foundation metadata: 4 hidden products, {len(ids)} parity entries; "
          f"{sum(f['status'] == 'pending' for f in parity['features'])} still pending. This is not feature parity acceptance.")


if __name__ == "__main__":
    check()
