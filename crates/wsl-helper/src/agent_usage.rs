use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
const MAX_FILES: usize = 256;
const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_LINE: usize = 4 * 1024 * 1024;
const DAY_MS: u64 = 86_400_000;
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub sessions: u32,
}
impl TokenTotals {
    fn add(&mut self, other: &Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.sessions = self.sessions.saturating_add(other.sessions);
    }
}
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UsageReport {
    pub claude: Option<TokenTotals>,
    pub codex: Option<TokenTotals>,
    pub truncated: bool,
}
struct Budget {
    bytes: usize,
    files: usize,
    entries: usize,
    truncated: bool,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            bytes: MAX_BYTES,
            files: 0,
            entries: 0,
            truncated: false,
        }
    }
}
// fill_buf keeps even an unterminated or oversized record bounded in memory.
fn records(mut reader: impl BufRead, budget: &mut Budget, mut visit: impl FnMut(Value)) {
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let buffer = match reader.fill_buf() {
            Ok(buffer) => buffer,
            Err(_) => {
                budget.truncated = true;
                break;
            }
        };
        if buffer.is_empty() {
            if !oversized && !line.is_empty() {
                if let Ok(value) = serde_json::from_slice(&line) {
                    visit(value);
                }
            }
            break;
        }
        if budget.bytes == 0 {
            budget.truncated = true;
            break;
        }
        let end = buffer.iter().position(|b| *b == b'\n').map(|n| n + 1);
        let count = end.unwrap_or(buffer.len()).min(budget.bytes);
        let complete = end == Some(count);
        if !oversized {
            if line.len().saturating_add(count) > MAX_LINE {
                oversized = true;
                line.clear();
                budget.truncated = true;
            } else {
                line.extend_from_slice(&buffer[..count]);
            }
        }
        reader.consume(count);
        budget.bytes -= count;
        if complete {
            if !oversized {
                if let Ok(value) = serde_json::from_slice(&line) {
                    visit(value);
                }
            }
            line.clear();
            oversized = false;
        }
    }
}
fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}
pub fn claude_project_dir(home: &Path, root: &str) -> PathBuf {
    let name: String = root
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    home.join(".claude/projects").join(name)
}
fn claude_read(
    lines: impl BufRead,
    totals: &mut TokenTotals,
    seen: &mut HashSet<String>,
    budget: &mut Budget,
) {
    let mut added = false;
    records(lines, budget, |value| {
        if value["type"] != "assistant" {
            return;
        }
        let message = &value["message"];
        let Some(id) = message["id"].as_str().filter(|id| !id.is_empty()) else {
            return;
        };
        let usage = &message["usage"];
        if !usage.is_object() || !seen.insert(id.into()) {
            return;
        }
        totals.add(&TokenTotals {
            input: count(usage, "input_tokens"),
            output: count(usage, "output_tokens"),
            cache_read: count(usage, "cache_read_input_tokens"),
            cache_write: count(usage, "cache_creation_input_tokens"),
            sessions: 0,
        });
        added = true;
    });
    if added {
        totals.sessions = totals.sessions.saturating_add(1);
    }
}
pub fn claude_totals(lines: impl BufRead, totals: &mut TokenTotals, seen: &mut HashSet<String>) {
    claude_read(lines, totals, seen, &mut Budget::default());
}
fn codex_read(lines: impl BufRead, root: &Path, budget: &mut Budget) -> Option<TokenTotals> {
    let mut owner = None;
    let mut last = None;
    records(lines, budget, |value| {
        if value["type"] == "session_meta" && owner.is_none() {
            owner = Some(value["payload"]["cwd"].as_str().is_some_and(|cwd| {
                // These are WSL POSIX paths even when parser tests run on Windows.
                cwd.starts_with('/')
                    && !cwd.starts_with("//")
                    && !cwd
                        .split('/')
                        .skip(1)
                        .any(|part| matches!(part, "" | "." | ".."))
                    && root.to_str().is_some_and(|root| {
                        cwd == root
                            || cwd
                                .strip_prefix(root)
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
            }));
        }
        if owner != Some(true)
            || value["type"] != "event_msg"
            || value["payload"]["type"] != "token_count"
        {
            return;
        }
        let usage = &value["payload"]["info"]["total_token_usage"];
        if usage.is_object() {
            last = Some(TokenTotals {
                input: count(usage, "input_tokens"),
                output: count(usage, "output_tokens"),
                cache_read: count(usage, "cached_input_tokens"),
                cache_write: 0,
                sessions: 1,
            });
        }
    });
    last
}
pub fn codex_session(lines: impl BufRead, root: &Path) -> Option<TokenTotals> {
    codex_read(lines, root, &mut Budget::default())
}
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}
fn no_links(path: &Path) -> bool {
    path.ancestors()
        .all(|part| fs::symlink_metadata(part).is_ok_and(|meta| !meta.file_type().is_symlink()))
}
fn files(
    dir: &Path,
    since: Option<u64>,
    budget: &mut Budget,
    mut visit: impl FnMut(BufReader<fs::File>, &mut Budget),
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => {
            budget.truncated = true;
            return;
        }
    };
    for entry in entries {
        if budget.files >= MAX_FILES || budget.bytes == 0 || budget.entries >= 4096 {
            budget.truncated = true;
            break;
        }
        budget.entries += 1;
        let Ok(entry) = entry else {
            budget.truncated = true;
            continue;
        };
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            budget.truncated = true;
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if let Some(since) = since {
            let Some(modified) = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            else {
                budget.truncated = true;
                continue;
            };
            if modified.as_millis() < u128::from(since) {
                continue;
            }
        }
        let Ok(file) = fs::File::open(path) else {
            budget.truncated = true;
            continue;
        };
        budget.files += 1;
        visit(BufReader::new(file), budget);
    }
}
pub fn usage(home: &Path, root: &Path, since_ms: u64, now_ms: u64) -> UsageReport {
    let mut report = UsageReport::default();
    let mut budget = Budget::default();
    if since_ms > now_ms {
        report.truncated = true;
        return report;
    }
    let Some(root_text) = root.to_str() else {
        report.truncated = true;
        return report;
    };
    let claude = claude_project_dir(home, root_text);
    if claude.is_dir() && no_links(&claude) {
        let mut totals = TokenTotals::default();
        let mut seen = HashSet::new();
        files(&claude, Some(since_ms), &mut budget, |reader, budget| {
            claude_read(reader, &mut totals, &mut seen, budget)
        });
        report.claude = Some(totals);
    }
    let codex = home.join(".codex/sessions");
    if codex.is_dir() && no_links(&codex) {
        let mut totals = TokenTotals::default();
        let first = since_ms / DAY_MS;
        let last = now_ms / DAY_MS;
        if last - first >= 31 {
            budget.truncated = true;
        }
        for day in first..=last.min(first + 30) {
            let (year, month, day) = civil_from_days(day as i64);
            let dir = codex.join(format!("{year:04}/{month:02}/{day:02}"));
            if dir.is_dir() && no_links(&dir) {
                files(&dir, None, &mut budget, |reader, budget| {
                    if let Some(session) = codex_read(reader, root, budget) {
                        totals.add(&session);
                    }
                });
            }
            if budget.files >= MAX_FILES || budget.bytes == 0 {
                break;
            }
        }
        report.codex = Some(totals);
    }
    report.truncated = budget.truncated;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const CLAUDE: &str = include_str!("../tests/fixtures/agent-usage/claude.jsonl");
    const CODEX: &str = include_str!("../tests/fixtures/agent-usage/codex.jsonl");

    #[test]
    fn oversized_lines_and_total_byte_limits_do_not_allocate_unbounded_records() {
        let mut input = vec![b'x'; MAX_LINE + 1];
        input.push(b'\n');
        input.extend_from_slice(CLAUDE.as_bytes());
        let mut budget = Budget::default();
        let mut totals = TokenTotals::default();
        claude_read(
            Cursor::new(input),
            &mut totals,
            &mut HashSet::new(),
            &mut budget,
        );
        assert!(budget.truncated);
        assert_eq!(totals.output, 30);
        let mut budget = Budget {
            bytes: 10,
            ..Budget::default()
        };
        let mut count = 0;
        records(Cursor::new(CLAUDE), &mut budget, |_| count += 1);
        assert!(budget.truncated);
        assert_eq!(budget.bytes, 0);
        assert_eq!(count, 0);
    }
    #[test]
    fn file_and_day_limits_are_reported_and_missing_tools_are_absent() {
        let home = tempfile::tempdir().unwrap();
        let root = Path::new("/fixture");
        let missing = usage(home.path(), root, 0, 0);
        assert!(missing.claude.is_none() && missing.codex.is_none());
        let dir = claude_project_dir(home.path(), "/fixture");
        fs::create_dir_all(&dir).unwrap();
        for i in 0..257 {
            fs::write(dir.join(format!("{i}.jsonl")), "{}\n").unwrap();
        }
        assert!(usage(home.path(), root, 0, 0).truncated);
        fs::remove_dir_all(&dir).unwrap();
        fs::create_dir_all(home.path().join(".codex/sessions")).unwrap();
        assert!(usage(home.path(), root, 0, 32 * DAY_MS).truncated);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_727), (2026, 10, 1));
    }
    #[test]
    fn codex_never_switches_owner_after_the_first_session_metadata() {
        let other = CODEX.replace("devbox-fix", "elsewhere");
        assert!(codex_session(
            Cursor::new(format!("{other}{CODEX}")),
            Path::new("/home/me/projects/devbox-fix")
        )
        .is_none());
    }
    #[cfg(unix)]
    #[test]
    fn usage_does_not_follow_a_session_file_link() {
        let home = tempfile::tempdir().unwrap();
        let dir = claude_project_dir(home.path(), "/fixture");
        fs::create_dir_all(&dir).unwrap();
        let target = home.path().join("outside.jsonl");
        fs::write(&target, CLAUDE).unwrap();
        std::os::unix::fs::symlink(&target, dir.join("link.jsonl")).unwrap();
        assert_eq!(
            usage(home.path(), Path::new("/fixture"), 0, 0)
                .claude
                .unwrap()
                .output,
            0
        );
    }

    #[test]
    fn claude_project_directories_replace_every_non_alphanumeric_byte() {
        assert_eq!(
            claude_project_dir(
                Path::new("/home/me"),
                "/home/me/projects/.worktrees/devbox_fix"
            ),
            Path::new("/home/me/.claude/projects/-home-me-projects--worktrees-devbox-fix")
        );
    }

    #[test]
    fn claude_usage_counts_each_message_once_and_skips_unknown_lines() {
        let mut totals = TokenTotals::default();
        let mut seen = HashSet::new();
        claude_totals(Cursor::new(CLAUDE), &mut totals, &mut seen);
        assert_eq!(
            (
                totals.input,
                totals.output,
                totals.cache_write,
                totals.cache_read
            ),
            (10, 30, 100, 200)
        );
    }

    #[test]
    fn codex_usage_takes_the_last_cumulative_count_for_sessions_inside_the_root() {
        let inside = codex_session(
            Cursor::new(CODEX),
            Path::new("/home/me/projects/devbox-fix"),
        )
        .unwrap();
        assert_eq!(
            (
                inside.input,
                inside.cache_read,
                inside.output,
                inside.sessions
            ),
            (300, 120, 30, 1)
        );
        assert!(codex_session(Cursor::new(CODEX), Path::new("/home/me/projects/devbox")).is_none());
    }

    #[test]
    fn usage_reads_both_tools_from_a_home_folder() {
        let home = tempfile::tempdir().unwrap();
        let root = Path::new("/home/me/projects/devbox-fix");
        let claude = claude_project_dir(home.path(), root.to_str().unwrap());
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("a.jsonl"), CLAUDE).unwrap();
        let day = home.path().join(".codex/sessions/2026/10/01");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("rollout-1.jsonl"), CODEX).unwrap();
        let since = 1_790_812_800_000; // 2026-10-01T00:00:00Z
        std::fs::OpenOptions::new()
            .write(true)
            .open(claude.join("a.jsonl"))
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_millis(since + 1))
            .unwrap();
        let report = usage(home.path(), root, since, since + 3_600_000);
        assert_eq!(report.claude.unwrap().output, 30);
        assert_eq!(report.codex.unwrap().output, 30);
        assert!(!report.truncated);
    }
}
