#!/usr/bin/env python3
"""Keep the packaged Windows acceptance matrix aligned with release sources."""

from __future__ import annotations

import json
import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG_PATH = ROOT / ".github/scripts/windows-packaged-smoke-config.json"
CATALOG_PATH = ROOT / "apps/catalog.json"


def read_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def source_text(directory: pathlib.Path, suffixes: set[str]) -> str:
    return "\n".join(
        path.read_text(encoding="utf-8", errors="ignore")
        for path in directory.rglob("*")
        if path.is_file() and path.suffix in suffixes
    )


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
        app_root = ROOT / catalog_app["appDir"]
        package = read_json(app_root / "package.json")
        tauri = read_json(app_root / "src-tauri/tauri.conf.json")
        windows = tauri.get("app", {}).get("windows", [])
        main_window = next((window for window in windows if window.get("label", "main") == "main"), None)

        if app.get("version") != package.get("version"):
            failures.append(f"{app_id}: packaged version differs from package.json")
        if app.get("identifier") != catalog_app.get("identifier") or app.get("identifier") != tauri.get("identifier"):
            failures.append(f"{app_id}: packaged identifier differs from catalog/Tauri")
        if main_window is None or app.get("title") != main_window.get("title"):
            failures.append(f"{app_id}: packaged title differs from the Tauri main window")

        expected_product_image = f"{tauri.get('productName')}.exe"
        process_names = app.get("additionalProcessNames", [])
        if expected_product_image not in process_names or len(process_names) != len(set(process_names)):
            failures.append(f"{app_id}: protected process names omit or duplicate the product image")
        if any(
            not isinstance(name, str)
            or re.fullmatch(r"[A-Za-z0-9 .+_-]+\.exe", name) is None
            or pathlib.PureWindowsPath(name).name != name
            for name in process_names
        ):
            failures.append(f"{app_id}: protected process name is unsafe")

        frontend = source_text(app_root / "src", {".ts", ".tsx", ".js", ".jsx", ".html"})
        markers = app.get("markers", [])
        if not markers or any(not isinstance(marker, str) or marker not in frontend for marker in markers):
            failures.append(f"{app_id}: a packaged UI marker is empty or absent from frontend source")

        rust = source_text(app_root / "src-tauri/src", {".rs"})
        probes = app.get("probes", [])
        if not probes:
            failures.append(f"{app_id}: at least one read-only packaged IPC probe is required")
        for probe in probes:
            command = probe.get("command") if isinstance(probe, dict) else None
            if not isinstance(command, str) or re.search(rf"\b{re.escape(command)}\b", rust) is None:
                failures.append(f"{app_id}: packaged IPC probe command is absent from Rust source")
        quit_command = app.get("quitCommand")
        if quit_command is not None and (
            not isinstance(quit_command, str)
            or re.search(rf"\b{re.escape(quit_command)}\b", rust) is None
        ):
            failures.append(f"{app_id}: orderly quit command is absent from Rust source")

    return failures


def main() -> int:
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
