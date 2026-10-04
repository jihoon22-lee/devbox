#!/usr/bin/env python3
"""Static fail-closed contract for stable candidate promotion."""

from __future__ import annotations

import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKFLOW = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
CANDIDATE_WORKFLOW = (
    ROOT / ".github/workflows/windows-package-candidate.yml"
).read_text(encoding="utf-8")

assert "build-prerelease-windows:" in WORKFLOW
assert "if: ${{ needs.preflight.outputs.prerelease == 'true' }}" in WORKFLOW
assert "build-windows:" not in WORKFLOW
assert "resolve-release-candidate.py" in WORKFLOW
assert "actions: read" in WORKFLOW
assert "github-token: ${{ github.token }}" in WORKFLOW
assert "repository: ${{ github.repository }}" in WORKFLOW
assert "run-id: ${{ steps.candidate.outputs.run_id }}" in WORKFLOW
assert "--artifact-kind candidate" in WORKFLOW
assert '--repository "$GITHUB_REPOSITORY"' in WORKFLOW
assert '--workflow-run "$CANDIDATE_RUN"' in WORKFLOW
assert "assets=(candidate/assets/*)" in WORKFLOW
assert "draft release requires exactly 7 staged assets" in WORKFLOW
assert WORKFLOW.index(
    "Independently verify candidate assets and provenance"
) < WORKFLOW.index("Atomically create a new draft release")
assert "needs.build-prerelease-windows.result == 'skipped'" in WORKFLOW
assert "needs.build-prerelease-windows.result == 'success'" in WORKFLOW

# A stable publication intentionally skips the prerelease builder. GitHub carries that
# skipped ancestor through the job graph, so the final verifier must replace the implicit
# success() guard and explicitly require the two jobs it consumes to have succeeded.
VERIFY_BLOCK = WORKFLOW.split("\n  verify:\n", maxsplit=1)[1]
assert "always() &&" in VERIFY_BLOCK
assert "needs.preflight.result == 'success'" in VERIFY_BLOCK
assert "needs.publish.result == 'success'" in VERIFY_BLOCK
assert VERIFY_BLOCK.index("always() &&") < VERIFY_BLOCK.index("runs-on: ubuntu-latest")

assert (
    "artifact_name=windows-package-candidate-$CANDIDATE_TAG-$CANDIDATE_COMMIT"
    in CANDIDATE_WORKFLOW
)
assert "retention-days: 14" in CANDIDATE_WORKFLOW

assert "published-runtime:" in WORKFLOW
assert "mode: smoke" in WORKFLOW
assert "./.github/workflows/windows-installer-acceptance.yml" in WORKFLOW
# GITHUB_ENV values become available only in subsequent Actions steps. The
# ownership-checking Git provisioner must not share the distro producer's step.
installer = CANDIDATE_WORKFLOW.split("\n  installer-acceptance:\n", 1)[1].split("\n  windows-wsl2:\n", 1)[0]
installer_steps = installer.split("\n      - ")
producer = next(i for i, step in enumerate(installer_steps) if "windows-knowledge-wsl.ps1" in step and "-Cleanup" not in step)
consumer = next(i for i, step in enumerate(installer_steps) if "windows-workspace-wsl-git.ps1" in step)
assert consumer > producer, "WSL fixture identity must cross an Actions step boundary before Git provisioning"

print("Stable candidate promotion workflow contract: PASS")
