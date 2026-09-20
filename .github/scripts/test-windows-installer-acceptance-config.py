#!/usr/bin/env python3
"""Keep the destructive Windows installer matrix pinned to repository identities."""

from __future__ import annotations

import json
import pathlib
import re


ROOT = pathlib.Path(__file__).resolve().parents[2]
CONFIG_PATH = ROOT / ".github/scripts/legacy-v0.7-windows-installer-acceptance-config.json"
SMOKE_CONFIG_PATH = ROOT / ".github/scripts/legacy-v0.7-windows-packaged-smoke-config.json"
CATALOG_PATH = ROOT / "apps/legacy-v0.7-catalog.json"
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
    assert config["baseline"]["tag"] == "v0.6.0"
    assert config["baseline"]["commit"] == "d2fa25a0a1f087459838449daded00c0b09764b4"

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

        assert app["identifier"] == next(a["identifier"] for a in released if a["id"] == app_id)
        assert app["identifier"] == smoke_by_id[app_id]["identifier"]
        assert app["legacyIdentifiers"] == smoke_by_id[app_id]["legacyIdentifiers"]
        if app["baseline"]:
            baseline_ids.add(app_id)

    assert set(smoke_by_id) == {app["id"] for app in configured}
    assert {app["id"] for app in configured if not app["baseline"]} == EXPECTED_NEW_APPS
    assert len(baseline_ids) == 15

    script = SCRIPT_PATH.read_text(encoding="utf-8")
    assert '. "$PSScriptRoot/windows-installer-helpers.ps1"' in script
    script += (ROOT / ".github/scripts/windows-installer-helpers.ps1").read_text(encoding="utf-8")
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
    assert "function Resolve-Owned-Install-State" in script
    ownership_registration = "$script:ownedInstalls[$App.id] = $ownedState"
    installed_validation = (
        "$state = Resolve-Install-State $App $Release $ownedState $ExpectedBinarySha256"
    )
    assert ownership_registration in script
    assert installed_validation in script
    assert script.index(ownership_registration) < script.index(installed_validation)
    assert "installed executable digest mismatch" not in script
    assert "$binarySha -ne $manifestApp.portable.sha256" not in script
    assert "installed executable digest is not reproducible for this installer release" in script
    assert "candidate update did not replace the baseline executable" in script
    assert "$baselineVersion -cne $candidateVersion" in script
    assert "lifecycle = $lifecycle" in script
    assert "elseif ([bool]$app.baseline)" in script
    assert "Invoke-Owned-Process $State.Uninstaller '/S'" in script
    assert "_?=" not in script

    assert workflow.startswith("name: Windows installer acceptance\n\non:\n  workflow_dispatch:\n")
    assert "\n  pull_request:" not in workflow
    assert "\n  push:" not in workflow
    assert "workflow_call:" in workflow
    assert "persist-credentials: false" in workflow
    assert "runs-on: windows-2025" in workflow
    assert "cancel-in-progress: false" in workflow
    assert "if: ${{ always() }}" in workflow
    assert "verify-downloaded-release.py" in workflow
    assert "prepare-suite-runtime.py" in workflow and "--smoke-only" in workflow
    assert "prepare-suite-fixture.py" in workflow and "windows-suite-delivery.ps1" in workflow
    assert "gh release download" in workflow
    assert "cargo build" not in workflow and "tauri build" not in workflow

    print("windows installer acceptance config: PASS")


if __name__ == "__main__":
    main()
