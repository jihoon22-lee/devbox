#!/usr/bin/env python3
"""Regression tests for the release manifest notice asset."""

import hashlib
import json
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".github" / "scripts" / "build-manifest.py"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


with tempfile.TemporaryDirectory() as directory:
    temp = Path(directory)
    app = temp / "app"
    tauri = app / "src-tauri"
    tauri.mkdir(parents=True)
    (tauri / "Cargo.toml").write_text(
        '[package]\nname = "demo"\nversion = "1.2.3"\n', encoding="utf-8"
    )
    catalog = temp / "catalog.json"
    catalog.write_text(
        json.dumps(
            {
                "schemaVersion": 1,
                "apps": [
                    {
                        "id": "demo",
                        "appDir": str(app),
                        "release": True,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    staging = temp / "staging"
    portable = staging / "demo" / "portable"
    installer = staging / "demo" / "installer"
    portable.mkdir(parents=True)
    installer.mkdir(parents=True)
    portable_bytes = b"portable"
    installer_bytes = b"installer"
    notice_bytes = b"notices\n"
    (portable / "demo.exe").write_bytes(portable_bytes)
    (installer / "demo_1.2.3_x64-setup.exe").write_bytes(installer_bytes)
    (staging / "THIRD_PARTY_NOTICES.md").write_bytes(notice_bytes)
    output = temp / "manifest.json"

    subprocess.run(
        [
            "python3",
            str(SCRIPT),
            str(staging),
            "v1.2.3",
            str(catalog),
            str(output),
        ],
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    manifest = json.loads(output.read_text(encoding="utf-8"))
    assert manifest["schemaVersion"] == 1
    assert manifest["releaseTag"] == "v1.2.3"
    assert len(manifest["apps"]) == 1
    assert manifest["apps"][0]["portable"]["sha256"] == digest(portable_bytes)
    assert manifest["apps"][0]["installer"]["sha256"] == digest(installer_bytes)
    assert manifest["notices"] == {
        "name": "THIRD_PARTY_NOTICES.md",
        "sha256": digest(notice_bytes),
        "size": len(notice_bytes),
    }

    (staging / "THIRD_PARTY_NOTICES.md").unlink()
    failed = subprocess.run(
        [
            "python3",
            str(SCRIPT),
            str(staging),
            "v1.2.3",
            str(catalog),
            str(output),
        ],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    assert failed.returncode != 0
    assert "THIRD_PARTY_NOTICES.md is missing" in failed.stderr

print("build-manifest notice tests passed")
