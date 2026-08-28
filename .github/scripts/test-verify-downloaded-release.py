#!/usr/bin/env python3
"""Unit tests for the offline downloaded-release verifier."""

from __future__ import annotations

import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[2]
VERIFIER = ROOT / ".github/scripts/verify-downloaded-release.py"
TAG = "v0.5.0-rc9"
COMMIT = "a" * 40


def identity(path: pathlib.Path) -> dict[str, object]:
    payload = path.read_bytes()
    return {
        "name": path.name,
        "size": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
    }


def fixture(root: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path, pathlib.Path]:
    assets = root / "assets"
    assets.mkdir()
    apps = []
    declared = []
    for index in range(15):
        app_id = f"app-{index:02d}"
        portable = assets / f"{app_id}.exe"
        installer = assets / f"{app_id}_0.5.0_x64-setup.exe"
        portable.write_bytes(f"portable:{app_id}".encode())
        installer.write_bytes(f"installer:{app_id}".encode())
        portable_identity = identity(portable)
        installer_identity = identity(installer)
        declared.extend((portable_identity, installer_identity))
        apps.append(
            {
                "id": app_id,
                "version": "0.5.0",
                "portable": portable_identity,
                "installer": installer_identity,
            }
        )

    notices = assets / "THIRD_PARTY_NOTICES.md"
    notices.write_text("fixture notices\n", encoding="utf-8")
    notices_identity = identity(notices)
    declared.append(notices_identity)
    manifest = {
        "schemaVersion": 1,
        "releaseTag": TAG,
        "generatedAt": "2026-08-29T00:00:00Z",
        "apps": apps,
        "notices": notices_identity,
    }
    manifest_path = assets / "release-manifest.json"
    manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

    remote = [
        {"name": item["name"], "size": item["size"], "digest": f"sha256:{item['sha256']}"}
        for item in declared
    ]
    manifest_remote = identity(manifest_path)
    remote.append(
        {
            "name": manifest_remote["name"],
            "size": manifest_remote["size"],
            "digest": f"sha256:{manifest_remote['sha256']}",
        }
    )
    release = {
        "tagName": TAG,
        "isDraft": False,
        "isPrerelease": True,
        "targetCommit": COMMIT,
        "assets": remote,
    }
    release_path = root / "release.json"
    release_path.write_text(json.dumps(release), encoding="utf-8")
    config_path = root / "config.json"
    config_path.write_text(
        json.dumps(
            {
                "schemaVersion": 1,
                "apps": [{"id": app["id"], "version": app["version"]} for app in apps],
            }
        ),
        encoding="utf-8",
    )
    return assets, release_path, config_path


def run_verifier(
    assets: pathlib.Path, release: pathlib.Path, config: pathlib.Path
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(VERIFIER),
            "--assets",
            str(assets),
            "--release",
            str(release),
            "--config",
            str(config),
            "--tag",
            TAG,
            "--commit",
            COMMIT,
            "--draft",
            "false",
            "--prerelease",
            "true",
        ],
        check=False,
        capture_output=True,
        text=True,
    )


def main() -> int:
    with tempfile.TemporaryDirectory() as temporary:
        assets, release, config = fixture(pathlib.Path(temporary))
        passed = run_verifier(assets, release, config)
        assert passed.returncode == 0, passed.stdout + passed.stderr
        passed_result = json.loads(passed.stdout)
        assert passed_result["status"] == "PASS"
        assert passed_result["draft"] is False
        assert passed_result["verifiedAssets"] == 32
        assert passed_result["configSha256"] == hashlib.sha256(config.read_bytes()).hexdigest()

        manifest_path = assets / "release-manifest.json"
        original_manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        invalid_manifest = {**original_manifest, "unexpected": True}
        manifest_path.write_text(json.dumps(invalid_manifest), encoding="utf-8")
        malformed = run_verifier(assets, release, config)
        assert malformed.returncode == 1, malformed.stdout + malformed.stderr
        assert "manifest envelope or schema mismatch" in json.loads(malformed.stdout)["failures"]

        manifest_path.write_text(json.dumps(original_manifest), encoding="utf-8")
        release_payload = json.loads(release.read_text(encoding="utf-8"))
        manifest_identity = identity(manifest_path)
        for asset in release_payload["assets"]:
            if asset["name"] == "release-manifest.json":
                asset["size"] = manifest_identity["size"]
                asset["digest"] = f"sha256:{manifest_identity['sha256']}"
        release.write_text(json.dumps(release_payload), encoding="utf-8")

        (assets / "app-00.exe").write_bytes(b"tampered")
        failed = run_verifier(assets, release, config)
        assert failed.returncode == 1, failed.stdout + failed.stderr
        result = json.loads(failed.stdout)
        assert result["status"] == "FAIL"
        assert any("app-00.exe" in failure for failure in result["failures"])

    print("VERIFY DOWNLOADED RELEASE TESTS OK: valid fixture passes and tampering fails")
    return 0


if __name__ == "__main__":
    sys.exit(main())
