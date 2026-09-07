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
print("Hidden product build/probe target and host boundary: PASS")
