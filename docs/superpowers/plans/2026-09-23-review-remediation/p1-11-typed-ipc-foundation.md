# P1-11 타입 기반 IPC 기반 공사와 Knowledge Activity 시범 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 문자열 `component`·`method`와 JSON `args`로 모든 호출을 한 `execute` 명령에 싣던 구조(A1)를 component별 Tauri command + 메서드별 타입 enum으로 바꾸는 공용 기반을 만들고, Knowledge의 Activity component(메서드 26개)로 시범 적용한다. 오류는 안정 코드 enum으로, 문구는 코드별 한국어 카탈로그로 옮긴다(A2, D17). TypeScript 타입은 Rust에서 생성한다(D18: ts-rs).

**Architecture:**
- 새 crate `product-ipc`: `ComponentRequest<C>`(와이어 모양은 지금과 같은 `{header, method, args}`, `method`/`args`는 serde 인접 태그 enum), `ComponentCall` trait(component id·허용 route·메서드 이름), 안정 오류 코드를 선언하는 `issue_codes!` macro, TS 결과 map 생성 함수.
- `product-shell-tauri::admit`: 네 제품 `execute`에 복사돼 있던 입장 절차(세션·route·deadline·replay·activation 확인, 동시 요청 예약, 운영 로그 guard)를 한 함수로. 반환하는 `Admission`이 결과를 기존 응답 모양(`{operation, value}`)으로 바꾼다.
- engine은 자기 API를 enum으로 소유한다(`activity_engine::api::ActivityCall`, `dispatch`). engine의 `__component_*` shim과 등록되지 않은 `#[tauri::command]` 속성은 지운다(A3).
- 제품 host는 component마다 Tauri command를 둔다(`plugin:knowledge|activity`). host 전용 메서드(수집 수명 설정 등)는 host enum으로 따로 두고 engine enum과 `#[serde(untagged)]`로 합친다.
- 프런트는 생성된 `ActivityCall` 유니온과 `ActivityResults` map으로 `activityCall("get_digest", { input })`처럼 부른다. 오류 코드 유니온(`ActivityIssue`)에 대한 `Record<ActivityIssue, string>` 카탈로그로 "모든 코드에 문구가 있다"를 타입으로 보장한다.
- Tauri `CommandArg` 확장 대신 명시적 `admit` 호출을 쓴다. `CommandItem` API가 Tauri 버전마다 바뀌어 유지 비용이 크고, 명시 호출도 같은 보장을 준다(ADR 0014에 기록).

**Tech Stack:** Rust, `ts-rs` 12.0.1(D23 승인), Tauri v2, TypeScript, Vitest

**Spec:** `review.md` §6 A1·A2·A3 · `00-roadmap.md` D17·D18·D23 · ADR `docs/adr/0014-typed-ipc.md`

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 와이어 호환: 요청 본문은 `{ request: { header, method, args } }`, 응답은 `{ operation, value }`, 실패 값은 `{ issue: <code> }` 그대로. 바뀌는 것은 명령 이름(`plugin:knowledge|execute` → `plugin:knowledge|activity`)뿐이다.
- 권한은 넓히지 않는다: 새 command도 `admit`가 기존 `authorize`와 같은 검사를 한다. capability 파일에 새 command를 허용 목록으로 추가한다.
- 생성 파일 위치: `packages/<product>-features/src/generated/`(ts-rs 기본 규칙대로 타입마다 `<타입 이름>.ts`, 메서드→결과 map은 `<component>-results.ts`). git에 넣고, CI가 "생성 결과와 커밋된 파일이 같다"를 확인한다. 설정은 코드의 `ts_rs::Config`로 하며 환경 변수·`.cargo/config.toml`은 쓰지 않는다.
- `u64`·`i64`는 TS `number`로 생성한다(`Config::with_large_int("number")`). 2^53을 넘는 값을 보내는 API는 없다(타임스탬프 ms·크기·ID).
- 이 PR에서 옮기는 것은 `knowledge.activity`뿐이다. 나머지 Knowledge component는 P1-12, API Studio·Control Center는 P1-13, Workspace는 P1-14.

## Review Focus

1. 옮긴 뒤에도 Activity 화면의 모든 동작(추적 시작·중지, 일/기간 조회, digest 생성·저장·Knowledge 전송, 내보내기, 프로젝트 설정, 개인정보 규칙 저장·적용, 자동 시작)이 같다. (Task 3·4 테스트, 사용자 실기)
2. 알 수 없는 `method`나 잘못된 `args` → `InvalidRequest` Problem(기존과 같은 조기 거부), 운영 로그에 `rejected`. (Task 1·2)
3. engine이 새 오류 문자열을 돌려줌(카탈로그에 없는 코드) → 프런트는 `unavailable` 문구를 보이고, 운영 로그에는 원래 코드가 남는다. (Task 1·4)
4. 생성 TS 파일을 고치지 않고 Rust 타입만 바꿈 → CI의 생성 결과 비교가 실패한다. (Task 5)
5. 같은 requestId로 두 번 호출(replay) → 두 번째는 `Replayed`. 동시 요청이 64개를 넘으면 `Overloaded`(기존 한도). (Task 2)

## Branch · PR

- 묶음: **B6** — 브랜치 `refactor/suite/typed-ipc-knowledge-api-control`, PR 제목 `refactor(suite): typed IPC for Knowledge, API Studio and Control Center`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(devbox-knowledge): typed component IPC, starting with Activity`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `crates/product-ipc/{Cargo.toml,src/lib.rs}` | 생성 | 요청 타입·trait·issue macro·TS map |
| `crates/product-shell-tauri/src/admission.rs` | 생성 | `admit`, `Admission`, 동시 요청 예약 |
| `crates/product-shell-tauri/src/lib.rs` | 수정 | 모듈·재수출, `ActiveRequests` 관리 |
| `crates/activity-engine/src/api.rs` | 생성 | `ActivityCall`, `ActivityIssue`, `dispatch` |
| `crates/activity-engine/src/{component.rs,commands/*.rs}` | 수정 | shim·`#[tauri::command]` 제거 |
| `apps/devbox-knowledge/src-tauri/src/activity_ipc.rs` | 생성 | `activity` command, host enum |
| `apps/devbox-knowledge/src-tauri/src/{component.rs,lib.rs}`, `capabilities/*.json` | 수정 | 등록·execute에서 activity 제거·권한 |
| `apps/devbox-knowledge/src-tauri/tests/typescript.rs` | 생성 | TS 생성 테스트 |
| `packages/knowledge-features/src/generated/*.ts` | 생성(자동) | 타입마다 한 파일(`KnowledgeActivityCall.ts`, `ActivityIssue.ts`, 입력·결과 타입) + `activity-results.ts` |
| `packages/knowledge-features/src/{transport.ts,activity/api.ts,activity/issues.ts}` | 수정·생성 | typed 호출·문구 카탈로그 |
| `apps/devbox-knowledge/src/transport.ts` | 수정 | component → command 이름 |
| `docs/adr/0014-typed-ipc.md` | 수정 | 상태 "채택", CommandArg 대신 명시 admit |
| `CONVENTIONS.md` | 수정 | "메서드 추가 절차" |

---

### Task 1: `product-ipc` crate

**Files:** Create `crates/product-ipc/Cargo.toml`, `crates/product-ipc/src/lib.rs`; Modify 루트 `Cargo.toml`

**Interfaces (Produces):**
- `ComponentRequest<C> { header: RouteRequest, call: C }` (`#[serde(flatten)] call`)
- `enum ExecutionClass { Normal, Control }`(취소·중지·연결 해제처럼 포화 중에도 받아야 하는 요청은 `Control`)
- `trait ComponentCall: DeserializeOwned + Send + 'static { const COMPONENT: &'static str; fn method(&self) -> &'static str; fn routes(&self) -> &'static [&'static str]; fn class(&self) -> ExecutionClass { ExecutionClass::Normal } }`
- `results_map(type_name: &str, entries: &[(&str, String)]) -> String`
- `issue_codes! { pub enum Name { Variant = "code", … } }` → enum + `code(self) -> &'static str` + `from_code(&str) -> Option<Self>` + `ALL: &[Self]` + `ts_rs::TS`(문자열 리터럴 유니온)
- 재수출: `pub use ts_rs;`

- [ ] **Step 1: crate 뼈대** — `crates/product-ipc/Cargo.toml`:

```toml
[package]
name = "product-ipc"
version = "0.1.0"
edition.workspace = true

[dependencies]
product-contract = { path = "../product-contract" }
serde = { workspace = true }
serde_json = { workspace = true }
ts-rs = { workspace = true }
```

  루트 `Cargo.toml`: `members`에 `"crates/product-ipc"`, `[workspace.dependencies]`에 `ts-rs = { version = "12.0.1", features = ["serde-json-impl", "no-serde-warnings"] }`와 `product-ipc = { path = "crates/product-ipc" }`.

- [ ] **Step 2: 실패하는 테스트** — `src/lib.rs` 테스트 모듈

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq, ts_rs::TS)]
    #[serde(tag = "method", content = "args", rename_all = "snake_case")]
    enum DemoCall {
        GetDay { date: String, day_start: i64 },
        StopTracking {},
    }

    impl ComponentCall for DemoCall {
        const COMPONENT: &'static str = "demo.activity";
        fn method(&self) -> &'static str {
            match self {
                Self::GetDay { .. } => "get_day",
                Self::StopTracking {} => "stop_tracking",
            }
        }
        fn routes(&self) -> &'static [&'static str] {
            &["activity"]
        }
    }

    issue_codes! {
        pub enum DemoIssue {
            Cancelled = "digest_cancelled",
            Unavailable = "unavailable",
        }
    }

    const HEADER: &str = r#"{"protocolVersion":1,"installationId":"i","sessionId":"s","requestId":"r","deadlineMs":1,"route":"activity"}"#;

    #[test]
    fn requests_keep_the_existing_wire_shape() {
        let json = format!(r#"{{"header":{HEADER},"method":"get_day","args":{{"date":"2026-09-24","day_start":0}}}}"#);
        let request: ComponentRequest<DemoCall> = serde_json::from_str(&json).unwrap();
        assert_eq!(request.call, DemoCall::GetDay { date: "2026-09-24".into(), day_start: 0 });
        assert_eq!(request.call.method(), "get_day");
        let unknown = format!(r#"{{"header":{HEADER},"method":"drop_tables","args":{{}}}}"#);
        assert!(serde_json::from_str::<ComponentRequest<DemoCall>>(&unknown).is_err());
    }

    #[test]
    fn issue_codes_round_trip_and_export_a_literal_union() {
        assert_eq!(DemoIssue::Cancelled.code(), "digest_cancelled");
        assert_eq!(DemoIssue::from_code("unavailable"), Some(DemoIssue::Unavailable));
        assert_eq!(DemoIssue::from_code("other"), None);
        assert_eq!(DemoIssue::ALL.len(), 2);
        let decl = <DemoIssue as ts_rs::TS>::decl(&ts_rs::Config::new());
        assert!(decl.contains("\"digest_cancelled\"") && decl.contains("\"unavailable\""), "{decl}");
    }

    #[test]
    fn results_map_lists_each_method() {
        let text = results_map("DemoResults", &[("get_day", "DaySummary".into()), ("stop_tracking", "null".into())]);
        assert_eq!(text, "export type DemoResults = {\n  get_day: DaySummary;\n  stop_tracking: null;\n};\n");
    }
}
```

  (`header` 필드 이름은 `product_contract::RouteRequest`의 serde 표현을 따른다. 필드가 다르면 fixture `packages/product-shell/fixtures/route-request.json`의 내용을 그대로 쓴다.)

- [ ] **Step 3: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p product-ipc` → 컴파일 실패.

- [ ] **Step 4: 구현** — `src/lib.rs`(테스트 모듈 위)

```rust
//! Typed component requests. The wire shape stays `{header, method, args}`;
//! each component declares its methods as an adjacently tagged serde enum,
//! and its failures as stable issue codes.
use product_contract::RouteRequest;
use serde::de::DeserializeOwned;
use serde::Deserialize;

pub use ts_rs;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", bound = "C: DeserializeOwned")]
pub struct ComponentRequest<C> {
    pub header: RouteRequest,
    #[serde(flatten)]
    pub call: C,
}

/// Admission pool. Control requests (cancel, stop, disconnect) have their
/// own small pool so they still get in while normal requests are saturated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionClass {
    Normal,
    Control,
}

pub trait ComponentCall: DeserializeOwned + Send + 'static {
    /// Catalog component id, for example `knowledge.activity`.
    const COMPONENT: &'static str;
    /// Stable method name used by allowlists and the operation log.
    fn method(&self) -> &'static str;
    /// Routes that may invoke this method.
    fn routes(&self) -> &'static [&'static str];
    /// Admission pool for this method.
    fn class(&self) -> ExecutionClass {
        ExecutionClass::Normal
    }
}

/// `export type <Name> = { method: Result; … };` consumed by the typed call helper.
pub fn results_map(type_name: &str, entries: &[(&str, String)]) -> String {
    let mut out = format!("export type {type_name} = {{\n");
    for (method, result) in entries {
        out.push_str(&format!("  {method}: {result};\n"));
    }
    out.push_str("};\n");
    out
}

/// Declare stable failure codes once. The enum serializes to its code and
/// exports a TypeScript string-literal union of every code.
#[macro_export]
macro_rules! issue_codes {
    ($vis:vis enum $name:ident { $($variant:ident = $code:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, $crate::ts_rs::TS)]
        $vis enum $name {
            $(#[serde(rename = $code)] $variant),+
        }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub fn code(self) -> &'static str {
                match self { $(Self::$variant => $code),+ }
            }
            pub fn from_code(code: &str) -> Option<Self> {
                match code { $($code => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}
```

  (내보내기는 Task 5에서 `export_all`로 하며 ts-rs 기본 규칙대로 타입마다 `<타입 이름>.ts` 파일이 생긴다. ts-rs 12에서 unit enum + `serde(rename)`이 문자열 리터럴 유니온으로 나오는지 Step 2 테스트가 확인한다. derive가 만드는 코드는 `ts_rs` 경로를 쓰므로, 이 macro나 `#[derive(TS)]`를 쓰는 crate는 `ts-rs = { workspace = true }`를 직접 의존한다.)

- [ ] **Step 5: 통과 확인** — Run: `cargo test -p product-ipc && python3 .github/scripts/check-dependencies.py check` → PASS(ts-rs·ts-rs-macros의 라이선스 MIT가 정책을 통과해야 한다. 막히면 `.github/dependency-policy.json`에 D23 근거로 추가).

- [ ] **Step 6: 커밋** — `git add -A && git commit -m "feat(crates): add typed component request primitives"`

---

### Task 2: `product_shell_tauri::admit`

**Files:** Create `crates/product-shell-tauri/src/admission.rs`; Modify `crates/product-shell-tauri/src/lib.rs`, `Cargo.toml`

**Interfaces:**
- Consumes: `ComponentCall`, 기존 `authorize`, P0-07 `begin_operation`
- Produces: `admit<C: ComponentCall>(window: &WebviewWindow, header: &RouteRequest, call: &C) -> Result<Admission, Problem>`; `Admission::problem(&self, ProblemCode) -> Problem`; `Admission::finish(self, result: Result<serde_json::Value, String>, classify: fn(&str) -> &'static str) -> Reply`; `Reply { operation: Operation, value: Value }`(Serialize); `ActiveRequests`(제품당 하나, setup에서 manage) + `ActiveRequests::reserve(&self, id: &str, class: ExecutionClass)`; 상수 `MAX_ACTIVE = 64`, `MAX_ACTIVE_CONTROLS = 8`(API Studio의 기존 한도)

- [ ] **Step 1: 실패하는 테스트** — `admission.rs`의 순수 부분(예약 집합)을 테스트한다.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use product_contract::ProblemCode;

    #[test]
    fn replayed_and_overflowing_requests_are_refused() {
        use product_ipc::ExecutionClass::{Control, Normal};
        let active = ActiveRequests::default();
        let first = active.reserve("request-1", Normal).unwrap();
        assert_eq!(active.reserve("request-1", Control).err(), Some(ProblemCode::Replayed));
        drop(first);
        assert!(active.reserve("request-1", Normal).is_ok());
        let held: Vec<_> = (0..MAX_ACTIVE).map(|index| active.reserve(&format!("r-{index}"), Normal).unwrap()).collect();
        assert_eq!(active.reserve("one-more", Normal).err(), Some(ProblemCode::Overloaded));
        // Saturated normal requests never block a cancel.
        let controls: Vec<_> = (0..MAX_ACTIVE_CONTROLS).map(|index| active.reserve(&format!("c-{index}"), Control).unwrap()).collect();
        assert_eq!(active.reserve("c-extra", Control).err(), Some(ProblemCode::Overloaded));
        drop((held, controls));
    }

    #[test]
    fn outcomes_map_to_operation_states() {
        assert!(matches!(outcome(&Ok(serde_json::json!(1)), |_| "x"), (OperationState::Succeeded {}, _)));
        let (state, value) = outcome(&Err("digest_cancelled".into()), |code| if code == "digest_cancelled" { "cancelled" } else { "unavailable" });
        assert!(matches!(state, OperationState::Cancelled {}));
        assert_eq!(value, serde_json::json!({"issue": "cancelled"}));
        let (state, value) = outcome(&Err("boom".into()), |_| "unavailable");
        assert!(matches!(state, OperationState::Failed { code: ProblemCode::Unavailable }));
        assert_eq!(value, serde_json::json!({"issue": "unavailable"}));
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p product-shell-tauri --lib admission` → 컴파일 실패.

- [ ] **Step 3: 구현** — `admission.rs`

```rust
//! Shared admission for typed component commands: native caller, session,
//! route, deadline and activation checks (via `authorize`), one active
//! reservation per request id, and the operation-log guard.
use crate::operation_log::{begin_operation, OperationGuard};
use product_contract::{Operation, OperationState, Problem, ProblemCode, Provenance, RouteRequest};
use product_ipc::{ComponentCall, ExecutionClass};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Manager, WebviewWindow};

pub const MAX_ACTIVE: usize = 64;
pub const MAX_ACTIVE_CONTROLS: usize = 8;

#[derive(Default)]
pub struct ActiveRequests(Arc<Mutex<HashMap<String, ExecutionClass>>>);

pub struct Reservation {
    active: Arc<Mutex<HashMap<String, ExecutionClass>>>,
    id: String,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.id);
        }
    }
}

impl ActiveRequests {
    pub fn reserve(&self, id: &str, class: ExecutionClass) -> Result<Reservation, ProblemCode> {
        let mut ids = self.0.lock().map_err(|_| ProblemCode::Unavailable)?;
        if ids.contains_key(id) {
            return Err(ProblemCode::Replayed);
        }
        let limit = match class {
            ExecutionClass::Normal => MAX_ACTIVE,
            ExecutionClass::Control => MAX_ACTIVE_CONTROLS,
        };
        if ids.values().filter(|held| **held == class).count() >= limit {
            return Err(ProblemCode::Overloaded);
        }
        ids.insert(id.to_owned(), class);
        Ok(Reservation { active: self.0.clone(), id: id.to_owned() })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub operation: Operation,
    pub value: Value,
}

pub struct Admission {
    provenance: Provenance,
    _reservation: Reservation,
    guard: OperationGuard,
}

pub fn admit<C: ComponentCall>(window: &WebviewWindow, header: &RouteRequest, call: &C) -> Result<Admission, Problem> {
    let guard = begin_operation(window, C::COMPONENT, call.method());
    let provenance = crate::authorize(window, header, C::COMPONENT)?;
    let problem = |code| Problem { code, provenance: provenance.clone() };
    if !call.routes().contains(&header.route.as_str()) {
        return Err(problem(ProblemCode::Unauthorized));
    }
    let active = window.try_state::<ActiveRequests>().ok_or_else(|| problem(ProblemCode::Unavailable))?;
    let reservation = active.reserve(&header.request_id, call.class()).map_err(problem)?;
    Ok(Admission { provenance, _reservation: reservation, guard })
}

pub(crate) fn outcome(result: &Result<Value, String>, classify: fn(&str) -> &'static str) -> (OperationState, Value) {
    match result {
        Ok(value) => (OperationState::Succeeded {}, value.clone()),
        Err(error) => {
            let issue = classify(error);
            let state = if issue == "cancelled" || issue.ends_with("_cancelled") {
                OperationState::Cancelled {}
            } else {
                OperationState::Failed { code: ProblemCode::Unavailable }
            };
            (state, json!({ "issue": issue }))
        }
    }
}

impl Admission {
    pub fn problem(&self, code: ProblemCode) -> Problem {
        Problem { code, provenance: self.provenance.clone() }
    }

    /// `classify` maps a native error string to the stable issue code the
    /// renderer shows; the operation log keeps the original string.
    pub fn finish(self, result: Result<Value, String>, classify: fn(&str) -> &'static str) -> Reply {
        let (state, value) = outcome(&result, classify);
        self.guard.finish(&state, result.as_ref().err().map(String::as_str));
        Reply { operation: Operation { provenance: self.provenance, outcome: state }, value }
    }
}
```

  `lib.rs`: `mod admission; pub use admission::{admit, ActiveRequests, Admission, Reply};`, `builder` setup에 `app.manage(ActiveRequests::default());`. `Cargo.toml`에 `product-ipc = { workspace = true }`.

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p product-shell-tauri --lib` → PASS.
- [ ] **Step 5: 커밋** — `git add -A && git commit -m "feat(suite): share typed command admission"`

---

### Task 3: Activity engine API

**Files:** Create `crates/activity-engine/src/api.rs`; Modify `crates/activity-engine/src/{lib.rs,component.rs,commands/*.rs}`, `Cargo.toml`

**Interfaces (Produces):** `activity_engine::api::{ActivityCall, ActivityIssue, dispatch(app: &AppHandle, call: ActivityCall) -> Result<Value, String>, typescript() -> String}`

- [ ] **Step 1: 실패하는 테스트** — `api.rs` 테스트 모듈

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_method_parses_with_its_arguments() {
        let samples = [
            (r#"{"method":"get_day","args":{"date":"2026-09-24","dayStart":0,"dayEnd":86400000}}"#, "get_day"),
            (r#"{"method":"stop_tracking","args":{}}"#, "stop_tracking"),
            (r#"{"method":"set_idle_threshold","args":{"thresholdMs":300000}}"#, "set_idle_threshold"),
            (r#"{"method":"set_autostart","args":{"enabled":true}}"#, "set_autostart"),
            (r#"{"method":"app_stats","args":{"start":0,"end":1}}"#, "app_stats"),
        ];
        for (json, method) in samples {
            let call: ActivityCall = serde_json::from_str(json).unwrap_or_else(|error| panic!("{json}: {error}"));
            assert_eq!(call.method(), method);
        }
        assert_eq!(METHODS.len(), 26);
    }

    #[test]
    fn unknown_error_strings_fall_back_to_unavailable() {
        assert_eq!(classify("digest_cancelled"), "digest_cancelled");
        assert_eq!(classify("disk on fire"), "unavailable");
    }
}
```

  (`args` 필드 이름은 지금 shim의 `Input` 구조체와 같은 camelCase다. enum에는 `rename_all_fields = "camelCase"`를 붙인다.)

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-activity-engine --lib api` → 컴파일 실패.

- [ ] **Step 3: 구현** — `api.rs`

```rust
//! Activity component API. The enum is the single list of methods; host
//! commands deserialize it directly and TypeScript types are generated from it.
use crate::commands::{/* 각 명령 모듈 */};
use serde::Deserialize;
use serde_json::Value;
use ts_rs::TS;

#[derive(Debug, Deserialize, TS)]
#[serde(tag = "method", content = "args", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ActivityCall {
    GetDigest { input: DigestInput },
    CancelDigest {},
    SaveDigest { request: SaveDigestRequest },
    SendDigestToKnowledge { input: DigestInput, regenerated_from: Option<String> },
    KnowledgeDraftHistory {},
    ExportLifeLog { input: ExportInput },
    SaveLifeLog { input: ExportInput },
    SetProjects { paths: Vec<String> },
    GetProjects {},
    ProbeProject { path: String },
    GetDay { date: String, day_start: i64, day_end: i64 },
    GetRange { label: String, day_start: i64, day_end: i64 },
    StartTracking {},
    StopTracking {},
    IsTracking {},
    SetIdleThreshold { threshold_ms: i64 },
    GetIdleThreshold {},
    GetPrivacyRules {},
    SetPrivacyRules { rules: PrivacyRules },
    RedactExisting {},
    AutostartStatus {},
    SetAutostart { enabled: bool },
    IntegrationSources {},
    ProjectAttribution { day_start: i64, day_end: i64 },
    Timeline { day_start: i64, day_end: i64 },
    AppStats { start: i64, end: i64 },
}

pub const METHODS: &[&str] = &[
    "get_digest", "cancel_digest", "save_digest", "send_digest_to_knowledge", "knowledge_draft_history",
    "export_life_log", "save_life_log", "set_projects", "get_projects", "probe_project", "get_day",
    "get_range", "start_tracking", "stop_tracking", "is_tracking", "set_idle_threshold",
    "get_idle_threshold", "get_privacy_rules", "set_privacy_rules", "redact_existing",
    "autostart_status", "set_autostart", "integration_sources", "project_attribution", "timeline", "app_stats",
];

impl ActivityCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::GetDigest { .. } => "get_digest",
            // … 26개 모두. 이름은 METHODS와 같은 순서·같은 문자열.
            Self::AppStats { .. } => "app_stats",
        }
    }
}

product_ipc::issue_codes! {
    pub enum ActivityIssue {
        Unavailable = "unavailable",
        Cancelled = "digest_cancelled",
        ArgsInvalid = "component_args_invalid",
        PrivacyRulesInvalid = "privacy_rules_invalid",
        PrivacyRulesSaveFailed = "privacy_rules_save_failed",
        PrivacyRedactionFailed = "privacy_redaction_failed",
        // Step 4에서 engine의 오류 문자열을 전부 모아 채운다.
    }
}

/// Stable code for a native error string; unknown strings become `unavailable`.
pub fn classify(error: &str) -> &'static str {
    ActivityIssue::from_code(error).unwrap_or(ActivityIssue::Unavailable).code()
}

pub async fn dispatch(app: &tauri::AppHandle, call: ActivityCall) -> Result<Value, String> {
    use tauri::Manager as _;
    let state = app.state();
    let value = match call {
        ActivityCall::GetDigest { input } => to_value(get_digest(state, input).await?),
        ActivityCall::CancelDigest {} => to_value(cancel_digest(state).await?),
        // … 각 팔은 지운 `__component_*` shim의 "let value = …" 줄과 같은 함수를 부른다(Step 4 표).
        ActivityCall::AppStats { start, end } => to_value(app_stats(state, start, end)?),
    };
    value
}

fn to_value<T: serde::Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}
```

- [ ] **Step 4: shim 제거와 결과 타입**
  - 각 `__component_<method>` shim의 호출 줄을 `dispatch`의 해당 팔로 옮긴 뒤 shim을 지운다. 호출 줄 목록(현재 코드 기준):

| method | 호출 | 결과 타입(ts-rs 대상) |
|---|---|---|
| get_digest | `get_digest(state, input).await?` | `DigestResponse` |
| cancel_digest | `cancel_digest(state).await?` | `bool` |
| save_digest | `save_digest(state, request).await?` | `SaveDigestResult` |
| send_digest_to_knowledge | `send_digest_to_knowledge(state, input, regenerated_from).await?` | 함수 반환 타입 |
| knowledge_draft_history | `knowledge_draft_history(state)?` | 함수 반환 타입 |
| export_life_log / save_life_log | `export_life_log(state, input).await?` / `save_life_log(…)` | 함수 반환 타입 |
| set_projects / get_projects | `set_projects(state, paths)?` / `get_projects(state)` | |
| probe_project | `probe_project(path).await?` | |
| get_day / get_range | `get_day(state, date, day_start, day_end).await?` / `get_range(state, label, …)` | `DaySummary` / `RangeSummary` |
| start/stop/is_tracking | 기존 shim 본문 | `bool` 또는 `()` |
| set/get_idle_threshold | 기존 shim 본문 | |
| get/set_privacy_rules, redact_existing | P0-02 이후 함수(`privacy::view`, `save_rules`, `redact_existing_inner`) | P0-02의 JSON 모양을 구조체로(`PrivacyView { rules, healthy }`, `SaveOutcome { saved, invalid }`) |
| autostart_status / set_autostart | `product_status(app)?` / `set_product_autostart(app, enabled)?` | |
| integration_sources, project_attribution, timeline, app_stats | 기존 shim 호출 | |

  - 결과 구조체·입력 구조체와 그 안의 타입에 `#[derive(ts_rs::TS)]`를 붙인다(`DigestInput`, `DigestResponse`, `SaveDigestRequest`, `SaveDigestResult`, `ExportInput`, `PrivacyRules`, `DaySummary`, `RangeSummary`, `Session`, `AppTotal`, … 컴파일러가 `TS` 미구현으로 알려 주는 타입 전부). `serde_json::Value` 필드는 `serde-json-impl`로 처리되고, `PathBuf`·`chrono` 타입이 있으면 `#[ts(type = "string")]`을 붙인다.
  - engine 함수의 `#[tauri::command]` 속성을 이 crate에서 모두 지운다(등록된 곳이 없다: `rg -n "generate_handler" crates/activity-engine` 0건 확인).
  - `component.rs`의 `COMMANDS`와 문자열 `dispatch`를 지우고 `api::METHODS`/`api::dispatch`만 남긴다(`component::initialize` 등 수명 함수는 유지).
  - engine에서 `Err("…")`·`.into()`로 만드는 오류 문자열을 `rg -o '"[a-z][a-z0-9_]+"' crates/activity-engine/src/commands | sort -u`로 모아 `ActivityIssue`에 넣는다(사용자에게 보일 필요가 없는 내부 코드는 넣지 않아도 되며 그 경우 `unavailable`로 보인다).

- [ ] **Step 5: 통과 확인** — Run: `cargo test -p devbox-activity-engine --lib` → PASS.
- [ ] **Step 6: 커밋** — `git add -A && git commit -m "refactor(devbox-knowledge): give the activity engine a typed API"`

---

### Task 4: Knowledge host와 프런트

**Files:** Create `apps/devbox-knowledge/src-tauri/src/activity_ipc.rs`, `packages/knowledge-features/src/activity/issues.ts`, `packages/knowledge-features/src/typed.ts`; Modify `apps/devbox-knowledge/src-tauri/src/{component.rs,lib.rs}`, `apps/devbox-knowledge/src-tauri/capabilities/*.json`, `apps/devbox-knowledge/src/transport.ts`, `packages/knowledge-features/src/{transport.ts,activity/api.ts}`

**Interfaces (Produces):**
- Rust: `#[tauri::command] activity(window, request: ComponentRequest<KnowledgeActivityCall>) -> Result<Reply, Problem>`; `KnowledgeActivityCall`(`#[serde(untagged)] enum { Host(HostActivityCall), Engine(ActivityCall) }`)
- TS: `typedCall<Calls extends {method: string; args: unknown}, Results>(component)`: `(method, args) => Promise<Results[method]>`; `activityMessages: Record<ActivityIssue, string>`

- [ ] **Step 1: 실패하는 테스트**

`activity_ipc.rs` 테스트 모듈:

```rust
    #[test]
    fn host_methods_and_engine_methods_both_parse() {
        let host: KnowledgeActivityCall = serde_json::from_str(r#"{"method":"lifecycle_status","args":{}}"#).unwrap();
        assert!(matches!(host, KnowledgeActivityCall::Host(_)));
        let engine: KnowledgeActivityCall = serde_json::from_str(r#"{"method":"stop_tracking","args":{}}"#).unwrap();
        assert!(matches!(engine, KnowledgeActivityCall::Engine(_)));
        assert!(serde_json::from_str::<KnowledgeActivityCall>(r#"{"method":"nope","args":{}}"#).is_err());
        assert_eq!(engine.routes(), &["activity"]);
    }
```

  (`lifecycle_status`는 `crate::lifecycle::METHODS`의 실제 이름으로 바꾼다.)

`packages/knowledge-features/src/activity/issues.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { activityMessages } from "./issues";

describe("activity issue catalog", () => {
  it("has a non-empty Korean message for every generated code", () => {
    for (const [code, message] of Object.entries(activityMessages)) {
      expect(message.trim().length, code).toBeGreaterThan(0);
    }
    expect(activityMessages.unavailable).toBeDefined();
  });
});
```

`apps/devbox-knowledge/src/transport.test.ts`(생성, `vi.mock("@tauri-apps/api/core")` 방식):

```ts
it("sends activity calls to the typed command with the same body shape", async () => {
  native.invoke.mockImplementation(async (command: string, args: { request: { header: { requestId: string }; method: string } }) => {
    if (command === "plugin:product-shell|describe") return fixtureDescription("knowledge");
    expect(command).toBe("plugin:knowledge|activity");
    expect(args.request.method).toBe("is_tracking");
    return { operation: { provenance: { product: "knowledge", component: "knowledge.activity", requestId: args.request.header.requestId, revision: catalog.catalogRevision }, outcome: { state: "succeeded" } }, value: true };
  });
  await import("./transport");
  const { activityCall } = await import("@devbox/knowledge-features/activity/api");
  await expect(activityCall("is_tracking", {})).resolves.toBe(true);
});
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-knowledge --lib activity_ipc && pnpm --filter @devbox/knowledge-features --filter devbox-knowledge exec vitest run` → FAIL.

- [ ] **Step 3: host 구현** — `activity_ipc.rs`

```rust
//! Typed Knowledge Activity command. Engine methods come from
//! `activity_engine::api`; host-only methods stay here.
use activity_engine::api::{self, ActivityCall};
use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, ComponentRequest};
use product_shell_tauri::{admit, Reply};
use serde::Deserialize;
use tauri::WebviewWindow;

#[derive(Debug, Deserialize, ts_rs::TS)]
#[serde(tag = "method", content = "args", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum HostActivityCall {
    // crate::lifecycle::METHODS 두 개를 variant로 옮긴다(기존 인자 모양 그대로).
}

#[derive(Debug, Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum KnowledgeActivityCall {
    Host(HostActivityCall),
    Engine(ActivityCall),
}

impl ComponentCall for KnowledgeActivityCall {
    const COMPONENT: &'static str = "knowledge.activity";
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["activity"]
    }
}

#[tauri::command]
pub async fn activity(window: WebviewWindow, request: ComponentRequest<KnowledgeActivityCall>) -> Result<Reply, Problem> {
    let admission = admit(&window, &request.header, &request.call)?;
    let app = window.app_handle();
    crate::startup::require_active(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    if matches!(&request.call, KnowledgeActivityCall::Engine(ActivityCall::ProjectAttribution { .. } | ActivityCall::GetDigest { .. } | ActivityCall::GetDay { .. } | ActivityCall::GetRange { .. })) {
        crate::project_provider::refresh(app, request.header.deadline_ms).await;
    }
    let result = match request.call {
        KnowledgeActivityCall::Host(call) => crate::lifecycle::dispatch_typed(app, call),
        KnowledgeActivityCall::Engine(ActivityCall::SendDigestToKnowledge { input, regenerated_from }) => {
            activity_engine::component::send_product_draft_typed(app, input, regenerated_from, |draft| {
                knowledge_vault_engine::component::offer_product_draft(app, draft)
            })
            .await
        }
        KnowledgeActivityCall::Engine(call) => {
            let method = call.method();
            api::dispatch(app, call).await.map(|value| crate::search::associate_activity(app, method, value))
        }
    };
    Ok(admission.finish(result, api::classify))
}
```

  - 기존 `component.rs` `execute`의 `knowledge.activity` 분기 세 개(lifecycle, send_digest_to_knowledge, 일반)를 위처럼 옮기고, `allowed()`의 `knowledge.activity` 팔을 지운다(이제 execute로 오면 InvalidRequest). `crate::lifecycle::dispatch_typed`와 `send_product_draft_typed`는 기존 문자열 버전의 본문을 enum 인자로 받게 한 것이다.
  - `lib.rs`의 knowledge plugin `invoke_handler`에 `crate::activity_ipc::activity`를 `execute`와 함께 등록한다. capability(`apps/devbox-knowledge/src-tauri/capabilities/*.json`)의 knowledge 권한 목록에 `allow-activity`(플러그인 권한 이름 규칙은 기존 `allow-execute`와 같게)를 추가하고, 플러그인 `build.rs`나 권한 파일이 command 목록을 요구하면 함께 추가한다.

- [ ] **Step 4: 프런트 구현**
  - `packages/knowledge-features/src/typed.ts`:

```ts
import { componentInvoke, type Component } from "./transport";

type CallOf<Calls, M> = Extract<Calls, { method: M }>;

/** Typed wrapper over the product transport: method names, argument shapes
 * and result types come from Rust-generated bindings. */
export function typedCall<Calls extends { method: string; args: unknown }, Results extends Record<Calls["method"], unknown>>(component: Component) {
  const invoke = componentInvoke(component);
  return <M extends Calls["method"]>(method: M, args: CallOf<Calls, M> extends { args: infer A } ? A : never): Promise<Results[M]> =>
    invoke<Results[M]>(method, args as Record<string, unknown>);
}
```

  - `activity/api.ts`: `import type { KnowledgeActivityCall } from "../generated/KnowledgeActivityCall"; import type { ActivityResults } from "../generated/activity-results";`와 `export const activityCall = typedCall<KnowledgeActivityCall, ActivityResults>("knowledge.activity");`를 추가하고, 파일 안의 `invoke("method", args)` 호출을 `activityCall("method", args)`로 바꾼다. 손으로 쓴 입력·결과 인터페이스(`ExportInput`, `ExportDailyDigest` 등)는 생성 타입의 `export type … = Generated…` 별칭으로 바꾸고, 모양이 다르면(선택 필드 등) 생성 타입을 따르도록 호출부를 고친다.
  - `activity/issues.ts`:

```ts
import type { ActivityIssue } from "../generated/ActivityIssue";

export const activityMessages: Record<ActivityIssue, string> = {
  unavailable: "활동 작업을 완료하지 못했습니다. 잠시 후 다시 시도해 주세요.",
  digest_cancelled: "요약 만들기를 취소했습니다.",
  component_args_invalid: "요청 형식이 올바르지 않습니다.",
  privacy_rules_invalid: "잘못된 규칙이 있어 저장하지 않았습니다. 표시된 줄을 고쳐 주세요.",
  privacy_rules_save_failed: "개인정보 규칙을 저장하지 못했습니다. 기존 규칙이 유지됩니다.",
  privacy_redaction_failed: "기존 기록에 규칙을 적용하지 못했습니다. 기록은 바뀌지 않았습니다.",
  // Task 3 Step 4에서 추가한 코드마다 한 줄. 빠지면 tsc가 실패한다.
};
```

    Knowledge 앱 `apps/devbox-knowledge/src/issues.ts`의 `issueError`가 activity 코드를 만나면 `activityMessages`를 먼저 보게 한다(기존 사전에 같은 코드가 있으면 그 줄을 지운다).
  - `apps/devbox-knowledge/src/transport.ts`: component별 명령 이름 map을 둔다.

```ts
const commandFor: Partial<Record<Component, string>> = { "knowledge.activity": "plugin:knowledge|activity" };
// invoke(commandFor[component] ?? "plugin:knowledge|execute", { request: { header, component, method, args } })
```

    typed command로 보낼 때는 본문에서 `component`를 뺀다(`{ request: { header, method, args } }`). 인접 태그 enum이 `deny_unknown_fields`라서 모르는 키가 있으면 거부된다. 기존 execute 경로는 P1-12에서 사라진다.

- [ ] **Step 5: 통과 확인** — Task 5를 먼저 해 생성 파일을 만든 뒤 Run: `cargo test -p devbox-knowledge --lib && pnpm --filter @devbox/knowledge-features --filter devbox-knowledge exec vitest run && pnpm --filter @devbox/knowledge-features --filter devbox-knowledge exec tsc --noEmit` → PASS.

- [ ] **Step 6: 커밋** — `git add -A && git commit -m "refactor(devbox-knowledge): route Activity through a typed command"`

---

### Task 5: TS 생성과 CI 검사

**Files:** Create `apps/devbox-knowledge/src-tauri/tests/typescript.rs`, `.github/scripts/check-generated-bindings.sh`; Modify `.github/workflows/ci.yml`, `CONVENTIONS.md`, `docs/adr/0014-typed-ipc.md`

- [ ] **Step 1: 생성 테스트** — `apps/devbox-knowledge/src-tauri/tests/typescript.rs`

```rust
//! Regenerates the TypeScript bindings consumed by packages/knowledge-features.
//! CI runs this test and fails if the committed files differ.
use std::path::PathBuf;
use ts_rs::{Config, TS};

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../packages/knowledge-features/src/generated")
}

#[test]
fn export_typescript_bindings() {
    let cfg = Config::new().with_large_int("number").with_out_dir(out_dir());
    devbox_knowledge_lib::activity_ipc::KnowledgeActivityCall::export_all(&cfg).unwrap();
    activity_engine::api::ActivityIssue::export_all(&cfg).unwrap();
    let results = product_ipc::results_map("ActivityResults", &devbox_knowledge_lib::activity_ipc::result_types(&cfg));
    std::fs::write(out_dir().join("activity-results.ts"), results).unwrap();
}
```

  `activity_ipc.rs`에 `pub fn result_types(cfg: &ts_rs::Config) -> Vec<(&'static str, String)>`를 추가한다: 각 메서드 이름과 결과 타입의 `<T as TS>::name(cfg)`(결과 타입 목록은 Task 3 Step 4 표). 결과 타입 선언이 import되도록, 생성 파일 머리에 필요한 `import type`을 넣거나 결과 타입들도 `export_all`로 같은 폴더에 내보낸다(ts-rs가 파일별로 import를 만든다).
  `activity_ipc` 모듈과 필요한 타입이 통합 테스트에서 보이도록 `lib.rs`에서 `pub mod activity_ipc;`로 연다.

- [ ] **Step 2: 생성·검사 스크립트** — `.github/scripts/check-generated-bindings.sh`

```bash
#!/usr/bin/env bash
set -euo pipefail
cargo test --locked -p devbox-knowledge --test typescript
pnpm exec biome format --write packages/knowledge-features/src/generated >/dev/null
git diff --exit-code -- packages/knowledge-features/src/generated || {
  echo "generated TypeScript bindings are stale; run .github/scripts/check-generated-bindings.sh and commit the result" >&2
  exit 1
}
```

  `ci.yml`의 Rust (Cargo workspace) job 끝에 `bash .github/scripts/check-generated-bindings.sh`를 추가한다(pnpm이 없으면 이 step 앞에 Frontend job과 같은 pnpm 준비 step을 넣거나, 포맷 줄을 빼고 ts-rs 출력 그대로 비교한다. 이 경우 `biome.json`에 `!**/generated`를 추가해 포맷 대상에서 뺀다).

- [ ] **Step 3: 실행과 커밋** — Run: `bash .github/scripts/check-generated-bindings.sh`(첫 실행은 파일을 만들고 diff로 실패) → 생성 파일을 `git add` 후 다시 실행해 PASS. 커밋: `git add -A && git commit -m "build(devbox-knowledge): generate TypeScript bindings for Activity"`

- [ ] **Step 4: 문서** — `docs/adr/0014-typed-ipc.md` 상태를 "채택"으로 바꾸고 "CommandArg 대신 명시 `admit`" 결정과 이유를 추가한다. `CONVENTIONS.md`에 "메서드 추가 절차"를 쓴다: ① engine `api.rs` enum에 variant와 `method()` 팔, `METHODS` ② `dispatch` 팔 ③ 새 오류 코드가 있으면 `issue_codes!`에 추가 ④ 결과 타입을 `result_types`에 추가 ⑤ `check-generated-bindings.sh` 실행 ⑥ 프런트 카탈로그 문구 추가(tsc가 누락을 알린다). 커밋: `git commit -am "docs(workspace): record the typed IPC decision and method checklist"`

---

### Task 6: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): Activity 화면에서 추적 시작·중지, 오늘·이번 주 보기, digest 만들기·저장·Knowledge로 보내기, CSV 내보내기, 개인정보 규칙 저장·기존 세션 적용, 자동 시작 켜기·끄기.
