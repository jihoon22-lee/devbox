#!/usr/bin/env python3
"""release-manifest.json 생성.

staging/ 디렉터리를 훑어 §5.4 스키마의 manifest를 만든다.
- id: 카탈로그에서
- version: 앱의 src-tauri/Cargo.toml에서
- portable / installer의 name·size·sha256: staging의 실제 파일에서 계산
- 앱 하나라도 portable 또는 installer가 없으면 실패한다 (조용한 누락 금지)

용법: build-manifest.py <staging-dir> <release-tag> <catalog> <output>
"""
import hashlib
import json
import re
import sys
from datetime import datetime, timezone


def sha256_of(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def cargo_version(app_dir: str) -> str:
    text = open(f"{app_dir}/src-tauri/Cargo.toml").read()
    m = re.search(r'^version\s*=\s*"([^"]+)"', text, re.M)
    if not m:
        raise SystemExit(f"cannot read version from: {app_dir}")
    return m.group(1)


def main() -> None:
    if len(sys.argv) != 5:
        raise SystemExit("usage: build-manifest.py <staging-dir> <release-tag> <catalog> <output>")
    staging, release_tag, catalog_path, output = sys.argv[1:]

    catalog = json.load(open(catalog_path))
    apps = [a for a in catalog["apps"] if a["release"]]

    manifest_apps = []
    for entry in apps:
        app_id = entry["id"]
        portable_dir = f"{staging}/{app_id}/portable"
        installer_dir = f"{staging}/{app_id}/installer"

        portable_files = sorted(
            f for f in os.listdir(portable_dir) if f.endswith(".exe")
        ) if os.path.isdir(portable_dir) else []
        installer_files = sorted(
            f for f in os.listdir(installer_dir) if f.endswith(".exe")
        ) if os.path.isdir(installer_dir) else []

        if len(portable_files) != 1:
            raise SystemExit(f"{app_id}: expected exactly 1 portable (found {portable_files})")
        if len(installer_files) != 1:
            raise SystemExit(f"{app_id}: expected exactly 1 installer (found {installer_files})")

        def asset(name: str, dirpath: str) -> dict:
            path = f"{dirpath}/{name}"
            return {
                "name": name,
                "sha256": sha256_of(path),
                "size": os.path.getsize(path),
            }

        manifest_apps.append({
            "id": app_id,
            "version": cargo_version(entry["appDir"]),
            "portable": asset(portable_files[0], portable_dir),
            "installer": asset(installer_files[0], installer_dir),
        })

    manifest = {
        "schemaVersion": 1,
        "releaseTag": release_tag,
        "generatedAt": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "apps": manifest_apps,
    }

    with open(output, "w") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"manifest written: {output} ({len(manifest_apps)} apps)")


if __name__ == "__main__":
    import os  # noqa: PLC0415 — 여기서만 사용
    main()
