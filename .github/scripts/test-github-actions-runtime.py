#!/usr/bin/env python3
"""Keep repository workflows on audited actions and the pinned Rust/Node toolchains."""

from __future__ import annotations

import pathlib
import re


ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOW_ROOT = ROOT / ".github/workflows"

toolchain = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
channel_match = re.search(r'^channel\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$', toolchain, re.MULTILINE)
assert channel_match, "rust-toolchain.toml must pin one exact Rust release"
RUST = channel_match.group(1)

NODE = (ROOT / ".nvmrc").read_text(encoding="utf-8").strip()
assert re.fullmatch(r"[0-9]+", NODE), ".nvmrc must pin one Node major version"

EXPECTED_REFERENCES = {
    "actions/checkout": "v7",
    "actions/setup-node": "v7",
    "actions/cache": "v6",
    "actions/cache/restore": "v6",
    "actions/cache/save": "v6",
    "actions/upload-artifact": "v7",
    "actions/download-artifact": "v8",
    "dtolnay/rust-toolchain": RUST,
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
    assert not re.search(r"^\s*node-version:", text, re.MULTILINE), \
        f"{workflow.name}: use node-version-file: .nvmrc instead of node-version"
    setup_node = len(re.findall(r"uses:\s*actions/setup-node@", text))
    node_files = len(re.findall(r"^\s*node-version-file:\s*\.nvmrc\s*$", text, re.MULTILINE))
    assert setup_node == node_files, f"{workflow.name}: every setup-node step must read .nvmrc"
    for version in re.findall(r'^\s*rust-version:\s*"?([0-9.]+)"?\s*$', text, re.MULTILINE):
        assert version == RUST, f"{workflow.name}: rust-version {version} must match rust-toolchain.toml {RUST}"
    if "pnpm install" in text:
        assert "corepack enable pnpm" in text, f"{workflow.name}: pnpm must use the repository packageManager pin"

assert all(count >= 1 for count in seen.values())
package = (ROOT / "package.json").read_text(encoding="utf-8")
assert '"packageManager": "pnpm@9.0.0"' in package
assert f'"node": ">={NODE} <{int(NODE) + 1}"' in package, "package.json engines.node must match .nvmrc"

print(f"GitHub Actions runtime policy: PASS (Rust {RUST}, Node {NODE}, pnpm 9 via Corepack)")
