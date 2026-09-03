#!/usr/bin/env python3
"""Flatten a complete staged Windows package set after shard assembly."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import shutil
import sys

APP_ID_PATTERN = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")
INSTALLER_PATTERN = re.compile(
    r"(?P<app>[a-z0-9]+(?:-[a-z0-9]+)*)_(?P<version>\d+\.\d+\.\d+)_x64-setup\.exe"
)
EXPECTED_RELEASE_APPS = 15
EXPECTED_FILES = EXPECTED_RELEASE_APPS * 2 + 2


class FlattenError(ValueError):
    """Raised when staged package topology is incomplete or unsafe."""


def _regular_file(path: pathlib.Path, label: str) -> pathlib.Path:
    if path.is_symlink() or not path.is_file() or path.stat().st_size <= 0:
        raise FlattenError(f"{label} must be a non-empty regular file")
    return path


def _only_file(directory: pathlib.Path, expected_name: str, label: str) -> pathlib.Path:
    if directory.is_symlink() or not directory.is_dir():
        raise FlattenError(f"{label} directory is missing or linked")
    entries = list(directory.iterdir())
    if len(entries) != 1 or entries[0].name != expected_name:
        raise FlattenError(f"{label} staging contract mismatch")
    return _regular_file(entries[0], label)


def _release_ids(catalog: object) -> list[str]:
    if not isinstance(catalog, dict) or not isinstance(catalog.get("apps"), list):
        raise FlattenError("release catalog must contain an applications array")
    app_ids = []
    for entry in catalog["apps"]:
        if not isinstance(entry, dict) or not isinstance(entry.get("release"), bool):
            raise FlattenError("release catalog contains an invalid application entry")
        if not entry["release"]:
            continue
        app_id = entry.get("id")
        if not isinstance(app_id, str) or APP_ID_PATTERN.fullmatch(app_id) is None:
            raise FlattenError("release catalog contains an unsafe application id")
        app_ids.append(app_id)
    if len(app_ids) != EXPECTED_RELEASE_APPS or len(set(app_ids)) != len(app_ids):
        raise FlattenError(
            "release catalog must contain exactly 15 unique applications"
        )
    return app_ids


def _manifest_names(manifest: object, app_ids: list[str]) -> dict[str, tuple[str, str]]:
    if not isinstance(manifest, dict) or not isinstance(manifest.get("apps"), list):
        raise FlattenError("release manifest must contain an applications array")
    names: dict[str, tuple[str, str]] = {}
    for entry in manifest["apps"]:
        if not isinstance(entry, dict):
            raise FlattenError("release manifest contains an invalid application entry")
        app_id = entry.get("id")
        portable = entry.get("portable")
        installer = entry.get("installer")
        if (
            not isinstance(app_id, str)
            or app_id in names
            or not isinstance(portable, dict)
            or not isinstance(installer, dict)
        ):
            raise FlattenError("release manifest application identity is invalid")
        portable_name = portable.get("name")
        installer_name = installer.get("name")
        installer_match = (
            INSTALLER_PATTERN.fullmatch(installer_name)
            if isinstance(installer_name, str)
            else None
        )
        if (
            portable_name != f"{app_id}.exe"
            or installer_match is None
            or installer_match.group("app") != app_id
        ):
            raise FlattenError(f"release manifest package names are invalid: {app_id}")
        names[app_id] = (portable_name, installer_name)
    if list(names) != app_ids:
        raise FlattenError("release manifest applications differ from the catalog")
    notices = manifest.get("notices")
    if not isinstance(notices, dict) or notices.get("name") != "THIRD_PARTY_NOTICES.md":
        raise FlattenError("release manifest notices identity is invalid")
    return names


def flatten(
    staging: pathlib.Path, catalog_path: pathlib.Path, output: pathlib.Path
) -> None:
    if staging.is_symlink():
        raise FlattenError("staging root must be a real directory")
    staging = staging.resolve(strict=True)
    catalog_path = catalog_path.resolve(strict=True)
    output = output.resolve(strict=False)
    if not staging.is_dir():
        raise FlattenError("staging root must be a real directory")
    if (
        output.exists()
        or staging == output
        or staging in output.parents
        or output in staging.parents
    ):
        raise FlattenError("flat output must be absent and separate from staging")

    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    app_ids = _release_ids(catalog)
    manifest_path = _regular_file(staging / "release-manifest.json", "release manifest")
    notices_path = _regular_file(staging / "THIRD_PARTY_NOTICES.md", "notices")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    package_names = _manifest_names(manifest, app_ids)

    expected_topology = {*app_ids, "THIRD_PARTY_NOTICES.md", "release-manifest.json"}
    if {entry.name for entry in staging.iterdir()} != expected_topology:
        raise FlattenError("staging root contains missing or undeclared entries")

    sources: list[pathlib.Path] = []
    for app_id in app_ids:
        app_directory = staging / app_id
        if app_directory.is_symlink() or not app_directory.is_dir():
            raise FlattenError(
                f"application staging directory is missing or linked: {app_id}"
            )
        if {entry.name for entry in app_directory.iterdir()} != {
            "portable",
            "installer",
        }:
            raise FlattenError(f"application staging topology mismatch: {app_id}")
        portable_name, installer_name = package_names[app_id]
        sources.append(
            _only_file(app_directory / "portable", portable_name, f"portable: {app_id}")
        )
        sources.append(
            _only_file(
                app_directory / "installer", installer_name, f"installer: {app_id}"
            )
        )
    sources.extend((notices_path, manifest_path))
    if (
        len(sources) != EXPECTED_FILES
        or len({source.name for source in sources}) != EXPECTED_FILES
    ):
        raise FlattenError("flat package source names are incomplete or duplicated")

    output.mkdir(parents=True)
    for source in sources:
        shutil.copyfile(source, output / source.name)
    entries = list(output.iterdir())
    if len(entries) != EXPECTED_FILES or any(
        entry.is_symlink() or not entry.is_file() or entry.stat().st_size <= 0
        for entry in entries
    ):
        raise FlattenError(
            "flat package set must contain exactly 32 non-empty regular files"
        )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--staging", required=True, type=pathlib.Path)
    parser.add_argument("--catalog", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    arguments = parser.parse_args(argv)
    try:
        flatten(arguments.staging, arguments.catalog, arguments.output)
    except (OSError, json.JSONDecodeError, FlattenError) as error:
        print(f"Windows package flatten rejected: {error}", file=sys.stderr)
        return 1
    print(f"Flattened exactly {EXPECTED_FILES} candidate assets in {arguments.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
