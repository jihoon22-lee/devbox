#!/usr/bin/env python3
"""Validate the release workflow's tag and prerelease authorization policy.

Stable releases may be started by an annotated ``vMAJOR.MINOR.PATCH`` tag push
or by an explicit manual dispatch.  A prerelease is intentionally narrower: it
must be an exact SemVer tag supplied to ``workflow_dispatch`` together with the
explicit ``allow_prerelease=true`` gate.  Keeping this policy in one small
dependency-free script makes the pre-build boundary testable and fail closed.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
from dataclasses import dataclass


# Release tags deliberately exclude build metadata.  The tag is the exact
# release identity, so accepting a lossy or ambiguous spelling here would make
# the later remote-tag and changelog checks refer to a different release.
_NUMERIC_IDENTIFIER = r"(?:0|[1-9][0-9]*)"
_ALPHANUMERIC_IDENTIFIER = r"(?:[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)"
_PRERELEASE_IDENTIFIER = rf"(?:{_NUMERIC_IDENTIFIER}|{_ALPHANUMERIC_IDENTIFIER})"
TAG_PATTERN = re.compile(
    rf"\Av(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
    rf"(?:-{_PRERELEASE_IDENTIFIER}(?:\.{_PRERELEASE_IDENTIFIER})*)?\Z"
)


class ReleaseInputError(ValueError):
    """Raised when a release workflow input violates the repository policy."""


@dataclass(frozen=True)
class ReleaseInput:
    """Validated release identity and publication state."""

    tag: str
    prerelease: bool
    make_latest: bool


def _parse_gate(value: str | bool) -> bool:
    """Parse only the exact values emitted by a GitHub boolean input."""

    if isinstance(value, bool):
        return value
    if value == "true":
        return True
    if value == "false":
        return False
    raise ReleaseInputError(
        "allow_prerelease must be exactly 'true' or 'false' (default: false)"
    )


def validate_release_input(
    event_name: str, tag: str, allow_prerelease: str | bool
) -> ReleaseInput:
    """Validate a release trigger before any build or release mutation.

    ``tag`` is always the exact value that the workflow will use.  No next-RC
    calculation, tag normalization, or implicit prerelease mode is performed.
    """

    if event_name not in {"push", "workflow_dispatch"}:
        raise ReleaseInputError(f"unsupported release event: {event_name!r}")
    if not isinstance(tag, str) or TAG_PATTERN.fullmatch(tag) is None:
        raise ReleaseInputError(
            "release tag must be an exact vMAJOR.MINOR.PATCH SemVer tag "
            "with an optional non-empty prerelease identifier"
        )

    gate = _parse_gate(allow_prerelease)
    prerelease = "-" in tag
    if prerelease:
        if event_name != "workflow_dispatch":
            raise ReleaseInputError(
                f"prerelease tag {tag} is rejected from {event_name}; "
                "run workflow_dispatch with the exact version and "
                "allow_prerelease=true"
            )
        if not gate:
            raise ReleaseInputError(
                f"prerelease tag {tag} requires the explicit "
                "workflow_dispatch input allow_prerelease=true"
            )

    return ReleaseInput(tag=tag, prerelease=prerelease, make_latest=not prerelease)


def _write_github_output(path: pathlib.Path, release: ReleaseInput) -> None:
    """Append validated outputs for a single GitHub Actions step."""

    with path.open("a", encoding="utf-8", newline="\n") as output:
        output.write(f"tag={release.tag}\n")
        output.write(f"prerelease={str(release.prerelease).lower()}\n")
        output.write(f"make_latest={str(release.make_latest).lower()}\n")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--event",
        required=True,
        help="GitHub event name (push or workflow_dispatch)",
    )
    parser.add_argument("--tag", required=True, help="Exact release tag")
    parser.add_argument(
        "--allow-prerelease",
        required=True,
        choices=("true", "false"),
        help="Explicit workflow_dispatch prerelease gate; defaults to false",
    )
    parser.add_argument(
        "--github-output",
        type=pathlib.Path,
        help="Append validated tag/state outputs to this GitHub Actions file",
    )
    arguments = parser.parse_args(argv)

    try:
        release = validate_release_input(
            arguments.event, arguments.tag, arguments.allow_prerelease
        )
    except ReleaseInputError as error:
        print(f"release input rejected: {error}", file=sys.stderr)
        return 1

    if arguments.github_output is not None:
        _write_github_output(arguments.github_output, release)
    print(
        "release input accepted: "
        f"tag={release.tag} prerelease={str(release.prerelease).lower()} "
        f"make_latest={str(release.make_latest).lower()}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
