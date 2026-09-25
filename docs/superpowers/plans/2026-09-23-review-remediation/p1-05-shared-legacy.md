# P1-05 공용 v0.7 계층 정리 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 제품들이 더 쓰지 않는 v0.7 공용 계층(옛 앱 실행기 `crates/launch`, `data-migration`, 가져오기용 transport 호출, legacy 카탈로그, v0.8 전환 추적 파일)을 지운다(A3, D9). engine crate 이름 변경(D22)은 기계적 변경만 모으는 P1-07에서 한다.

**Architecture:** `crates/launch`의 `launch*`·`installed_targets`는 v0.8에서 이미 모든 옛 앱 ID를 거부한다(`refuse_retired_product`). 그래서 이를 부르는 engine 경로는 지금도 항상 "사용할 수 없음"으로 끝난다. 이 경로를 지우고, `custom_root`가 쓰는 설치 루트 locator 파서만 installation-tools로 옮긴 뒤 crate를 없앤다. applink는 제품 안 handoff(`OpenRequest`, handoff store, engine `PendingOpen`)가 쓰므로 남기고 argv 조립·파싱만 지운다.

**Tech Stack:** Rust(Cargo workspace), Python, Markdown

**Spec:** `review.md` §6 A3 · `00-roadmap.md` D9·D22

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 선행: P1-02·P1-03·P1-04 머지. `rg -n "data_migration|devbox_data_migration" apps crates`가 `crates/data-migration` 밖에서 0건이어야 시작한다.
- 남기는 것: `crates/applink`의 `OpenRequest`·handoff store·payload 타입·`log_source`·`task_control`·`toolbox_text`·`webhook_log`, engine의 `applink.rs`(`PendingOpen` 수신 버퍼), `crates/integration`, `product_contract::migration_backup::Verified`(Control Center 배포 journal의 owner evidence가 이 타입으로 저장돼 있다. 형식 호환을 위해 남긴다).

## Review Focus

1. 제품에서 "다른 앱에서 열기" 같은 메뉴가 v0.7 앱 목록을 보여 주던 곳 → 이제 목록이 비거나 제품 host가 가로챈 v0.8 대상만 나온다(이미 host가 가로채던 동작과 같음). (Task 2 테스트)
2. `custom_root`의 설치 루트 locator 파싱이 옮긴 뒤에도 같은 입력을 같은 결과로 판정한다. (Task 2, 옮긴 테스트)
3. engine 허용 목록에서 메서드를 지웠는데 프런트가 아직 그 메서드를 부름 → 해당 화면 동작이 "사용할 수 없음"에서 "허용되지 않은 요청"으로 바뀐다. 지운 메서드마다 `rg -n '"<method>"' packages apps`로 프런트 호출이 없는지 확인한다. (Task 2)
4. transport `Call` 변형을 지운 뒤, 새 버전 Control Center가 옛 버전 제품에 연결될 때 → Suite는 같은 generation의 제품끼리만 연결하므로 섞이지 않는다(변형 삭제 안전). (Task 3)
5. `check-source-cutover.py` 대체 후에도 "네 제품·14개 engine·standalone 없음·node lock LF" 검사가 계속 돈다. (Task 4)

## Branch · PR

- 묶음: **B3** — 브랜치 `refactor/suite/archive-and-remove-v07`, PR 제목 `refactor(suite): archive history, remove v0.7 imports and unify dependencies`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(crates): retire the v0.7 launcher, migration crate and legacy catalog`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## 정리 규칙(`devbox_launch` 호출부)

| 호출 형태 | 처리 |
|---|---|
| `installed_targets(capability)`로 목록을 만들어 UI에 주는 함수(`open_targets` 계열) | 빈 목록(`Vec::new()`/`json!([])`)을 돌려주고, 목록을 걸러 내던 `core/open_targets.rs` 모듈은 쓰는 곳이 없어지면 지운다 |
| `installed_targets(..)`가 비었는지 보고 오류를 내는 가드 + 그 뒤 `launch_open(..)` | 가드가 내던 기존 "대상 없음" 오류를 곧바로 돌려준다(예: `API_TARGET_UNAVAILABLE_ERROR`, `LOG_LENS_TARGET_UNAVAILABLE_ERROR`, `provider_unavailable`). 함수가 오류만 돌려주게 되면 호출하는 dispatch 팔까지 따라가서, 제품 host가 같은 메서드를 가로채는지 확인한다(`rg -n '"<method>"' apps/*/src-tauri/src`). 가로채면 engine 쪽 메서드를 허용 목록·dispatch에서 지운다 |
| `launch_open_owned_with_environment`·`OwnedProcess`(projects-engine Start Workspace) | 옛 앱을 여는 단계를 지우고, 결과 목록에서 해당 항목을 빼거나 "사용할 수 없음"으로 표시하는 기존 분기를 쓴다 |
| `open_argv`·`build_argv`·`parse_argv` | 지운다. 테스트도 함께 |
| `custom_root.rs`의 `parse_install_root_locator`, `InstallRootLocator`, `INSTALL_ROOT_SCHEMA_VERSION`, `MAX_INSTALL_ROOT_LOCATOR_BYTES` | Task 2 Step 3에서 `crates/installation-tools/src/core/install_root.rs`로 옮긴다 |

---

### Task 1: guard 전 범위

- [ ] `check-no-legacy.py`의 `SCOPES`에 추가한다.

```python
    "shared": [
        "crates",
        "Cargo.toml",
        ".github/scripts",
        ".github/workflows",
    ],
```

  `PATTERNS`에 추가한다: `r"\bdevbox_launch\b"`, `r"\bdevbox-launch\b"`, `r"crates/launch\b"`, `r"\bdata[-_]migration\b"`, `r"\bVerifyMigrationSources\b"`, `r"\bListMigrationBackups\b"`, `r"\bVerifyMigrationBackup\b"`, `r"\bmigration_source\b"`, `r"v0\.8-feature-parity"`, `r"v0\.8-data-inventory"`.
  `scan()`의 제외 조건에 `".github/scripts/check-no-legacy.py"`와 `test-check-no-legacy.py` 자신을 추가한다(패턴 문자열이 걸리지 않게): `if path.name in {"check-no-legacy.py", "test-check-no-legacy.py"}: continue`.
- [ ] `ci.yml`의 guard 줄들을 `python3 .github/scripts/check-no-legacy.py --scope all` 한 줄로 바꾼다.
- [ ] Run: `python3 .github/scripts/check-no-legacy.py --scope shared` → FAIL. 커밋: `git commit -am "test(crates): extend the legacy guard to shared crates"`

---

### Task 2: `crates/launch` 제거

**Files:** `crates/launch/`(삭제), `Cargo.toml`(workspace members), 호출부가 있는 engine·installation-tools, `crates/installation-tools/src/core/{install_root.rs(생성),custom_root.rs,mod.rs}`

- [ ] **Step 1: 호출부 목록 고정** — Run: `rg -n "devbox_launch::|use devbox_launch" crates apps > /tmp/launch-sites.txt; wc -l /tmp/launch-sites.txt`. 이 목록의 모든 줄이 이 Task가 끝날 때 사라져야 한다.

- [ ] **Step 2: 실패하는 테스트** — 대표 경로 두 곳에 "대상 없음" 동작을 고정한다.

`crates/webhook-host/src/commands.rs` 테스트 모듈:

```rust
    #[test]
    fn external_api_handoff_is_unavailable_without_a_launcher() {
        assert_eq!(api_handoff_target_available(), false);
    }
```

  (`publish_api_handoff`의 대상 확인을 `fn api_handoff_target_available() -> bool { false }`로 떼어 내고, `publish_api_handoff`는 `if !api_handoff_target_available() { return Err(API_TARGET_UNAVAILABLE_ERROR.to_string()); }`로 시작하게 한다. API Studio 제품은 이 경로 대신 `apps/devbox-api-studio/src-tauri/src/handoff.rs`의 내부 handoff를 쓴다.)

`crates/installation-tools/src/core/install_root.rs` 테스트(옮긴 뒤 이름만 바뀐 기존 locator 테스트): `crates/launch/src/installed.rs`의 `parse_install_root_locator` 테스트를 그대로 옮긴다.

- [ ] **Step 3: 구현**
  - `crates/launch/src/installed.rs`에서 `InstallRootLocator`, `INSTALL_ROOT_SCHEMA_VERSION`, `MAX_INSTALL_ROOT_LOCATOR_BYTES`, `parse_install_root_locator`와 그 테스트를 `crates/installation-tools/src/core/install_root.rs`로 옮기고, `custom_root.rs`의 `use devbox_launch::{…}`를 `use super::install_root::{…}`로 바꾼다.
  - `/tmp/launch-sites.txt`의 호출부를 "정리 규칙"대로 고친다. engine별 예상 결과:
    - `ports-engine/commands/correlation.rs`, `logs-engine/commands.rs`, `http-client-engine/commands/toolbox.rs`, `toolbox-engine/commands/handoff.rs`, `runtime-engine/log_lens.rs`, `activity-engine/commands/handoff.rs`, `webhook-host/commands.rs`: 외부 앱 handoff 발행 → 기존 "대상 없음" 오류를 바로 반환. 제품 host가 같은 메서드를 가로채는지 확인해 engine 허용 목록에서 지울 수 있으면 지운다.
    - `content-index-engine/commands/actions.rs`, `knowledge-vault-engine/commands/docs.rs`, `repositories-engine/commands.rs`, `projects-engine/commands/profile_actions.rs`: `open_targets`는 빈 목록, `open_in`은 `provider_unavailable`. 이 메서드들은 Knowledge·Workspace host가 이미 가로챈다(예: Knowledge `component.rs`의 `"open_targets" => Ok(json!([]))`, `"open_in" => Err("provider_unavailable")`).
    - `runtime-engine/commands.rs`의 `launch_open("code-pad", …)`, `projects-engine/commands/task_control.rs`, `projects-engine/commands/workspace.rs`(Start Workspace의 옛 앱 실행 단계), `projects-engine/commands/preflight.rs`(설치 대상 점검): 옛 앱 실행 단계를 지운다.
  - `Cargo.toml` workspace `members`에서 `crates/launch`를 지우고, 각 crate `Cargo.toml`의 `devbox-launch`(또는 `launch`) 의존을 지운다. `git rm -r crates/launch`.
  - `crates/applink`: `build_argv`·`parse_argv`와 argv 전용 도우미를 지운다(`cargo check --workspace` 오류로 확인). `lib.rs` 머리 주석의 "수신 앱 13개" 문장을 "제품 host가 handoff store로 전달하는 요청 형식"으로 바꾼다.

- [ ] **Step 4: 통과 확인** — Run: `cargo check --workspace --all-targets && cargo test -p devbox-webhook-host -p devbox-installation-tools --lib && ! rg -n "devbox_launch" crates apps` → PASS(마지막 명령은 일치 0건이라 성공).

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(crates): retire the v0.7 app launcher"`

---

### Task 3: migration 공용 코드

**Files:** `crates/data-migration/`(삭제), `crates/product-contract/src/{migration_source.rs(삭제),transport.rs,lib.rs}`, `crates/suite-runtime/src/health.rs`, 네 제품 `federation.rs`, `Cargo.toml`, `.github/workflows/product-foundation.yml`

- [ ] **Step 1: 실패하는 테스트** — `crates/product-contract/src/transport.rs` 테스트 모듈에 추가한다.

```rust
    #[test]
    fn retired_migration_calls_no_longer_parse() {
        for kind in ["verifyMigrationSources", "listMigrationBackups", "verifyMigrationBackup"] {
            let json = format!(r#"{{"kind":"{kind}"}}"#);
            assert!(serde_json::from_str::<Call>(&json).is_err(), "{kind}");
        }
        assert!(serde_json::from_str::<Call>(r#"{"kind":"readMigrationStatus"}"#).is_ok());
    }
```

  (`Call`의 serde tag 이름·대소문자는 파일의 `#[serde(tag = …, rename_all = …)]`를 보고 맞춘다.)

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p product-contract --lib transport` → FAIL.

- [ ] **Step 3: 구현**
  - `transport.rs`에서 `VerifyMigrationSources`·`ListMigrationBackups`·`VerifyMigrationBackup` 변형과 `validate_call`의 해당 팔을 지운다. 네 제품 `federation.rs`의 `migration_retired` 팔, `suite-runtime/src/health.rs`의 `backups`·`sources` 함수와 그 호출을 지운다.
  - `migration_source.rs`를 지우고 `lib.rs`에서 모듈 선언을 뺀다.
  - `Cargo.toml` members에서 `crates/data-migration`을 지우고 `git rm -r crates/data-migration`. `product-foundation.yml`·`ci.yml`·`resolve-ci-scope.py`의 `crates/data-migration` 경로를 지운다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p product-contract -p suite-runtime --lib && cargo check --workspace --all-targets && python3 .github/scripts/test-ci-scope.py` → PASS.

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(crates): drop retired migration transport calls and the migration crate"`

---

### Task 4: legacy 카탈로그·전환 추적 파일

**Files:** `apps/legacy-v0.7-catalog.json`, `apps/v0.8-feature-parity.json`, `apps/v0.8-data-inventory.json`, `docs/v0.8-acceptance.md`(삭제, 보관소로), `crates/catalog/src/lib.rs`, `.github/scripts/{check-source-cutover.py,check-product-foundation.py,check-catalog.sh}`, 문서 링크

- [ ] **Step 1: 보관소로 복사** — P1-01에서 만든 `jihoon22-lee/devbox-archive`에 세 파일과 `docs/v0.8-acceptance.md`를 원래 경로로 복사해 push한다(P1-01 Task 1 Step 2와 같은 명령, 파일 목록만 다름).
- [ ] **Step 2: 검사 스크립트 먼저 고치기(실패 확인)** — `check-source-cutover.py`를 아래로 바꾸고 Run: `python3 .github/scripts/check-source-cutover.py` → 파일을 지우기 전이므로 PASS, 지운 뒤에도 PASS여야 한다.

```python
#!/usr/bin/env python3
"""Four public products, library-only engines and reviewed embedded bytes."""
from pathlib import Path
import json, tomllib, subprocess, hashlib, re

ROOT = Path(__file__).resolve().parents[2]
products = {'devbox-workspace', 'devbox-api-studio', 'devbox-knowledge', 'devbox-control-center'}
assert {p.name for p in (ROOT / 'apps').iterdir() if p.is_dir()} == products
assert {p['id'] for p in json.loads((ROOT / 'apps/catalog.json').read_text())['apps']} == products
assert not (ROOT / 'apps/legacy-v0.7-catalog.json').exists(), 'legacy catalog must stay retired'
assert not (ROOT / 'crates/launch').exists(), 'the v0.7 launcher must stay retired'
engines = list((ROOT / 'crates').glob('*-engine')) + [ROOT / 'crates/webhook-host', ROOT / 'crates/installation-tools']
assert len(engines) == 14
for engine in engines:
    assert not any((engine / name).exists() for name in ['tauri.conf.json', 'build.rs', 'src/main.rs', 'capabilities', 'icons'])
    manifest = tomllib.loads((engine / 'Cargo.toml').read_text())
    assert manifest['lib']['crate-type'] == ['rlib']
    assert 'standalone' not in manifest.get('features', {})
lock_path = 'crates/editor-engine/src/lsp/node-lock.json'
attribute = subprocess.check_output(['git', 'check-attr', 'eol', '--', lock_path], cwd=ROOT, text=True).strip()
assert attribute == f'{lock_path}: eol: lf', 'reviewed Node lock requires checkout LF identity'
expected = re.search(r'REVIEWED_NODE_LOCK_SHA256: &str =\s*"([a-f0-9]{64})"', (ROOT / 'crates/editor-engine/src/lsp/node_lock.rs').read_text()).group(1)
assert hashlib.sha256((ROOT / lock_path).read_bytes()).hexdigest() == expected
print('Source cutover: four products, 14 library engines, no retired launcher or catalog')
```

- [ ] **Step 3: 삭제와 정리**
  - `git rm apps/legacy-v0.7-catalog.json apps/v0.8-feature-parity.json apps/v0.8-data-inventory.json docs/v0.8-acceptance.md`.
  - `check-product-foundation.py`에서 두 JSON을 읽는 부분(`parity`, `data_inventory`)과 그 단언을 지운다.
  - `crates/catalog/src/lib.rs`: `parse_catalog`·`select_catalog`·`capable_targets`·`capable_producers`와 legacy `Catalog`·`CatalogApp` 타입을 쓰는 곳이 없으면(`cargo check`로 확인) 지우고, `crates/catalog/tests/catalog.rs`의 legacy 테스트를 지운다. `products` 모듈은 남긴다.
  - 링크: `README.md`, `CONVENTIONS.md:366`, 앱 README 세 곳, `docs/projects.md:22`, `docs/architecture.md:24`의 `v0.8-acceptance.md` 링크를 보관소 URL로 바꾼다. `CONVENTIONS.md:61`의 legacy catalog 설명 줄을 지운다.
- [ ] **Step 4: 통과 확인** — Run: `python3 .github/scripts/check-source-cutover.py && python3 .github/scripts/check-product-foundation.py && bash .github/scripts/check-catalog.sh && cargo test -p catalog` → PASS.
- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(crates): retire the legacy catalog and v0.8 cutover trackers"`

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. 전체 범위 변경이므로 `pnpm verify:all`.
- [ ] PR 본문에 "제품 동작 변화 없음(옛 앱 실행 경로는 이미 거부되던 것)"을 적는다.
