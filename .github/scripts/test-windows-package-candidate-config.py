#!/usr/bin/env python3
"""Static fail-closed contract for the unpublished Windows package workflow."""

from __future__ import annotations

import pathlib


ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/windows-package-candidate.yml").read_text(encoding="utf-8")
BUILD_SCRIPT = (ROOT / ".github/scripts/build-windows-packages.ps1").read_text(encoding="utf-8")
FLATTEN_SCRIPT = (ROOT / ".github/scripts/flatten-windows-packages.ps1").read_text(encoding="utf-8")

assert WORKFLOW.startswith("name: Windows package candidate\n\non:\n  workflow_dispatch:\n")
assert "\n  pull_request:" not in WORKFLOW
assert "\n  push:" not in WORKFLOW
assert "permissions:\n  contents: read" in WORKFLOW
assert "cancel-in-progress: false" in WORKFLOW
assert "ref: ${{ inputs.candidate_commit }}" in WORKFLOW
assert "candidate_commit is not the exact current origin/main" in WORKFLOW
assert "candidate tag already exists" in WORKFLOW
assert "validate-release-input.py" in WORKFLOW
assert "build-windows-packages.ps1" in WORKFLOW
assert "build-manifest.py" in WORKFLOW
assert "flatten-windows-packages.ps1" in WORKFLOW
assert "build-candidate-metadata.py" in WORKFLOW
assert "--artifact-kind candidate" in WORKFLOW
assert "windows-installer-acceptance.ps1" in WORKFLOW
assert "stable baseline annotated tag identity mismatch" in WORKFLOW
assert "retention-days: 7" in WORKFLOW
assert "gh release create" not in WORKFLOW
assert "gh release edit" not in WORKFLOW
assert "gh release upload" not in WORKFLOW

assert "$apps.Count -ne 15" in BUILD_SCRIPT
assert "pnpm tauri build --bundles nsis" in BUILD_SCRIPT
assert "THIRD_PARTY_NOTICES.md" in BUILD_SCRIPT
assert "staging root must be absent or empty" in BUILD_SCRIPT
assert "Remove-Item" not in BUILD_SCRIPT

assert "$files.Count -ne 32" in FLATTEN_SCRIPT
assert "release-manifest.json" in FLATTEN_SCRIPT
assert "Remove-Item" not in FLATTEN_SCRIPT

print("Windows package candidate workflow contract: PASS")
