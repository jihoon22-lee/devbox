# P0-07 운영 로그와 panic 기록 — Implementation Plan

> **For agentic workers:** Claude Code는 REQUIRED SUB-SKILL `superpowers:executing-plans`로 이 계획을 과제 순서대로 실행한다. Codex는 같은 순서를 직접 따른다. 단계는 체크박스(`- [ ]`)로 추적한다. 시작 전에 `00-roadmap.md` §3·§4를 읽는다.

**Goal:** 네 제품이 component 호출의 결과(실패·취소·거부·느린 성공)와 panic 위치를 제품별 로컬 로그 파일에 남기고, Control Center 지원 번들이 네 제품의 최근 기록을 담게 한다(A7·D15).

**Architecture:** 고정 스키마 JSON Lines 로그를 `product_contract::operation_log`에 순수 코드로 둔다(쓰기·14일 정리·요약 읽기). 문자열 필드는 짧은 식별자만 그대로 두고 나머지는 `msg-<sha256 앞 8자리>`로 바꾸므로 인자·값·경로·문장은 구조적으로 기록될 수 없다. `product-shell-tauri`가 시작할 때 로그를 열고 panic hook을 설치하며, 각 제품 `execute`는 `begin_operation` guard로 결과를 넘긴다. guard가 결과 없이 버려지면 "rejected"로 기록한다. Control Center는 자기 데이터 폴더 이름의 설치 접미사로 같은 설치의 네 제품 로그 폴더를 찾아 요약한다.

**Tech Stack:** Rust(std, serde_json, sha2 — 모두 기존 의존성), Tauri v2

**Spec:** `review.md` §6 A7 · `00-roadmap.md` D15

## Global Constraints

- `00-roadmap.md` §3 전부 적용.
- 로그 위치: `%LOCALAPPDATA%\<제품 identifier>\logs\operations-YYYY-MM-DD.jsonl`(날짜는 UTC). 외부 전송 없음.
- 기록 항목(이 외 없음): `tsMs`, `version`, `product`, `component`, `method`, `durationMs`, `outcome`(`succeeded|failed|cancelled|rejected|panicked|limit`), `code`(선택).
- 성공은 250ms 이상 걸린 호출만 기록한다(1초 폴링이 파일을 채우지 않도록). 실패·취소·거부·panic은 모두 기록한다.
- 보관 14일(오늘 포함 14개 파일), 하루 파일 상한 4MiB(넘으면 `limit` 한 줄을 남기고 그날은 멈춤).
- 로그 실패는 어떤 명령도 실패시키지 않는다.
- 새 의존성 없음. D23에서 승인한 `tracing` 계열은 쓰지 않는다: 자유 문장 로그를 허용하지 않는 고정 스키마가 D15의 "코드만" 규칙을 구조로 보장한다. 릴리스 PDB 보관도 하지 않는다: panic 위치(파일:행)는 심볼 없이도 바이너리에 남는다.

## Review Focus

1. 오류 문자열이 한국어 문장이거나 경로를 담음(`"C:\\Users\\me\\vault 없음"`) → 로그에는 `msg-xxxxxxxx`만 남는다. (Task 1)
2. 자정(UTC)을 넘겨 계속 실행 → 다음 날 파일로 넘어가고, 15일째 시작 시 가장 오래된 파일이 지워진다. (Task 1)
3. panic이 로그 mutex를 쥔 스레드에서 남 → hook이 막히지 않는다(`try_lock`). (Task 1·2)
4. 손으로 고친 로그 줄(알 수 없는 필드, 긴 문장) → 지원 번들 요약에서 버려지거나 digest로 바뀐다. (Task 1·3)
5. 개발용 portable 빌드(제품마다 설치 접미사가 다름) → 번들에는 Control Center 자신의 로그만 "available", 나머지는 "missing"으로 나오고 실패하지 않는다. (Task 3)

## Branch · PR

- 묶음: **B2** — 브랜치 `fix/suite/files-webhooks-logs-describe`, PR 제목 `fix(suite): cloud files, webhook bodies, operation logs and one describe per session`(로드맵 §6). 이 계획은 묶음 PR 안의 커밋들이다.
- 이 계획의 절 제목(묶음 PR 본문·커밋 범위 표시): `feat(suite): record operation outcomes and panics in local logs`
- 마지막 과제의 `§4.4–§4.9`는 묶음의 마지막 계획에서만 한다. 그 전 계획에서는 PR 본문 초안에 이 계획의 절(요약·변경·계획과 다르게 한 점·Windows 실기 항목)만 더한다(로드맵 §4.0).

## File Structure

| 파일 | 변경 | 책임 |
|---|---|---|
| `crates/product-contract/src/operation_log.rs` | 생성 | 스키마·토큰화·날짜 파일·쓰기·정리·요약 |
| `crates/product-contract/src/lib.rs` | 수정 | `pub mod operation_log;` |
| `crates/product-shell-tauri/src/operation_log.rs` | 생성 | 시작 시 열기, panic hook, `OperationGuard` |
| `crates/product-shell-tauri/src/lib.rs` | 수정 | 모듈 등록, setup에서 초기화, 재수출 |
| `apps/devbox-knowledge/src-tauri/src/component.rs` | 수정 | guard 연결 |
| `apps/devbox-api-studio/src-tauri/src/component.rs` | 수정 | guard 연결(마이그레이션 분기 포함) |
| `apps/devbox-workspace/src-tauri/src/component.rs` | 수정 | guard 연결 |
| `apps/devbox-control-center/src-tauri/src/tools_host.rs` | 수정 | guard 연결 |
| `crates/installation-tools/Cargo.toml` | 수정 | `product-contract` 의존 |
| `crates/installation-tools/src/core/support_bundle.rs` | 수정 | `operations` 절, schema 2 |
| `crates/installation-tools/src/commands/diagnostics.rs` | 수정 | 제품 로그 폴더 찾기·요약 전달 |
| `packages/control-center-features/src/manager/api.ts:1690` | 수정 | 브라우저 mock 절 목록 |
| `CONVENTIONS.md:73` | 수정 | 로깅 규칙 |

---

### Task 1: 고정 스키마 로그 (`product-contract`)

**Files:** Create `crates/product-contract/src/operation_log.rs`; Modify `crates/product-contract/src/lib.rs`

**Interfaces (Produces):**
- `Outcome { Succeeded, Failed, Cancelled, Rejected, Panicked, Limit }` (serde lowercase)
- `Entry { ts_ms: u64, version, product, component, method: String, duration_ms: u64, outcome: Outcome, code: Option<String> }` + `Entry::new(ts_ms, version: &str, product: &str, component: &str, method: &str, duration_ms, outcome, code: Option<&str>) -> Entry`
- `token(&str) -> String`, `should_record(Outcome, u64) -> bool`, `day_file_name(u64) -> String`
- `OperationLog::open(PathBuf) -> io::Result<Self>`, `with_limit(PathBuf, u64)`, `append(&self, &Entry)`, `try_append(&self, &Entry)`
- `prune(&Path, now_ms: u64) -> usize`
- `Summary { state: String, file_count: usize, byte_length: u64, counts: Counts, recent: Vec<Entry>, truncated: bool }`, `Counts { succeeded, failed, cancelled, rejected, panicked: u64 }`, `summarize(&Path, max_entries: usize, max_read_bytes: u64) -> Summary`
- 상수 `RETENTION_DAYS = 14`, `MAX_DAY_BYTES = 4 MiB`, `SLOW_OPERATION_MS = 250`, `DAY_MS = 86_400_000`

- [ ] **Step 1: 실패하는 테스트** — `operation_log.rs`를 테스트 모듈만 먼저 만든다(구현은 Step 3). `lib.rs`에 `pub mod operation_log;`를 추가한다.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const DAY0: u64 = 19_990 * DAY_MS; // 2024-09-24T00:00:00Z

    fn entry(ts_ms: u64, outcome: Outcome, code: Option<&str>) -> Entry {
        Entry::new(ts_ms, "0.9.0", "knowledge", "knowledge.notes", "write_file", 12, outcome, code)
    }

    #[test]
    fn identifiers_pass_and_anything_else_becomes_a_digest() {
        assert_eq!(token("vault_unavailable"), "vault_unavailable");
        assert_eq!(token("component.rs:212"), "component.rs:212");
        let hashed = token("C:\\Users\\me\\vault 없음");
        assert!(hashed.starts_with("msg-") && hashed.len() == 12, "{hashed}");
        assert_eq!(hashed, token("C:\\Users\\me\\vault 없음"));
        assert!(token(&"a".repeat(65)).starts_with("msg-"));
        assert!(token("").starts_with("msg-"));
        let line = serde_json::to_string(&entry(DAY0, Outcome::Failed, Some("경로 /home/me 없음"))).unwrap();
        assert!(!line.contains("/home/me") && !line.contains("경로"), "{line}");
    }

    #[test]
    fn fast_successes_are_skipped_and_everything_else_is_kept() {
        assert!(!should_record(Outcome::Succeeded, SLOW_OPERATION_MS - 1));
        assert!(should_record(Outcome::Succeeded, SLOW_OPERATION_MS));
        for outcome in [Outcome::Failed, Outcome::Cancelled, Outcome::Rejected, Outcome::Panicked] {
            assert!(should_record(outcome, 0));
        }
    }

    #[test]
    fn files_are_named_by_utc_day() {
        assert_eq!(day_file_name(0), "operations-1970-01-01.jsonl");
        assert_eq!(day_file_name(DAY0), "operations-2024-09-24.jsonl");
        assert_eq!(day_file_name(DAY0 + DAY_MS - 1), "operations-2024-09-24.jsonl");
        assert_eq!(day_file_name(11_016 * DAY_MS), "operations-2000-02-29.jsonl");
        assert_eq!(parse_day("operations-2000-02-29.jsonl"), Some(11_016));
        assert_eq!(parse_day("operations-2000-02-30.jsonl"), None);
        assert_eq!(parse_day("other.jsonl"), None);
    }

    #[test]
    fn appends_roll_over_by_day_and_prune_keeps_fourteen_days() {
        let dir = tempfile::tempdir().unwrap();
        let log = OperationLog::open(dir.path().join("logs")).unwrap();
        log.append(&entry(DAY0, Outcome::Failed, Some("a")));
        log.append(&entry(DAY0 + 13 * DAY_MS, Outcome::Failed, Some("b")));
        log.append(&entry(DAY0 + 14 * DAY_MS, Outcome::Failed, Some("c")));
        let logs = dir.path().join("logs");
        assert_eq!(std::fs::read_dir(&logs).unwrap().count(), 3);
        std::fs::write(logs.join("notes.txt"), "keep").unwrap();
        assert_eq!(prune(&logs, DAY0 + 14 * DAY_MS), 1);
        assert!(!logs.join(day_file_name(DAY0)).exists());
        assert!(logs.join(day_file_name(DAY0 + 13 * DAY_MS)).exists());
        assert!(logs.join("notes.txt").exists());
        assert_eq!(prune(&dir.path().join("missing"), DAY0), 0);
    }

    #[test]
    fn a_full_day_file_gets_one_limit_marker_then_stops() {
        let dir = tempfile::tempdir().unwrap();
        let log = OperationLog::with_limit(dir.path().to_path_buf(), 400).unwrap();
        for _ in 0..10 {
            log.append(&entry(DAY0, Outcome::Failed, Some("x")));
        }
        let text = std::fs::read_to_string(dir.path().join(day_file_name(DAY0))).unwrap();
        assert_eq!(text.matches("\"outcome\":\"limit\"").count(), 1);
        let lines = text.lines().count();
        log.append(&entry(DAY0, Outcome::Failed, Some("x")));
        let again = std::fs::read_to_string(dir.path().join(day_file_name(DAY0))).unwrap();
        assert_eq!(again.lines().count(), lines);
    }

    #[test]
    fn try_append_does_not_wait_for_a_held_lock() {
        let dir = tempfile::tempdir().unwrap();
        let log = OperationLog::open(dir.path().to_path_buf()).unwrap();
        let held = log.state.lock().unwrap();
        log.try_append(&entry(DAY0, Outcome::Panicked, None));
        drop(held);
        assert!(!dir.path().join(day_file_name(DAY0)).exists());
    }

    #[test]
    fn summary_lists_newest_problems_first_and_sanitizes_foreign_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log = OperationLog::open(dir.path().to_path_buf()).unwrap();
        log.append(&entry(DAY0, Outcome::Failed, Some("first")));
        log.append(&entry(DAY0 + 1, Outcome::Succeeded, None));
        log.append(&entry(DAY0 + DAY_MS, Outcome::Cancelled, None));
        log.append(&entry(DAY0 + DAY_MS + 1, Outcome::Failed, Some("last")));
        drop(log);
        let path = dir.path().join(day_file_name(DAY0 + DAY_MS));
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("not json\n");
        text.push_str(r#"{"tsMs":1,"version":"0.9.0","product":"knowledge","component":"knowledge.notes","method":"C:\\Users\\me","durationMs":1,"outcome":"failed"}"#);
        text.push('\n');
        text.push_str(r#"{"tsMs":1,"extra":true}"#);
        text.push('\n');
        std::fs::write(&path, text).unwrap();

        let summary = summarize(dir.path(), 3, 1024 * 1024);
        assert_eq!(summary.state, "available");
        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.counts.failed, 3);
        assert_eq!(summary.counts.cancelled, 1);
        assert_eq!(summary.counts.succeeded, 1);
        assert_eq!(summary.recent.len(), 3);
        assert!(summary.recent[0].method.starts_with("msg-"));
        assert_eq!(summary.recent[1].code.as_deref(), Some("last"));
        assert_eq!(summary.recent[2].outcome, Outcome::Cancelled);
        assert!(summary.truncated);
        assert_eq!(summarize(&dir.path().join("missing"), 3, 1024).state, "missing");
    }
}
```

- [ ] **Step 2: 실패 확인**

Run: `source ~/.cargo/env && cargo test -p product-contract --lib operation_log`
Expected: 컴파일 실패(`Entry`, `token` 등 없음).

- [ ] **Step 3: 구현** — 같은 파일의 테스트 모듈 위에 작성한다.

```rust
//! Fixed-schema operation log shared by every product. A line says which
//! component method ran, how long it took and how it ended. Arguments,
//! values, paths and free-form messages are never written: any field that is
//! not a short identifier is replaced by a digest, so repeated failures still
//! group together without revealing what they contained.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const RETENTION_DAYS: i64 = 14;
pub const MAX_DAY_BYTES: u64 = 4 * 1024 * 1024;
pub const SLOW_OPERATION_MS: u64 = 250;
pub const DAY_MS: u64 = 86_400_000;
const MAX_TOKEN_BYTES: usize = 64;
const FILE_PREFIX: &str = "operations-";
const FILE_SUFFIX: &str = ".jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
    Rejected,
    Panicked,
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub ts_ms: u64,
    pub version: String,
    pub product: String,
    pub component: String,
    pub method: String,
    pub duration_ms: u64,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl Entry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ts_ms: u64,
        version: &str,
        product: &str,
        component: &str,
        method: &str,
        duration_ms: u64,
        outcome: Outcome,
        code: Option<&str>,
    ) -> Self {
        Self {
            ts_ms,
            version: token(version),
            product: token(product),
            component: token(component),
            method: token(method),
            duration_ms,
            outcome,
            code: code.map(token),
        }
    }

    fn sanitized(self) -> Self {
        Self::new(
            self.ts_ms,
            &self.version,
            &self.product,
            &self.component,
            &self.method,
            self.duration_ms,
            self.outcome,
            self.code.as_deref(),
        )
    }
}

/// Short identifiers stay readable; everything else becomes `msg-<8 hex>`.
pub fn token(value: &str) -> String {
    let identifier = !value.is_empty()
        && value.len() <= MAX_TOKEN_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'));
    if identifier {
        return value.to_string();
    }
    let digest = Sha256::digest(value.as_bytes());
    format!(
        "msg-{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3]
    )
}

pub fn should_record(outcome: Outcome, duration_ms: u64) -> bool {
    outcome != Outcome::Succeeded || duration_ms >= SLOW_OPERATION_MS
}

// Proleptic Gregorian conversions (Howard Hinnant's civil calendar algorithms).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let month = i64::from(month);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn day_file_name(ts_ms: u64) -> String {
    let (year, month, day) = civil_from_days((ts_ms / DAY_MS) as i64);
    format!("{FILE_PREFIX}{year:04}-{month:02}-{day:02}{FILE_SUFFIX}")
}

fn parse_day(name: &str) -> Option<i64> {
    let date = name.strip_prefix(FILE_PREFIX)?.strip_suffix(FILE_SUFFIX)?;
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    // Reject impossible dates such as February 30 by round-tripping.
    (days >= 0 && day_file_name(days as u64 * DAY_MS) == name).then_some(days)
}

struct DayFile {
    name: String,
    file: Option<File>,
    bytes: u64,
    capped: bool,
}

pub struct OperationLog {
    dir: PathBuf,
    limit: u64,
    state: Mutex<DayFile>,
}

impl OperationLog {
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        Self::with_limit(dir, MAX_DAY_BYTES)
    }

    pub fn with_limit(dir: PathBuf, limit: u64) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            limit,
            state: Mutex::new(DayFile {
                name: String::new(),
                file: None,
                bytes: 0,
                capped: false,
            }),
        })
    }

    pub fn append(&self, entry: &Entry) {
        if let Ok(mut state) = self.state.lock() {
            self.write(&mut state, entry);
        }
    }

    /// For the panic hook: the panicking thread may already hold the lock.
    pub fn try_append(&self, entry: &Entry) {
        if let Ok(mut state) = self.state.try_lock() {
            self.write(&mut state, entry);
        }
    }

    fn write(&self, state: &mut DayFile, entry: &Entry) {
        let name = day_file_name(entry.ts_ms);
        if state.name != name {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join(&name))
                .ok();
            let bytes = file
                .as_ref()
                .and_then(|file| file.metadata().ok())
                .map_or(0, |metadata| metadata.len());
            *state = DayFile {
                name,
                file,
                bytes,
                capped: bytes >= self.limit,
            };
        }
        if state.capped {
            return;
        }
        let Some(file) = state.file.as_mut() else {
            return;
        };
        let Ok(mut line) = serde_json::to_vec(entry) else {
            return;
        };
        line.push(b'\n');
        if state.bytes + line.len() as u64 > self.limit {
            state.capped = true;
            let marker = Entry {
                component: "operation-log".into(),
                method: "limit".into(),
                duration_ms: 0,
                outcome: Outcome::Limit,
                code: None,
                ..entry.clone()
            };
            if let Ok(mut marker) = serde_json::to_vec(&marker) {
                marker.push(b'\n');
                let _ = file.write_all(&marker);
            }
            return;
        }
        if file.write_all(&line).is_ok() {
            state.bytes += line.len() as u64;
        }
    }
}

/// Delete day files older than the retention window. Other files are kept.
pub fn prune(dir: &Path, now_ms: u64) -> usize {
    let today = (now_ms / DAY_MS) as i64;
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(day) = name.to_str().and_then(parse_day) else {
            continue;
        };
        let regular = fs::symlink_metadata(entry.path()).is_ok_and(|metadata| metadata.is_file());
        if regular && day <= today - RETENTION_DAYS && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub succeeded: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub rejected: u64,
    pub panicked: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub state: String,
    pub file_count: usize,
    pub byte_length: u64,
    pub counts: Counts,
    pub recent: Vec<Entry>,
    pub truncated: bool,
}

/// Newest-first summary for the support bundle. Every entry is re-sanitized,
/// so an edited file cannot smuggle text into the bundle.
pub fn summarize(dir: &Path, max_entries: usize, max_read_bytes: u64) -> Summary {
    let mut summary = Summary::default();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            summary.state = "missing".into();
            return summary;
        }
        Err(_) => {
            summary.state = "unreadable".into();
            return summary;
        }
    };
    let mut days: Vec<(i64, PathBuf, u64)> = entries
        .flatten()
        .filter_map(|entry| {
            let day = entry.file_name().to_str().and_then(parse_day)?;
            let metadata = fs::symlink_metadata(entry.path()).ok()?;
            metadata.is_file().then(|| (day, entry.path(), metadata.len()))
        })
        .collect();
    days.sort_by(|left, right| right.0.cmp(&left.0));
    summary.state = "available".into();
    summary.file_count = days.len();
    let mut budget = max_read_bytes;
    for (_, path, length) in days {
        summary.byte_length += length;
        if length > budget {
            summary.truncated = true;
            continue;
        }
        budget -= length;
        let Ok(text) = fs::read_to_string(&path) else {
            summary.truncated = true;
            continue;
        };
        for line in text.lines().rev() {
            let Ok(entry) = serde_json::from_str::<Entry>(line) else {
                continue;
            };
            let entry = entry.sanitized();
            match entry.outcome {
                Outcome::Succeeded => summary.counts.succeeded += 1,
                Outcome::Failed => summary.counts.failed += 1,
                Outcome::Cancelled => summary.counts.cancelled += 1,
                Outcome::Rejected => summary.counts.rejected += 1,
                Outcome::Panicked => summary.counts.panicked += 1,
                Outcome::Limit => {}
            }
            if entry.outcome == Outcome::Succeeded {
                continue;
            }
            if summary.recent.len() < max_entries {
                summary.recent.push(entry);
            } else {
                summary.truncated = true;
            }
        }
    }
    summary
}
```

- [ ] **Step 4: 통과 확인**

Run: `cargo test -p product-contract --lib operation_log`
Expected: PASS(7 tests).

- [ ] **Step 5: 커밋**

```bash
git add crates/product-contract/src/operation_log.rs crates/product-contract/src/lib.rs
git commit -m "feat(suite): add fixed-schema operation log"
```

---

### Task 2: 셸 초기화·panic hook·제품 연결

**Files:** Create `crates/product-shell-tauri/src/operation_log.rs`; Modify `crates/product-shell-tauri/src/lib.rs`, 네 제품의 `execute`

**Interfaces:**
- Consumes: Task 1의 `OperationLog`, `Entry`, `Outcome`, `should_record`, `prune`
- Produces: `product_shell_tauri::begin_operation(&WebviewWindow, component: &str, method: &str) -> OperationGuard`, `OperationGuard::finish(self, &OperationState, failure: Option<&str>)`

- [ ] **Step 1: 실패하는 테스트** — `crates/product-shell-tauri/src/operation_log.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use product_contract::ProblemCode;

    #[test]
    fn operation_states_map_to_log_outcomes() {
        assert_eq!(outcome_of(&OperationState::Succeeded {}), Outcome::Succeeded);
        assert_eq!(outcome_of(&OperationState::Cancelled {}), Outcome::Cancelled);
        assert_eq!(
            outcome_of(&OperationState::Failed { code: ProblemCode::Unavailable }),
            Outcome::Failed
        );
        assert_eq!(outcome_of(&OperationState::Stale {}), Outcome::Failed);
    }

    #[test]
    fn panic_locations_keep_only_the_file_name() {
        assert_eq!(location_token("crates\\knowledge\\src\\component.rs", 212), "component.rs:212");
        assert_eq!(location_token("/home/runner/work/devbox/src/lib.rs", 9), "lib.rs:9");
    }

    #[test]
    fn a_dropped_guard_records_a_rejection_and_a_finished_one_does_not_double_count() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(OperationLog::open(dir.path().to_path_buf()).unwrap());
        let sink = Sink { log: Some(log), product: "knowledge".into(), version: "0.9.0".into() };
        drop(OperationGuard::start(&sink, "knowledge.notes", "write_file"));
        OperationGuard::start(&sink, "knowledge.notes", "read_file")
            .finish(&OperationState::Failed { code: product_contract::ProblemCode::Unavailable }, Some("vault_unavailable"));
        let summary = product_contract::operation_log::summarize(dir.path(), 10, 1 << 20);
        assert_eq!(summary.counts.rejected, 1);
        assert_eq!(summary.counts.failed, 1);
        assert_eq!(summary.recent[0].code.as_deref(), Some("vault_unavailable"));
    }
}
```

`crates/product-shell-tauri/Cargo.toml`에 `[dev-dependencies] tempfile = "3"`이 없으면 추가한다(lockfile에 이미 있다).

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p product-shell-tauri --lib operation_log` → 컴파일 실패.

- [ ] **Step 3: 구현** — 같은 파일 테스트 모듈 위에 작성한다.

```rust
//! Records one fixed-schema line per finished component call. Logging never
//! fails or delays a command; see `product_contract::operation_log`.
use product_contract::operation_log::{self, Entry, OperationLog, Outcome};
use product_contract::OperationState;
use std::sync::{Arc, Once};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tauri::{Manager, WebviewWindow};

pub(crate) struct Sink {
    log: Option<Arc<OperationLog>>,
    product: String,
    version: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

pub(crate) fn outcome_of(state: &OperationState) -> Outcome {
    match state {
        OperationState::Succeeded {} => Outcome::Succeeded,
        OperationState::Cancelled {} => Outcome::Cancelled,
        OperationState::Failed { .. } | OperationState::Stale {} | OperationState::Running {} => {
            Outcome::Failed
        }
    }
}

pub(crate) fn location_token(file: &str, line: u32) -> String {
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    format!("{name}:{line}")
}

/// Open `<app local data>/logs`, drop day files past retention and install
/// the panic hook. A missing or unwritable directory disables logging only.
pub(crate) fn initialize(app: &tauri::App, product: &str) {
    let version = app.package_info().version.to_string();
    let log = app
        .path()
        .app_local_data_dir()
        .ok()
        .map(|dir| dir.join("logs"))
        .and_then(|dir| {
            operation_log::prune(&dir, now_ms());
            OperationLog::open(dir).ok()
        })
        .map(Arc::new);
    if let Some(log) = &log {
        install_panic_hook(log.clone(), product.to_string(), version.clone());
    }
    app.manage(Sink {
        log,
        product: product.to_string(),
        version,
    });
}

fn install_panic_hook(log: Arc<OperationLog>, product: String, version: String) {
    static HOOK: Once = Once::new();
    HOOK.call_once(move || {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|location| location_token(location.file(), location.line()))
                .unwrap_or_else(|| "unknown".into());
            log.try_append(&Entry::new(
                now_ms(),
                &version,
                &product,
                "panic",
                &location,
                0,
                Outcome::Panicked,
                None,
            ));
            previous(info);
        }));
    });
}

#[must_use = "finish the guard with the operation outcome"]
pub struct OperationGuard {
    log: Option<Arc<OperationLog>>,
    product: String,
    version: String,
    component: String,
    method: String,
    started: Instant,
    finished: bool,
}

impl OperationGuard {
    pub(crate) fn start(sink: &Sink, component: &str, method: &str) -> Self {
        Self {
            log: sink.log.clone(),
            product: sink.product.clone(),
            version: sink.version.clone(),
            component: component.to_string(),
            method: method.to_string(),
            started: Instant::now(),
            finished: false,
        }
    }

    /// `failure` is the raw native error; non-identifiers are digested.
    pub fn finish(mut self, outcome: &OperationState, failure: Option<&str>) {
        self.record(outcome_of(outcome), failure);
        self.finished = true;
    }

    fn record(&self, outcome: Outcome, code: Option<&str>) {
        let Some(log) = &self.log else {
            return;
        };
        let duration_ms = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        if operation_log::should_record(outcome, duration_ms) {
            log.append(&Entry::new(
                now_ms(),
                &self.version,
                &self.product,
                &self.component,
                &self.method,
                duration_ms,
                outcome,
                code,
            ));
        }
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.record(Outcome::Rejected, None);
        }
    }
}

/// Start timing a component call. Early returns (validation, authorization,
/// replay, shutdown) drop the guard and are recorded as `rejected`.
pub fn begin_operation(window: &WebviewWindow, component: &str, method: &str) -> OperationGuard {
    match window.try_state::<Sink>() {
        Some(sink) => OperationGuard::start(&sink, component, method),
        None => OperationGuard::start(
            &Sink {
                log: None,
                product: String::new(),
                version: String::new(),
            },
            component,
            method,
        ),
    }
}
```

`crates/product-shell-tauri/src/lib.rs`
- 파일 상단 `mod installation;` 아래에 `mod operation_log;`와 `pub use operation_log::{begin_operation, OperationGuard};`를 추가한다.
- `builder(product)`의 `.setup(move |app| { … })`에서 `app.manage(ShellState { … });` 다음 줄에 `operation_log::initialize(app, product);`를 추가한다.

- [ ] **Step 4: 네 제품 연결** — 각 파일에서 아래 두 곳만 바꾼다.

`apps/devbox-knowledge/src-tauri/src/component.rs` `execute`:
  - 함수 첫 줄(`let rejected = …` 앞)에 추가: `let operation = product_shell_tauri::begin_operation(&window, &request.component, &request.method);`
  - 끝의 결과 매핑을 아래로 바꾼다.

```rust
    let (outcome, value, failure) = match value {
        Ok(value) => (OperationState::Succeeded {}, value, None),
        Err(error) => {
            let code = issue(&error);
            (
                if code == "cancelled" {
                    OperationState::Cancelled {}
                } else {
                    OperationState::Failed {
                        code: ProblemCode::Unavailable,
                    }
                },
                json!({"issue": code}),
                Some(error),
            )
        }
    };
    operation.finish(&outcome, failure.as_deref());
```

`apps/devbox-api-studio/src-tauri/src/component.rs` `execute`:
  - 첫 줄에 같은 `let operation = …begin_operation(&window, &request.component, &request.method);`
  - `if request.component == "api-studio.migration" { return Ok(match … ); }` 블록을 아래로 바꾼다(동작 동일, 기록만 추가).

```rust
    if request.component == "api-studio.migration" {
        let result = crate::migration::dispatch(app, &request.method, request.args).await;
        let (outcome, value, failure) = match result {
            Ok(value) => (OperationState::Succeeded {}, value, None),
            Err(error) => {
                let issue = crate::migration::issue(&error);
                (
                    if issue == "cancelled" {
                        OperationState::Cancelled {}
                    } else {
                        OperationState::Failed {
                            code: ProblemCode::Unavailable,
                        }
                    },
                    serde_json::json!({ "issue": issue }),
                    Some(error),
                )
            }
        };
        operation.finish(&outcome, failure.as_deref());
        return Ok(Response {
            operation: Operation {
                provenance,
                outcome,
            },
            value,
        });
    }
```

  - 끝의 결과 매핑: `let (outcome, value) = match value {`를 `let (outcome, value, failure) = match value {`로, `Ok(value) => (OperationState::Succeeded {}, value),`를 `Ok(value) => (OperationState::Succeeded {}, value, None),`로, `Err(error)` 팔의 반환을 `(outcome, serde_json::json!({ "issue": issue }), Some(error))`로 바꾸고 매핑 뒤에 `operation.finish(&outcome, failure.as_deref());`를 추가한다.

`apps/devbox-workspace/src-tauri/src/component.rs` `execute`(`async fn execute(` 검색):
  - 첫 줄에 `let operation = product_shell_tauri::begin_operation(&window, &request.component, &request.method);`
  - 끝의 매핑을 아래로 바꾼다.

```rust
    let (outcome, value, failure) = match result {
        Ok(value) => (OperationState::Succeeded {}, value, None),
        Err(issue) => (
            OperationState::Failed {
                code: ProblemCode::Unavailable,
            },
            json!({"issue":issue}),
            Some(issue.to_string()),
        ),
    };
    operation.finish(&outcome, failure.as_deref());
```

`apps/devbox-control-center/src-tauri/src/tools_host.rs` `execute`:
  - `let component = if … { "control-center.delivery" } else { "control-center.tools" };` 바로 다음 줄에 `let operation = product_shell_tauri::begin_operation(&window, component, &request.method);`
  - 끝의 매핑을 Workspace와 같은 모양(`(outcome, value, failure)`, `Err(issue)` 팔에 `Some(issue.to_string())`)으로 바꾸고 `operation.finish(&outcome, failure.as_deref());`를 추가한다. 응답 JSON의 `manager_tools_unavailable` 치환은 그대로 둔다(로그에는 원래 코드가 간다).

- [ ] **Step 5: 통과 확인**

Run: `cargo test -p product-shell-tauri --lib operation_log && cargo check -p devbox-knowledge -p devbox-api-studio -p devbox-workspace -p devbox-control-center`
Expected: PASS, 컴파일 성공. (패키지 이름은 `cargo metadata --no-deps --format-version 1 | python3 -c "import json,sys;print([p['name'] for p in json.load(sys.stdin)['packages'] if p['name'].startswith('devbox-')])"`로 확인한다.)

- [ ] **Step 6: 커밋**

```bash
git add crates/product-shell-tauri apps/devbox-knowledge/src-tauri/src/component.rs apps/devbox-api-studio/src-tauri/src/component.rs apps/devbox-workspace/src-tauri/src/component.rs apps/devbox-control-center/src-tauri/src/tools_host.rs
git commit -m "feat(suite): log component outcomes and panics from every product"
```

---

### Task 3: 지원 번들에 제품 로그 요약

**Files:** `crates/installation-tools/Cargo.toml`, `src/core/support_bundle.rs`, `src/commands/diagnostics.rs`, `packages/control-center-features/src/manager/api.ts`, `CONVENTIONS.md`

**Interfaces:**
- Consumes: Task 1의 `summarize`, `Summary`
- Produces: `support_bundle::OperationLogSummary { product: String, summary: Summary }`, `build_bundle(…, operations: Vec<OperationLogSummary>, cancel)`(인자 추가), `diagnostics::suite_log_dirs(data_root: &Path, own_identifier: &str, products: &[(String, String)]) -> Vec<(String, PathBuf)>`

- [ ] **Step 1: 실패하는 테스트**

`support_bundle.rs` 테스트 모듈:

```rust
    #[test]
    fn operation_summaries_are_included_without_changing_the_source_revision() {
        let root = std::env::temp_dir().join("devbox-support-bundle-operations");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let summary = product_contract::operation_log::Summary {
            state: "available".into(),
            ..Default::default()
        };
        let with = build_bundle(
            &catalog(),
            &root,
            Vec::new(),
            Vec::new(),
            vec![OperationLogSummary { product: "knowledge".into(), summary }],
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let without = build_bundle(
            &catalog(),
            &root,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let text = String::from_utf8(with.bytes).unwrap();
        assert!(text.contains("\"operations\""));
        assert!(text.contains("\"schemaVersion\": 2"));
        assert_eq!(with.source_revision, without.source_revision);
        let _ = fs::remove_dir_all(root);
    }
```

  기존 `build_bundle(` 호출 테스트 3곳에는 `Vec::new(),`를 `Arc::new(AtomicBool::new(false))` 앞 인자로 추가한다.

`diagnostics.rs` 테스트 모듈:

```rust
    #[test]
    fn suite_log_dirs_share_the_installation_suffix() {
        let products = vec![
            ("knowledge".to_string(), "com.devbox.v08.knowledge".to_string()),
            ("control-center".to_string(), "com.devbox.v08.controlcenter".to_string()),
        ];
        let root = std::path::Path::new("/data");
        let dirs = suite_log_dirs(root, "com.devbox.v08.controlcenter.iabc123", &products);
        assert_eq!(
            dirs,
            vec![
                ("knowledge".to_string(), root.join("com.devbox.v08.knowledge.iabc123").join("logs")),
                ("control-center".to_string(), root.join("com.devbox.v08.controlcenter.iabc123").join("logs")),
            ]
        );
        assert!(suite_log_dirs(root, "com.devbox.v08.controlcenter", &products).is_empty());
        assert!(suite_log_dirs(root, "com.devbox.v08.controlcenter.i../x", &products).is_empty());
    }
```

- [ ] **Step 2: 실패 확인** — Run: `cargo test -p devbox-installation-tools --lib support_bundle diagnostics` → 컴파일 실패.

- [ ] **Step 3: 구현**

`crates/installation-tools/Cargo.toml` `[dependencies]`에 `product-contract = { path = "../product-contract" }`를 추가한다.

`support_bundle.rs`
- 구조체를 추가한다.

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogSummary {
    pub product: String,
    pub summary: product_contract::operation_log::Summary,
}
```

- `SupportBundleDocument`의 `logs` 아래에 `operations: Vec<OperationLogSummary>,`를 추가한다.
- `build_bundle` 인자 `installed: Vec<SupportInstalledApp>,` 다음에 `operations: Vec<OperationLogSummary>,`를 추가하고, 문서 생성부에서 `schema_version: 1`을 `schema_version: 2`로, `logs: …` 다음에 `operations,`를 넣는다. `source_revision` 계산에는 넣지 않는다(미리보기와 내보내기 사이에 로그가 늘어나도 사용자가 본 바이트를 그대로 내보낸다. 진단·설치 정보와 같은 규칙).

`diagnostics.rs`
- 도우미를 추가한다.

```rust
/// Suite members share the installation suffix of their data identifiers
/// (`<product identifier>.i<suffix>`), so Control Center can find the other
/// products' log folders. Portable builds have per-product suffixes and
/// simply report the other folders as missing.
pub(crate) fn suite_log_dirs(
    data_root: &std::path::Path,
    own_identifier: &str,
    products: &[(String, String)],
) -> Vec<(String, PathBuf)> {
    let Some((_, suffix)) = own_identifier.rsplit_once(".i") else {
        return Vec::new();
    };
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Vec::new();
    }
    products
        .iter()
        .map(|(id, identifier)| {
            (
                id.clone(),
                data_root.join(format!("{identifier}.i{suffix}")).join("logs"),
            )
        })
        .collect()
}

fn operation_summaries(app: &tauri::AppHandle, data_root: &std::path::Path) -> Vec<support_bundle::OperationLogSummary> {
    let Ok(catalog) = devbox_catalog::products::ProductCatalog::parse(devbox_catalog::products::SOURCE) else {
        return Vec::new();
    };
    let products: Vec<(String, String)> = catalog
        .products
        .iter()
        .map(|product| (product.id.clone(), product.identifier.clone()))
        .collect();
    suite_log_dirs(data_root, &app.config().identifier, &products)
        .into_iter()
        .map(|(product, dir)| support_bundle::OperationLogSummary {
            product,
            summary: product_contract::operation_log::summarize(&dir, 100, 2 * 1024 * 1024),
        })
        .collect()
}
```

  (`devbox_catalog::products` 경로는 `crates/product-shell-tauri/src/lib.rs`의 `use catalog::products::{…, ProductCatalog, SOURCE};`와 같은 모듈이다. installation-tools에서는 `devbox-catalog` 별칭으로 가져온다.)
- `preview_support_bundle`의 `build_bundle(` 호출에서 `installed_for_bundle(&app),` 다음 인자로 `operation_summaries(&app, &root),`를 넣는다. 같은 함수의 `included_sections`에 `"operation-log".to_string(),`를 `"log-metadata"` 다음에 추가한다.

`packages/control-center-features/src/manager/api.ts:1690`(브라우저 mock)의 `includedSections` 배열과 `App.test.tsx:385`의 같은 배열에 `"operation-log"`를 `"log-metadata"` 다음에 추가한다.

`CONVENTIONS.md:73`의 `` - `log`, `env_logger` (로깅)`` 줄을 아래로 바꾼다.

```markdown
  - 운영 로그는 `product_contract::operation_log`(고정 스키마 JSONL, 제품 데이터 폴더 `logs/`, 14일 보관)만 쓴다. 인자·값·경로·자유 문장은 기록하지 않는다. `log`·`env_logger`·`tracing`은 쓰지 않는다.
```

- [ ] **Step 4: 통과 확인**

Run: `cargo test -p devbox-installation-tools --lib && pnpm --filter @devbox/control-center-features exec vitest run src/manager/App.test.tsx`
Expected: PASS.

- [ ] **Step 5: 커밋**

```bash
git add crates/installation-tools packages/control-center-features/src/manager CONVENTIONS.md Cargo.lock
git commit -m "feat(devbox-control-center): include product operation logs in the support bundle"
```

---

### Task 4: PR 완료

- [ ] `00-roadmap.md` §4.4–§4.9를 수행한다.
- [ ] PR 본문 "Windows 실기 확인" 절(사용자 확인 대기):
  1. 설치본에서 Knowledge를 열고 존재하지 않는 vault 경로 작업 등으로 오류를 한 번 낸다.
  2. `%LOCALAPPDATA%\com.devbox.v08.knowledge.i*\logs\operations-<오늘 UTC 날짜>.jsonl`에 `"outcome":"failed"` 줄이 있고 경로·문장이 없는지 본다.
  3. Control Center › 진단에서 지원 번들 미리보기 → 내보내기. `operations` 절에 네 제품이 있고 Knowledge의 `recent`에 방금 실패가 있다.
