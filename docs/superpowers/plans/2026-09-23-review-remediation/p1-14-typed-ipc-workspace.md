# P1-14 Workspace를 타입 IPC로(PR 2개) — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** 약 3,000줄의 Workspace `component.rs`(문자열 허용 목록 → if/else 체인 → engine 문자열 dispatch)를 component별 타입 command로 나누고, 흩어진 동시 실행 한도(`Pool` + `Semaphore`와 숫자 2/4/8…)와 프런트의 "이 component면 deadline +24초" 목록을 Rust의 선언형 정책 하나로 모은다(A1 권고 1–3, A2, A3). 크기 때문에 PR을 둘로 나눈다.

**Architecture:**
- **실행 차로(lane)**: `Runtime`의 pool·semaphore를 `Lane` enum(`Terminal`, `TerminalIo`, `TerminalStop`, `Engine`, `EngineStop`, `Source`, `Files`, `Lsp`, `Metadata`, `Probes`, `Dialogs`, `Context`)과 `lane_policy(lane) -> LanePolicy { requests: usize, workers: usize }` 표로 바꾼다. 각 call enum이 `fn lane(&self) -> Lane`을 선언하고, command는 `admit` 다음에 `runtime.enter(lane)`로 permit을 받는다. 숫자는 **지금 값 그대로** 표에 옮긴다(동작 불변).
- **deadline 예산**: 각 call enum이 `fn deadline_budget_ms(&self) -> u64`(기본 5,000, 긴 작업 29,000)를 선언하고, 생성 TS `deadlineBudgets`로 프런트가 header의 `deadlineMs`를 정한다. native `authorize`의 30초 상한은 그대로다.
- **component command**: PR A(`runtime`, `processes`, `process_actions`, `logs`, `terminal`, `problems`, `commands`), PR B(`files`, `lsp`, `source`, `registry`, `setup`, `definitions`, `dependencies`)로 나누고, PR B 끝에서 `execute`를 지운다. `workspace.migration`은 `workspace.setup`이 된다.
- engine API: `runtime_engine::api`(runtime·processes·process_actions·logs에 해당하는 engine 메서드), `terminal_engine::api`, `repositories_engine::api::SourceCall`(`SOURCE_COMMANDS`), `editor_engine::api`(파일·LSP engine 메서드), `projects_engine::api`(정의·registry engine 메서드), `ports_engine::api`, `logs_engine::api`. 각 engine의 shim·`#[tauri::command]`를 지운다.

**Tech Stack:** Rust, ts-rs 12, Tauri v2, TypeScript

**Spec:** `review.md` §6 A1·A2·A3, §5 P5 · `00-roadmap.md` D17·D18 · ADR 0014

## Global Constraints

- `00-roadmap.md` §3 전부 적용. P1-11 규칙 그대로.
- **메서드 목록의 기준은 지금 코드의 허용 함수다**(빠짐없이 옮긴다): `component.rs` `allowed()`와 `migration_method()`(P1-04 이후), `runtime_host::allowed`, `terminal_host::wsl_management`, `development_host::Sessions::handles`, `source_host::management`, `repositories_engine::component::SOURCE_COMMANDS`, `files_host::allowed`, `lsp_host::allowed`, 정의·의존성·registry 메서드 목록. 각 PR 첫 과제에서 이 함수들이 허용하는 (component, route, method) 전체를 표로 뽑아 테스트 fixture로 고정하고, 옮긴 뒤 같은 표가 나오는지 확인한다.
- 인자 크기 상한: 기존 64MiB(파일·로그·LSP 본문), 2MiB(정의·런타임), 64KiB(나머지)는 typed 역직렬화 전에 확인할 수 없으므로, 본문 필드에 크기 검사를 둔다(`String` 필드 길이 확인 → `component_args_invalid`). 전체 요청은 Tauri IPC가 받는다(P5: 다시 직렬화해 크기를 재던 비용이 없어진다).
- 종료 중(`shutdown_started`)이면 모든 command가 `Unavailable`로 거부한다(기존).
- 인자·결과 JSON 모양은 바꾸지 않는다.

## Review Focus

1. 옮기기 전후의 (component, route, method) 허용 표가 같다(삭제한 v0.7 메서드 제외). (PR A·B 첫 과제 테스트)
2. 터미널 출력 읽기가 많은 중에도 "터미널 중지"가 들어간다(`TerminalStop` 차로가 따로). 런타임 작업 20개 실행 중에도 "작업 중지"가 들어간다(`EngineStop`). (PR A 테스트)
3. 파일 저장 대기 중 다른 저장·미리보기가 순서를 지키고(`Files` 차로 + filesystem permit), LSP 작업이 파일 저장을 막지 않는다. (PR B 테스트)
4. 긴 작업(WSL 목록·WSL 미리보기·Git fetch·의존성 보강·런타임 시작)이 5초 기본 deadline이 아니라 29초 예산으로 나간다(프런트 목록이 Rust 예산으로 바뀜). (PR A·B 테스트)
5. activation `import` 단계에서 `workspace.setup.start_empty`가 허용되고, 그 밖의 command는 `Unavailable`. (PR B)

---

## PR A — 런타임·터미널 계열

- 묶음: **B7** — 브랜치 `refactor/devbox-workspace/typed-ipc`, PR 제목 `refactor(devbox-workspace): typed IPC for runtime, terminal, files, source and projects`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(devbox-workspace): typed commands for runtime, terminal and problems`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

### Task A1: 허용 표 고정

**Files:** Create `apps/devbox-workspace/src-tauri/src/ipc/allow_table.rs`(테스트 전용), `apps/devbox-workspace/src-tauri/tests/fixtures/allow-table.json`

- [ ] **Step 1: 표 뽑기** — 테스트 모듈에서 후보 (component, route, method) 조합을 만들어 지금 `allowed()`가 true인 것만 모은다. 후보 메서드 이름은 각 허용 함수의 `matches!`·`COMMANDS` 목록에서 모은다(`rg -o '"[a-z_]+"' <파일>`로 문자열을 뽑아 후보로 쓴다). route 후보는 `apps/products.json`의 Workspace route 9개.

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn allow_table_matches_the_committed_fixture() {
        let table = super::current_allow_table(); // Vec<(String, String, String)> sorted
        let expected: Vec<(String, String, String)> =
            serde_json::from_str(include_str!("../../tests/fixtures/allow-table.json")).unwrap();
        assert_eq!(table, expected);
    }
}
```

  처음 실행 때 `current_allow_table()` 결과를 fixture로 저장한다(`UPDATE_ALLOW_TABLE=1` 환경 변수가 있으면 파일을 쓰게 하고, CI에서는 비교만 한다). 이 fixture가 두 PR 동안 "옮긴 뒤에도 같은 권한"의 기준이다. PR B가 끝나 `allowed()`가 사라지면 `current_allow_table()`은 새 enum들의 `METHODS`·`routes()`에서 같은 표를 만들도록 바꾼다.

- [ ] **Step 2: 커밋** — `git add -A && git commit -m "test(devbox-workspace): pin the component allow table"`

### Task A2: 실행 차로와 deadline 예산

**Files:** Create `apps/devbox-workspace/src-tauri/src/ipc/lanes.rs`; Modify `component.rs`(Runtime 필드)

**Interfaces (Produces):** `Lane` enum, `LanePolicy { requests: usize, workers: usize }`, `const fn lane_policy(Lane) -> LanePolicy`, `Runtime::enter(&self, Lane) -> Result<LanePermit, &'static str>`(요청 permit + worker semaphore permit), `const DEFAULT_BUDGET_MS: u64 = 5_000`, `const LONG_BUDGET_MS: u64 = 29_000`

- [ ] **Step 1: 실패하는 테스트** — `lanes.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policies_keep_the_existing_limits() {
        assert_eq!(lane_policy(Lane::TerminalIo), LanePolicy { requests: 2, workers: 4 });
        assert_eq!(lane_policy(Lane::Terminal), LanePolicy { requests: 2, workers: 4 });
        assert_eq!(lane_policy(Lane::TerminalStop), LanePolicy { requests: 2, workers: 2 });
        assert_eq!(lane_policy(Lane::Engine), LanePolicy { requests: 2, workers: 4 });
        assert_eq!(lane_policy(Lane::EngineStop), LanePolicy { requests: 2, workers: 2 });
        assert_eq!(lane_policy(Lane::Source), LanePolicy { requests: 2, workers: 2 });
        assert_eq!(lane_policy(Lane::Files), LanePolicy { requests: 2, workers: 2 });
        assert_eq!(lane_policy(Lane::Lsp), LanePolicy { requests: 2, workers: 2 });
    }

    #[tokio::test]
    async fn a_saturated_lane_does_not_block_its_stop_lane() {
        let lanes = Lanes::default();
        let busy: Vec<_> = (0..2).map(|_| lanes.try_enter(Lane::Engine).unwrap()).collect();
        assert!(lanes.try_enter(Lane::Engine).is_err());
        assert!(lanes.try_enter(Lane::EngineStop).is_ok());
        drop(busy);
    }
}
```

  (숫자는 지금 `Runtime::default()`의 `Semaphore::new(n)`과 `Pool::reserve()`(기본 2)·`reserve_with_limit(n)` 호출에서 옮긴다. 위 값과 다르면 코드 값을 따른다. `reserve_with_limit`로 다른 한도를 쓰는 곳은 표에 별도 차로로 만든다.)

- [ ] **Step 2: 구현** — `lanes.rs`에 `Lane`, `LanePolicy`, `lane_policy`, `Lanes { pools: [Pool; N], workers: [Arc<Semaphore>; N] }`, `try_enter`(요청 permit), `enter`(요청 permit + worker permit을 async로)를 만들고, `Runtime`의 개별 필드(`terminal_requests`, `terminal_io_workers`, …)를 `lanes: Lanes` 하나로 바꾼다. 기존 코드의 `runtime.terminal_io_requests.reserve()` 같은 호출은 `runtime.lanes.try_enter(Lane::TerminalIo)`로 바꾼다(이 단계는 동작 불변 리팩터링).
- [ ] **Step 3: 통과 확인·커밋** — Run: `source ~/.cargo/env && cargo test -p devbox-workspace --lib` → PASS(Task A1 표 테스트 포함). `git commit -am "refactor(devbox-workspace): declare execution lanes in one table"`

### Task A3: engine API(runtime·terminal·ports·logs)

- [ ] 각 engine에 `api.rs`를 만든다(P1-11 Task 3 방식): `runtime_engine::api::{RuntimeCall, ProcessCall, ProcessActionCall, LogsCall}`(각 목록은 `runtime_host::allowed`가 허용하는 engine 메서드; `legacy_control_method`가 막던 메서드는 enum에 넣지 않는다), `terminal_engine::api::TerminalCall`, `ports_engine::api::PortsCall`(processes가 쓰는 경우), `logs_engine::api::LogsCall`. 각 enum에 `lane()`(중지·취소 계열은 `EngineStop`/`TerminalStop`, 출력 읽기는 `TerminalIo`, 나머지는 `Engine`/`Terminal`)과 `deadline_budget_ms()`(현재 프런트 `native.ts:212`의 긴 작업 규칙: `workspace.runtime`·`workspace.processes`·`workspace.process-actions`·`workspace.logs`·`workspace.terminal` 전체는 29,000)를 둔다. 테스트: 각 enum의 목록·파싱·lane·예산.
- [ ] Run: `cargo test -p devbox-runtime-engine -p devbox-terminal-engine -p devbox-ports-engine -p devbox-logs-engine --lib` → PASS. 커밋: `git commit -am "refactor(devbox-workspace): typed APIs for runtime, terminal, ports and logs engines"`

### Task A4: host command 일곱 개

**Files:** Create `apps/devbox-workspace/src-tauri/src/ipc/{mod,runtime,terminal,problems,commands}.rs`; Modify `component.rs`, `lib.rs`, `capabilities/*.json`

- [ ] **Step 1: 실패하는 테스트** — `ipc/mod.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::ComponentCall;

    #[test]
    fn runtime_family_routes_lanes_and_budgets() {
        let stop: runtime::WorkspaceRuntimeCall = serde_json::from_str(r#"{"method":"stop_task","args":{"taskId":"t"}}"#).unwrap();
        assert_eq!(stop.lane(), lanes::Lane::EngineStop);
        assert_eq!(stop.deadline_budget_ms(), lanes::LONG_BUDGET_MS);
        assert_eq!(stop.routes(), &["tasks"]);
        let dashboard: terminal::WorkspaceTerminalCall = serde_json::from_str(r#"{"method":"dashboard_snapshot","args":{}}"#).unwrap();
        assert_eq!(dashboard.routes(), &["terminal", "runtime"]);
        let problems: problems::ProblemsCall = serde_json::from_str(r#"{"method":"snapshot","args":{}}"#).unwrap();
        assert!(problems.routes().contains(&"problems") && problems.routes().contains(&"files"));
    }
}
```

  (`stop_task`·인자 이름은 runtime engine의 실제 중지 메서드로 바꾼다.)

- [ ] **Step 2: 구현** — 기존 `execute`의 terminal·runtime·processes·process-actions·logs·problems·commands 분기(`execute_terminal_main`, `execute_runtime` 등 도우미 함수)를 각 command로 옮긴다. 각 command: `if runtime.shutdown_started { Unavailable }` → `admit` → `runtime.lanes.enter(call.lane())` → 기존 도우미 호출(문자열 method 대신 enum) → `admission.finish(result, classify)`. host 전용 메서드(터미널 세션 관리 `terminal_sessions`·`open_terminal_profile`…, 개발 세션 7개, WSL 관리 6개, `workspace_task_source`, `open_webhook_log`)는 host enum. `lib.rs`의 `invoke_handler`에 새 command를 `execute`와 함께 등록하고, `execute`의 해당 분기와 허용 목록 팔을 지운다.
- [ ] **Step 3: 프런트** — `apps/devbox-workspace/src/native.ts`의 `componentCall`에 component → command map을 두고 옮긴 component는 새 command로 보낸다. deadline: `header.deadlineMs = now + (deadlineBudgets[component]?.[method] ?? 5000)`(생성 파일 `deadline-budgets.ts`, Task A5)로 바꾸고, `native.ts:212`의 하드코딩 조건문에서 옮긴 component 부분을 지운다. 이 PR에서 옮긴 기능의 `api.ts`를 `typedCall`로 바꾼다(`packages/workspace-features/src/{runtime,tasks,logs,terminal}/**`, `apps/devbox-workspace/src/{Terminal*,Problems,NativeRuntimeRoutes}.tsx`).
- [ ] **Step 4: 통과 확인·커밋** — Run: `cargo test -p devbox-workspace --lib && pnpm --filter devbox-workspace --filter @devbox/workspace-features exec vitest run && pnpm --filter devbox-workspace --filter @devbox/workspace-features exec tsc --noEmit` → PASS. `git add -A && git commit -m "refactor(devbox-workspace): route runtime, terminal and problems through typed commands"`

### Task A5: 생성·문구

- [ ] `apps/devbox-workspace/src-tauri/tests/typescript.rs`(출력 `packages/workspace-features/src/generated`)를 만들고 옮긴 enum·issue·결과 map과 `deadline-budgets.ts`(`export const deadlineBudgets: Partial<Record<Component, Partial<Record<string, number>>>> = {…}` — `LONG_BUDGET_MS`인 메서드만)를 쓴다. `check-generated-bindings.sh`에 Workspace 줄을 추가한다.
- [ ] `native.ts`의 `issues` 사전과 `runtimeIssues.ts`·`wslIssues.ts`에서 옮긴 component 코드를 component별 `Record<…Issue, string>` 카탈로그(`packages/workspace-features/src/issues/{runtime,terminal,problems}.ts`)로 옮긴다. 카탈로그 테스트(P1-12 Task 5 Step 1과 같은 모양)를 추가한다.
- [ ] Run: `bash .github/scripts/check-generated-bindings.sh && pnpm --filter @devbox/workspace-features exec vitest run src/issues` → PASS. 커밋: `git commit -am "build(devbox-workspace): generate runtime and terminal bindings"`

### Task A6: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. PR 본문 "Windows 실기 확인"(사용자 확인 대기): 작업 시작·중지·로그 보기, 프로세스 목록·종료, 터미널 열기·입력·중지·복원, WSL 대시보드, 문제 목록.

---

## PR B — 파일·소스·프로젝트 계열과 execute 제거

- 묶음: **B7** — 브랜치 `refactor/devbox-workspace/typed-ipc`, PR 제목 `refactor(devbox-workspace): typed IPC for runtime, terminal, files, source and projects`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(devbox-workspace): typed commands for files, source and projects`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

### Task B1: engine API(editor·repositories·projects)

- [ ] `editor_engine::api::{FilesCall, LspCall}`(파일·LSP engine 메서드), `repositories_engine::api::SourceCall`(`SOURCE_COMMANDS` 전부), `projects_engine::api::{DefinitionsCall, RegistryEngineCall, DependenciesCall}`을 P1-11 방식으로 만든다. lane: 파일 → `Files`, LSP → `Lsp`, 소스 → `Source`, 정의·registry → `Metadata`, 대화상자(`pick_*`) → `Dialogs`, 파일 시스템 조사 → `Probes`. 예산: `files`의 `reconnect_wsl_files, open_file, send_editor_selection`, `registry`의 `list_wsl_distros, preview_wsl, apply_registration, cancel_registration, select_project, clear_project`, `source`·`lsp`·`dependencies` 전체는 29,000(현재 `native.ts:212` 규칙). 테스트: 목록·파싱·lane·예산.
- [ ] Run: `cargo test -p devbox-editor-engine -p devbox-repositories-engine -p devbox-projects-engine --lib` → PASS. 커밋.

### Task B2: host command 일곱 개와 execute 제거

**Files:** Create `ipc/{files,lsp,source,registry,setup,definitions,dependencies}.rs`; Delete `component.rs`의 `execute`·`allowed`·`migration_method`와 이를 위한 도우미(옮긴 뒤 남는 공용 도우미는 `ipc/mod.rs`로)

- [ ] **Step 1: 실패하는 테스트** — `ipc/mod.rs`에 추가

```rust
    #[test]
    fn setup_is_the_only_component_allowed_while_the_suite_awaits_activation() {
        let setup: setup::SetupCall = serde_json::from_str(r#"{"method":"start_empty","args":{}}"#).unwrap();
        assert_eq!(setup::SetupCall::COMPONENT, "workspace.setup");
        assert!(setup::SetupCall::IMPORT_PHASE);
        let files: files::WorkspaceFilesCall = serde_json::from_str(r#"{"method":"list_directory","args":{"path":"."}}"#).unwrap();
        assert_eq!(files.lane(), lanes::Lane::Files);
    }

    #[test]
    fn the_allow_table_is_unchanged() {
        assert_eq!(super::allow_table::current_allow_table(), super::allow_table::committed());
    }
```

  (`list_directory`는 files의 실제 메서드 이름으로 바꾼다. `IMPORT_PHASE`는 이 enum이 activation import 단계에서 `authorize_owner_migration` 대신 쓰는 표시다 — 아래 Step 2.)

- [ ] **Step 2: 구현**
  - `crates/product-ipc`의 `ComponentCall`에 `const IMPORT_PHASE: bool = false;`를 추가하고, `admit`은 이 값이 true면 `authorize_owner_migration`(import 단계 허용 admission)을 쓴다. P1-04 이후 이 admission을 쓰는 곳은 Workspace setup뿐이다. `product_contract::activation::Activation::allows`에서 `{product}.migration` 허용 줄을 지운다(Knowledge는 P1-12에서 `.setup`으로 옮겼고, API Studio는 P1-03에서 component가 없어졌다).
  - 기존 `execute`의 files·lsp·source·registry·migration·definitions·dependencies 분기를 각 command로 옮긴다. files는 filesystem permit(`runtime.filesystem_permit(write, deadline)`), LSP는 문서 관찰(`document_observation`)과 problems 소유자 알림, registry의 `select_project`·`clear_project`는 `product_shell_tauri::replace_project_context`를 그대로 쓴다.
  - `lib.rs`의 `invoke_handler`에서 `execute`를 지우고 14개 command만 남긴다. capability 갱신.
  - catalog: `workspace.migration` → `workspace.setup`(authority `store-setup`), `catalogRevision` +1, fixture 갱신.
- [ ] **Step 3: 프런트** — 남은 component를 command map에 넣고 `plugin:workspace|execute`를 지운다. `native.ts:212`의 조건문을 지우고 `deadlineBudgets`만 쓴다. `nativeCall("workspace.migration", …)`(RegistryGate)은 `setupCall`로. 남은 기능 `api.ts`(`files`, `source`, `dependencies`, `overview`)를 `typedCall`로 바꾸고, `native.ts`의 `issues` 사전을 component별 카탈로그로 옮긴 뒤 파일에서 지운다.
- [ ] **Step 4: 통과 확인·커밋** — Run: `cargo test -p devbox-workspace -p product-contract -p product-shell-tauri --lib && bash .github/scripts/check-generated-bindings.sh && pnpm --filter devbox-workspace --filter @devbox/workspace-features exec vitest run && pnpm --filter devbox-workspace --filter @devbox/workspace-features exec tsc --noEmit && ! rg -n 'plugin:workspace\|execute' apps packages` → PASS. `git add -A && git commit -m "refactor(devbox-workspace): remove the string execute command"`

### Task B3: 정리

- [ ] `rg -n "__component_|#\[tauri::command\]" crates/*-engine crates/webhook-host crates/installation-tools` → 등록된 command만 남았는지 본다(engine에는 0건이어야 한다). 남은 것이 있으면 지운다(A3 완료).
- [ ] `docs/adr/0014-typed-ipc.md`에 "네 제품 모두 전환 완료, 실행 차로 표와 deadline 예산은 Rust가 소유"를 추가한다.
- [ ] 커밋: `git commit -am "docs(workspace): record the completed typed IPC migration"`

### Task B4: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9. 전체 범위라 `pnpm verify:all`.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 프로젝트 등록(Windows·WSL)·선택·해제, 파일 열기·저장·이름 바꾸기, LSP 진단·이동, Git 상태·diff·stage·commit·fetch, 정의 신뢰·편집, 의존성 목록·보강, 첫 실행 자동 시작.
