#!/usr/bin/env python3
"""Keep repository workflows off retired JavaScript action runtimes."""

from __future__ import annotations

import pathlib
import re


ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOW_ROOT = ROOT / ".github/workflows"
EXPECTED_REFERENCES = {
    "actions/checkout": "v7",
    "actions/setup-node": "v7",
    "actions/cache": "v6",
    "actions/upload-artifact": "v7",
    "actions/download-artifact": "v8",
    "dtolnay/rust-toolchain": "stable",
    "Swatinem/rust-cache": "v2",
    "EmbarkStudios/cargo-deny-action": "3c6349835b2b7b196a839186cb8b78e02f7b5f25",
}


workflows = sorted(WORKFLOW_ROOT.glob("*.yml"))
assert workflows
combined = "\n".join(path.read_text(encoding="utf-8") for path in workflows)
assert "pnpm/action-setup" not in combined

seen: dict[str, int] = {name: 0 for name in EXPECTED_REFERENCES}
for workflow in workflows:
    text = workflow.read_text(encoding="utf-8")
    for action, reference in re.findall(r"^\s*-?\s*uses:\s*([^@\s]+)@([^\s#]+)", text, re.MULTILINE):
        assert action in EXPECTED_REFERENCES, f"{workflow.name}: unaudited action reference: {action}@{reference}"
        expected = EXPECTED_REFERENCES[action]
        assert reference == expected, f"{workflow.name}: {action}@{reference} must be {action}@{expected}"
        seen[action] += 1
    if "pnpm install" in text:
        assert "corepack enable pnpm" in text, f"{workflow.name}: pnpm must use the repository packageManager pin"

assert all(count >= 1 for count in seen.values())
assert '"packageManager": "pnpm@9.0.0"' in (ROOT / "package.json").read_text(encoding="utf-8")

print("GitHub Actions runtime policy: PASS (audited official/Rust action references, pnpm 9 via Corepack)")
