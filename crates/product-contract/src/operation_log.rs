//! Fixed-schema operation log shared by every product. A line says which
//! component method ran, how long it took and how it ended. Arguments,
//! values, paths and free-form messages are never written: any field that is
//! not a short identifier is replaced by a digest, so repeated failures still
//! group together without revealing what they contained.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
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
    opaque_token(value)
}

/// Digest untrusted fields even when they resemble a short identifier.
pub fn opaque_token(value: &str) -> String {
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
    if date.len() != 10 || !date.is_ascii() {
        return None;
    }
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
            prune(&self.dir, entry.ts_ms);
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .read(true)
                .open(self.dir.join(&name))
                .ok();
            let bytes = file
                .as_ref()
                .and_then(|file| file.metadata().ok())
                .map_or(0, |metadata| metadata.len());
            let capped = bytes >= self.limit
                || file
                    .as_ref()
                    .is_some_and(|file| has_limit_marker(file, bytes));
            *state = DayFile {
                name,
                file,
                bytes,
                capped,
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

// A short final marker can leave the file below the byte cap. Preserve that
// day's stopped state across application restarts without reading the full log.
fn has_limit_marker(file: &File, bytes: u64) -> bool {
    let Ok(mut reader) = file.try_clone() else {
        return false;
    };
    if reader
        .seek(SeekFrom::Start(bytes.saturating_sub(1024)))
        .is_err()
    {
        return false;
    }
    let mut tail = String::new();
    if reader.take(1024).read_to_string(&mut tail).is_err() {
        return false;
    }
    tail.lines()
        .last()
        .and_then(|line| serde_json::from_str::<Entry>(line).ok())
        .is_some_and(|entry| entry.outcome == Outcome::Limit)
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
            metadata
                .is_file()
                .then(|| (day, entry.path(), metadata.len()))
        })
        .collect();
    days.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    summary.state = "available".into();
    summary.file_count = days.len();
    let mut budget = max_read_bytes;
    for (_, path, length) in days {
        summary.byte_length += length;
        if length > budget {
            summary.truncated = true;
            continue;
        }
        let mut text = String::new();
        let read = File::open(&path).and_then(|file| {
            file.take(budget.saturating_add(1))
                .read_to_string(&mut text)
        });
        let Ok(read) = read else {
            summary.truncated = true;
            continue;
        };
        if read as u64 > budget {
            summary.truncated = true;
            budget = 0;
            continue;
        }
        budget -= read as u64;
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

#[cfg(test)]
mod tests {
    use super::*;

    const DAY0: u64 = 19_990 * DAY_MS; // 2024-09-24T00:00:00Z

    fn entry(ts_ms: u64, outcome: Outcome, code: Option<&str>) -> Entry {
        Entry::new(
            ts_ms,
            "0.9.0",
            "knowledge",
            "knowledge.notes",
            "write_file",
            12,
            outcome,
            code,
        )
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
        let line = serde_json::to_string(&entry(DAY0, Outcome::Failed, Some("경로 /home/me 없음")))
            .unwrap();
        assert!(
            !line.contains("/home/me") && !line.contains("경로"),
            "{line}"
        );
    }

    #[test]
    fn fast_successes_are_skipped_and_everything_else_is_kept() {
        assert!(!should_record(Outcome::Succeeded, SLOW_OPERATION_MS - 1));
        assert!(should_record(Outcome::Succeeded, SLOW_OPERATION_MS));
        for outcome in [
            Outcome::Failed,
            Outcome::Cancelled,
            Outcome::Rejected,
            Outcome::Panicked,
        ] {
            assert!(should_record(outcome, 0));
        }
    }

    #[test]
    fn files_are_named_by_utc_day() {
        assert_eq!(day_file_name(0), "operations-1970-01-01.jsonl");
        assert_eq!(day_file_name(DAY0), "operations-2024-09-24.jsonl");
        assert_eq!(
            day_file_name(DAY0 + DAY_MS - 1),
            "operations-2024-09-24.jsonl"
        );
        assert_eq!(
            day_file_name(11_016 * DAY_MS),
            "operations-2000-02-29.jsonl"
        );
        assert_eq!(parse_day("operations-2000-02-29.jsonl"), Some(11_016));
        assert_eq!(parse_day("operations-2000-02-30.jsonl"), None);
        assert_eq!(parse_day("other.jsonl"), None);
        assert_eq!(
            parse_day("operations-9223372036854775807-01-01.jsonl"),
            None
        );
    }

    #[test]
    fn appends_roll_over_by_day_and_prune_keeps_fourteen_days() {
        let dir = tempfile::tempdir().unwrap();
        let log = OperationLog::open(dir.path().join("logs")).unwrap();
        log.append(&entry(DAY0, Outcome::Failed, Some("a")));
        log.append(&entry(DAY0 + 13 * DAY_MS, Outcome::Failed, Some("b")));
        log.append(&entry(DAY0 + 14 * DAY_MS, Outcome::Failed, Some("c")));
        let logs = dir.path().join("logs");
        assert_eq!(std::fs::read_dir(&logs).unwrap().count(), 2);
        std::fs::write(logs.join("notes.txt"), "keep").unwrap();
        assert_eq!(prune(&logs, DAY0 + 14 * DAY_MS), 0);
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
    fn a_short_limit_marker_is_respected_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let log = OperationLog::with_limit(dir.path().to_path_buf(), 300).unwrap();
        let large = Entry::new(
            DAY0,
            "0.8.1",
            "knowledge",
            &"a".repeat(64),
            &"b".repeat(64),
            0,
            Outcome::Failed,
            Some(&"c".repeat(64)),
        );
        log.append(&large);
        let path = dir.path().join(day_file_name(DAY0));
        let before = std::fs::read(&path).unwrap();
        assert!(before.len() < 300);
        drop(log);
        let log = OperationLog::with_limit(dir.path().to_path_buf(), 300).unwrap();
        log.append(&entry(DAY0, Outcome::Failed, None));
        assert_eq!(std::fs::read(path).unwrap(), before);
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
        assert_eq!(
            summarize(&dir.path().join("missing"), 3, 1024).state,
            "missing"
        );
    }
}
