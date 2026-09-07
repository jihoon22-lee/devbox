#!/usr/bin/env python3
"""Register one hidden shell route and its executable fixture/document stub."""
import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REGISTRY = "apps/product-feature-fixtures.json"


def encoded(value):
    return json.dumps(value, ensure_ascii=False, indent=2) + "\n"


def scaffold(root, owner, route, label):
    if not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,94}[a-z0-9])?", route):
        raise ValueError("route must be a bounded lowercase slug")
    if not label.strip() or len(label) > 96 or any(ord(c) < 32 for c in label):
        raise ValueError("label must be a single nonempty line, at most 96 characters")
    catalog_path, registry_path = root / "apps/products.json", root / REGISTRY
    original_catalog, original_registry = catalog_path.read_text(), registry_path.read_text()
    catalog, registry = json.loads(original_catalog), json.loads(original_registry)
    if owner not in {p["id"] for p in catalog["products"]}:
        raise ValueError("owner must be a registered product")
    feature = {"id": f"{owner}.{route}", "owner": owner, "route": route, "label": label,
               "component": f"{owner}.shell", "command": f"{owner}.open-{route}",
               "authority": "shell-read", "status": "foundation"}
    existing = next((f for f in catalog["features"] if f["id"] == feature["id"]), None)
    if existing is not None and existing != feature:
        raise ValueError("existing route differs; review its registration manually")
    if any(entry["featureId"] == feature["id"] for entry in registry["entries"]):
        raise ValueError("feature fixture is already registered")
    fixture = f"packages/product-shell/fixtures/features/{feature['id']}.json"
    document = f"docs/features/{feature['id']}.md"
    outputs = {
        fixture: encoded({"featureId": feature["id"], "owner": owner, "route": route,
                          "authority": "shell-read", "availability": "foundation"}),
        document: f"# {label}\n\nOwner: `{owner}` · route: `{route}` · status: foundation\n\n"
                  f"Native authority: `shell-read`; fixture: [{feature['id']}](../../{fixture}).\n\n"
                  "This registers a pending feature in the hidden development shell.\n"
                  "Domain implementation, data ownership, migration and acceptance remain pending.\n\n"
                  f"Windows native development: pass `--route={route}` to the debug product executable.\n"
                  f"Browser development in `apps/devbox-{owner}` accepts `?route={route}` and is\n"
                  "explicitly labelled synthetic. No legacy executable is dispatched.\n",
    }
    if any((root / relative).exists() for relative in outputs):
        raise ValueError("refusing to overwrite an existing fixture or document")
    if existing is None:
        catalog["features"].append(feature)
        catalog["catalogRevision"] += 1
    registry["entries"].append({"featureId": feature["id"], "fixture": fixture, "document": document})
    # Exclusive creates avoid overwriting another contributor's file. All checks
    # precede writes; restore owned files if a local write fails partway through.
    created = []
    try:
        for relative, content in outputs.items():
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open("x", encoding="utf-8") as stream:
                created.append(target)
                stream.write(content)
        catalog_path.write_text(encoded(catalog), encoding="utf-8")
        registry_path.write_text(encoded(registry), encoding="utf-8")
    except OSError:
        catalog_path.write_text(original_catalog, encoding="utf-8")
        registry_path.write_text(original_registry, encoding="utf-8")
        for target in created:
            target.unlink(missing_ok=True)
        raise
    return feature["id"]


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("owner")
    parser.add_argument("route")
    parser.add_argument("--label", required=True)
    args = parser.parse_args()
    try:
        print(f"Registered {scaffold(ROOT, args.owner, args.route, args.label)} (foundation only)")
    except (ValueError, OSError) as error:
        parser.exit(1, f"Feature scaffold failed: {error}\n")
