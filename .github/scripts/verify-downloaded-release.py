#!/usr/bin/env python3
"""Independently verify a downloaded devbox release without mutating it."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import sys


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--assets", required=True, type=pathlib.Path)
    parser.add_argument("--release", required=True, type=pathlib.Path)
    parser.add_argument("--config", required=True, type=pathlib.Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--draft", required=True, choices=("true", "false"))
    parser.add_argument("--prerelease", required=True, choices=("true", "false"))
    arguments = parser.parse_args()

    assets_directory = arguments.assets.resolve(strict=True)
    release = json.loads(arguments.release.read_text(encoding="utf-8"))
    config = json.loads(arguments.config.read_text(encoding="utf-8"))
    manifest_path = assets_directory / "release-manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    failures: list[str] = []

    if release.get("tagName") != arguments.tag:
        failures.append("release tag mismatch")
    expected_prerelease = arguments.prerelease == "true"
    expected_draft = arguments.draft == "true"
    if release.get("isDraft") is not expected_draft or release.get("isPrerelease") is not expected_prerelease:
        failures.append("release draft/prerelease state mismatch")
    if release.get("targetCommit") != arguments.commit:
        failures.append("release target commit mismatch")
    if manifest.get("releaseTag") != arguments.tag:
        failures.append("manifest release tag mismatch")
    if (
        set(manifest) != {"schemaVersion", "releaseTag", "generatedAt", "apps", "notices"}
        or manifest.get("schemaVersion") != 1
        or not isinstance(manifest.get("generatedAt"), str)
        or re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", manifest["generatedAt"]) is None
    ):
        failures.append("manifest envelope or schema mismatch")

    apps = manifest.get("apps", [])
    if not isinstance(apps, list):
        failures.append("manifest applications must be an array")
        apps = []
    if len(apps) != 15 or len({app.get("id") for app in apps if isinstance(app, dict)}) != 15:
        failures.append("manifest application count or identity mismatch")
    configured_apps = config.get("apps", [])
    if not isinstance(configured_apps, list):
        failures.append("acceptance config applications must be an array")
        configured_apps = []
    configured_versions = {
        app.get("id"): app.get("version") for app in configured_apps if isinstance(app, dict)
    }
    manifest_versions = {app.get("id"): app.get("version") for app in apps if isinstance(app, dict)}
    if (
        set(config) != {"schemaVersion", "apps"}
        or config.get("schemaVersion") != 1
        or len(configured_apps) != 15
        or len(configured_versions) != 15
        or configured_versions != manifest_versions
    ):
        failures.append("manifest application identities or versions differ from acceptance config")

    expected: dict[str, dict] = {}
    for app in apps:
        if not isinstance(app, dict):
            failures.append("manifest application entry is not an object")
            continue
        if set(app) != {"id", "version", "portable", "installer"}:
            failures.append("manifest application shape mismatch")
        app_id = app.get("id", "")
        version = app.get("version", "")
        if not re.fullmatch(r"[a-z0-9-]+", app_id):
            failures.append("invalid application id")
        if not re.fullmatch(r"\d+\.\d+\.\d+", version):
            failures.append(f"invalid version: {app_id}")
        for kind in ("portable", "installer"):
            item = app.get(kind, {})
            if not isinstance(item, dict):
                failures.append(f"invalid {kind} manifest entry: {app_id}")
                continue
            if set(item) != {"name", "size", "sha256"}:
                failures.append(f"invalid {kind} manifest shape: {app_id}")
            name = item.get("name", "")
            if name in expected:
                failures.append(f"duplicate manifest asset: {name}")
            expected[name] = item
        if app.get("portable", {}).get("name") != f"{app_id}.exe":
            failures.append(f"portable name mismatch: {app_id}")
        if app.get("installer", {}).get("name") != f"{app_id}_{version}_x64-setup.exe":
            failures.append(f"installer name mismatch: {app_id}")

    notices = manifest.get("notices", {})
    if not isinstance(notices, dict):
        failures.append("notices manifest entry is not an object")
        notices = {}
    if set(notices) != {"name", "size", "sha256"}:
        failures.append("notices manifest shape mismatch")
    if notices.get("name") != "THIRD_PARTY_NOTICES.md":
        failures.append("notices manifest entry mismatch")
    expected[notices.get("name", "")] = notices
    if len(expected) != 31:
        failures.append("manifest-declared asset count mismatch")

    downloaded_names = {item.name for item in assets_directory.iterdir() if item.is_file()}
    required_names = set(expected) | {"release-manifest.json"}
    missing = sorted(required_names - downloaded_names)
    undeclared = sorted(downloaded_names - required_names)
    if missing:
        failures.append(f"missing assets: {len(missing)}")
    if undeclared:
        failures.append(f"undeclared assets: {len(undeclared)}")
    if len(downloaded_names) != 32:
        failures.append("downloaded release asset count mismatch")

    local: dict[str, dict] = {}
    for name, item in expected.items():
        candidate = assets_directory / name
        if not candidate.is_file():
            continue
        size = candidate.stat().st_size
        digest = sha256(candidate)
        local[name] = {"size": size, "sha256": digest}
        if size != item.get("size"):
            failures.append(f"size mismatch: {name}")
        if digest != item.get("sha256"):
            failures.append(f"sha256 mismatch: {name}")

    manifest_identity = {
        "size": manifest_path.stat().st_size,
        "sha256": sha256(manifest_path),
    }
    local["release-manifest.json"] = manifest_identity

    remote_assets = release.get("assets", [])
    if not isinstance(remote_assets, list):
        failures.append("GitHub release assets must be an array")
        remote_assets = []
    remote = {asset.get("name"): asset for asset in remote_assets if isinstance(asset, dict)}
    if len(remote_assets) != 32 or len(remote) != 32:
        failures.append("GitHub release asset count or duplicate mismatch")
    if set(remote) != required_names:
        failures.append("GitHub release asset names mismatch")
    for name, local_identity in local.items():
        remote_identity = remote.get(name, {})
        if remote_identity.get("size") != local_identity["size"]:
            failures.append(f"GitHub size mismatch: {name}")
        remote_digest = remote_identity.get("digest")
        if remote_digest != f"sha256:{local_identity['sha256']}":
            failures.append(f"GitHub digest mismatch: {name}")

    result = {
        "schemaVersion": 1,
        "tag": arguments.tag,
        "commit": arguments.commit,
        "draft": expected_draft,
        "prerelease": expected_prerelease,
        "releaseAssets": len(remote_assets),
        "downloadedAssets": len(downloaded_names),
        "manifestApps": len(apps),
        "manifestDeclaredAssets": len(expected),
        "verifiedAssets": len(local),
        "configSha256": sha256(arguments.config),
        "missing": len(missing),
        "undeclared": len(undeclared),
        "failures": failures,
        "status": "PASS" if not failures else "FAIL",
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0 if not failures else 1


if __name__ == "__main__":
    sys.exit(main())
