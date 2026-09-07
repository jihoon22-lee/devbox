#!/usr/bin/env python3
"""Require every registered feature fixture/document and reject orphan fixtures."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def check(root=ROOT):
    registry = json.loads((root / "apps/product-feature-fixtures.json").read_text())
    catalog = json.loads((root / "apps/products.json").read_text())
    assert registry["schemaVersion"] == 1
    features = {f["id"]: f for f in catalog["features"]}
    seen = set()
    paths = set()
    for entry in registry["entries"]:
        feature_id = entry["featureId"]
        assert feature_id not in seen and feature_id in features
        seen.add(feature_id)
        assert entry["fixture"] == f"packages/product-shell/fixtures/features/{feature_id}.json"
        assert entry["document"] == f"docs/features/{feature_id}.md"
        paths.add(root / entry["fixture"])
        assert (root / entry["document"]).is_file()
        fixture = json.loads((root / entry["fixture"]).read_text())
        feature = features[feature_id]
        assert fixture == {"featureId": feature_id, "owner": feature["owner"],
                           "route": feature["route"], "authority": feature["authority"],
                           "availability": feature["status"]}
    assert paths == set((root / "packages/product-shell/fixtures/features").glob("*.json"))


if __name__ == "__main__":
    check()
    print("Product feature fixture registrations: PASS")
