#!/usr/bin/env python3
"""Keep the destructive Windows installer matrix pinned to repository identities."""

from __future__ import annotations

import json
import pathlib
import re


ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG_PATH = ROOT / ".github/scripts/windows-installer-acceptance-config.json"
SMOKE_CONFIG_PATH = ROOT / ".github/scripts/windows-packaged-smoke-config.json"
CATALOG_PATH = ROOT / "apps/catalog.json"
EXPECTED_NEW_APPS: set[str] = set()
SCRIPT_PATH = ROOT / ".github/scripts/windows-installer-acceptance.ps1"
WORKFLOW_PATH = ROOT / ".github/workflows/windows-installer-acceptance.yml"


def load_json(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> None:
    config = load_json(CONFIG_PATH)
    smoke = load_json(SMOKE_CONFIG_PATH)
    catalog = load_json(CATALOG_PATH)

    assert set(config) == {"schemaVersion", "baseline", "apps"}
    assert config["schemaVersion"] == 1
    assert set(config["baseline"]) == {"tag", "commit"}
    assert config["baseline"]["tag"] == "v0.5.1"
    assert config["baseline"]["commit"] == "300cb158d1f0c23973857549a1aeddd9997c3f16"

    released = [app for app in catalog["apps"] if app["release"]]
    configured = config["apps"]
    assert [app["id"] for app in configured] == [app["id"] for app in released]
    assert len(configured) == 15
    assert len({app["id"] for app in configured}) == 15

    smoke_by_id = {app["id"]: app for app in smoke["apps"]}
    baseline_ids = set()
    for app in configured:
        assert set(app) == {
            "id",
            "productName",
            "binaryName",
            "identifier",
            "legacyIdentifiers",
            "baseline",
        }
        app_id = app["id"]
        assert re.fullmatch(r"[a-z0-9-]+", app_id)
        assert app["binaryName"] == f"{app_id}.exe"
        assert isinstance(app["baseline"], bool)
        assert len(app["legacyIdentifiers"]) == len(set(app["legacyIdentifiers"]))

        tauri = load_json(ROOT / f"apps/{app_id}/src-tauri/tauri.conf.json")
        assert app["productName"] == tauri["productName"]
        assert app["identifier"] == tauri["identifier"]
        assert app["identifier"] == smoke_by_id[app_id]["identifier"]
        assert app["legacyIdentifiers"] == smoke_by_id[app_id]["legacyIdentifiers"]
        if app["baseline"]:
            baseline_ids.add(app_id)

    assert set(smoke_by_id) == {app["id"] for app in configured}
    assert {app["id"] for app in configured if not app["baseline"]} == EXPECTED_NEW_APPS
    assert len(baseline_ids) == 15

    script = SCRIPT_PATH.read_text(encoding="utf-8")
    workflow = WORKFLOW_PATH.read_text(encoding="utf-8")
    for parameter in {
        "Config",
        "BaselineAssets",
        "BaselineMetadata",
        "CandidateAssets",
        "CandidateMetadata",
        "CandidateTag",
        "CandidateCommit",
        "Output",
        "ScratchRoot",
    }:
        assert f"${parameter}" in script
        assert f"-{parameter} " in workflow
    assert "$env:GITHUB_ACTIONS -ne 'true'" in script
    assert "Assert-Descendant $Output $ScratchRoot" in script
    assert "$baselineApps.Count -ne 15" in script
    assert "v0.4.2" not in script
    assert "Remove-Item -Recurse" not in script
    assert "Stop-Process -Name" not in script
    assert "Invoke-Expression" not in script
    assert "function Get-Optional-Property" in script
    assert "$Object.PSObject.Properties.Match($Name)" in script
    assert "$Object.PSObject.Properties[$Name]" not in script
    optional_registry_values = {
        "DisplayName",
        "DisplayVersion",
        "Publisher",
        "DisplayIcon",
        "InstallLocation",
        "UninstallString",
    }
    for property_name in optional_registry_values:
        assert f"Get-Optional-Property $value '{property_name}'" in script
        assert f"$value.{property_name}" not in script

    assert workflow.startswith("name: Windows installer acceptance\n\non:\n  workflow_dispatch:\n")
    assert "\n  pull_request:" not in workflow
    assert "\n  push:" not in workflow
    assert workflow.count("contents: read") == 2
    assert "persist-credentials: false" in workflow
    assert "runs-on: windows-2025" in workflow
    assert "cancel-in-progress: false" in workflow
    assert "if: ${{ always() }}" in workflow
    assert "status -cne 'PASS'" in workflow
    assert "schemaVersion -ne 1" in workflow
    assert "baseline lifecycle evidence is incomplete" in workflow
    assert "[int]$evidence.releases.baseline.assets -ne 32" in workflow
    assert "$baselineApps.Count -ne 15 -or $newApps.Count -ne 0" in workflow
    assert "registryKeyResidue" in workflow
    assert "cleanup or failure state is not clean" in workflow

    print("windows installer acceptance config: PASS")


if __name__ == "__main__":
    main()
