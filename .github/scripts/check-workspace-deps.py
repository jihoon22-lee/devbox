#!/usr/bin/env python3
"""External crates declared in [workspace.dependencies] must be used through workspace = true."""
from __future__ import annotations
import pathlib, sys, tomllib

def sections(manifest: dict):
    yield from (manifest.get(key, {}) for key in ("dependencies", "dev-dependencies", "build-dependencies"))
    for target in manifest.get("target", {}).values():
        yield from (target.get(key, {}) for key in ("dependencies", "dev-dependencies", "build-dependencies"))

def violations(root: pathlib.Path) -> list[str]:
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]
    shared = set(workspace.get("dependencies", {}))
    found = []
    for member in workspace["members"]:
        path = root / member / "Cargo.toml"
        manifest = tomllib.loads(path.read_text())
        for deps in sections(manifest):
            for name, spec in deps.items():
                if isinstance(spec, dict) and "path" in spec:
                    continue
                if name not in shared:
                    continue
                if isinstance(spec, dict) and spec.get("workspace") is True:
                    continue
                found.append(f"{path.relative_to(root)}: {name} must use workspace = true")
    return found

def windows_versions(root: pathlib.Path) -> set[str]:
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    return {package["version"] for package in lock["package"] if package["name"] == "windows"}

if __name__ == "__main__":
    root = pathlib.Path(__file__).resolve().parents[2]
    problems = violations(root)
    if "0.58.0" in windows_versions(root):
        problems.append("Cargo.lock: windows 0.58 must not return")
    print("\n".join(problems) or "workspace dependencies OK")
    sys.exit(1 if problems else 0)
