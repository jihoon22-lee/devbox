# P1-13 API Studio·Control Center를 타입 IPC로 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** API Studio(요청·웹훅·변환)와 Control Center(도구·배포)의 문자열 `execute` 명령을 P1-11 방식의 component별 타입 command로 바꾸고, API Studio의 문자열 오류 투영(`component_errors.rs` + `component-errors.json`)을 안정 코드 enum과 문구 카탈로그로 대체한다(A1·A2·A3, D17·D18).

**Architecture:** API Studio: `api`, `webhooks`, `transforms` command. engine enum은 `http_client_engine::api::ApiCall`(52), `webhook_host::api::WebhookCall`(22), `toolbox_engine::api::ToolboxCall`(16). host 전용 메서드(API 작업공간, Knowledge 초안, handoff 전송·탐색, 모의 응답 초안, 창 수명)는 host enum. 취소·중지·연결 해제는 `ExecutionClass::Control`로 선언해 포화 중에도 들어오게 한다(기존 `CONTROL_COMMANDS`). Control Center: `tools`(installation-tools), `delivery`(Suite 설치·업데이트·복구) command. 이미 타입 command인 launcher(`commands.rs`)는 그대로 둔다.

**Tech Stack:** Rust, ts-rs 12, Tauri v2, TypeScript

**Spec:** `review.md` §6 A1·A2·A3 · `00-roadmap.md` D17·D18 · ADR 0014

## Global Constraints

- `00-roadmap.md` §3 전부 적용. P1-11 규칙(와이어 호환, 권한, 생성 파일) 그대로.
- route 규칙을 메서드마다 옮긴다(현재 `allowed()`와 같게):
  - `api`: route `requests`·`protocols`·`history`. 단 Knowledge 초안 5개, handoff 전송(`send_selection_to_toolbox`, `send_mock_draft`)은 `protocols`에서 금지 → 해당 variant의 `routes()`는 `["requests", "history"]`.
  - `webhooks`: `webhooks`. `transforms`: `transforms`.
  - Control Center `tools`: 메서드별(`dev_setup_*` → `environment`, `related_*` → `tools`, 진단·지원 번들 → `diagnostics`·`recovery`). `delivery`: 기존 `tools_host.rs`의 route 조건 그대로(업데이트 계열 `updates`·`recovery`, 복원 `recovery`, `record_suite_health` → `updates`·`recovery` 등).
- `Control` 등급(기존 `apps/devbox-api-studio/src-tauri/src/component.rs:21` `CONTROL_COMMANDS` 전부): `cancel_request, cancel_mcp_http, disconnect_mcp_http, cancel_mcp_oauth, cancel_mcp_stdio, disconnect_mcp_stdio, cancel_grpc, disconnect_grpc, stop_sse_stream, close_websocket, disconnect_websocket, stop_server` 등 — 파일의 목록을 그대로 옮긴다.
- API Studio의 migration 관련 요소는 P1-03에서 이미 없다.

## Review Focus

1. 요청 64개가 진행 중(포화)일 때 "요청 취소"·"MCP 연결 해제"가 거부되지 않는다(Control 풀). (Task 1 테스트)
2. `protocols` 화면에서 Knowledge 초안 저장 호출 → `Unauthorized`(기존과 같은 route 제한). (Task 2 테스트)
3. 기존 `component-errors.json`에 있던 오류 문구가 카탈로그에 빠짐 없이 옮겨진다(생성 유니온 + `Record`). (Task 3)
4. Control Center 업데이트 다운로드 중 "취소"가 즉시 받아들여진다(`cancel_suite_update`는 Control). (Task 4)
5. 두 제품 모두 `execute` 명령이 사라지고, 남은 호출이 없다(`rg`). (Task 5)

## Branch · PR

- 묶음: **B6** — 브랜치 `refactor/suite/typed-ipc-knowledge-api-control`, PR 제목 `refactor(suite): typed IPC for Knowledge, API Studio and Control Center`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(suite): typed commands for API Studio and Control Center`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: engine API 세 개

**Files:** Create `crates/http-client-engine/src/api.rs`, `crates/webhook-host/src/api.rs`, `crates/toolbox-engine/src/api.rs`; Modify 각 `component.rs`, `commands/*.rs`, `Cargo.toml`

- [ ] **Step 1: 실패하는 테스트** — 각 `api.rs`에 P1-11 Task 3과 같은 "목록·파싱" 테스트를 둔다. http-client-engine에는 등급 테스트를 더한다.

```rust
    #[test]
    fn cancel_and_disconnect_methods_use_the_control_pool() {
        use product_ipc::{ComponentCall as _, ExecutionClass};
        for json in [
            r#"{"method":"cancel_request","args":{"requestId":"r"}}"#,
            r#"{"method":"disconnect_mcp_stdio","args":{"sessionId":"s"}}"#,
        ] {
            let call: ApiCall = serde_json::from_str(json).unwrap();
            assert_eq!(call.class(), ExecutionClass::Control, "{json}");
        }
        let send: ApiCall = serde_json::from_str(r#"{"method":"send_request","args":{"request":{}}}"#).unwrap_or_else(|_| panic!("adjust sample args to SendRequest"));
        assert_eq!(send.class(), ExecutionClass::Normal);
        assert_eq!(API_METHODS.len(), 52);
    }
```

  (샘플 인자는 각 shim의 `Input` 구조체를 보고 실제 필수 필드로 채운다. `ApiCall`이 `ComponentCall`을 직접 구현하지 않으면 `fn class(&self) -> ExecutionClass`를 inherent 메서드로 두고 host 합성 enum이 위임한다.)

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-http-client-engine -p devbox-webhook-host -p devbox-toolbox-engine --lib api` → 컴파일 실패.
- [ ] **Step 3: 구현** — 세 crate 각각 `…Call` enum(메서드 목록은 각 `component.rs`의 `COMMANDS` 그대로: http-client 52, webhook 22, toolbox 16), `…_METHODS`, `…Issue`(오류 문자열 전부 + `apps/devbox-api-studio/component-errors.json`의 해당 component 코드), `classify`, `dispatch`, `class()`를 만든다. shim·`#[tauri::command]`를 지우고 입력·결과 타입에 `ts_rs::TS`를 붙인다. `http-client-engine`의 `take_pending_open`, `toolbox-engine`의 `take_pending_open`은 engine 메서드로 남는다.
- [ ] **Step 4: 통과 확인·커밋** — Run: `cargo test -p devbox-http-client-engine -p devbox-webhook-host -p devbox-toolbox-engine --lib` → PASS. `git commit -am "refactor(devbox-api-studio): give request, webhook and transform engines typed APIs"`

---

### Task 2: API Studio host command

**Files:** Create `apps/devbox-api-studio/src-tauri/src/ipc/{mod,api,webhooks,transforms}.rs`; Modify `lib.rs`, `capabilities/*.json`; Delete `component.rs`, `component_errors.rs`, `apps/devbox-api-studio/component-errors.json`

**Interfaces (Produces):** `ipc::{api, webhooks, transforms}` commands; `StudioApiCall = Host(HostApiCall) | Engine(ApiCall)`, `StudioWebhookCall`, `StudioTransformCall`

- [ ] **Step 1: 실패하는 테스트** — `ipc/mod.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::{ComponentCall, ExecutionClass};

    #[test]
    fn routes_and_pools_match_the_previous_allowlist() {
        let draft: api::StudioApiCall = serde_json::from_str(r#"{"method":"save_knowledge_draft","args":{"draft":{}}}"#).unwrap();
        assert!(!draft.routes().contains(&"protocols"));
        let workspace: api::StudioApiCall = serde_json::from_str(r#"{"method":"api_workspace_state","args":{}}"#).unwrap();
        assert!(workspace.routes().contains(&"protocols"));
        let stop: webhooks::StudioWebhookCall = serde_json::from_str(r#"{"method":"stop_server","args":{}}"#).unwrap();
        assert_eq!(stop.class(), ExecutionClass::Control);
        let quit: webhooks::StudioWebhookCall = serde_json::from_str(r#"{"method":"quit_product","args":{}}"#).unwrap();
        assert!(matches!(quit, webhooks::StudioWebhookCall::Host(_)));
    }
}
```

  (샘플 인자는 실제 host 함수 입력으로 맞춘다.)

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-api-studio --lib ipc` → FAIL.
- [ ] **Step 3: 구현** — 기존 `component.rs` `execute`의 분기를 command 세 개로 나눈다.

| command | host enum 메서드 | engine | 입장 뒤 부수 작업 |
|---|---|---|---|
| `api` | `api_workspace` 8개(`api_workspace_state, save_openapi_definition, list_openapi_definitions, get_openapi_definition, delete_openapi_definition, save_api_workspace, select_api_workspace, delete_api_workspace`), Knowledge 초안 5개(`save/send/list/get/delete_knowledge_draft`), `send_selection_to_toolbox`, `send_mock_draft`, `peek_pending_navigation`, `ack_pending_navigation`, `pick_multipart_file` | `ApiCall` | `lifecycle::require_open`, `api_workspace` 메서드는 `request.header.context`를 넘긴다(기존), 전송 계열 deadline 연장은 프런트가 정한다(현행) |
| `webhooks` | `peek/accept/discard_mock_draft`, `send_history_to_api`, `send_fixture_to_api`, `send_history_to_log_lens`, `send_fixture_to_log_lens`, 창 수명 4개(`lifecycle_status, set_close_policy, hide_main_window, quit_product`) | `WebhookCall` | `lifecycle::require_open`(창 수명 메서드는 제외) |
| `transforms` | `open_workspace_selection`, `read_clipboard_text`, Knowledge 초안 5개, `send_mock_draft`, `create_api_request_handoff` | `ToolboxCall` | `lifecycle::require_open` |

  각 command는 `admit` → 부수 작업 → dispatch → `admission.finish(result, classify)`. `classify`는 host enum이면 host `…Issue`, engine이면 engine `classify`. `lib.rs` plugin `invoke_handler`를 `generate_handler![ipc::api, ipc::webhooks, ipc::transforms]`로 바꾸고 `component.rs`의 plugin setup(상태 초기화)을 `ipc/mod.rs`의 `plugin()`으로 옮긴다. `component_errors.rs`와 `component-errors.json`, 이를 읽던 CI 검사(`rg -n "component-errors" .github`)를 지운다. capability의 `allow-execute`를 새 권한 세 개로 바꾼다.
- [ ] **Step 4: 통과 확인·커밋** — Run: `cargo test -p devbox-api-studio --lib` → PASS. `git add -A && git commit -m "refactor(devbox-api-studio): replace execute with typed component commands"`

---

### Task 3: API Studio 프런트

**Files:** `apps/devbox-api-studio/src-tauri/tests/typescript.rs`(생성), `packages/api-studio-features/src/{transport.ts,typed.ts,generated/*,requests/**/api.ts,webhooks/api.ts,transforms/**/api.ts,issues/*.ts}`, `apps/devbox-api-studio/src/{transport.ts,componentErrors.ts,componentErrors.test.ts}`, `.github/scripts/check-generated-bindings.sh`

- [ ] **Step 1: 실패하는 테스트** — `packages/api-studio-features/src/issues/catalog.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { apiMessages, transformMessages, webhookMessages } from "./catalog";

describe("API Studio issue catalogs", () => {
  it.each([["api", apiMessages], ["webhooks", webhookMessages], ["transforms", transformMessages]] as const)("%s has a message for every code", (_, catalog) => {
    for (const [code, message] of Object.entries(catalog)) expect(message.trim(), code).not.toBe("");
  });
});
```

- [ ] **Step 2: 생성** — `tests/typescript.rs`(P1-11과 같은 구조, 출력 `packages/api-studio-features/src/generated`)와 `check-generated-bindings.sh`에 API Studio 줄(`cargo test --locked -p devbox-api-studio --test typescript` + 해당 폴더 diff)을 추가한다. Run: `bash .github/scripts/check-generated-bindings.sh` → 파일 생성 후 PASS.
- [ ] **Step 3: 호출부**
  - `packages/api-studio-features/src/typed.ts`: P1-11 Knowledge `typed.ts`와 같은 `typedCall`(import 경로만 이 패키지의 `transport`).
  - 각 기능 `api.ts`의 `componentInvoke("api-studio.api")(method, args)` 호출을 `apiCall(method, args)`(`typedCall<StudioApiCall, ApiResults>("api-studio.api")`)로, webhooks·transforms도 같게 바꾼다. 손으로 쓴 요청·응답 인터페이스는 생성 타입 별칭으로 바꾼다(모양이 다르면 생성 타입을 따르고 호출부를 고친다).
  - `apps/devbox-api-studio/src/transport.ts`: `commandFor` map(`api-studio.api` → `plugin:api-studio|api`, …)으로 명령 이름을 고르고, 본문은 `{ request: { header, method, args } }`. `componentFailure`(`componentErrors.ts`)는 새 카탈로그(`packages/api-studio-features/src/issues/catalog.ts`의 `Record<…Issue, string>` 세 개)에서 문구를 찾게 바꾸고, `component-errors.json`을 읽던 코드를 지운다.
  - `Component` 유니온에서 `api-studio.migration`이 이미 없는지 확인한다(P1-03).
- [ ] **Step 4: 통과 확인·커밋** — Run: `pnpm --filter @devbox/api-studio-features --filter devbox-api-studio exec vitest run && pnpm --filter @devbox/api-studio-features --filter devbox-api-studio exec tsc --noEmit && pnpm exec biome ci .` → PASS. `git add -A && git commit -m "refactor(devbox-api-studio): call typed commands with generated types"`

---

### Task 4: Control Center

**Files:** Create `crates/installation-tools/src/api.rs`, `apps/devbox-control-center/src-tauri/src/ipc/{mod,tools,delivery}.rs`, `apps/devbox-control-center/src-tauri/tests/typescript.rs`; Modify `apps/devbox-control-center/src-tauri/src/{lib.rs,tools_host.rs(삭제)}`, `capabilities/*.json`, `apps/devbox-control-center/src/{Tools.tsx,Updates.tsx,Recovery.tsx,Restore.tsx,Health.tsx,Inventory.tsx}`, `packages/control-center-features/src/{transport.ts,manager/api.ts,generated/*}`

**Interfaces (Produces):** `installation_tools::api::{ToolsCall, ToolsIssue, dispatch}`; `ipc::{tools, delivery}` commands; `DeliveryCall`, `DeliveryIssue`

- [ ] **Step 1: 실패하는 테스트** — `apps/devbox-control-center/src-tauri/src/ipc/mod.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::{ComponentCall, ExecutionClass};

    #[test]
    fn delivery_methods_keep_their_routes_and_cancel_is_control() {
        let cancel: delivery::DeliveryCall = serde_json::from_str(r#"{"method":"cancel_suite_update","args":{}}"#).unwrap();
        assert_eq!(cancel.class(), ExecutionClass::Control);
        let health: delivery::DeliveryCall = serde_json::from_str(r#"{"method":"record_suite_health","args":{"product":"knowledge"}}"#).unwrap();
        assert_eq!(health.routes(), &["updates", "recovery"]);
        let audit: tools::ControlToolsCall = serde_json::from_str(r#"{"method":"dev_setup_audit","args":{}}"#).unwrap();
        assert_eq!(audit.routes(), &["environment"]);
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-control-center --lib ipc` → FAIL.
- [ ] **Step 3: 구현**
  - `crates/installation-tools/src/api.rs`: P1-02 이후 남은 메서드 14개(`run_diagnosis, preview_support_bundle, cancel_support_bundle, export_support_bundle, dev_setup_audit, import_dev_setup_configuration, discard_dev_setup_configuration, export_dev_setup_configuration, apply_dev_setup_configuration, cancel_dev_setup_apply, related_tools, install_related_tool, launch_related_tool, open_related_url`)로 `ToolsCall`을 만든다. 취소 계열(`cancel_support_bundle`, `cancel_dev_setup_apply`)은 Control. `component.rs`의 문자열 `allowed`·`dispatch`와 `input::<…>` 도우미를 enum dispatch로 대체한다.
  - `ipc/delivery.rs`: `tools_host.rs`의 delivery 분기(`suite_inventory, open_installation_folder, suite_recovery, restore_inventory, restore_action, record_suite_health, check_suite_update, suite_update_status, download_suite_update, cancel_suite_update, launch_suite_update`와 P0-04에서 더한 캐시 메서드)를 `DeliveryCall`로 옮긴다. 인자 검증(예: `restore_action`의 id·action 확인)은 enum 필드 타입과 기존 검사 함수로 옮긴다. `cancel_suite_update`는 Control.
  - `ipc/tools.rs`: `ControlToolsCall`(engine `ToolsCall` 위임)과 route 규칙.
  - `lib.rs`: plugin `invoke_handler`를 `generate_handler![ipc::tools, ipc::delivery]`로 바꾸고 `tools_host.rs`를 지운다. capability 갱신.
  - 프런트: `packages/control-center-features/src/transport.ts`의 `configureProductTransport` 서명을 component를 받도록 바꾸고(`(component: "control-center.tools" | "control-center.delivery", method, args)`), `Tools.tsx`의 transport가 `plugin:control-center|tools`로 보낸다. `Updates.tsx`·`Recovery.tsx`·`Restore.tsx`·`Health.tsx`·`Inventory.tsx`에서 `invoke("plugin:control-center|execute", …)`를 `deliveryCall(method, args)`(생성 타입 기반 `typedCall`)로 바꾼다. 문구 카탈로그 `Record<DeliveryIssue, string>`·`Record<ToolsIssue, string>`를 `packages/control-center-features/src/issues.ts`에 둔다.
  - 생성: `tests/typescript.rs`(출력 `packages/control-center-features/src/generated`)와 `check-generated-bindings.sh` 줄 추가.
- [ ] **Step 4: 통과 확인·커밋** — Run: `cargo test -p devbox-installation-tools -p devbox-control-center --lib && bash .github/scripts/check-generated-bindings.sh && pnpm --filter @devbox/control-center-features --filter devbox-control-center exec vitest run && pnpm --filter @devbox/control-center-features --filter devbox-control-center exec tsc --noEmit` → PASS. `git add -A && git commit -m "refactor(devbox-control-center): replace execute with typed tools and delivery commands"`

---

### Task 5: PR 완료

- [ ] `! rg -n 'plugin:(api-studio|control-center)\|execute' apps packages` → 0건.
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): API Studio 요청 보내기·취소, MCP stdio 연결·해제, gRPC 호출, 웹훅 리스너 시작·기록·fixture·재전송·규칙, 변환(해시·JWT·diff), Knowledge 초안 보내기. Control Center 진단 실행·지원 번들, 환경 감사, 관련 도구 목록, 업데이트 확인·다운로드·취소, 데이터 및 복구 목록.
