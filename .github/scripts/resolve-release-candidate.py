#!/usr/bin/env python3
"""Resolve a trusted, successful Windows package candidate for a release."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
from collections.abc import Callable

WORKFLOW_PATH = ".github/workflows/windows-package-candidate.yml"
STABLE_TAG_PATTERN = re.compile(
    r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
)
REPOSITORY_PATTERN = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+")
COMMIT_PATTERN = re.compile(r"[0-9a-f]{40}")
DIGEST_PATTERN = re.compile(r"sha256:[0-9a-f]{64}")
TIMESTAMP_PATTERN = re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z")


class CandidateResolutionError(RuntimeError):
    """Raised when no artifact can satisfy the trusted candidate contract."""


def _positive_integer(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value > 0


def artifact_name(tag: str, commit: str) -> str:
    """Return the immutable artifact identity shared with the candidate workflow."""

    if STABLE_TAG_PATTERN.fullmatch(tag) is None:
        raise CandidateResolutionError(
            "candidate promotion requires a stable SemVer tag"
        )
    if COMMIT_PATTERN.fullmatch(commit) is None:
        raise CandidateResolutionError(
            "candidate commit must be 40 lowercase hexadecimal characters"
        )
    return f"windows-package-candidate-{tag}-{commit}"


def _trusted_run(run: object, repository: str, commit: str, run_id: int) -> bool:
    if not isinstance(run, dict):
        return False
    head_repository = run.get("head_repository")
    return (
        run.get("id") == run_id
        and run.get("path") == WORKFLOW_PATH
        and run.get("event") == "workflow_dispatch"
        and run.get("status") == "completed"
        and run.get("conclusion") == "success"
        and run.get("head_sha") == commit
        and run.get("head_branch") == "main"
        and isinstance(head_repository, dict)
        and head_repository.get("full_name") == repository
        and _positive_integer(run.get("run_attempt"))
    )


def select_candidate(
    artifacts_response: object,
    repository: str,
    commit: str,
    tag: str,
    fetch_run: Callable[[int], object],
) -> dict[str, object]:
    """Select the newest non-expired artifact whose complete run is trusted."""

    if REPOSITORY_PATTERN.fullmatch(repository) is None:
        raise CandidateResolutionError("candidate repository is invalid")
    expected_name = artifact_name(tag, commit)
    if not isinstance(artifacts_response, dict):
        raise CandidateResolutionError("GitHub artifact response must be an object")
    artifacts = artifacts_response.get("artifacts")
    if not isinstance(artifacts, list):
        raise CandidateResolutionError("GitHub artifact response is missing artifacts")

    candidates: list[tuple[str, int, dict[str, object], int]] = []
    for candidate in artifacts:
        if not isinstance(candidate, dict):
            continue
        workflow_run = candidate.get("workflow_run")
        if not isinstance(workflow_run, dict):
            continue
        artifact_id = candidate.get("id")
        run_id = workflow_run.get("id")
        created_at = candidate.get("created_at")
        if (
            candidate.get("name") != expected_name
            or candidate.get("expired") is not False
            or not _positive_integer(artifact_id)
            or not _positive_integer(candidate.get("size_in_bytes"))
            or not isinstance(candidate.get("digest"), str)
            or DIGEST_PATTERN.fullmatch(candidate["digest"]) is None
            or not isinstance(created_at, str)
            or TIMESTAMP_PATTERN.fullmatch(created_at) is None
            or not _positive_integer(run_id)
            or workflow_run.get("head_sha") != commit
            or workflow_run.get("head_branch") != "main"
            or workflow_run.get("repository_id")
            != workflow_run.get("head_repository_id")
            or not _positive_integer(workflow_run.get("repository_id"))
        ):
            continue
        candidates.append((created_at, artifact_id, candidate, run_id))

    for _, _, candidate, run_id in sorted(
        candidates, key=lambda item: (item[0], item[1]), reverse=True
    ):
        run = fetch_run(run_id)
        if _trusted_run(run, repository, commit, run_id):
            return {
                "artifact_id": candidate["id"],
                "artifact_name": expected_name,
                "artifact_digest": candidate["digest"],
                "run_id": run_id,
                "run_url": f"https://github.com/{repository}/actions/runs/{run_id}",
            }

    raise CandidateResolutionError(
        "no non-expired candidate from a successful trusted workflow run matches the tag and commit"
    )


def _gh_api(endpoint: str, fields: tuple[str, ...] = ()) -> object:
    command = ["gh", "api", "--method", "GET", endpoint]
    for field in fields:
        command.extend(("-f", field))
    completed = subprocess.run(command, check=False, capture_output=True, text=True)
    if completed.returncode != 0:
        raise CandidateResolutionError(f"GitHub API request failed for {endpoint}")
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise CandidateResolutionError(
            f"GitHub API returned invalid JSON for {endpoint}"
        ) from error


def _write_github_output(path: pathlib.Path, selected: dict[str, object]) -> None:
    with path.open("a", encoding="utf-8", newline="\n") as output:
        for key in (
            "run_id",
            "run_url",
            "artifact_id",
            "artifact_name",
            "artifact_digest",
        ):
            output.write(f"{key}={selected[key]}\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--github-output", required=True, type=pathlib.Path)
    arguments = parser.parse_args(argv)

    try:
        expected_name = artifact_name(arguments.tag, arguments.commit)
        artifacts = _gh_api(
            f"repos/{arguments.repository}/actions/artifacts",
            (f"name={expected_name}", "per_page=100"),
        )
        selected = select_candidate(
            artifacts,
            arguments.repository,
            arguments.commit,
            arguments.tag,
            lambda run_id: _gh_api(
                f"repos/{arguments.repository}/actions/runs/{run_id}"
            ),
        )
        _write_github_output(arguments.github_output, selected)
    except CandidateResolutionError as error:
        print(f"release candidate rejected: {error}", file=sys.stderr)
        return 1

    print(
        "release candidate accepted: "
        f"run={selected['run_id']} artifact={selected['artifact_name']} "
        f"digest={selected['artifact_digest']}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
