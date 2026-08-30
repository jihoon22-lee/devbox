#!/usr/bin/env python3
"""Regression tests for unpublished Windows candidate metadata."""

from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github/scripts/build-candidate-metadata.py"
TAG = "v0.6.0"
COMMIT = "a" * 40


def invoke(assets: pathlib.Path, output: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--assets",
            str(assets),
            "--tag",
            TAG,
            "--commit",
            COMMIT,
            "--repository",
            "jihoon22-lee/devbox",
            "--workflow-run",
            "123456",
            "--output",
            str(output),
        ],
        check=False,
        capture_output=True,
        text=True,
    )


with tempfile.TemporaryDirectory() as directory:
    root = pathlib.Path(directory)
    assets = root / "assets"
    assets.mkdir()
    for index in range(31):
        (assets / f"asset-{index:02d}.bin").write_bytes(f"asset:{index}".encode())
    manifest = assets / "release-manifest.json"
    manifest.write_text("{}\n", encoding="utf-8")
    output = root / "evidence" / "candidate-metadata.json"

    passed = invoke(assets, output)
    assert passed.returncode == 0, passed.stdout + passed.stderr
    metadata = json.loads(output.read_text(encoding="utf-8"))
    assert metadata["artifactKind"] == "candidate"
    assert metadata["schemaVersion"] == 1
    assert metadata["tagName"] == TAG
    assert metadata["targetCommit"] == COMMIT
    assert metadata["isDraft"] is None
    assert metadata["isPrerelease"] is False
    assert metadata["workflowRun"] == 123456
    assert len(metadata["assets"]) == 32
    assert [item["name"] for item in metadata["assets"]] == sorted(
        item.name for item in assets.iterdir()
    )
    manifest_entry = next(item for item in metadata["assets"] if item["name"] == manifest.name)
    assert manifest_entry["digest"] == f"sha256:{hashlib.sha256(manifest.read_bytes()).hexdigest()}"

    (assets / "asset-00.bin").unlink()
    failed = invoke(assets, root / "evidence-2" / "candidate-metadata.json")
    assert failed.returncode != 0
    assert "exactly 32 unique regular files" in failed.stderr

print("candidate metadata tests: PASS")
