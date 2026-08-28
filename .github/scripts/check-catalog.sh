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
#   7. release 앱은 third-party notices를 installer resource로 포함한다
#   8. catalog v2 revision/capability/action 계약이 유효하다
#   9. 모든 release 앱이 W4 second-instance focus 계약을 설치·초기화한다
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
if set(cat) != {"schemaVersion", "catalogRevision", "apps"}:
    report("catalog top-level field 집합이 schema v2와 맞지 않는다")
if cat.get("schemaVersion") != 2:
    report(f"schemaVersion != 2: {cat.get('schemaVersion')}")
revision = cat.get("catalogRevision")
if not isinstance(revision, int) or isinstance(revision, bool) or revision <= 0:
    report(f"catalogRevision이 양의 정수가 아니다: {revision!r}")

apps = cat["apps"]
ids = {a["id"] for a in apps}
dirs = {d for d in os.listdir("apps") if os.path.isdir(f"apps/{d}")}
if len(ids) != len(apps):
    report("catalog app id가 중복된다")

def valid_slug(value):
    return isinstance(value, str) and bool(re.fullmatch(r"[a-z0-9](?:[a-z0-9+_.-]*[a-z0-9])?", value))

def valid_version(value):
    return bool(re.fullmatch(r"v[1-9][0-9]*", value))

def capability_shape(value):
    if value in {"path", "workspace", "query", "profile", "task"}:
        return "basic"
    if isinstance(value, str) and value.startswith("handoff:"):
        parts = value.removeprefix("handoff:").split("/")
        if len(parts) == 2 and valid_slug(parts[0]) and valid_version(parts[1]):
            return "handoff"
    if isinstance(value, str) and value.startswith("snapshot:"):
        parts = value.removeprefix("snapshot:").split("/")
        if len(parts) == 3 and valid_slug(parts[0]) and valid_slug(parts[1]) and valid_version(parts[2]):
            return "snapshot"
    return None

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

identifiers = set()
cargo_packages = set()
app_dirs = set()
expected_app_fields = {
    "id", "displayName", "productName", "identifier", "cargoPackage", "appDir",
    "release", "managerVisible", "selfManaged", "accepts", "produces", "actions",
}
expected_action_fields = {"actionId", "actionVersion", "label", "target", "payloadKind"}

for a in apps:
    app_dir = a["appDir"]
    app_id = a["id"]
    if set(a) != expected_app_fields:
        report(f"{app_id}: app field 집합이 schema v2와 맞지 않는다")
    for field, seen in (("identifier", identifiers), ("cargoPackage", cargo_packages), ("appDir", app_dirs)):
        value = a.get(field)
        if value in seen:
            report(f"{app_id}: {field}가 중복된다")
        seen.add(value)
    cargo_path = f"{app_dir}/src-tauri/Cargo.toml"
    lib_path = f"{app_dir}/src-tauri/src/lib.rs"
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
    if not re.fullmatch(r"com\.devbox\.[a-z0-9]+(?:\.[a-z0-9]+)*", a["identifier"]):
        report(f"{app_id}: identifier가 com.devbox. 로 시작하지 않는다: {a['identifier']}")

    # 7. release package는 lockfile 기반 notices를 반드시 포함
    resources = tauri.get("bundle", {}).get("resources", [])
    if a.get("release") and "../../../THIRD_PARTY_NOTICES.md" not in resources:
        report(f"{app_id}: bundle.resources에 THIRD_PARTY_NOTICES.md가 없다")

    # 9. W4에서는 15개 release 앱 모두 두 번째 프로세스가 새 window를
    # 남기지 않고 기존 main window를 복구·focus해야 한다. dependency만 선언하고
    # 초기화를 빠뜨리는 회귀도 함께 차단한다.
    if a.get("release"):
        lib = open(lib_path).read() if os.path.isfile(lib_path) else ""
        if not re.search(r'^tauri-plugin-single-instance\s*=\s*"2"', cargo, re.M):
            report(f"{app_id}: tauri-plugin-single-instance dependency가 없다")
        if "tauri_plugin_single_instance::init" not in lib:
            report(f"{app_id}: single-instance plugin을 초기화하지 않는다")

    # 8. v2 capability/action은 정적이고 versioned인 선언만 허용
    accepts = a.get("accepts")
    produces = a.get("produces")
    actions = a.get("actions")
    if not isinstance(accepts, list) or not isinstance(produces, list) or not isinstance(actions, list):
        report(f"{app_id}: accepts/produces/actions가 배열이 아니다")
        continue
    if any(not isinstance(item, str) for item in accepts) or len(accepts) != len(set(accepts)) or any(capability_shape(item) not in {"basic", "handoff"} for item in accepts):
        report(f"{app_id}: accepts capability가 중복되거나 유효하지 않다")
    if any(not isinstance(item, str) for item in produces) or len(produces) != len(set(produces)) or any(capability_shape(item) not in {"handoff", "snapshot"} for item in produces):
        report(f"{app_id}: produces capability가 중복되거나 유효하지 않다")
    if any(capability_shape(item) == "snapshot" and not item.startswith(f"snapshot:{app_id}/") for item in produces):
        report(f"{app_id}: snapshot producer가 app id와 맞지 않는다")
    action_ids = set()
    for action in actions:
        if not isinstance(action, dict):
            report(f"{app_id}: action이 object가 아니다")
            continue
        if set(action) != expected_action_fields:
            report(f"{app_id}: action field 집합이 schema v2와 맞지 않는다")
        action_id = action.get("actionId")
        if not valid_slug(action_id) or action_id in action_ids:
            report(f"{app_id}: actionId가 유효하지 않거나 중복된다")
        action_ids.add(action_id)
        if not isinstance(action.get("actionVersion"), int) or isinstance(action.get("actionVersion"), bool) or action["actionVersion"] <= 0:
            report(f"{app_id}: actionVersion이 양의 정수가 아니다")
        if not isinstance(action.get("label"), str) or not action["label"].strip():
            report(f"{app_id}: action label이 비어 있다")
        payload = action.get("payloadKind")
        target = action.get("target")
        if capability_shape(payload) != "handoff":
            report(f"{app_id}: action payloadKind가 versioned handoff가 아니다")
        target_app = next((candidate for candidate in apps if candidate["id"] == target), None)
        if target_app is None or payload not in target_app.get("accepts", []):
            report(f"{app_id}: action target이 payloadKind를 받지 않는다")

sys.exit(1 if failures else 0)
PY

python3 .github/scripts/test-windows-packaged-smoke-config.py
python3 .github/scripts/test-verify-downloaded-release.py
node --check .github/scripts/windows-packaged-smoke.mjs
