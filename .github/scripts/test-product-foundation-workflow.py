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

# Windows domain/WAL/vault unit tests belong to the required CI Windows job.
# The native workflow must not run a duplicate copy of the same unit suite.
ci = (root / ".github/workflows/ci.yml").read_text()
windows_job = ci.split("  rust-windows:\n", 1)[1]
assert "runs-on: windows-latest" in windows_job
assert 'run-rust-scope.sh test "$RUST_SCOPE" "$RUST_PACKAGES"' in windows_job, "Windows domain regressions must execute when affected, not merely compile"
assert 'RUST_PACKAGES: ${{ needs.scope.outputs.rust_packages }}' in windows_job
assert "product-native-authority-" not in workflow, "do not duplicate CI's Windows unit suite"
assert "windows-knowledge-lifecycle.mjs" in workflow
for source in ("packages/knowledge-features/**", "crates/knowledge-vault-engine/**", "crates/activity-engine/**", "crates/content-index-engine/**"):
    assert source in workflow, "native Knowledge consumers require acceptance on source changes"

import subprocess
subprocess.run(["node", str(root / ".github/scripts/check-api-studio-routes.mjs"), "--self-test"], check=True)
subprocess.run(["node", str(root / ".github/scripts/check-knowledge-routes.mjs"), "--self-test"], check=True)

check = runpy.run_path(str(root / ".github/scripts/check-product-foundation.py"))["check"]
original_read = Path.read_text

for source in ("packages/workspace-features/**", "crates/projects-engine/**",
               "crates/repositories-engine/**", "crates/editor-engine/**",
               "crates/runtime-engine/**", "crates/logs-engine/**",
               "crates/ports-engine/**", "crates/terminal-engine/**",
               ".github/scripts/copy-owned-terminal-profile.ps1"):
    assert source in workflow, "native Workspace consumers require acceptance on source changes"

for filename, changed_fields in [
    ("terminal.json", {"windows": ["*"]}),
    ("terminal.json", {"permissions": ["core:default", "workspace:allow-execute"]}),
    ("terminal.json", {"remote": {"urls": ["https://example.invalid"]}}),
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
print("Terminal companion capabilities remain separate and local: PASS")
