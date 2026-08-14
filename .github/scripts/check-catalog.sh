#!/usr/bin/env bash
# 카탈로그 · Cargo workspace · pnpm workspace · 앱별 버전 정합성 검사.
# 실패 시 무엇이 어긋났는지 정확히 출력하고 exit 1한다.
#
# 검사 항목:
#   1. 카탈로그 id 집합 == apps/ 하위 디렉터리 집합
#   2. 카탈로그 cargoPackage ⊆ 루트 Cargo.toml [workspace] members가 가리키는 패키지
#   3. 각 앱의 Cargo.toml / tauri.conf.json / package.json version 3자 일치
#   4. 카탈로그 identifier · productName == 해당 앱 tauri.conf.json 값
#   5. 카탈로그 appDir이 존재하고 package.json을 가진다
#   6. 모든 identifier가 com.devbox. 로 시작한다
#
# 의존: bash + python3 (러너에 이미 존재). jq 사용 금지.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

python3 - <<'PY'
import json
import os
import re
import sys

failures = False

def report(msg):
    global failures
    print(f"FAIL: {msg}", file=sys.stderr)
    failures = True

catalog_path = "apps/catalog.json"
if not os.path.exists(catalog_path):
    report("apps/catalog.json이 없다")
    sys.exit(1)

cat = json.load(open(catalog_path))
if cat.get("schemaVersion") != 1:
    report(f"schemaVersion != 1: {cat.get('schemaVersion')}")

apps = cat["apps"]
ids = {a["id"] for a in apps}
dirs = {d for d in os.listdir("apps") if os.path.isdir(f"apps/{d}")}

# 1. id 집합 == 디렉터리 집합
if ids != dirs:
    report(
        f"카탈로그 id와 apps/ 디렉터리 불일치. "
        f"only in catalog: {sorted(ids - dirs)}, only in apps/: {sorted(dirs - ids)}"
    )

# 2. cargoPackage ⊆ workspace members 패키지
members_toml = open("Cargo.toml").read()
member_block = re.search(r"\[workspace\].*?members\s*=\s*\[(.*?)\]", members_toml, re.S)
member_paths = []
if member_block:
    member_paths = re.findall(r'"([^"]+)"', member_block.group(1))
member_packages = set()
for m in member_paths:
    manifest = f"{m}/Cargo.toml"
    if os.path.exists(manifest):
        txt = open(manifest).read()
        name = re.search(r'^name\s*=\s*"([^"]+)"', txt, re.M)
        if name:
            member_packages.add(name.group(1))
missing = {a["cargoPackage"] for a in apps} - member_packages
if missing:
    report(f"cargoPackage가 workspace members에 없음: {sorted(missing)}")

for a in apps:
    app_dir = a["appDir"]
    app_id = a["id"]
    cargo_path = f"{app_dir}/src-tauri/Cargo.toml"
    tauri_path = f"{app_dir}/src-tauri/tauri.conf.json"
    pkg_path = f"{app_dir}/package.json"

    # 5. appDir 존재 + package.json
    if not os.path.isdir(app_dir):
        report(f"{app_id}: appDir이 없다: {app_dir}")
        continue
    if not os.path.isfile(pkg_path):
        report(f"{app_id}: package.json이 없다")

    # 3. version 3자 일치
    cargo = open(cargo_path).read()
    cargo_ver = re.search(r'^version\s*=\s*"([^"]+)"', cargo, re.M)
    if not cargo_ver:
        report(f"{app_id}: Cargo.toml version을 읽을 수 없다")
        continue
    tauri = json.load(open(tauri_path))
    pkg = json.load(open(pkg_path))
    if not (cargo_ver.group(1) == tauri.get("version") == pkg.get("version")):
        report(
            f"{app_id}: version 불일치 "
            f"Cargo={cargo_ver.group(1)} tauri={tauri.get('version')} package.json={pkg.get('version')}"
        )

    # 4. identifier · productName == tauri.conf.json
    if a["identifier"] != tauri.get("identifier"):
        report(f"{app_id}: identifier 불일치 catalog={a['identifier']} tauri={tauri.get('identifier')}")
    if a["productName"] != tauri.get("productName"):
        report(f"{app_id}: productName 불일치 catalog={a['productName']} tauri={tauri.get('productName')}")

    # 6. identifier가 com.devbox. 로 시작
    if not a["identifier"].startswith("com.devbox."):
        report(f"{app_id}: identifier가 com.devbox. 로 시작하지 않는다: {a['identifier']}")

sys.exit(1 if failures else 0)
PY
