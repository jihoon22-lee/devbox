#!/usr/bin/env python3
"""Build immutable local metadata for an unpublished Windows package candidate."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
from datetime import datetime, timezone


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--assets", required=True, type=pathlib.Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--workflow-run", required=True, type=int)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    arguments = parser.parse_args()

    if re.fullmatch(r"v\d+\.\d+\.\d+", arguments.tag) is None:
        raise SystemExit("candidate tag must be a stable semver tag")
    if re.fullmatch(r"[0-9a-f]{40}", arguments.commit) is None:
        raise SystemExit("candidate commit must be 40 lowercase hexadecimal characters")
    if re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", arguments.repository) is None:
        raise SystemExit("candidate repository is invalid")
    if arguments.workflow_run <= 0:
        raise SystemExit("candidate workflow run must be positive")

    assets = arguments.assets.resolve(strict=True)
    output = arguments.output.resolve(strict=False)
    if output.parent == assets or assets in output.parents:
        raise SystemExit("candidate metadata must remain outside the flat asset directory")
    entries = list(assets.iterdir())
    files = sorted(entries, key=lambda item: item.name)
    if (
        len(files) != 32
        or len({item.name for item in files}) != 32
        or any(not item.is_file() or item.is_symlink() for item in files)
    ):
        raise SystemExit("candidate assets must contain exactly 32 unique regular files")
    if "release-manifest.json" not in {item.name for item in files}:
        raise SystemExit("candidate release manifest is missing")

    metadata = {
        "artifactKind": "candidate",
        "schemaVersion": 1,
        "tagName": arguments.tag,
        "targetCommit": arguments.commit,
        "isDraft": None,
        "isPrerelease": False,
        "repository": arguments.repository,
        "workflowRun": arguments.workflow_run,
        "generatedAt": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "assets": [
            {
                "name": item.name,
                "size": item.stat().st_size,
                "digest": f"sha256:{sha256(item)}",
            }
            for item in files
        ],
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(metadata, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"candidate metadata written: {output} ({len(files)} assets)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
