# P1-12 Knowledge 전체를 타입 IPC로 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4와 `CONVENTIONS.md`의 "메서드 추가 절차"(P1-11)를 읽는다.

**Goal:** Knowledge의 나머지 component(노트·일일 기록, 검색·검색 설정·열기, 시작·노트 폴더, 종료 확인)를 P1-11의 타입 IPC로 옮기고 Knowledge의 문자열 `execute` 명령을 없앤다. `knowledge.migration` component 이름을 `knowledge.setup`으로 바꾼다(A1·A2·A3, D17·D18).

**Architecture:** component마다 Tauri command 하나: `notes`, `search`, `search_settings`, `opener`, `setup`, `commands`. engine이 가진 메서드는 engine의 `api.rs` enum(`knowledge_vault_engine::api::NotesCall`, `DailyCall`, `content_index_engine::api::{SearchCall, SearchSettingsCall, OpenerCall}`), host만 가진 메서드는 host enum으로 두고 `#[serde(untagged)]`로 합친다(P1-11 방식). 설치 검토용 `knowledge.commands`는 `admit`의 설치 검토 변형을 쓰고, `knowledge.setup`은 activation `import` 단계에서도 허용된다. 모든 오류 코드는 component별 `issue_codes!` enum이 되고 프런트 문구 카탈로그가 `Record<Issue, string>`로 완결성을 보장한다.

**Tech Stack:** Rust, ts-rs 12, Tauri v2, TypeScript

**Spec:** `review.md` §6 A1·A2·A3, §3 B6 · `00-roadmap.md` D17·D18 · ADR 0014

## Global Constraints

- `00-roadmap.md` §3 전부 적용. P1-11의 와이어 호환·권한·생성 파일 규칙을 그대로 따른다.
- 메서드 목록(빠짐없이 옮긴다):
  - notes(engine 38): `save_image_asset, take_pending_open, get_root, list_tree, read_file, open_inbound_note, write_file, create_file, create_directory, preview_rename, apply_rename, discard_rename_preview, delete_file, entry_path, reveal_entry, preview_quick_capture, save_quick_capture, discard_quick_capture_preview, list_templates, create_template, update_template, delete_template, preview_template, save_template, discard_template_preview, search_docs, list_tags, shortcut_status, preview_knowledge_draft, save_knowledge_draft, discard_knowledge_draft, renew_knowledge_draft, render_markdown, analyze_wikilinks, wikilink_candidates, backlinks, knowledge_watcher_status` + P0-03의 `save_note_journal, clear_note_journal, load_note_journal, discard_other_vault_journal`(engine 소유; P0-03의 2026-09-25 합의 반영)
  - notes(host): `read_clipboard_text, open_external_url, preview_session_summary, open_session_summary, open_result_draft`와 고정 응답 `set_root`(항상 `vault_binding_unavailable`), `open_targets`(항상 `[]`), `open_in`(항상 `provider_unavailable`)
  - daily: `preview_daily, save_daily, discard_daily`(`daily_note`은 계속 허용하지 않는다)
  - search(engine 7): `take_pending_open, list_roots, index_status, search_files, search_content, list_saved_queries, watcher_statuses` / search(host 5): `source_query, source_poll, source_cancel, source_reference, source_saved_reference`
  - search_settings(engine 6): `add_root, remove_root, index_now, cancel_index, save_saved_query, delete_saved_query`
  - opener: `open_file, reveal_file`(host `crate::search::open` 경유), `open_targets`(host: Workspace 대상 목록), `open_in`(host: `file_send::send`)
  - setup: `status, start_empty, continue_existing` + 노트 폴더 7개(`vault_change_status, schedule_vault_change, cancel_vault_change, prepare_vault_change, apply_vault_change, discard_vault_preview, vault_change_job`)
  - commands: `pending_quit, decide_quit`
  - activity(P1-11)의 host 메서드 `get_close_policy, set_close_policy`는 이미 옮겼다.
- 저널 계약은 P0-03 Task 4·5를 유지한다. `save_note_journal {path, content, baseRevision}`·`clear_note_journal {path}` 입력을 바꾸지 않고 native가 캐시한 현재 루트를 사용한다. `load_note_journal {}` 결과는 `JournalView { entries: JournalEntryView[], otherVaultCount }`이며 entries는 현재 폴더만 포함한다. `discard_other_vault_journal {}`는 `null`을 반환한다. 저장용 `vaultRoot`는 생성된 응답 타입에 노출하지 않는다. 전체 8개 한도에서 자동 삭제하지 않는 정책·명시적 삭제·연결 장애 시 캐시 경계를 유지한다.
- 요청 전 부수 작업을 그대로 옮긴다: search의 `source_query`가 `current_project`일 때, opener 전체, activity 일부에서 `project_provider::refresh`; notes의 디스크 작업은 `spawn_blocking`(기존 `notes_dispatch`); setup·commands는 `startup::require_active`를 건너뛴다.

## Review Focus

1. 연결이 끊긴 vault(오프라인 드라이브)에서 노트 목록을 여는 동안 Activity·검색이 멈추지 않는다(notes 작업이 계속 blocking worker에서 돈다). (Task 2)
2. activation `import` 단계에서 Knowledge 시작(`setup.start_empty`)이 허용되고 notes 작업은 `Unavailable`로 거부된다. (Task 4 테스트)
3. 설치 검토 중(`health` 단계) 종료 확인(`commands.pending_quit`)이 동작한다(설치 검토 admission). (Task 4)
4. 검색 결과에서 "Workspace에서 열기" → opener가 Workspace 대상 하나(설치·연결 필요 표시 포함)를 보여 주고 전송한다. (Task 3)
5. 빠진 문구: B6에서 코드로 바꾼 오류(`preview_stale`, `preview_expired`, 빠른 기록 코드 등)가 전부 카탈로그에 있다(tsc가 보장). (Task 5)

## Branch · PR

- 묶음: **B6** — 브랜치 `refactor/suite/typed-ipc-knowledge-api-control`, PR 제목 `refactor(suite): typed IPC for Knowledge, API Studio and Control Center`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `refactor(devbox-knowledge): move every component to typed commands`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

---

### Task 1: engine API — knowledge-vault-engine

**Files:** Create `crates/knowledge-vault-engine/src/api.rs`; Modify `component.rs`, `commands/*.rs`, `Cargo.toml`(`ts-rs`, `product-ipc`)

- [ ] **Step 1: 실패하는 테스트** — `api.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_methods_are_listed_once_and_parse() {
        let mut names: Vec<_> = NOTES_METHODS.to_vec();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), NOTES_METHODS.len());
        for method in ["set_root", "open_targets", "open_in", "daily_note"] {
            assert!(!NOTES_METHODS.contains(&method), "{method} is host-owned or retired");
        }
        let call: NotesCall = serde_json::from_str(r#"{"method":"read_file","args":{"path":"a.md"}}"#).unwrap();
        assert_eq!(call.method(), "read_file");
        let daily: DailyCall = serde_json::from_str(r#"{"method":"discard_daily","args":{"previewId":"p"}}"#).unwrap();
        assert_eq!(daily.method(), "discard_daily");
        for method in ["save_note_journal", "clear_note_journal", "load_note_journal", "discard_other_vault_journal"] {
            assert!(NOTES_METHODS.contains(&method), "missing journal method: {method}");
        }
        let discard: NotesCall = serde_json::from_str(r#"{"method":"discard_other_vault_journal","args":{}}"#).unwrap();
        assert_eq!(discard.method(), "discard_other_vault_journal");
    }

    #[test]
    fn b6_codes_are_stable_issue_codes() {
        for code in ["preview_stale", "preview_expired", "quick_capture_body_required", "note_conflict"] {
            assert!(NotesIssue::from_code(code).is_some(), "{code}");
        }
        assert_eq!(classify("anything else"), "unavailable");
    }
}
```

  (`read_file`·`discard_daily`의 실제 인자 이름은 각 shim의 `Input`을 보고 맞춘다.)

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-knowledge-vault-engine --lib api` → 컴파일 실패.
- [ ] **Step 3: 구현** — P1-11 Task 3과 같은 방식으로 `NotesCall`(위 목록의 engine 메서드), `DailyCall`(daily 3개; `commands/daily.rs`의 문자열 `dispatch`를 enum 인자로), `NOTES_METHODS`, `NotesIssue`(engine 오류 문자열 전부 + P0-03 코드: `quick_capture_*`, `preview_stale`, `preview_expired`, `journal_unavailable`, `journal_limit`), `classify`, `dispatch`, `daily_dispatch`를 만든다. 입력·결과 타입에 `ts_rs::TS`를 붙인다. 저널은 응답용 `JournalEntryView`·`JournalView`를 생성하고, `notes-results.ts`의 load 결과를 배열로 되돌리지 않으며 discard 결과는 null로 고정한다. `__component_*` shim을 지우기 전에 새 메서드의 순수 dispatch와 오류 분류를 연결한다. 이후 `__component_*` shim과 `#[tauri::command]` 속성을 지운다. `component.rs`에는 `initialize`·`configure_document_helper`·`offer_product_draft`·`create_private_vault` 같은 host용 공개 함수만 남긴다.
- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-knowledge-vault-engine --lib` → PASS.
- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-knowledge): give the vault engine a typed API"`

---

### Task 2: engine API — content-index-engine

**Files:** Create `crates/content-index-engine/src/api.rs`; Modify `component.rs`, `commands/*.rs`, `Cargo.toml`

- [ ] **Step 1: 실패하는 테스트** — `api.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_settings_and_opener_methods_are_separate_enums() {
        assert!(serde_json::from_str::<SearchCall>(r#"{"method":"add_root","args":{"path":"C:/x"}}"#).is_err());
        assert!(serde_json::from_str::<SearchSettingsCall>(r#"{"method":"add_root","args":{"path":"C:/x"}}"#).is_ok());
        assert!(serde_json::from_str::<OpenerCall>(r#"{"method":"reveal_file","args":{"path":"C:/x"}}"#).is_ok());
        assert_eq!(SEARCH_METHODS.len() + SETTINGS_METHODS.len() + OPENER_METHODS.len(), 15);
    }
}
```

  (`open_targets`·`open_in`은 engine이 아니라 host opener가 처리하므로 engine `OpenerCall`은 `open_file`·`reveal_file` 두 개다: 7 + 6 + 2 = 15. 인자 이름은 shim을 따른다.)

- [ ] **Step 2–5:** P1-11 Task 3과 같은 방식으로 `SearchCall`, `SearchSettingsCall`, `OpenerCall`, `SearchIssue`, `classify`, `dispatch_search`, `dispatch_settings`, `dispatch_opener`를 만들고 shim·속성을 지운다. Run: `cargo test -p devbox-content-index-engine --lib` → PASS. 커밋: `git commit -am "refactor(devbox-knowledge): give the content index engine a typed API"`

---

### Task 3: host command 여섯 개

**Files:** Create `apps/devbox-knowledge/src-tauri/src/ipc/{mod,notes,search,setup,commands}.rs`(P1-11의 `activity_ipc.rs`도 `ipc/activity.rs`로 옮긴다); Modify `lib.rs`, `component.rs`(삭제 대상), `capabilities/*.json`

**Interfaces (Produces):** `ipc::{notes, search, search_settings, opener, setup, commands, activity}` Tauri commands; `KnowledgeNotesCall`, `KnowledgeSearchCall`, `KnowledgeOpenerCall`, `SetupCall`, `QuitCall` host 합성 enum; `ipc::result_types_*`

- [ ] **Step 1: 실패하는 테스트** — `ipc/mod.rs` 테스트 모듈

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::ComponentCall;

    #[test]
    fn each_component_owns_its_routes() {
        let notes: notes::KnowledgeNotesCall = serde_json::from_str(r#"{"method":"list_tags","args":{}}"#).unwrap();
        assert_eq!(notes.routes(), &["notes", "daily"]);
        let clipboard: notes::KnowledgeNotesCall = serde_json::from_str(r#"{"method":"read_clipboard_text","args":{}}"#).unwrap();
        assert!(matches!(clipboard, notes::KnowledgeNotesCall::Host(_)));
        let setup: setup::SetupCall = serde_json::from_str(r#"{"method":"start_empty","args":{}}"#).unwrap();
        assert_eq!(setup::SetupCall::COMPONENT, "knowledge.setup");
        assert_eq!(setup.routes(), &["notes"]);
        let quit: commands::QuitCall = serde_json::from_str(r#"{"method":"pending_quit","args":{}}"#).unwrap();
        assert!(commands::QuitCall::INSTALLATION_REVIEW);
        assert_eq!(quit.method(), "pending_quit");
    }

    #[test]
    fn the_string_execute_command_is_gone() {
        let lib = include_str!("../lib.rs");
        assert!(!lib.contains("component::plugin"), "execute plugin must be removed");
    }
}
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-knowledge --lib ipc` → FAIL.
- [ ] **Step 3: `ComponentCall`에 설치 검토 표시 추가** — `crates/product-ipc/src/lib.rs`의 trait에 `const INSTALLATION_REVIEW: bool = false;`를 추가하고, `product_shell_tauri::admit`이 이 값이 `true`면 `authorize_installation_review`를, 아니면 `authorize`를 쓰게 한다(`admission.rs`). `crates/product-contract/src/activation.rs`의 `allows`에 `import` 단계에서 `{product}.setup`도 허용하도록 한 줄을 더한다(`{product}.migration`은 Workspace가 P1-14에서 옮길 때까지 남긴다). activation 테스트에 `assert!(a.allows("knowledge", "knowledge.setup"))`(import 단계)를 추가한다.
- [ ] **Step 4: host command 구현** — 기존 `component.rs` `execute`의 분기를 아래처럼 나눈다.

| command | 입장 뒤 부수 작업 | dispatch |
|---|---|---|
| `notes` | `startup::require_active`, 디스크 작업은 `spawn_blocking`(`notes_dispatch`) | Host: 클립보드·외부 URL·세션 요약·결과 초안·고정 응답 3개, Daily: `daily_dispatch`, Engine: `knowledge_vault_engine::api::dispatch` |
| `search` | `require_active`, `source_query`가 current_project면 `project_provider::refresh` | Host: `crate::search::dispatch_typed`·`federation::saved_reference`, Engine: `dispatch_search` |
| `search_settings` | `require_active` | `dispatch_settings` |
| `opener` | `require_active`, `project_provider::refresh` | `open_file`·`reveal_file`: `crate::search::open` 경유 engine opener, `open_targets`: 설치된 Workspace 대상, `open_in`: `crate::file_send::send` |
| `setup` | (active 검사 없음) | `crate::startup::dispatch_typed` / `crate::vault_binding::dispatch_typed` |
| `commands` | (active 검사 없음, 설치 검토 admission) | `crate::lifecycle::quit_dispatch_typed` |

  각 command는 P1-11 `activity`와 같은 모양(`admit` → 부수 작업 → dispatch → `admission.finish(result, classify)`)이다. `routes()`: notes는 `["notes", "daily"]`, search·search_settings·opener는 `["search"]`, setup·commands는 `["notes"]`. `lib.rs`에서 knowledge plugin의 `invoke_handler`를 `tauri::generate_handler![ipc::activity, ipc::notes, ipc::search, ipc::search_settings, ipc::opener, ipc::setup, ipc::commands]`로 바꾸고, `component.rs`에 남은 공용 부분(plugin setup·lifecycle 이벤트)을 `ipc/mod.rs`의 `plugin()`으로 옮긴 뒤 `component.rs`를 지운다. capability 파일에 새 command 권한을 추가하고 `allow-execute`를 지운다.
- [ ] **Step 5: 통과 확인** — Run: `cargo test -p devbox-knowledge --lib && cargo test -p product-contract -p product-shell-tauri --lib` → PASS.
- [ ] **Step 6: 커밋** — `git add -A && git commit -m "refactor(devbox-knowledge): replace execute with typed component commands"`

---

### Task 4: 카탈로그 이름 변경과 activation

- [ ] **Step 1: 실패하는 테스트** — `packages/product-shell/src/feature-fixtures.test.ts`(또는 카탈로그를 검사하는 기존 테스트)에 추가한다.

```ts
it("names the Knowledge startup component knowledge.setup", () => {
  const ids = catalog.components.filter((component) => component.owner === "knowledge").map((component) => component.id);
  expect(ids).toContain("knowledge.setup");
  expect(ids).not.toContain("knowledge.migration");
});
```

- [ ] **Step 2: 구현** — `apps/products.json`의 `knowledge.migration`을 `{"id": "knowledge.setup", "authority": "store-setup", …}`로 바꾸고 `catalogRevision` +1, 하드코딩된 revision을 갱신한다(P1-02 방법). `crates/catalog`가 authority 값 목록을 검사하면 `store-setup`을 추가한다.
- [ ] **Step 3: 확인·커밋** — Run: `pnpm --filter @devbox/product-shell exec vitest run && cargo test -p catalog` → PASS. `git commit -am "refactor(devbox-knowledge): rename the setup component"`

---

### Task 5: 프런트·생성 타입·문구 카탈로그

**Files:** `apps/devbox-knowledge/src-tauri/tests/typescript.rs`, `packages/knowledge-features/src/{transport.ts,typed.ts,generated/*,notes/api.ts,search/api.ts,notes/issues.ts,search/issues.ts}`, `apps/devbox-knowledge/src/{transport.ts,issues.ts,Startup.tsx,VaultSettings.tsx,VaultSetup.tsx,QuitGuard.tsx,LifecycleSettings.tsx}`

- [ ] **Step 1: 실패하는 테스트** — `packages/knowledge-features/src/issues.test.ts`

```ts
import { describe, expect, it } from "vitest";
import { notesMessages } from "./notes/issues";
import { searchMessages } from "./search/issues";
import { setupMessages } from "./setup/issues";

describe("Knowledge issue catalogs", () => {
  it.each([["notes", notesMessages], ["search", searchMessages], ["setup", setupMessages]] as const)("%s has a message for every code", (_, catalog) => {
    for (const [code, message] of Object.entries(catalog)) expect(message.trim(), code).not.toBe("");
  });
});
```

- [ ] **Step 2: 생성** — `tests/typescript.rs`에 새 enum·issue·결과 map을 추가한다(`KnowledgeNotesCall`, `KnowledgeSearchCall`, `SearchSettingsCall`, `KnowledgeOpenerCall`, `SetupCall`, `QuitCall`, `NotesIssue`, `SearchIssue`, `SetupIssue`; `notes-results.ts` 등). Run: `bash .github/scripts/check-generated-bindings.sh` → 첫 실행은 diff로 실패, 파일을 추가한 뒤 PASS.
- [ ] **Step 3: 호출부 교체**
  - `packages/knowledge-features/src/transport.ts`: `Component` 유니온을 `"knowledge.notes" | "knowledge.activity" | "knowledge.search" | "knowledge.search-settings" | "knowledge.opener" | "knowledge.setup" | "knowledge.commands"`로 바꾸고, `componentInvoke`의 search 소유자 추론(`searchSettings`·`searchOpeners` 집합)을 지운다. 호출부는 component를 명시한 typed 함수(`notesCall`, `searchCall`, `searchSettingsCall`, `openerCall`, `setupCall`, `quitCall`)를 쓴다.
  - `apps/devbox-knowledge/src/transport.ts`: `commandFor`를 전체 component로 채우고 `execute` 기본값을 지운다(모르는 component면 오류).

```ts
const commandFor: Record<Component, string> = {
  "knowledge.activity": "plugin:knowledge|activity",
  "knowledge.notes": "plugin:knowledge|notes",
  "knowledge.search": "plugin:knowledge|search",
  "knowledge.search-settings": "plugin:knowledge|search_settings",
  "knowledge.opener": "plugin:knowledge|opener",
  "knowledge.setup": "plugin:knowledge|setup",
  "knowledge.commands": "plugin:knowledge|commands",
};
```

  - `componentInvoke("knowledge.migration")`를 쓰던 파일(`Startup.tsx`, `VaultSettings.tsx`, `VaultSetup.tsx` 등: `rg -n 'knowledge.migration' apps packages`)을 `setupCall`로 바꾼다.
  - 문구: `apps/devbox-knowledge/src/issues.ts`의 사전을 component별 `Record<…Issue, string>`(`notes/issues.ts`, `search/issues.ts`, `setup/issues.ts`, activity는 P1-11)로 옮기고, `issueError(component, code)`가 해당 카탈로그에서 찾게 한다. 카탈로그에 없는 코드(생성 유니온 밖)는 `unavailable` 문구.
- [ ] **Step 4: 통과 확인** — Run: `pnpm --filter @devbox/knowledge-features --filter devbox-knowledge exec vitest run && pnpm --filter @devbox/knowledge-features --filter devbox-knowledge exec tsc --noEmit && pnpm exec biome ci .` → PASS.
- [ ] **Step 5: 커밋** — `git add -A && git commit -m "refactor(devbox-knowledge): call typed commands with generated types"`

---

### Task 6: PR 완료

- [ ] `rg -n '"component_method_invalid"|plugin:knowledge\|execute' apps/devbox-knowledge packages/knowledge-features` → 0건.
- [ ] `00-roadmap.md` §4.4–§4.9.
- [ ] PR 본문 "Windows 실기 확인"(사용자 확인 대기): 노트 열기·저장·이름 바꾸기·삭제·템플릿·빠른 기록·일일 기록, 검색·검색 폴더 추가·색인·저장 검색, 검색 결과 열기·탐색기에서 보기·Workspace로 보내기, 노트 폴더 변경 예약·적용, 창 닫기 시 저장 확인.
