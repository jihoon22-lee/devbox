#!/usr/bin/env python3
"""Build a deterministic bounded matrix from the release catalog."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys

APP_ID_PATTERN = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")
EXPECTED_RELEASE_APPS = 15
MIN_SHARDS = 2
MAX_SHARDS = 4


class ShardPlanError(ValueError):
    """Raised when a safe complete package shard plan cannot be produced."""


def release_app_ids(catalog: object) -> list[str]:
    if not isinstance(catalog, dict) or not isinstance(catalog.get("apps"), list):
        raise ShardPlanError("release catalog must contain an applications array")

    selected: list[str] = []
    for entry in catalog["apps"]:
        if not isinstance(entry, dict) or not isinstance(entry.get("release"), bool):
            raise ShardPlanError(
                "release catalog contains an invalid application entry"
            )
        if not entry["release"]:
            continue
        app_id = entry.get("id")
        if not isinstance(app_id, str) or APP_ID_PATTERN.fullmatch(app_id) is None:
            raise ShardPlanError("release catalog contains an unsafe application id")
        selected.append(app_id)

    if len(selected) != EXPECTED_RELEASE_APPS:
        raise ShardPlanError(
            f"release catalog must contain exactly {EXPECTED_RELEASE_APPS} applications"
        )
    if len(set(selected)) != len(selected):
        raise ShardPlanError("release catalog contains duplicate application ids")
    return selected


def build_matrix(app_ids: list[str], shard_count: int) -> dict[str, object]:
    if not MIN_SHARDS <= shard_count <= MAX_SHARDS:
        raise ShardPlanError(
            f"package shard count must be between {MIN_SHARDS} and {MAX_SHARDS}"
        )
    if len(app_ids) < shard_count:
        raise ShardPlanError("package shard count exceeds the application count")
    if any(
        not isinstance(app_id, str) or APP_ID_PATTERN.fullmatch(app_id) is None
        for app_id in app_ids
    ) or len(set(app_ids)) != len(app_ids):
        raise ShardPlanError("package shard input contains unsafe or duplicate ids")

    shards: list[list[str]] = [[] for _ in range(shard_count)]
    for index, app_id in enumerate(app_ids):
        shards[index % shard_count].append(app_id)

    sizes = [len(shard) for shard in shards]
    if not sizes or min(sizes) == 0 or max(sizes) - min(sizes) > 1:
        raise ShardPlanError("package shard plan is empty or unbalanced")
    flattened = [app_id for shard in shards for app_id in shard]
    if len(flattened) != len(app_ids) or set(flattened) != set(app_ids):
        raise ShardPlanError(
            "package shard plan does not cover every application exactly once"
        )

    return {
        "include": [
            {
                "shard": f"{index + 1:02d}",
                "apps": ",".join(shard),
                "app_count": len(shard),
            }
            for index, shard in enumerate(shards)
        ]
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", required=True, type=pathlib.Path)
    parser.add_argument("--shards", required=True, type=int)
    parser.add_argument("--github-output", required=True, type=pathlib.Path)
    arguments = parser.parse_args(argv)

    try:
        catalog = json.loads(arguments.catalog.read_text(encoding="utf-8"))
        app_ids = release_app_ids(catalog)
        matrix = build_matrix(app_ids, arguments.shards)
    except (OSError, json.JSONDecodeError, ShardPlanError) as error:
        print(f"Windows package shard plan rejected: {error}", file=sys.stderr)
        return 1

    compact_matrix = json.dumps(matrix, ensure_ascii=True, separators=(",", ":"))
    with arguments.github_output.open("a", encoding="utf-8", newline="\n") as output:
        output.write(f"matrix={compact_matrix}\n")
        output.write(f"shard_count={len(matrix['include'])}\n")
        output.write(f"app_count={len(app_ids)}\n")

    for shard in matrix["include"]:
        print(f"shard {shard['shard']}: {shard['apps']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
