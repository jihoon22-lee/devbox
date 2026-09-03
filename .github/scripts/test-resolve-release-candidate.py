#!/usr/bin/env python3
"""Unit tests for trusted Windows package candidate resolution."""

from __future__ import annotations

import importlib.util
import pathlib
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github/scripts/resolve-release-candidate.py"
SPEC = importlib.util.spec_from_file_location("resolve_release_candidate", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)

REPOSITORY = "jihoon22-lee/devbox"
COMMIT = "a" * 40
TAG = "v0.7.0"
ARTIFACT_NAME = f"windows-package-candidate-{TAG}-{COMMIT}"


def artifact(
    artifact_id: int,
    run_id: int,
    *,
    created_at: str,
    expired: bool = False,
    commit: str = COMMIT,
) -> dict[str, object]:
    return {
        "id": artifact_id,
        "name": ARTIFACT_NAME,
        "size_in_bytes": 1024,
        "digest": f"sha256:{'b' * 64}",
        "expired": expired,
        "created_at": created_at,
        "workflow_run": {
            "id": run_id,
            "repository_id": 123,
            "head_repository_id": 123,
            "head_branch": "main",
            "head_sha": commit,
        },
    }


def run(run_id: int, **overrides: object) -> dict[str, object]:
    payload: dict[str, object] = {
        "id": run_id,
        "path": MODULE.WORKFLOW_PATH,
        "event": "workflow_dispatch",
        "status": "completed",
        "conclusion": "success",
        "head_sha": COMMIT,
        "head_branch": "main",
        "head_repository": {"full_name": REPOSITORY},
        "run_attempt": 1,
    }
    payload.update(overrides)
    return payload


class ResolveReleaseCandidateTests(unittest.TestCase):
    def test_selects_newest_trusted_non_expired_candidate(self) -> None:
        artifacts = {
            "artifacts": [
                artifact(11, 101, created_at="2026-09-01T00:00:00Z"),
                artifact(12, 102, created_at="2026-09-02T00:00:00Z"),
                artifact(13, 103, created_at="2026-09-03T00:00:00Z", expired=True),
            ]
        }
        runs = {101: run(101), 102: run(102)}

        selected = MODULE.select_candidate(
            artifacts, REPOSITORY, COMMIT, TAG, runs.__getitem__
        )

        self.assertEqual(selected["run_id"], 102)
        self.assertEqual(selected["artifact_id"], 12)
        self.assertEqual(selected["artifact_name"], ARTIFACT_NAME)

    def test_skips_newer_artifact_when_its_run_is_not_trusted(self) -> None:
        artifacts = {
            "artifacts": [
                artifact(21, 201, created_at="2026-09-01T00:00:00Z"),
                artifact(22, 202, created_at="2026-09-02T00:00:00Z"),
            ]
        }
        runs = {
            201: run(201),
            202: run(202, conclusion="failure"),
        }

        selected = MODULE.select_candidate(
            artifacts, REPOSITORY, COMMIT, TAG, runs.__getitem__
        )

        self.assertEqual(selected["run_id"], 201)

    def test_rejects_wrong_source_workflow_repository_and_event(self) -> None:
        variants = (
            {"path": ".github/workflows/release.yml"},
            {"head_repository": {"full_name": "someone/fork"}},
            {"event": "pull_request"},
            {"head_sha": "b" * 40},
            {"head_branch": "feature"},
            {"status": "in_progress"},
        )
        for overrides in variants:
            with self.subTest(overrides=overrides):
                current_overrides = overrides
                artifacts = {
                    "artifacts": [artifact(31, 301, created_at="2026-09-03T00:00:00Z")]
                }
                with self.assertRaisesRegex(
                    MODULE.CandidateResolutionError, "no non-expired candidate"
                ):
                    MODULE.select_candidate(
                        artifacts,
                        REPOSITORY,
                        COMMIT,
                        TAG,
                        lambda run_id, trusted=current_overrides: run(
                            run_id, **trusted
                        ),
                    )

    def test_rejects_malformed_or_mismatched_artifacts(self) -> None:
        malformed = artifact(41, 401, created_at="2026-09-03T00:00:00Z")
        malformed["digest"] = "sha256:invalid"
        wrong_commit = artifact(
            42,
            402,
            created_at="2026-09-03T00:00:00Z",
            commit="b" * 40,
        )
        with self.assertRaisesRegex(
            MODULE.CandidateResolutionError, "no non-expired candidate"
        ):
            MODULE.select_candidate(
                {"artifacts": [malformed, wrong_commit]},
                REPOSITORY,
                COMMIT,
                TAG,
                lambda run_id: run(run_id),
            )

    def test_candidate_identity_is_strict(self) -> None:
        self.assertEqual(MODULE.artifact_name(TAG, COMMIT), ARTIFACT_NAME)
        for invalid_tag in ("0.7.0", "v01.7.0", "v0.7.0-rc1", "v0.7"):
            with (
                self.subTest(tag=invalid_tag),
                self.assertRaises(MODULE.CandidateResolutionError),
            ):
                MODULE.artifact_name(invalid_tag, COMMIT)
        with self.assertRaises(MODULE.CandidateResolutionError):
            MODULE.artifact_name(TAG, "A" * 40)


if __name__ == "__main__":
    unittest.main()
