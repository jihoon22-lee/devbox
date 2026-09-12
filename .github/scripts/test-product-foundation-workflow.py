#!/usr/bin/env python3
"""The product builds and native probe must share a cwd-independent target root."""
import re
import copy
import io
import json
import runpy
from contextlib import redirect_stdout
from unittest.mock import patch
from pathlib import Path, PureWindowsPath

root = Path(__file__).resolve().parents[2]
workflow = (root / ".github/workflows/product-foundation.yml").read_text()
match = re.search(r"^\s+CARGO_TARGET_DIR:\s*(.+)$", workflow, re.MULTILINE)
assert match, "hidden product builds must declare the shared Cargo target"
workspace = PureWindowsPath("D:/fixture/devbox")
target = PureWindowsPath(match.group(1).strip().replace("${{ github.workspace }}", str(workspace)))
assert target.is_absolute(), "pnpm --filter changes cwd; a relative Cargo target diverges per app"
for product in ("workspace", "api-studio", "knowledge", "control-center"):
    app_cwd = workspace / "apps" / f"devbox-{product}" / "src-tauri"
    assert app_cwd / target == workspace / "target"
probe = (root / ".github/scripts/windows-product-foundation.mjs").read_text()
assert 'path.resolve("target/debug"' in probe
assert 'process.env.RUNNER_ENVIRONMENT, "github-hosted"' in probe
assert 'process.env.GITHUB_ACTIONS, "true"' in probe
assert "contents: read" in workflow
installer = (root / ".github/scripts/windows-product-installation.ps1").read_text()
assert "$env:RUNNER_ENVIRONMENT -ne 'github-hosted'" in installer
assert "$env:GITHUB_ACTIONS -ne 'true'" in installer
assert "baseline manifest digest mismatch" in installer
assert "baseline tag commit mismatch" in installer
assert "product uninstall changed anchor installation or shortcut" in installer
assert "window-state-v1.json" in installer
assert "FindMainWindow($process.Id, $Title)" in installer
assert "owner != processId || !IsWindowVisible(window)" in installer
assert '. "$PSScriptRoot/windows-installer-helpers.ps1"' in installer
assert "windows-product-installation.ps1" in workflow
assert "Legacy performance baseline (Windows)" in workflow
assert "ref: 3a23f49c85aa3c3d04b86f227e8aa184ef964085" in workflow
performance = (root / ".github/scripts/windows-product-performance.ps1").read_text()
assert "$env:RUNNER_ENVIRONMENT -ne 'github-hosted'" in performance
assert "verify-downloaded-release.py" in performance
assert "baseline manifest digest mismatch" in performance
assert "--performance" in performance
print("Hidden product build/probe target and host boundary: PASS")

workflow_probe = (root / ".github/scripts/windows-api-workflow.mjs").read_text()
assert 'process.env.RUNNER_ENVIRONMENT, "github-hosted"' in workflow_probe
assert 'process.env.GITHUB_ACTIONS, "true"' in workflow_probe
assert "windows-api-workflow.mjs" in workflow
assert "windows-process-identity.mjs" in workflow
assert "Page.handleJavaScriptDialog" in workflow_probe
assert "captureMasked: true" in workflow_probe
assert "restartPreservesDraft: true" in workflow_probe

assert "-p api-playground -p webhook-lab -p developer-toolbox" in workflow, "B02 must execute the actual Windows domain regressions, not only compile dependencies"
assert "-p devbox-knowledge" in workflow, "B03 must execute Windows vault handle and migration regressions"
assert "windows-knowledge-migration.mjs" in workflow
assert workflow.index("- name: Verify Knowledge migration") < workflow.index("- name: Verify anchor and product installer coexistence"), "migration claims absent legacy profiles before installer coexistence creates them"
for source in ("packages/knowledge-features/**", "apps/knowledge-base/src-tauri/**", "apps/life-log/src-tauri/**", "apps/everything-plus/src-tauri/**"):
    assert source in workflow, "native Knowledge consumers require acceptance on source changes"

import subprocess
subprocess.run(["node", str(root / ".github/scripts/check-api-studio-routes.mjs"), "--self-test"], check=True)
subprocess.run(["node", str(root / ".github/scripts/check-knowledge-routes.mjs"), "--self-test"], check=True)

# Acceptance is an explicit evidence-bearing state, not a synonym for a mapped
# path. Mutate only in-memory metadata; source/capability files remain read-only.
check = runpy.run_path(str(root / ".github/scripts/check-product-foundation.py"))["check"]
parity_path = root / "apps/v0.8-feature-parity.json"
inventory_path = root / "apps/v0.8-data-inventory.json"
parity = json.loads(parity_path.read_text())
inventory = json.loads(inventory_path.read_text())
original_read = Path.read_text
def rejects_acceptance(change):
    changed_parity, changed_inventory = copy.deepcopy(parity), copy.deepcopy(inventory)
    change(changed_parity, changed_inventory)
    def read(path, *args, **kwargs):
        if path == parity_path:
            return json.dumps(changed_parity)
        if path == inventory_path:
            return json.dumps(changed_inventory)
        return original_read(path, *args, **kwargs)
    with patch.object(Path, "read_text", read), redirect_stdout(io.StringIO()):
        try:
            check(root)
        except AssertionError:
            return
    raise AssertionError("invalid acceptance metadata was accepted")

internal = next(index for index, feature in enumerate(parity["features"]) if feature["status"] == "verified")
imported = next(index for index, group in enumerate(inventory["groups"]) if group["importerStatus"] == "verified")
def unowned_provider(p, _):
    p["features"][internal].update(status="pending", producerVerified=True)
    p["features"][internal].pop("integrationOwnerIssue", None)
rejects_acceptance(lambda p, _: p["features"][internal].pop("verifiedSourceCommit"))
rejects_acceptance(lambda p, _: p["features"][internal].update(status="pending"))
rejects_acceptance(unowned_provider)
rejects_acceptance(lambda _, d: d["groups"][imported].update(evidence=[]))
rejects_acceptance(lambda _, d: d["groups"][imported].update(importerStatus="assumed"))
print("Unverified internal features, unowned provider handoffs and unevidenced imports are rejected: PASS")

for source in ("packages/workspace-features/**", "apps/workbench/src-tauri/**",
               "apps/repo-manager/src-tauri/**", "apps/code-pad/src-tauri/**",
               "apps/run-manager/src-tauri/**", "apps/log-lens/src-tauri/**",
               "apps/port-manager/src-tauri/**", "apps/wsl-desktop/src-tauri/**",
               ".github/scripts/copy-owned-terminal-profile.ps1"):
    assert source in workflow, "native Workspace consumers require acceptance on source changes"

for filename, changed_fields in [
    ("terminal.json", {"windows": ["*"]}),
    ("terminal.json", {"permissions": ["core:default", "workspace:allow-execute"]}),
    ("terminal-export.json", {"windows": ["main"]}),
    ("terminal-export.json", {"remote": {"urls": ["https://example.invalid"]}}),
]:
    capability_path = root / "apps/devbox-workspace/src-tauri/capabilities" / filename
    capability = json.loads(original_read(capability_path))
    capability.update(changed_fields)
    def read_capability(path, *args, **kwargs):
        return json.dumps(capability) if path == capability_path else original_read(path, *args, **kwargs)
    with patch.object(Path, "read_text", read_capability), redirect_stdout(io.StringIO()):
        try:
            check(root)
        except AssertionError:
            continue
    raise AssertionError("broadened Terminal capability was accepted")
print("Terminal companion/export capabilities remain separate and local: PASS")
