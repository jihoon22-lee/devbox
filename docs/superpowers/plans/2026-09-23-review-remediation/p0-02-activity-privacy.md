# P0-02 Activity 개인정보 규칙 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md`의 §3 공통 제약과 §4 PR 공통 절차를 읽는다.

**Goal:** Activity 개인정보 규칙을 사용자가 실제로 입력·저장할 수 있게 하고, 규칙이 잘못됐거나 읽히지 않으면 창 제목을 저장하지 않는(fail-closed) 동작으로 바꾼다.

**Architecture:** 규칙 검증·컴파일을 `CompiledRules`(순수 로직)로 모으고, `AppState`에 컴파일된 규칙을 캐시해 추적 루프가 2초마다 DB를 읽고 정규식을 다시 컴파일하지 않게 한다. 저장 명령은 검증 결과를 값으로 돌려주고, UI는 줄 단위 textarea 초안을 저장 버튼으로 제출한다.

**Tech Stack:** Rust(`regex` 1, `rusqlite` 0.32), React 19 + TypeScript, Vitest + Testing Library

**Spec:** `review.md` §3 B1 · `00-roadmap.md` §2 결정 D4·D5

## Global Constraints

- `00-roadmap.md` §3 전부 적용 (UI 문구 한국어, 코드·커밋 영어, Conventional Commits, D4 테스트 정책).
- 정규식 규칙 한 목록 최대 64개, 규칙 하나 최대 512자, `regex` size limit 1 MiB.
- 규칙 JSON 저장 키는 기존 `privacy_rules`를 그대로 쓴다(데이터 호환).
- 치환 문자열은 기존과 같은 `[redacted]`.

## Review Focus

1. 쉼표·공백·한글이 든 패턴(`patient \d{1,3}`, `InPrivate - Microsoft Edge`, `은행 거래`) → 입력값 그대로 저장되고 적용된다. (Task 1, Task 6 테스트)
2. 저장된 JSON이 손상됐거나 읽을 수 없을 때 → 제목을 저장하지 않고 세션 시간만 기록, UI는 경고를 표시한다. (Task 2, Task 6)
3. DB가 쓰기 불가일 때 저장 → 오류를 돌려주고 캐시된 규칙은 바뀌지 않는다. (Task 4)
4. 규칙 저장 직후 추적 루프 → 다음 세션부터 새 규칙이 적용된다(재시작 불필요). (Task 3)
5. `기존 세션에 적용` 도중 실패 → 기존 기록은 하나도 바뀌지 않는다(트랜잭션). (Task 4)

## Branch · PR

- 묶음: **B1** — 브랜치 `fix/suite/bootstrap-privacy-autosave-connection`, PR 제목 `fix(suite): pin toolchains and fix activity privacy, Knowledge autosave and Suite connections`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `fix(devbox-knowledge): make activity privacy rules enterable and fail closed`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).
- PR 본문: `00-roadmap.md` §4.6 템플릿(`Ledger: #<번호>` 줄 포함).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `crates/activity-engine/src/core/privacy.rs` | 전면 교체 | 규칙 타입, 검증, 컴파일, 적용 |
| `crates/activity-engine/src/core/db.rs` | 수정 | `try_set_setting` 추가 |
| `crates/activity-engine/src/commands/privacy.rs` | 전면 교체 | `PrivacyState`, 저장/조회/소급 적용 명령과 shim |
| `crates/activity-engine/src/commands/tracking.rs` | 수정 | `AppState.privacy` 필드, 추적 루프가 캐시 사용 |
| `crates/activity-engine/src/component.rs` | 수정 | `AppState` 생성 시 `privacy` 초기화 |
| `apps/devbox-knowledge/src-tauri/src/component.rs` | 수정 | 새 오류 코드 매핑 |
| `apps/devbox-knowledge/src/issues.ts` | 수정 | 새 오류 문구 |
| `packages/knowledge-features/src/activity/api.ts` | 수정 | 새 응답 타입 |
| `packages/knowledge-features/src/activity/PrivacyRulesPanel.tsx` | 생성 | 규칙 편집 UI |
| `packages/knowledge-features/src/activity/PrivacyRulesPanel.test.tsx` | 생성 | UI 테스트 |
| `packages/knowledge-features/src/activity/App.tsx` | 수정 | 기존 인라인 패널을 컴포넌트로 교체 |
| `packages/knowledge-features/src/activity/App.contextMenu.test.tsx` | 수정 | `getPrivacyRules` mock 응답 형태 |

---

### Task 1: 규칙 검증·컴파일 (`CompiledRules`)

**Files:**
- Modify: `crates/activity-engine/src/core/privacy.rs` (파일 전체 교체)

**Interfaces:**
- Produces: `PrivacyRules`(기존 필드 유지), `RuleField`, `RuleProblem`, `InvalidRule`, `CompiledRules::{compile, fail_closed, apply}`, `parse_stored_rules`, `REDACTED`, `MAX_RULES_PER_LIST`, `MAX_RULE_CHARS`
- 제거: 자유 함수 `apply(&PrivacyRules, ..)`, `parse_rules(&str)` — Task 3·4에서 호출부를 모두 바꾼다.

- [ ] **Step 1: 실패하는 테스트 작성** — `privacy.rs`의 기존 `#[cfg(test)] mod tests`를 아래로 교체한다(구현 교체는 Step 3).

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn rules(processes: &[&str], excluded: &[&str], redact: &[&str]) -> PrivacyRules {
        PrivacyRules {
            excluded_processes: processes.iter().map(|s| s.to_string()).collect(),
            excluded_title_patterns: excluded.iter().map(|s| s.to_string()).collect(),
            redact_title_patterns: redact.iter().map(|s| s.to_string()).collect(),
            mask_all_titles: false,
        }
    }

    #[test]
    fn empty_rules_pass_titles_through() {
        let compiled = CompiledRules::compile(&PrivacyRules::default()).unwrap();
        assert_eq!(
            compiled.apply("chrome.exe", "GitHub"),
            Some(("chrome.exe".to_string(), "GitHub".to_string()))
        );
    }

    #[test]
    fn quantifier_with_comma_is_a_single_working_pattern() {
        let compiled = CompiledRules::compile(&rules(&[], &[], &[r"patient \d{1,3}"])).unwrap();
        assert_eq!(
            compiled.apply("hosp.exe", "patient 123 chart").unwrap().1,
            "[redacted] chart"
        );
    }

    #[test]
    fn spaces_and_hangul_are_matched_verbatim() {
        let compiled =
            CompiledRules::compile(&rules(&[], &["InPrivate - Microsoft Edge", "은행 거래"], &[]))
                .unwrap();
        assert_eq!(compiled.apply("msedge.exe", "뉴스 - InPrivate - Microsoft Edge").unwrap().1, "");
        assert_eq!(compiled.apply("app.exe", "은행 거래 내역").unwrap().1, "");
        assert_eq!(compiled.apply("app.exe", "은행").unwrap().1, "은행");
    }

    #[test]
    fn excluded_process_drops_session_case_insensitively() {
        let compiled = CompiledRules::compile(&rules(&["LockApp.exe"], &[], &[])).unwrap();
        assert!(compiled.apply("lockapp.exe", "Lock screen").is_none());
        assert!(compiled.apply("chrome.exe", "x").is_some());
    }

    #[test]
    fn mask_all_titles_keeps_session_without_title() {
        let mut value = rules(&[], &[], &["secret"]);
        value.mask_all_titles = true;
        let compiled = CompiledRules::compile(&value).unwrap();
        assert_eq!(compiled.apply("a.exe", "secret doc"), Some(("a.exe".into(), String::new())));
    }

    #[test]
    fn syntax_error_is_reported_with_field_and_index() {
        let error = CompiledRules::compile(&rules(&[], &["ok", "(unclosed"], &[])).unwrap_err();
        assert_eq!(
            error,
            vec![InvalidRule {
                field: RuleField::ExcludedTitlePatterns,
                index: 1,
                problem: RuleProblem::Syntax
            }]
        );
    }

    #[test]
    fn empty_too_long_and_too_many_are_rejected() {
        let long = "a".repeat(MAX_RULE_CHARS + 1);
        let error = CompiledRules::compile(&rules(&["  "], &[long.as_str()], &[])).unwrap_err();
        assert!(error.contains(&InvalidRule {
            field: RuleField::ExcludedProcesses,
            index: 0,
            problem: RuleProblem::Empty
        }));
        assert!(error.contains(&InvalidRule {
            field: RuleField::ExcludedTitlePatterns,
            index: 0,
            problem: RuleProblem::TooLong
        }));
        let many: Vec<String> = (0..=MAX_RULES_PER_LIST).map(|n| format!("p{n}")).collect();
        let value = PrivacyRules { redact_title_patterns: many, ..PrivacyRules::default() };
        assert!(CompiledRules::compile(&value).unwrap_err().contains(&InvalidRule {
            field: RuleField::RedactTitlePatterns,
            index: MAX_RULES_PER_LIST,
            problem: RuleProblem::TooMany
        }));
    }

    #[test]
    fn fail_closed_never_keeps_a_title() {
        let compiled = CompiledRules::fail_closed();
        assert_eq!(compiled.apply("a.exe", "private"), Some(("a.exe".into(), String::new())));
    }

    #[test]
    fn stored_rules_parse_missing_as_default_and_garbage_as_error() {
        assert_eq!(parse_stored_rules(None), Ok(PrivacyRules::default()));
        assert!(parse_stored_rules(Some("{not json")).is_err());
        assert_eq!(
            parse_stored_rules(Some(r#"{"maskAllTitles":true}"#)).unwrap(),
            PrivacyRules { mask_all_titles: true, ..PrivacyRules::default() }
        );
    }
}
```

- [ ] **Step 2: 실패 확인**

Run: `source ~/.cargo/env && cargo test -p devbox-activity-engine --lib core::privacy`
Expected: 컴파일 오류(`CompiledRules`, `InvalidRule` 등 미정의).

- [ ] **Step 3: 구현** — `privacy.rs`의 테스트 모듈 위쪽 전체를 아래로 교체한다.

```rust
//! Privacy rules (pure logic). Rules apply **before** a session reaches the
//! DB; they are not a UI filter. Excluded or replaced text must never be
//! stored, logged, or written to an integration snapshot.

use regex::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};
use serde::{Deserialize, Serialize};

pub const MAX_RULES_PER_LIST: usize = 64;
pub const MAX_RULE_CHARS: usize = 512;
const REGEX_SIZE_LIMIT: usize = 1 << 20;
pub const REDACTED: &str = "[redacted]";

/// Stored and edited rule set. Field names are the persisted wire format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyRules {
    /// Exact process names (case-insensitive). A match drops the session.
    #[serde(default)]
    pub excluded_processes: Vec<String>,
    /// A match keeps the session but stores an empty title.
    #[serde(default)]
    pub excluded_title_patterns: Vec<String>,
    /// Every match is replaced with `[redacted]`.
    #[serde(default)]
    pub redact_title_patterns: Vec<String>,
    /// Never store any title.
    #[serde(default)]
    pub mask_all_titles: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RuleField {
    ExcludedProcesses,
    ExcludedTitlePatterns,
    RedactTitlePatterns,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleProblem {
    Empty,
    TooLong,
    TooMany,
    Syntax,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InvalidRule {
    pub field: RuleField,
    /// Zero-based position in the submitted list. `TooMany` reports the first
    /// entry beyond the limit.
    pub index: usize,
    pub problem: RuleProblem,
}

/// Validated rules, compiled once and shared by the collector.
#[derive(Debug)]
pub struct CompiledRules {
    excluded_processes: Vec<String>,
    excluded_titles: RegexSet,
    redactions: Vec<Regex>,
    mask_all_titles: bool,
}

fn check_list(field: RuleField, values: &[String], problems: &mut Vec<InvalidRule>) {
    if values.len() > MAX_RULES_PER_LIST {
        problems.push(InvalidRule { field, index: MAX_RULES_PER_LIST, problem: RuleProblem::TooMany });
    }
    for (index, value) in values.iter().enumerate().take(MAX_RULES_PER_LIST) {
        if value.trim().is_empty() {
            problems.push(InvalidRule { field, index, problem: RuleProblem::Empty });
        } else if value.chars().count() > MAX_RULE_CHARS {
            problems.push(InvalidRule { field, index, problem: RuleProblem::TooLong });
        }
    }
}

fn build_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern).size_limit(REGEX_SIZE_LIMIT).build()
}

fn has_problem(problems: &[InvalidRule], field: RuleField, index: usize) -> bool {
    problems.iter().any(|p| p.field == field && p.index == index)
}

impl CompiledRules {
    /// Validate and compile every rule. Nothing is partially accepted.
    pub fn compile(rules: &PrivacyRules) -> Result<Self, Vec<InvalidRule>> {
        let mut problems = Vec::new();
        check_list(RuleField::ExcludedProcesses, &rules.excluded_processes, &mut problems);
        check_list(RuleField::ExcludedTitlePatterns, &rules.excluded_title_patterns, &mut problems);
        check_list(RuleField::RedactTitlePatterns, &rules.redact_title_patterns, &mut problems);
        for (index, pattern) in rules.excluded_title_patterns.iter().enumerate().take(MAX_RULES_PER_LIST) {
            if !has_problem(&problems, RuleField::ExcludedTitlePatterns, index) && build_regex(pattern).is_err() {
                problems.push(InvalidRule { field: RuleField::ExcludedTitlePatterns, index, problem: RuleProblem::Syntax });
            }
        }
        let mut redactions = Vec::new();
        for (index, pattern) in rules.redact_title_patterns.iter().enumerate().take(MAX_RULES_PER_LIST) {
            if has_problem(&problems, RuleField::RedactTitlePatterns, index) {
                continue;
            }
            match build_regex(pattern) {
                Ok(regex) => redactions.push(regex),
                Err(_) => problems.push(InvalidRule { field: RuleField::RedactTitlePatterns, index, problem: RuleProblem::Syntax }),
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        let excluded_titles = RegexSetBuilder::new(&rules.excluded_title_patterns)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()
            .map_err(|_| {
                vec![InvalidRule { field: RuleField::ExcludedTitlePatterns, index: 0, problem: RuleProblem::Syntax }]
            })?;
        Ok(Self {
            excluded_processes: rules.excluded_processes.iter().map(|p| p.trim().to_lowercase()).collect(),
            excluded_titles,
            redactions,
            mask_all_titles: rules.mask_all_titles,
        })
    }

    /// Used when stored rules cannot be read or compiled: the session keeps
    /// its process and duration but no title is ever stored.
    pub fn fail_closed() -> Self {
        Self {
            excluded_processes: Vec::new(),
            excluded_titles: RegexSet::empty(),
            redactions: Vec::new(),
            mask_all_titles: true,
        }
    }

    /// `None` drops the whole session. `Some((app, title))` is what may be stored.
    pub fn apply(&self, app: &str, title: &str) -> Option<(String, String)> {
        let app_lower = app.to_lowercase();
        if self.excluded_processes.iter().any(|p| *p == app_lower) {
            return None;
        }
        if self.mask_all_titles || self.excluded_titles.is_match(title) {
            return Some((app.to_string(), String::new()));
        }
        let mut out = title.to_string();
        for regex in &self.redactions {
            out = regex.replace_all(&out, REDACTED).into_owned();
        }
        Some((app.to_string(), out))
    }
}

/// Stored JSON → rules. A missing value means "no rules". Anything unreadable
/// is an error so callers fail closed instead of silently collecting titles.
pub fn parse_stored_rules(json: Option<&str>) -> Result<PrivacyRules, ()> {
    match json {
        None => Ok(PrivacyRules::default()),
        Some(text) => serde_json::from_str(text).map_err(|_| ()),
    }
}
```

- [ ] **Step 4: 통과 확인** (crate의 다른 모듈은 Task 3·4 전까지 컴파일되지 않으므로 이 단계는 `privacy.rs`만 빌드되는지 확인한다)

Run: `source ~/.cargo/env && cargo test -p devbox-activity-engine --lib core::privacy`
Expected: `commands/privacy.rs`, `commands/tracking.rs`에서 옛 `apply`/`parse_rules` 참조 컴파일 오류만 남는다. 이 오류는 Task 3·4에서 해소한다. Task 1은 Task 4 Step 4에서 함께 통과를 확인한다.

- [ ] **Step 5: 커밋은 Task 4 끝에서 한 번에 한다** (crate가 컴파일되는 상태로만 커밋).

---

### Task 2: 설정 쓰기 오류를 드러내는 `try_set_setting`

**Files:**
- Modify: `crates/activity-engine/src/core/db.rs` (`set_setting` 바로 아래에 추가)

**Interfaces:**
- Produces: `pub fn try_set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()>`

- [ ] **Step 1: 테스트 추가** — `db.rs`의 `#[cfg(test)] mod tests` 안에 추가.

```rust
    #[test]
    fn try_set_setting_reports_write_failure() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        try_set_setting(&conn, "k", "v1").unwrap();
        assert_eq!(get_setting(&conn, "k", ""), "v1");
        conn.execute_batch("PRAGMA query_only = ON").unwrap();
        assert!(try_set_setting(&conn, "k", "v2").is_err());
        assert_eq!(get_setting(&conn, "k", ""), "v1");
    }
```

(테스트 모듈의 기존 import에 `Connection`, `migrate`가 없으면 `use super::*;`로 충분한지 확인하고, 없으면 `use rusqlite::Connection;`를 추가한다.)

- [ ] **Step 2: 구현**

```rust
/// Like `set_setting`, but the caller learns whether the value was stored.
pub fn try_set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map(|_| ())
}
```

- [ ] **Step 3: 커밋은 Task 4 끝에서 함께.**

---

### Task 3: 컴파일된 규칙 캐시와 추적 루프

**Files:**
- Modify: `crates/activity-engine/src/commands/tracking.rs`
- Modify: `crates/activity-engine/src/component.rs:22-32`

**Interfaces:**
- Consumes: `CompiledRules`, `PrivacyState`(Task 4에서 정의; 이 Task와 Task 4는 같은 커밋)
- Produces: `AppState.privacy: crate::commands::privacy::PrivacyState`

- [ ] **Step 1: `AppState`에 필드 추가** (`tracking.rs:122`)

```rust
pub struct AppState {
    /// Native-owned snapshot namespace; None preserves the standalone legacy contract.
    pub integration_root: Option<std::path::PathBuf>,
    /// Compiled privacy rules shared by the collector and the settings UI.
    pub privacy: crate::commands::privacy::PrivacyState,
    pub db: Mutex<Connection>,
    // (나머지 필드는 그대로)
}
```

- [ ] **Step 2: 추적 루프가 캐시를 쓰도록 수정**
  - `use crate::core::privacy::{apply as apply_privacy, parse_rules, PrivacyRules};`를 `use crate::core::privacy::CompiledRules;`로 바꾼다.
  - `fn privacy_rules(conn: &Connection) -> PrivacyRules`를 삭제한다.
  - `spawn_poller`에서 `let rules = { let conn = state.db.lock().unwrap(); privacy_rules(&conn) };`를 `let rules = state.privacy.current();`로 바꾼다.
  - `stop_tracking_runtime`의 `insert_filtered(&conn, &c, &privacy_rules(&conn))`를 `insert_filtered(&conn, &c, &state.privacy.current())`로 바꾼다.
  - `insert_filtered`를 아래로 교체한다.

```rust
/// Apply privacy rules **before** insert, then store or skip the session.
fn insert_filtered(
    conn: &Connection,
    closed: &ClosedSession,
    rules: &CompiledRules,
) -> rusqlite::Result<()> {
    let Some((app, title)) = rules.apply(&closed.app, &closed.title) else {
        return Ok(());
    };
    insert_session(
        conn,
        &ClosedSession { app, title, start_ts: closed.start_ts, end_ts: closed.end_ts },
    )
}
```

- [ ] **Step 3: `AppState` 생성부 수정** — `component.rs:22`의 구조체 리터럴에서 `db: Mutex::new(conn)` **앞에** 다음 필드를 둔다(필드 평가 순서 때문에 `conn` 이동 전에 읽어야 한다).

```rust
        privacy: crate::commands::privacy::PrivacyState::load(&conn),
```

`tracking.rs` 테스트의 `collector_state`도 같은 위치에 `privacy: super::super::privacy::PrivacyState::load(&conn),`를 `db:` 앞에 추가한다(모듈 경로는 `crate::commands::privacy::PrivacyState::load(&conn)`로 써도 된다).

- [ ] **Step 4: 새 테스트 추가** (`tracking.rs` tests 모듈)

```rust
    #[test]
    fn saved_rules_apply_to_the_next_session_without_restart() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        let state = collector_state(conn);
        let rules = crate::core::privacy::PrivacyRules {
            redact_title_patterns: vec![r"invoice \d{1,4}".into()],
            ..Default::default()
        };
        let saved = crate::commands::privacy::save_rules(&state, rules).unwrap();
        assert!(saved.saved);
        super::start_tracking_inner(&state).unwrap();
        state.sessionizer.lock().unwrap().observe(
            "mail.exe".into(),
            "invoice 2024 draft".into(),
            super::now_ms() - 10,
        );
        super::stop_tracking_runtime(&state, false).unwrap();
        let conn = state.db.lock().unwrap();
        let title: String = conn.query_row("SELECT title FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "[redacted] draft");
    }

    #[test]
    fn unreadable_stored_rules_fail_closed() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        conn.execute("INSERT INTO settings VALUES ('privacy_rules', '{broken')", []).unwrap();
        let state = collector_state(conn);
        assert!(!state.privacy.healthy());
        super::start_tracking_inner(&state).unwrap();
        state.sessionizer.lock().unwrap().observe(
            "fixture.exe".into(),
            "private synthetic title".into(),
            super::now_ms() - 10,
        );
        super::stop_tracking_runtime(&state, false).unwrap();
        let conn = state.db.lock().unwrap();
        let title: String = conn.query_row("SELECT title FROM sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "");
    }
```

- [ ] **Step 5: 커밋은 Task 4 끝에서 함께.**

---

### Task 4: 저장·조회·소급 적용 명령

**Files:**
- Modify: `crates/activity-engine/src/commands/privacy.rs` (파일 전체 교체)

**Interfaces:**
- Consumes: Task 1의 `CompiledRules`, `PrivacyRules`, `InvalidRule`, `parse_stored_rules`; Task 2의 `try_set_setting`
- Produces:
  - `pub struct PrivacyState` + `load(&Connection) -> Self`, `current(&self) -> Arc<CompiledRules>`, `healthy(&self) -> bool`
  - `pub struct PrivacyRulesView { rules: PrivacyRules, healthy: bool }` (JSON: `{ "rules": {...}, "healthy": bool }`)
  - `pub struct PrivacySaveResult { saved: bool, invalid: Vec<InvalidRule> }` (JSON: `{ "saved": bool, "invalid": [...] }`)
  - `pub(crate) fn save_rules(state: &AppState, rules: PrivacyRules) -> Result<PrivacySaveResult, String>`
  - 오류 코드: `privacy_rules_save_failed`, `privacy_rules_invalid`, `privacy_redaction_failed`
  - component 메서드 이름은 기존 그대로: `get_privacy_rules`, `set_privacy_rules`, `redact_existing` (응답 형태만 바뀜)

- [ ] **Step 1: 테스트 작성** — 새 파일 하단에 들어갈 테스트.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::privacy::{RuleField, RuleProblem};

    fn state() -> AppState {
        let conn = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        crate::commands::tracking::test_state(conn)
    }

    #[test]
    fn invalid_rules_are_returned_not_stored() {
        let state = state();
        let result = save_rules(
            &state,
            PrivacyRules { excluded_title_patterns: vec!["(".into()], ..Default::default() },
        )
        .unwrap();
        assert!(!result.saved);
        assert_eq!(result.invalid[0].field, RuleField::ExcludedTitlePatterns);
        assert_eq!(result.invalid[0].problem, RuleProblem::Syntax);
        let conn = state.db.lock().unwrap();
        assert_eq!(crate::core::db::get_setting(&conn, RULES_KEY, "missing"), "missing");
    }

    #[test]
    fn write_failure_keeps_previous_compiled_rules() {
        let state = state();
        save_rules(&state, PrivacyRules { mask_all_titles: true, ..Default::default() }).unwrap();
        state.db.lock().unwrap().execute_batch("PRAGMA query_only = ON").unwrap();
        let error = save_rules(&state, PrivacyRules::default()).unwrap_err();
        assert_eq!(error, "privacy_rules_save_failed");
        assert_eq!(state.privacy.current().apply("a.exe", "t").unwrap().1, "");
    }

    #[test]
    fn redaction_is_all_or_nothing() {
        let state = state();
        {
            let conn = state.db.lock().unwrap();
            conn.execute(
                "INSERT INTO sessions (app, title, start_ts, end_ts, duration_ms) VALUES ('a.exe','secret one',0,10,10),('b.exe','plain',10,20,10)",
                [],
            )
            .unwrap();
        }
        save_rules(&state, PrivacyRules { redact_title_patterns: vec!["secret".into()], ..Default::default() }).unwrap();
        assert_eq!(redact_existing_inner(&state).unwrap(), 1);
        let conn = state.db.lock().unwrap();
        let title: String = conn
            .query_row("SELECT title FROM sessions WHERE app = 'a.exe'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "[redacted] one");
    }

    #[test]
    fn redaction_refuses_to_run_with_unreadable_rules() {
        let conn = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        conn.execute("INSERT INTO settings VALUES ('privacy_rules', '{broken')", []).unwrap();
        let state = crate::commands::tracking::test_state(conn);
        assert_eq!(redact_existing_inner(&state).unwrap_err(), "privacy_rules_invalid");
    }
}
```

`tracking.rs`에 테스트용 생성자를 추가한다(테스트 모듈의 `collector_state`를 이 함수로 대체):

```rust
#[cfg(test)]
pub(crate) fn test_state(conn: rusqlite::Connection) -> AppState {
    AppState {
        integration_root: None,
        tracking: AtomicBool::new(product_consent(&conn)),
        tracking_control: Mutex::new(()),
        persist_tracking_consent: true,
        privacy: crate::commands::privacy::PrivacyState::load(&conn),
        db: Mutex::new(conn),
        sessionizer: Mutex::new(crate::core::sessionizer::Sessionizer::new()),
        snapshot_writer: Mutex::new(()),
        digest_operations: Arc::new(DigestOperationState::default()),
        digest_handles: crate::core::digest::DigestHandleStore::default(),
    }
}
```

그리고 `tracking.rs` 테스트의 `fn collector_state(conn) -> AppState` 본문을 `super::test_state(conn)`로 바꾼다.

- [ ] **Step 2: 실패 확인**

Run: `source ~/.cargo/env && cargo test -p devbox-activity-engine --lib privacy`
Expected: 컴파일 오류(`PrivacyState`, `save_rules` 미정의).

- [ ] **Step 3: 구현** — `commands/privacy.rs` 전체를 아래 + Step 1 테스트로 교체한다.

```rust
//! Privacy rule commands. Rules are stored as JSON under the `privacy_rules`
//! setting and compiled once into [`PrivacyState`].

use crate::commands::tracking::AppState;
use crate::core::privacy::{parse_stored_rules, CompiledRules, InvalidRule, PrivacyRules};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

pub(crate) const RULES_KEY: &str = "privacy_rules";
const MAX_RULES_JSON_BYTES: usize = 256 * 1024;

/// Compiled rules plus whether the stored value was readable.
pub struct PrivacyState {
    compiled: RwLock<Arc<CompiledRules>>,
    healthy: AtomicBool,
}

impl PrivacyState {
    pub fn load(conn: &Connection) -> Self {
        let (compiled, healthy) = load_compiled(conn);
        Self { compiled: RwLock::new(Arc::new(compiled)), healthy: AtomicBool::new(healthy) }
    }
    pub fn current(&self) -> Arc<CompiledRules> {
        self.compiled
            .read()
            .map(|rules| rules.clone())
            .unwrap_or_else(|_| Arc::new(CompiledRules::fail_closed()))
    }
    pub fn healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }
    fn replace(&self, compiled: CompiledRules) {
        if let Ok(mut rules) = self.compiled.write() {
            *rules = Arc::new(compiled);
            self.healthy.store(true, Ordering::SeqCst);
        }
    }
}

fn stored_rules(conn: &Connection) -> Result<PrivacyRules, ()> {
    let text = crate::core::db::get_setting_bounded(conn, RULES_KEY, "", MAX_RULES_JSON_BYTES)
        .map_err(|_| ())?;
    parse_stored_rules(if text.is_empty() { None } else { Some(&text) })
}

fn load_compiled(conn: &Connection) -> (CompiledRules, bool) {
    match stored_rules(conn).ok().and_then(|rules| CompiledRules::compile(&rules).ok()) {
        Some(compiled) => (compiled, true),
        None => (CompiledRules::fail_closed(), false),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyRulesView {
    pub rules: PrivacyRules,
    pub healthy: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySaveResult {
    pub saved: bool,
    pub invalid: Vec<InvalidRule>,
}

pub(crate) fn view(state: &AppState) -> Result<PrivacyRulesView, String> {
    let conn = state.db.lock().map_err(|_| "privacy_rules_invalid".to_string())?;
    let rules = stored_rules(&conn).unwrap_or_default();
    Ok(PrivacyRulesView { rules, healthy: state.privacy.healthy() })
}

pub(crate) fn save_rules(state: &AppState, rules: PrivacyRules) -> Result<PrivacySaveResult, String> {
    let compiled = match CompiledRules::compile(&rules) {
        Ok(compiled) => compiled,
        Err(invalid) => return Ok(PrivacySaveResult { saved: false, invalid }),
    };
    let json = serde_json::to_string(&rules).map_err(|_| "privacy_rules_save_failed".to_string())?;
    let conn = state.db.lock().map_err(|_| "privacy_rules_save_failed".to_string())?;
    crate::core::db::try_set_setting(&conn, RULES_KEY, &json)
        .map_err(|_| "privacy_rules_save_failed".to_string())?;
    drop(conn);
    state.privacy.replace(compiled);
    Ok(PrivacySaveResult { saved: true, invalid: Vec::new() })
}

pub(crate) fn redact_existing_inner(state: &AppState) -> Result<i64, String> {
    if !state.privacy.healthy() {
        return Err("privacy_rules_invalid".into());
    }
    let rules = state.privacy.current();
    let mut conn = state.db.lock().map_err(|_| "privacy_redaction_failed".to_string())?;
    apply_to_existing(&mut conn, &rules).map_err(|_| "privacy_redaction_failed".into())
}

#[tauri::command]
pub fn get_privacy_rules(state: tauri::State<'_, Arc<AppState>>) -> Result<PrivacyRulesView, String> {
    view(&state)
}

#[tauri::command]
pub fn set_privacy_rules(
    state: tauri::State<'_, Arc<AppState>>,
    rules: PrivacyRules,
) -> Result<PrivacySaveResult, String> {
    save_rules(&state, rules)
}

/// Apply the current rules to stored sessions (user action). All-or-nothing.
#[tauri::command]
pub fn redact_existing(state: tauri::State<'_, Arc<AppState>>) -> Result<i64, String> {
    redact_existing_inner(&state)
}

fn apply_to_existing(conn: &mut Connection, rules: &CompiledRules) -> rusqlite::Result<i64> {
    let transaction = conn.transaction()?;
    let rows: Vec<(i64, String, String)> = {
        let mut statement = transaction.prepare("SELECT id, app, title FROM sessions")?;
        let rows = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut affected = 0i64;
    for (id, app, title) in rows {
        match rules.apply(&app, &title) {
            None => {
                transaction.execute("DELETE FROM sessions WHERE id = ?1", rusqlite::params![id])?;
                affected += 1;
            }
            Some((new_app, new_title)) if new_app != app || new_title != title => {
                transaction.execute(
                    "UPDATE sessions SET app = ?1, title = ?2 WHERE id = ?3",
                    rusqlite::params![new_app, new_title, id],
                )?;
                affected += 1;
            }
            Some(_) => {}
        }
    }
    transaction.commit()?;
    Ok(affected)
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_get_privacy_rules(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = view(&component_app.state::<Arc<AppState>>())?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_set_privacy_rules(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        rules: PrivacyRules,
    }
    let Input { rules } =
        serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = save_rules(&component_app.state::<Arc<AppState>>(), rules)?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}

/// Typed product adapter; the caller enforces native owner/session authorization.
pub(crate) async fn __component_redact_existing(
    component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager as _;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let Input {} = serde_json::from_value(args).map_err(|_| "component_args_invalid".to_owned())?;
    let value = redact_existing_inner(&component_app.state::<Arc<AppState>>())?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
}
```

- [ ] **Step 4: 통과 확인** (Task 1–4 전체)

Run: `source ~/.cargo/env && cargo test -p devbox-activity-engine --lib`
Expected: PASS. 옛 `apply`/`parse_rules`를 참조하는 곳이 남아 있으면 `rg -n "privacy::\{?apply|parse_rules" crates/activity-engine`로 찾아 `CompiledRules`로 바꾼다.

- [ ] **Step 5: 커밋**

```bash
git add crates/activity-engine
git commit -m "fix(devbox-knowledge): validate privacy rules and fail closed on unreadable rules"
```

---

### Task 5: Knowledge host 오류 코드와 문구

**Files:**
- Modify: `apps/devbox-knowledge/src-tauri/src/component.rs` (`fn issue`, 테스트 모듈)
- Modify: `apps/devbox-knowledge/src/issues.ts`

- [ ] **Step 1: 테스트 추가** — `component.rs` 테스트 모듈의 `issue` 단언 옆(`assert_eq!(issue("activity_consent_save_failed"), ...)` 근처)에 추가.

```rust
        assert_eq!(issue("privacy_rules_save_failed"), "privacy_rules_save_failed");
        assert_eq!(issue("privacy_rules_invalid"), "privacy_rules_invalid");
        assert_eq!(issue("privacy_redaction_failed"), "privacy_redaction_failed");
```

- [ ] **Step 2: 실패 확인** — Run: `source ~/.cargo/env && cargo test -p devbox-knowledge --lib component` → FAIL (`operation_failed`로 매핑됨).

- [ ] **Step 3: 구현** — `fn issue`의 `"activity_consent_save_failed" => "consent_save_failed",` 줄 바로 아래에 추가.

```rust
        "privacy_rules_save_failed" => "privacy_rules_save_failed",
        "privacy_rules_invalid" => "privacy_rules_invalid",
        "privacy_redaction_failed" => "privacy_redaction_failed",
```

`apps/devbox-knowledge/src/issues.ts`의 `messages`에 추가.

```ts
  privacy_rules_save_failed: "개인정보 규칙을 저장하지 못했습니다. 이전 규칙을 계속 사용합니다.",
  privacy_rules_invalid: "저장된 개인정보 규칙을 읽지 못해 창 제목 저장을 멈췄습니다. 규칙을 다시 저장해 주세요.",
  privacy_redaction_failed: "기존 기록에 규칙을 적용하지 못했습니다. 기록은 바뀌지 않았습니다.",
```

- [ ] **Step 4: 통과 확인** — Run: `cargo test -p devbox-knowledge --lib component` → PASS.

- [ ] **Step 5: 커밋**

```bash
git add apps/devbox-knowledge
git commit -m "fix(devbox-knowledge): map privacy rule errors to user messages"
```

---

### Task 6: API 타입과 `PrivacyRulesPanel`

**Files:**
- Modify: `packages/knowledge-features/src/activity/api.ts:478-497`
- Create: `packages/knowledge-features/src/activity/PrivacyRulesPanel.tsx`
- Create: `packages/knowledge-features/src/activity/PrivacyRulesPanel.test.tsx`
- Modify: `packages/knowledge-features/src/activity/App.tsx` (state 473행, loadSettings 912–925행, JSX 1385–1440행, import)
- Modify: `packages/knowledge-features/src/activity/App.contextMenu.test.tsx` (`getPrivacyRules` mock)

**Interfaces:**
- Consumes: Task 4 JSON 형태
- Produces:
  - `type PrivacyRuleField = "excludedProcesses" | "excludedTitlePatterns" | "redactTitlePatterns"`
  - `type PrivacyRuleProblem = "empty" | "too_long" | "too_many" | "syntax"`
  - `interface InvalidPrivacyRule { field: PrivacyRuleField; index: number; problem: PrivacyRuleProblem }`
  - `interface PrivacyRulesView { rules: PrivacyRules; healthy: boolean }`
  - `interface PrivacySaveResult { saved: boolean; invalid: InvalidPrivacyRule[] }`
  - `getPrivacyRules(): Promise<PrivacyRulesView>`, `setPrivacyRules(rules): Promise<PrivacySaveResult>`
  - `export function toLines(values: string[]): string`, `export function fromLines(text: string): string[]` (PrivacyRulesPanel.tsx)

- [ ] **Step 1: api.ts 교체** — `export interface PrivacyRules`부터 `redactExisting`까지를 아래로 바꾼다.

```ts
export interface PrivacyRules {
  excludedProcesses: string[];
  excludedTitlePatterns: string[];
  redactTitlePatterns: string[];
  maskAllTitles: boolean;
}
export type PrivacyRuleField = "excludedProcesses" | "excludedTitlePatterns" | "redactTitlePatterns";
export type PrivacyRuleProblem = "empty" | "too_long" | "too_many" | "syntax";
export interface InvalidPrivacyRule { field: PrivacyRuleField; index: number; problem: PrivacyRuleProblem }
export interface PrivacyRulesView { rules: PrivacyRules; healthy: boolean }
export interface PrivacySaveResult { saved: boolean; invalid: InvalidPrivacyRule[] }

export const EMPTY_PRIVACY_RULES: PrivacyRules = {
  excludedProcesses: [], excludedTitlePatterns: [], redactTitlePatterns: [], maskAllTitles: false,
};

export async function getPrivacyRules(): Promise<PrivacyRulesView> {
  if (!isTauri()) return { rules: EMPTY_PRIVACY_RULES, healthy: true };
  return invoke<PrivacyRulesView>("get_privacy_rules");
}

export async function setPrivacyRules(rules: PrivacyRules): Promise<PrivacySaveResult> {
  if (!isTauri()) return { saved: true, invalid: [] };
  return invoke<PrivacySaveResult>("set_privacy_rules", { rules });
}

export async function redactExisting(): Promise<number> {
  if (!isTauri()) return 0;
  return invoke<number>("redact_existing");
}
```

- [ ] **Step 2: 실패하는 UI 테스트 작성** — `PrivacyRulesPanel.test.tsx`

```tsx
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { assertNoA11yViolations } from "@devbox/a11y/testing";
import { afterEach, describe, expect, it, vi } from "vitest";
import PrivacyRulesPanel, { fromLines, toLines } from "./PrivacyRulesPanel";
import { EMPTY_PRIVACY_RULES } from "./api";

const api = vi.hoisted(() => ({
  setPrivacyRules: vi.fn(),
  redactExisting: vi.fn(),
}));
vi.mock("./api", async (original) => ({ ...(await original<typeof import("./api")>()), ...api }));

afterEach(() => { cleanup(); vi.clearAllMocks(); });

describe("line helpers", () => {
  it("keeps commas and inner spaces, drops blank lines", () => {
    expect(fromLines("patient \\d{1,3}\n\n  InPrivate - Microsoft Edge  \n")).toEqual([
      "patient \\d{1,3}", "InPrivate - Microsoft Edge",
    ]);
    expect(toLines(["a", "b"])).toBe("a\nb");
  });
});

describe("PrivacyRulesPanel", () => {
  it("keeps typed commas and spaces and saves one rule per line", async () => {
    api.setPrivacyRules.mockResolvedValue({ saved: true, invalid: [] });
    const onSaved = vi.fn();
    render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={onSaved} />);
    const redact = screen.getByLabelText("제목 치환 정규식");
    fireEvent.change(redact, { target: { value: "patient \\d{1,3}\nInPrivate - " } });
    expect((redact as HTMLTextAreaElement).value).toBe("patient \\d{1,3}\nInPrivate - ");
    fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(api.setPrivacyRules).toHaveBeenCalledWith({
      ...EMPTY_PRIVACY_RULES,
      redactTitlePatterns: ["patient \\d{1,3}", "InPrivate -"],
    });
  });

  it("shows validation problems and does not report success", async () => {
    api.setPrivacyRules.mockResolvedValue({
      saved: false,
      invalid: [{ field: "excludedTitlePatterns", index: 1, problem: "syntax" }],
    });
    const onSaved = vi.fn();
    render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={onSaved} />);
    fireEvent.change(screen.getByLabelText("제목을 저장하지 않을 정규식"), { target: { value: "ok\n(" } });
    fireEvent.click(screen.getByRole("button", { name: "규칙 저장" }));
    expect(await screen.findByText("2번째 규칙: 정규식 문법 오류입니다.")).toBeTruthy();
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("warns when stored rules were unreadable and blocks retroactive apply", () => {
    render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy={false} onSaved={vi.fn()} />);
    expect(screen.getByRole("alert").textContent).toContain("창 제목 저장을 멈췄습니다");
    expect((screen.getByRole("button", { name: "기존 세션에 적용" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("requires saving before applying rules to stored sessions", () => {
    render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("제외할 프로세스"), { target: { value: "LockApp.exe" } });
    expect((screen.getByRole("button", { name: "기존 세션에 적용" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("has no accessibility violations", async () => {
    const { container } = render(<PrivacyRulesPanel initial={EMPTY_PRIVACY_RULES} healthy onSaved={vi.fn()} />);
    await assertNoA11yViolations(container);
  });
});
```

- [ ] **Step 3: 실패 확인** — Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/activity/PrivacyRulesPanel.test.tsx` → FAIL(모듈 없음).

- [ ] **Step 4: 컴포넌트 구현** — `PrivacyRulesPanel.tsx`

```tsx
import { useEffect, useMemo, useState } from "react";
import {
  redactExisting, setPrivacyRules,
  type InvalidPrivacyRule, type PrivacyRuleField, type PrivacyRuleProblem, type PrivacyRules,
} from "./api";

export function toLines(values: string[]): string {
  return values.join("\n");
}

export function fromLines(text: string): string[] {
  return text.split(/\r?\n/).map((line) => line.trim()).filter((line) => line.length > 0);
}

const PROBLEMS: Record<PrivacyRuleProblem, string> = {
  empty: "빈 규칙입니다.",
  too_long: "512자를 넘습니다.",
  too_many: "규칙은 64개까지 입력할 수 있습니다.",
  syntax: "정규식 문법 오류입니다.",
};

interface FieldProps {
  id: PrivacyRuleField;
  label: string;
  hint: string;
  value: string;
  onChange: (value: string) => void;
  problems: InvalidPrivacyRule[];
}

function RuleField({ id, label, hint, value, onChange, problems }: FieldProps) {
  const errorId = `${id}-errors`;
  return (
    <div className="privacy-row">
      <label htmlFor={id}>{label}</label>
      <span className="dim" id={`${id}-hint`}>{hint}</span>
      <textarea
        id={id}
        rows={3}
        value={value}
        spellCheck={false}
        aria-invalid={problems.length > 0}
        aria-describedby={problems.length > 0 ? `${id}-hint ${errorId}` : `${id}-hint`}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
      {problems.length > 0 && (
        <ul id={errorId} className="field-error">
          {problems.map((problem) => (
            <li key={`${problem.index}-${problem.problem}`}>{`${problem.index + 1}번째 규칙: ${PROBLEMS[problem.problem]}`}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default function PrivacyRulesPanel({ initial, healthy, onSaved }: {
  initial: PrivacyRules;
  healthy: boolean;
  onSaved: (rules: PrivacyRules) => void;
}) {
  const [processes, setProcesses] = useState(toLines(initial.excludedProcesses));
  const [excluded, setExcluded] = useState(toLines(initial.excludedTitlePatterns));
  const [redact, setRedact] = useState(toLines(initial.redactTitlePatterns));
  const [maskAll, setMaskAll] = useState(initial.maskAllTitles);
  const [invalid, setInvalid] = useState<InvalidPrivacyRule[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");

  useEffect(() => {
    setProcesses(toLines(initial.excludedProcesses));
    setExcluded(toLines(initial.excludedTitlePatterns));
    setRedact(toLines(initial.redactTitlePatterns));
    setMaskAll(initial.maskAllTitles);
    setInvalid([]);
  }, [initial]);

  const draft = useMemo<PrivacyRules>(() => ({
    excludedProcesses: fromLines(processes),
    excludedTitlePatterns: fromLines(excluded),
    redactTitlePatterns: fromLines(redact),
    maskAllTitles: maskAll,
  }), [processes, excluded, redact, maskAll]);
  const dirty = JSON.stringify(draft) !== JSON.stringify(initial);
  const problemsFor = (field: PrivacyRuleField) => invalid.filter((problem) => problem.field === field);

  const save = async () => {
    setBusy(true); setError(""); setNotice("");
    try {
      const result = await setPrivacyRules(draft);
      if (result.saved) {
        setInvalid([]);
        setNotice("규칙을 저장했습니다. 다음 기록부터 적용됩니다.");
        onSaved(draft);
      } else {
        setInvalid(result.invalid);
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const applyToStored = async () => {
    setBusy(true); setError(""); setNotice("");
    try {
      const count = await redactExisting();
      setNotice(`기존 세션 ${count}개에 규칙을 적용했습니다.`);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="panel" aria-labelledby="privacy-rules-heading">
      <h2 id="privacy-rules-heading">개인정보 보호 규칙</h2>
      {!healthy && (
        <p role="alert">저장된 규칙을 읽지 못해 창 제목 저장을 멈췄습니다. 규칙을 확인하고 다시 저장해 주세요.</p>
      )}
      <RuleField id="excludedProcesses" label="제외할 프로세스" hint="한 줄에 하나, 대소문자 구분 없이 정확히 일치하면 세션 전체를 저장하지 않습니다."
        value={processes} onChange={setProcesses} problems={problemsFor("excludedProcesses")} />
      <RuleField id="excludedTitlePatterns" label="제목을 저장하지 않을 정규식" hint="한 줄에 하나. 일치하면 세션은 남기고 제목만 비웁니다."
        value={excluded} onChange={setExcluded} problems={problemsFor("excludedTitlePatterns")} />
      <RuleField id="redactTitlePatterns" label="제목 치환 정규식" hint="한 줄에 하나. 일치한 부분을 [redacted]로 바꿉니다."
        value={redact} onChange={setRedact} problems={problemsFor("redactTitlePatterns")} />
      <label className="row">
        <input type="checkbox" checked={maskAll} onChange={(event) => setMaskAll(event.currentTarget.checked)} />
        모든 제목을 저장하지 않음
      </label>
      <div className="row">
        <button className="btn" disabled={busy || !dirty} onClick={() => void save()}>규칙 저장</button>
        <button className="btn" disabled={busy || dirty || !healthy} onClick={() => void applyToStored()}>기존 세션에 적용</button>
      </div>
      {notice && <p role="status">{notice}</p>}
      {error && <p role="alert">{error}</p>}
      <div className="dim">규칙은 DB 저장 전에 적용됩니다. 제외한 원문은 어디에도 남지 않습니다.</div>
    </section>
  );
}
```

- [ ] **Step 5: App.tsx 연결**
  - import: `import PrivacyRulesPanel from "./PrivacyRulesPanel";`를 추가하고, `./api` import 목록에서 `setPrivacyRules`, `redactExisting`을 제거한다(다른 곳에서 쓰지 않음을 `rg -n "setPrivacyRules|redactExisting" packages/knowledge-features/src/activity/App.tsx`로 확인).
  - 473행 state를 `const [privacy, setPrivacy] = useState<PrivacyRules>(EMPTY_PRIVACY_RULES);`와 `const [privacyHealthy, setPrivacyHealthy] = useState(true);`로 바꾸고 `EMPTY_PRIVACY_RULES`를 import한다.
  - `loadSettings`의 `if (privacyRules.status === "fulfilled") setPrivacy(privacyRules.value);`를 아래로 바꾼다.

```tsx
      if (privacyRules.status === "fulfilled") {
        setPrivacy(privacyRules.value.rules);
        setPrivacyHealthy(privacyRules.value.healthy);
      }
```

  - 1385–1440행 `<section className="panel"> <h2>개인정보 보호 규칙</h2> … </section>` 전체를 아래 한 줄로 바꾼다.

```tsx
          <PrivacyRulesPanel initial={privacy} healthy={privacyHealthy} onSaved={(rules) => { setPrivacy(rules); setPrivacyHealthy(true); }} />
```

  - `App.contextMenu.test.tsx`의 `getPrivacyRules` mock을 `vi.fn().mockResolvedValue({ rules: { excludedProcesses: [], excludedTitlePatterns: [], redactTitlePatterns: [], maskAllTitles: false }, healthy: true })`로 바꾼다. `App.test.ts`에 같은 mock이 있으면 같이 바꾼다(`rg -n "getPrivacyRules" packages/knowledge-features/src`).

- [ ] **Step 6: 통과 확인**

Run: `pnpm --filter @devbox/knowledge-features exec vitest run src/activity`
Expected: PASS (새 테스트 5개 포함).
Run: `pnpm --filter @devbox/knowledge-features build`
Expected: 타입 오류 없음.

- [ ] **Step 7: 커밋**

```bash
git add packages/knowledge-features/src/activity
git commit -m "fix(devbox-knowledge): edit privacy rules line by line and save explicitly"
```

---

### Task 7: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9 절차대로 `pnpm verify:affected` → PR → CI → 머지 → main CI → ledger 기록 → 정리.
- [ ] PR 본문의 Windows 실기 체크리스트(사용자 확인 대기로 기록):
  1. Knowledge > 활동 > 설정에서 세 목록에 여러 줄(쉼표·공백·한글 포함)을 입력하고 저장한다.
  2. 추적을 켜고 규칙에 걸리는 제목의 창(예: Edge InPrivate)을 2초 이상 앞에 둔다.
  3. 타임라인에서 제목이 비었거나 `[redacted]`로 바뀌었는지 확인한다.
  4. 잘못된 정규식 `(`을 저장하면 오류 문구가 나오고 이전 규칙이 유지되는지 확인한다.
