#!/usr/bin/env python3
"""The product builds and native probe must share a cwd-independent target root."""
import re
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
