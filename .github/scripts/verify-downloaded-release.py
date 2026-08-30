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


def is_safe_leaf(value: object) -> bool:
    return (
        isinstance(value, str)
        and value not in {"", ".", ".."}
        and "/" not in value
        and "\\" not in value
        and all(ord(character) >= 32 and ord(character) != 127 for character in value)
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--assets", required=True, type=pathlib.Path)
    parser.add_argument("--release", required=True, type=pathlib.Path)
    parser.add_argument("--config", required=True, type=pathlib.Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--artifact-kind", choices=("release", "candidate"), default="release")
    parser.add_argument("--draft", choices=("true", "false"))
    parser.add_argument("--prerelease", choices=("true", "false"))
    arguments = parser.parse_args()

    assets_directory = arguments.assets.resolve(strict=True)
    release = json.loads(arguments.release.read_text(encoding="utf-8"))
    config = json.loads(arguments.config.read_text(encoding="utf-8"))
    manifest_path = assets_directory / "release-manifest.json"
    if manifest_path.is_symlink():
        raise SystemExit("release manifest must not be a symbolic link")
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(release, dict):
        raise SystemExit("artifact metadata must be an object")
    if not isinstance(config, dict):
        raise SystemExit("acceptance config must be an object")
    if not isinstance(manifest, dict):
        raise SystemExit("release manifest must be an object")
    failures: list[str] = []

    if release.get("tagName") != arguments.tag:
        failures.append("artifact tag mismatch")
    if arguments.artifact_kind == "release":
        if arguments.draft is None or arguments.prerelease is None:
            raise SystemExit("release verification requires --draft and --prerelease")
        expected_prerelease = arguments.prerelease == "true"
        expected_draft: bool | None = arguments.draft == "true"
        if release.get("artifactKind") not in (None, "release"):
            failures.append("release metadata artifact kind mismatch")
        if release.get("isDraft") is not expected_draft or release.get("isPrerelease") is not expected_prerelease:
            failures.append("release draft/prerelease state mismatch")
    else:
        if arguments.draft is not None or arguments.prerelease is not None:
            raise SystemExit("candidate verification does not accept release publication flags")
        expected_prerelease = False
        expected_draft = None
        expected_candidate_fields = {
            "artifactKind",
            "schemaVersion",
            "tagName",
            "targetCommit",
            "isDraft",
            "isPrerelease",
            "repository",
            "workflowRun",
            "generatedAt",
            "assets",
        }
        if (
            set(release) != expected_candidate_fields
            or release.get("artifactKind") != "candidate"
            or release.get("schemaVersion") != 1
            or release.get("isDraft") is not None
            or release.get("isPrerelease") is not False
            or re.fullmatch(r"v\d+\.\d+\.\d+", arguments.tag) is None
            or not isinstance(release.get("repository"), str)
            or re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", release["repository"]) is None
            or not isinstance(release.get("workflowRun"), int)
            or isinstance(release.get("workflowRun"), bool)
            or release["workflowRun"] <= 0
            or not isinstance(release.get("generatedAt"), str)
            or re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", release["generatedAt"]) is None
        ):
            failures.append("candidate metadata envelope mismatch")
    if release.get("targetCommit") != arguments.commit:
        failures.append("artifact target commit mismatch")
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
    manifest_ids = [
        app.get("id") for app in apps if isinstance(app, dict) and isinstance(app.get("id"), str)
    ]
    if len(apps) != 15 or len(manifest_ids) != 15 or len(set(manifest_ids)) != 15:
        failures.append("manifest application count or identity mismatch")
    configured_apps = config.get("apps", [])
    if not isinstance(configured_apps, list):
        failures.append("acceptance config applications must be an array")
        configured_apps = []
    configured_versions = {
        app.get("id"): app.get("version")
        for app in configured_apps
        if isinstance(app, dict) and isinstance(app.get("id"), str)
    }
    manifest_versions = {
        app.get("id"): app.get("version")
        for app in apps
        if isinstance(app, dict) and isinstance(app.get("id"), str)
    }
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
        valid_app_id = isinstance(app_id, str) and re.fullmatch(r"[a-z0-9-]+", app_id) is not None
        valid_version = isinstance(version, str) and re.fullmatch(r"\d+\.\d+\.\d+", version) is not None
        if not valid_app_id:
            failures.append("invalid application id")
        if not valid_version:
            failures.append(f"invalid version: {app_id}")
        for kind in ("portable", "installer"):
            item = app.get(kind, {})
            if not isinstance(item, dict):
                failures.append(f"invalid {kind} manifest entry: {app_id}")
                continue
            if set(item) != {"name", "size", "sha256"}:
                failures.append(f"invalid {kind} manifest shape: {app_id}")
            name = item.get("name", "")
            if not is_safe_leaf(name):
                failures.append(f"unsafe {kind} manifest asset name: {app_id}")
                continue
            size = item.get("size")
            digest = item.get("sha256")
            if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
                failures.append(f"invalid {kind} manifest asset size: {app_id}")
            if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
                failures.append(f"invalid {kind} manifest asset digest: {app_id}")
            if name in expected:
                failures.append(f"duplicate manifest asset: {name}")
            else:
                expected[name] = item
            expected_name = (
                f"{app_id}.exe"
                if kind == "portable" and valid_app_id
                else f"{app_id}_{version}_x64-setup.exe"
                if kind == "installer" and valid_app_id and valid_version
                else None
            )
            if expected_name is not None and name != expected_name:
                failures.append(f"{kind} name mismatch: {app_id}")

    notices = manifest.get("notices", {})
    if not isinstance(notices, dict):
        failures.append("notices manifest entry is not an object")
        notices = {}
    if set(notices) != {"name", "size", "sha256"}:
        failures.append("notices manifest shape mismatch")
    notices_name = notices.get("name")
    if not is_safe_leaf(notices_name):
        failures.append("unsafe notices manifest asset name")
    elif notices_name != "THIRD_PARTY_NOTICES.md":
        failures.append("notices manifest entry mismatch")
    else:
        if notices_name in expected:
            failures.append(f"duplicate manifest asset: {notices_name}")
        else:
            expected[notices_name] = notices
    notices_size = notices.get("size")
    notices_digest = notices.get("sha256")
    if not isinstance(notices_size, int) or isinstance(notices_size, bool) or notices_size <= 0:
        failures.append("invalid notices manifest asset size")
    if not isinstance(notices_digest, str) or re.fullmatch(r"[0-9a-f]{64}", notices_digest) is None:
        failures.append("invalid notices manifest asset digest")
    if len(expected) != 31:
        failures.append("manifest-declared asset count mismatch")

    downloaded_files = [item for item in assets_directory.iterdir() if item.is_file()]
    downloaded_names = {item.name for item in downloaded_files}
    linked_names = sorted(item.name for item in downloaded_files if item.is_symlink())
    if linked_names:
        failures.append(f"downloaded assets contain symbolic links: {len(linked_names)}")
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
        if not candidate.is_file() or candidate.is_symlink():
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
        failures.append("artifact metadata assets must be an array")
        remote_assets = []
    remote: dict[str, dict] = {}
    for asset in remote_assets:
        if not isinstance(asset, dict):
            failures.append("artifact metadata asset is not an object")
            continue
        name = asset.get("name")
        if not is_safe_leaf(name):
            failures.append("artifact metadata contains an unsafe asset name")
            continue
        if name in remote:
            failures.append(f"artifact metadata contains a duplicate asset: {name}")
            continue
        remote[name] = asset
    if len(remote_assets) != 32 or len(remote) != 32:
        failures.append("artifact metadata asset count or duplicate mismatch")
    if set(remote) != required_names:
        failures.append("artifact metadata asset names mismatch")
    for name, local_identity in local.items():
        remote_identity = remote.get(name, {})
        if remote_identity.get("size") != local_identity["size"]:
            failures.append(f"artifact metadata size mismatch: {name}")
        remote_digest = remote_identity.get("digest")
        if remote_digest != f"sha256:{local_identity['sha256']}":
            failures.append(f"artifact metadata digest mismatch: {name}")

    result = {
        "schemaVersion": 1,
        "artifactKind": arguments.artifact_kind,
        "tag": arguments.tag,
        "commit": arguments.commit,
        "draft": expected_draft,
        "prerelease": expected_prerelease,
        "releaseAssets": len(remote_assets) if arguments.artifact_kind == "release" else 0,
        "metadataAssets": len(remote_assets),
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
