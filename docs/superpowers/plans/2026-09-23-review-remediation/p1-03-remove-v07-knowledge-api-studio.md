# P1-03 v0.7 잔재 제거 ② Knowledge·API Studio — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** Knowledge와 API Studio에서 v0.7 앱(knowledge-base·life-log·everything-plus·api-playground·webhook-lab·developer-toolbox) 데이터 가져오기·백업·출처 검증 코드와 화면을 지우고, 처음 실행하면 시작 화면 없이 바로 쓸 수 있게 한다(D9, UX §7 첫 실행).

**Architecture:** Suite health 계약(`Call::ReadMigrationStatus` → `migration_status::Summary`)은 유지하고 값을 새로 계산한다: `busy`=시작 작업 중, `setup_selected`=저장소 있음, `review_required`=저장소가 있는데 아직 준비되지 않음, `mappings`=없음. 출처·백업 호출(`VerifyMigrationSources`, `ListMigrationBackups`, `VerifyMigrationBackup`)은 `"migration_retired"`로 거부한다(변형 자체는 P1-05에서 지운다). Knowledge는 `knowledge.migration` component를 시작·노트 폴더 변경 용도로만 남긴다(이름은 P1-12에서 바꾼다). API Studio는 시작 gate와 `api-studio.migration` component를 없앤다.

**Tech Stack:** Rust, React/TypeScript, Python(guard)

**Spec:** `review.md` §6 A3, §7 · `00-roadmap.md` D9·Q1

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 지우지 않는 것: Knowledge `startup.rs`(자동 시작으로 단순화), `vault_binding.rs`·`core/vault_binding.rs`(노트 폴더 변경), `vault_owner.rs`(v0.7 앱 쓰기 감시만 제거), `core/stores.rs`, `lifecycle.rs`, `search.rs`, `federation.rs`의 v0.7 무관 Call. API Studio `component_errors.rs`, `handoff.rs`, `mock_draft.rs`, `knowledge.rs`, `core/{api_workspace,knowledge,lifecycle,mock_draft,openapi_definitions}.rs`, `service_worker.rs`, `selection_receive.rs`, `webhook_logs.rs`.
- 노트 폴더가 기본 private vault 밖에 있으면 사용자가 승인한 binding(approval)만 받는다. v0.7 knowledge-base 설정으로 승인을 대신하던 경로는 없어진다.
- catalog `components`에서 `api-studio.migration`을 지우고 `catalogRevision`을 1 올린다.

## Review Focus

1. 새 설치에서 Knowledge 첫 실행 → "Knowledge 시작" 화면 없이 노트 화면이 뜬다(자동 `start_empty`). 저장소가 이미 있으면 자동 `continue_existing`. (Task 2)
2. 시작 중 오류(`future_schema` 등) → 자동 재시도 없이 오류와 "다시 시도"만 보인다. (Task 2)
3. Suite activation `import` 단계(아직 확정 전)에서 Knowledge를 열면 저장소가 "prepared"로 준비되고 health 요약이 `setupSelected: true`, `reviewRequired: false`가 된다 → Control Center owner evidence 기록 가능. (Task 2)
4. API Studio 첫 실행 → 가져오기 화면 없이 요청 화면이 뜨고 localStorage 기반 기존 기능이 동작한다. (Task 3)
5. private vault 밖의 노트 폴더인데 approval이 없는 저장소(손으로 만든 상태) → `vault_binding_invalid`로 멈추고 폴더 재선택을 안내한다. (Task 2)

## Branch · PR

- 묶음: **B3** — 브랜치 `refactor/suite/archive-and-remove-v07`, PR 제목 `refactor(suite): archive history, remove v0.7 imports and unify dependencies`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(suite): remove v0.7 imports from Knowledge and API Studio`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## 삭제 목록

| 경로 | 비고 |
|---|---|
| `apps/devbox-knowledge/src-tauri/src/migration.rs` | 필요한 상태 계산은 `startup.rs`로 옮김 |
| `apps/devbox-knowledge/src-tauri/src/core/{import_plan,import_rows}.rs` | `validate_owned_store`·`Source`는 `core/stores.rs`로 옮김 |
| `apps/devbox-knowledge/src/{MigrationSetup,MigrationSettings}.tsx`와 테스트 | |
| `apps/devbox-api-studio/src-tauri/src/{migration,migration_export}.rs` | 시작 상태는 없앰 |
| `apps/devbox-api-studio/src-tauri/src/core/{import_model,import_repository,native_source_backup}.rs` | |
| `apps/devbox-api-studio/src-tauri/src/platform/{legacy_profile,browser_profile,browser_snapshot,source_guard,owned_copy}.rs` | `platform/mod.rs` 정리 |
| `apps/devbox-api-studio/src/migration/` 전체 | `MigrationStartup` 등 |
| `packages/api-studio-features/src/migration/` 전체 | `apiStorage.ts` 등 |
| `crates/http-client-engine/src/commands/migration.rs` | legacy API 저장소 어댑터 |
| `.github/scripts/windows-api-migration.mjs` | workflow step도 제거 |
| `.github/scripts/windows-knowledge-migration.mjs` | 노트 폴더 소유·창 수명 검사는 `windows-knowledge-lifecycle.mjs`로 옮겨 유지 |

## 정리 규칙(컴파일 오류가 날 때)

| 오류가 가리키는 것 | 처리 |
|---|---|
| `crate::migration::{METHODS, dispatch, initialize, finish_recovery, scheduled, suite_status, suite_sources, suite_backups, operation_rows}` (Knowledge) | `METHODS`·`dispatch` 분기는 지운다. `initialize` 호출은 지우고 "paused" 값은 `false`. `finish_recovery`는 호출째 지운다. `scheduled`는 응답 필드째 지운다. `suite_status`는 `startup::suite_status`로(Task 2). `suite_sources`·`suite_backups` 분기는 `Err("migration_retired")`. `operation_rows`는 `Vec::new()` |
| `crate::migration::{require_active, initialize, dispatch, issue, suite_*}` (API Studio) | `require_active` 호출은 지운다. `initialize` 호출은 지운다. `api-studio.migration` 분기는 통째로 지운다. `suite_status`는 Task 3의 `status_summary`로, 나머지 `suite_*`는 `Err("migration_retired")` |
| `import_rows::Source` / `validate_owned_store` | `core::stores::{StoreKind, validate_store}`(Task 2) |
| `vault_owner::acquire(base, vault, legacy_guard)` | 세 번째 인자를 없애고 v0.7 SQLite 쓰기 감시 코드를 지운다 |
| `http_client_engine` migration 함수 참조 | 호출하던 API Studio 코드와 함께 지운다 |

---

### Task 1: guard 범위 넓히기

- [ ] `.github/scripts/check-no-legacy.py`의 `SCOPES`에 추가한다.

```python
    "knowledge-api-studio": [
        "apps/devbox-knowledge",
        "apps/devbox-api-studio",
        "packages/knowledge-features",
        "packages/api-studio-features",
        "crates/http-client-engine",
    ],
```

  `PATTERNS`에 추가한다: `r"com\.devbox\.(knowledgebase|lifelog|everythingplus|apiplayground|webhooklab|developertoolbox)\b"`, `r"\bMigrationStartup\b"`, `r"\bMigrationSetup\b"`, `r"\bimport_plan\b"`, `r"\bimport_rows\b"`, `r"\blist_import_sources\b"`, `r"\bprepare_migration\b"`, `r"\bmigration_export\b"`, `r"\blegacy_profile\b"`.
- [ ] `ci.yml`의 guard 줄을 `python3 .github/scripts/check-no-legacy.py --scope control-center && python3 .github/scripts/check-no-legacy.py --scope knowledge-api-studio`로 바꾼다.
- [ ] Run: `python3 .github/scripts/check-no-legacy.py --scope knowledge-api-studio` → FAIL(현재 코드). 커밋: `git commit -am "test(suite): extend the legacy guard to Knowledge and API Studio"`

---

### Task 2: Knowledge

**Files:** 삭제 목록의 Knowledge 항목, `src-tauri/src/{startup,component,federation,vault_owner,lib}.rs`, `src-tauri/src/core/{stores,mod}.rs`, `src/Startup.tsx`, `src/Knowledge.tsx`

**Interfaces (Produces):**
- `core::stores::StoreKind { Notes, Activity, Search }` + `fn key(self) -> &'static str`, `core::stores::validate_store(&Connection, StoreKind) -> Result<(), String>` (기존 `import_rows::validate_owned_store`와 같은 검사)
- `startup::suite_status(app) -> Result<Value, &'static str>` (Summary JSON)

- [ ] **Step 1: 실패하는 테스트**

`startup.rs`의 기존 테스트 모듈 `suite_preparation_tests`에 추가한다(순수 계산을 떼어 테스트한다).

```rust
    #[test]
    fn health_summary_reflects_store_readiness_only() {
        let fresh = summary_flags(false, false, false);
        assert_eq!((fresh.busy, fresh.setup_selected, fresh.review_required), (false, false, false));
        let starting = summary_flags(true, true, false);
        assert!(starting.busy && starting.setup_selected && starting.review_required);
        let ready = summary_flags(false, true, true);
        assert!(!ready.busy && ready.setup_selected && !ready.review_required);
    }
```

  `summary_flags(busy: bool, store_exists: bool, ready: bool) -> Flags`(`Flags { busy, setup_selected, review_required }`)는 Step 3에서 만든다.

새 파일 `apps/devbox-knowledge/src/Startup.auto.test.tsx`로 만든다(기존 `Startup.test.tsx`는 mock 구성이 달라 섞지 않는다. 기존 파일에서 "기존 앱 데이터 가져오기"나 수동 시작 버튼을 기대하던 테스트는 Step 4에서 지운다).

```tsx
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@devbox/product-shell/api", async (original) => ({ ...(await original<typeof import("@devbox/product-shell/api")>()), nativeMode: true }));
vi.mock("@devbox/knowledge-features/transport", () => ({ componentInvoke: () => native.invoke }));
import { Startup } from "./Startup";

afterEach(() => native.invoke.mockReset());

it("starts an empty store automatically on first run", async () => {
  native.invoke.mockImplementation(async (method: string) =>
    method === "status" ? { active: false, hasExisting: false } : { active: true });
  render(<Startup><p>notes ready</p></Startup>);
  expect(await screen.findByText("notes ready")).toBeInTheDocument();
  expect(native.invoke).toHaveBeenCalledWith("start_empty");
  expect(screen.queryByText("기존 앱 데이터 가져오기")).toBeNull();
});

it("continues an existing store automatically", async () => {
  native.invoke.mockImplementation(async (method: string) =>
    method === "status" ? { active: false, hasExisting: true } : { active: true });
  render(<Startup><p>notes ready</p></Startup>);
  await screen.findByText("notes ready");
  expect(native.invoke).toHaveBeenCalledWith("continue_existing");
});

it("shows the error and a retry button instead of looping", async () => {
  native.invoke.mockImplementation(async (method: string) => {
    if (method === "status") return { active: false, hasExisting: false };
    throw Object.assign(new Error("저장소를 준비하지 못했습니다."), { name: "store_unavailable" });
  });
  render(<Startup><p>notes ready</p></Startup>);
  expect(await screen.findByRole("alert")).toHaveTextContent("저장소를 준비하지 못했습니다.");
  await waitFor(() => expect(native.invoke).toHaveBeenCalledTimes(2));
  expect(screen.getByRole("button", { name: "다시 시도" })).toBeEnabled();
});
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-knowledge --lib startup && pnpm --filter devbox-knowledge exec vitest run src/Startup.auto.test.tsx` → FAIL.

- [ ] **Step 3: native 구현**
  - `core/stores.rs`: `import_rows.rs`에서 `Source` enum(이름을 `StoreKind`로)·`key()`·`validate_owned_store`(이름을 `validate_store`로)와 이들이 쓰는 schema 상수·도우미를 옮기고 테스트도 옮긴다. `startup.rs`의 `validate_prepared_stores`는 `StoreKind::{Notes, Activity, Search}`와 `stores::validate_store`를 쓴다.
  - 삭제 목록의 Knowledge native 파일을 지우고 `lib.rs`·`core/mod.rs` 선언을 정리한다.
  - `startup.rs`
    - `Startup.legacy` 필드와 `initialize`의 `let legacy = …`, `crate::migration::initialize(…)` 호출을 지운다. `paused_migration`은 없어지고 `paused_binding`만 본다.
    - `binding()`: `let legacy = …`를 `let external = …`로 이름만 바꾸고, `if legacy && approval.is_none() { import_rows::verify_legacy_binding(…)?; }`를 `if external && approval.is_none() { return Err("vault_binding_invalid".into()); }`로 바꾼다. 반환 튜플의 두 번째 값도 `external`.
    - `owner()`: `legacy_path`(com.devbox.knowledgebase) 계산과 그에 딸린 approval 재확인 블록을 지우고 `vault_owner::acquire(&state.lease_base, &vault)`를 부른다.
    - `dispatch`: `migration::METHODS` 분기와 `finish_recovery` 호출을 지운다. `status` 응답에서 `scheduled`를 지운다.
    - 아래 함수를 추가한다.

```rust
#[derive(Debug, PartialEq, Eq)]
struct Flags {
    busy: bool,
    setup_selected: bool,
    review_required: bool,
}

fn summary_flags(busy: bool, store_exists: bool, ready: bool) -> Flags {
    Flags {
        busy,
        setup_selected: store_exists,
        review_required: store_exists && !ready,
    }
}

/// Suite health observation. No import plans remain; readiness is the store.
pub(crate) fn suite_status(app: &tauri::AppHandle) -> Result<Value, &'static str> {
    let state = app.try_state::<Startup>().ok_or("migration_unavailable")?;
    let busy = state.operation.load(Ordering::Acquire);
    let exists = stores::read(&state.root)
        .map_err(|_| "migration_unavailable")?
        .is_some();
    let flags = summary_flags(busy, exists, require_ready(app).is_ok());
    let native = serde_json::to_vec(&(flags.busy, flags.setup_selected, flags.review_required))
        .map_err(|_| "migration_unavailable")?;
    let summary = product_contract::migration_status::Summary::new(
        "knowledge",
        env!("CARGO_PKG_VERSION"),
        flags.busy,
        flags.setup_selected,
        flags.review_required,
        &native,
    )?;
    serde_json::to_value(summary).map_err(|_| "migration_unavailable")
}
```

  (`mappings`는 붙이지 않는다. 옛 `with_mappings(import_plan::mapping_summary(…))` 호출은 지운 파일과 함께 사라진다.)
  - `vault_owner.rs`: `acquire`의 세 번째 인자(legacy SQLite 경로)와 그 경로의 쓰기 핸들 감시·quiesce 코드를 지운다. 파일 머리 주석의 "Legacy SQLite write handles must quiesce…" 문장을 지운다.
  - `component.rs` `allowed`: `"knowledge.migration"` 팔을 `route == "notes" && (crate::startup::COMMANDS.contains(&method) || crate::vault_binding::METHODS.contains(&method))`로 줄인다. 테스트의 import 메서드 기대값도 고친다.
  - `federation.rs`: `Call::ReadMigrationStatus {}`는 `crate::startup::suite_status(&app)`, `VerifyMigrationSources`·`ListMigrationBackups`·`VerifyMigrationBackup`은 `Err("migration_retired")`. `crate::migration::operation_rows(&app)?`는 `Vec::new()`.

- [ ] **Step 4: 프런트 구현** — `apps/devbox-knowledge/src/Startup.tsx`
  - `MigrationSetup` lazy import, `showImport` state, "기존 앱 데이터 가져오기" 버튼과 그 분기를 지운다.
  - 상태 확인 뒤 저장소가 활성·준비 상태가 아니고, 오류·노트 폴더 변경·재연결 대기가 없으면 `start()`를 **한 번** 자동 실행한다(실패하면 다시 자동 실행하지 않는다).

```tsx
  const [autoStarted, setAutoStarted] = useState(false);
  useEffect(() => {
    if (!nativeMode || loading || active || autoStarted || error || showVault || reconnect || blocked) return;
    setAutoStarted(true);
    void start();
  }, [loading, active, autoStarted, error, showVault, reconnect, blocked]);
```

  `start` 함수는 이 effect보다 위(컴포넌트 본문, 조건부 return 앞)로 옮기고 `useCallback` 없이 그대로 둔다(자동 실행은 `autoStarted`로 한 번만). 시작 화면의 문구는 "저장소를 준비하고 있습니다…"로, 버튼은 오류가 있을 때만 "다시 시도"로 보인다.
  - `Knowledge.tsx`에서 `MigrationSettings`를 쓰는 곳이 있으면 지운다.

- [ ] **Step 5: 통과 확인** — Run: `cargo test -p devbox-knowledge --lib && cargo test -p devbox-knowledge-vault-engine --lib && pnpm --filter devbox-knowledge exec vitest run` → PASS. (`devbox-knowledge-vault-engine`는 `vault_owner` 변경 영향 확인용.)

- [ ] **Step 6: 커밋** — `git add -A && git commit -m "refactor(devbox-knowledge): remove v0.7 imports and start stores automatically"`

---

### Task 3: API Studio

**Files:** 삭제 목록의 API Studio 항목, `src-tauri/src/{component,federation,lib,service_worker}.rs`, `src-tauri/src/platform/mod.rs`, `src/Studio.tsx`, `src/transport.ts`, `apps/products.json`, `crates/http-client-engine/src/commands/mod.rs`

**Interfaces (Produces):** `crate::federation::status_summary() -> Result<Value, &'static str>` (항상 준비됨)

- [ ] **Step 1: 실패하는 테스트**

`federation.rs` 테스트 모듈(없으면 생성):

```rust
    #[test]
    fn api_studio_reports_a_ready_store_without_imports() {
        let value = status_summary().unwrap();
        assert_eq!(value["setupSelected"], true);
        assert_eq!(value["reviewRequired"], false);
        assert_eq!(value["busy"], false);
        assert!(value.get("mappings").is_none_or(serde_json::Value::is_null));
    }
```

`apps/devbox-api-studio/src/Studio.native.test.tsx`에 추가한다(기존 mock 재사용).

```tsx
it("opens the requests screen without a migration step", async () => {
  // 기존 도우미로 describe와 execute mock을 설정한다. api-studio.migration 호출이 오면 실패시킨다.
  native.invoke.mockImplementation(async (command: string, args?: { request?: { component?: string } }) => {
    if (command === "plugin:product-shell|describe") return fixtureDescription("api-studio");
    if (args?.request?.component === "api-studio.migration") throw new Error("migration must not be called");
    return defaultReply(command, args); // 파일의 기존 기본 응답 도우미 이름으로 바꾼다
  });
  render(<Studio />);
  expect(await screen.findByRole("navigation", { name: "제품 화면" })).toBeInTheDocument();
  expect(screen.queryByText(/기존 데이터를 확인/)).toBeNull();
});
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-api-studio --lib federation && pnpm --filter devbox-api-studio exec vitest run src/Studio.native.test.tsx` → FAIL.

- [ ] **Step 3: 구현**
  - 삭제 목록의 API Studio native·프런트 파일과 `crates/http-client-engine/src/commands/migration.rs`를 지우고 `mod` 선언을 정리한다.
  - `component.rs`: `api-studio.migration` 분기(`if request.component == "api-studio.migration" { … }`)와 `crate::migration::require_active(app)` 검사, `allowed`의 migration 팔을 지운다. `plugin()` setup의 `crate::migration::initialize(app)`를 지운다.
  - `federation.rs`: 아래 함수를 추가하고 `ReadMigrationStatus`에 쓴다. 나머지 세 legacy Call은 `Err("migration_retired")`.

```rust
pub(crate) fn status_summary() -> Result<Value, &'static str> {
    // API Studio keeps its data in the product's own stores from the first
    // start; there is no setup choice left to review.
    let summary = product_contract::migration_status::Summary::new(
        "api-studio",
        env!("CARGO_PKG_VERSION"),
        false,
        true,
        false,
        b"api-studio-store-ready",
    )?;
    serde_json::to_value(summary).map_err(|_| "migration_unavailable")
}
```
  - `service_worker.rs`가 migration 상태를 보던 곳이 있으면 지운다.
  - `src/Studio.tsx`: `MigrationStartup` 래퍼를 지운다. `src/transport.ts`: `migrationFailure` import와 `component === "api-studio.migration"` 분기를 지운다. `routeFor`와 `Component` 타입에서 `"api-studio.migration"`을 지운다(`packages/api-studio-features/src/transport.ts`의 `Component` 유니온 포함).
  - `apps/products.json`: `components`에서 `api-studio.migration`을 지우고 `catalogRevision` +1. 하드코딩된 revision을 P1-02와 같은 방법(`rg`)으로 갱신한다.
  - `Studio.delivery.test.tsx` 등에서 migration gate를 기대하던 테스트를 지운다.

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-api-studio --lib && cargo test -p devbox-http-client-engine --lib && pnpm --filter devbox-api-studio --filter @devbox/api-studio-features exec vitest run` → PASS.

- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-api-studio): remove the v0.7 import startup"`

---

### Task 4: CI·문서

- [ ] `.github/workflows/product-foundation.yml`: paths의 `windows-api-migration.mjs`·`windows-knowledge-migration.mjs`를 지우고, "Verify API Studio migration from pinned legacy storage" step을 지운다. "Verify Knowledge migration, vault ownership and window lifetime" step은 `windows-knowledge-migration.mjs`에서 가져오기 시나리오(legacy fixture 설치·import 적용·rollback)를 뺀 나머지(노트 폴더 소유·창 수명)를 `windows-knowledge-lifecycle.mjs`로 옮겨 부르고 step 이름을 "Verify Knowledge vault ownership and window lifetime"으로 바꾼다. `test-product-foundation-workflow.py`의 기대값을 맞춘다.
- [ ] `apps/devbox-knowledge/README.md`, `apps/devbox-api-studio/README.md`의 가져오기 절을 지우고 "처음 실행하면 바로 시작한다"로 바꾼다.
- [ ] Run: `python3 .github/scripts/check-no-legacy.py --scope knowledge-api-studio && python3 .github/scripts/test-product-foundation-workflow.py` → PASS.
- [ ] 커밋: `git add -A && git commit -m "ci(suite): drop Knowledge and API Studio import acceptance"`

---

### Task 5: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Knowledge 데이터 폴더(`%LOCALAPPDATA%\com.devbox.v08.knowledge.i*`)를 백업하고 지운 뒤 Knowledge를 열면 시작 화면 없이 노트가 뜬다. 백업을 되돌리면 기존 노트가 그대로 열린다. API Studio도 가져오기 화면 없이 열린다.
