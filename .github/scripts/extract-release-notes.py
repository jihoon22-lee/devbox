#!/usr/bin/env python3
"""Write one non-empty release section from a changelog to stdout."""

from __future__ import annotations

import re
import sys
from pathlib import Path


RELEASE_HEADING = re.compile(r"^## \[([^\]\r\n]+)\](?:\s+-.*)?\s*$")


class MissingSectionError(Exception):
    """Raised when the requested release heading is not present."""


class EmptySectionError(Exception):
    """Raised when the requested release body has no non-whitespace content."""


def release_tag(line: str) -> str | None:
    """Return the exact tag from a release-level heading, if *line* is one."""

    match = RELEASE_HEADING.fullmatch(line.rstrip("\r\n"))
    return match.group(1) if match else None


def extract_section(changelog_path: Path, tag: str) -> str:
    """Return the requested heading's body, preserving its source text exactly."""

    with changelog_path.open("r", encoding="utf-8", newline="") as changelog:
        lines = changelog.readlines()

    found = False
    body: list[str] = []
    for line in lines:
        heading_tag = release_tag(line)
        if not found:
            if heading_tag == tag:
                found = True
            continue
        if heading_tag is not None:
            break
        body.append(line)

    if not found:
        raise MissingSectionError
    if not any(line.strip() for line in body):
        raise EmptySectionError
    return "".join(body)


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(f"usage: {argv[0]} <changelog-path> <release-tag>", file=sys.stderr)
        return 2

    changelog_path = Path(argv[1])
    tag = argv[2]
    if not tag.strip():
        print("release tag must not be empty or whitespace-only", file=sys.stderr)
        return 2

    try:
        section = extract_section(changelog_path, tag)
    except FileNotFoundError:
        print(
            f"changelog file not found: {changelog_path}; provide a readable changelog path",
            file=sys.stderr,
        )
        return 1
    except OSError as error:
        print(f"could not read changelog {changelog_path}: {error}", file=sys.stderr)
        return 1
    except MissingSectionError:
        print(
            f"release notes section for tag '{tag}' not found in {changelog_path}; "
            f"add a non-empty '## [{tag}]' section",
            file=sys.stderr,
        )
        return 1
    except EmptySectionError:
        print(
            f"release notes section for tag '{tag}' is empty or whitespace-only in "
            f"{changelog_path}; add release notes before creating the release",
            file=sys.stderr,
        )
        return 1

    sys.stdout.write(section)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
