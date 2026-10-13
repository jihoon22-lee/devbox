//! Bounded metadata-only correlation of Codex's primary CLI thread and current turn.
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
const MAX_FILES: usize = 2048;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 128 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Resources {
    Planned,
    Owned,
    Partial {
        path_exists: bool,
        branch_exists: Option<bool>,
        worktree_registered: Option<bool>,
    },
}
impl Default for Resources {
    fn default() -> Self {
        Self::Planned
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Binding {
    pub launch_ms: i64,
    #[serde(default)]
    pub resources: Resources,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub rollout: Option<PathBuf>,
    pub turn_started_ms: Option<i64>,
}
impl Binding {
    pub fn new(launch_ms: i64) -> Self {
        Self {
            launch_ms,
            resources: Resources::Planned,
            thread_id: None,
            turn_id: None,
            rollout: None,
            turn_started_ms: None,
        }
    }
    pub fn matches(&self, thread: &str, turn: &str) -> bool {
        self.thread_id.as_deref() == Some(thread) && self.turn_id.as_deref() == Some(turn)
    }
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS, thiserror::Error,
)]
#[serde(rename_all = "camelCase")]
pub enum Diagnostic {
    #[error("primary Codex metadata is missing")]
    Missing,
    #[error("primary Codex metadata is ambiguous")]
    Ambiguous,
    #[error("Codex metadata exceeds bounded limits")]
    LimitExceeded,
    #[error("Codex metadata is invalid or truncated")]
    Invalid,
    #[error("Codex metadata path is unsafe")]
    UnsafePath,
    #[error("Codex metadata cannot be read")]
    Unavailable,
}
#[derive(Deserialize)]
struct Record {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    payload: Box<serde_json::value::RawValue>,
}
// Other Codex origins use structured source enums. Inspect only the provenance
// discriminators before decoding the metadata contract of an eligible CLI user.
#[derive(Deserialize)]
struct Provenance {
    source: Box<serde_json::value::RawValue>,
    thread_source: Option<Box<serde_json::value::RawValue>>,
}
impl Provenance {
    fn primary(&self) -> bool {
        serde_json::from_str::<String>(self.source.get())
            .ok()
            .as_deref()
            == Some("cli")
            && self
                .thread_source
                .as_ref()
                .and_then(|raw| serde_json::from_str::<String>(raw.get()).ok())
                .as_deref()
                == Some("user")
    }
}
#[derive(Deserialize)]
struct Session {
    id: String,
    timestamp: String,
    cwd: String,
    source: String,
    thread_source: Option<String>,
}
#[derive(Deserialize)]
struct Lifecycle {
    #[serde(rename = "type")]
    kind: String,
    turn_id: Option<String>,
}
#[derive(Deserialize)]
struct Context {
    turn_id: String,
    cwd: String,
}
fn safe_path(path: &Path) -> Result<(), Diagnostic> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(m) if m.file_type().is_symlink() => return Err(Diagnostic::UnsafePath),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Diagnostic::Unavailable),
        }
    }
    Ok(())
}
fn files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
    depth: usize,
    launch_ms: i64,
    known_thread: Option<&str>,
    searched: &mut usize,
) -> Result<(), Diagnostic> {
    if depth > 4 {
        return Err(Diagnostic::LimitExceeded);
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(Diagnostic::Unavailable),
    };
    for entry in entries {
        *searched += 1;
        if *searched > 8192 {
            return Err(Diagnostic::LimitExceeded);
        }
        let entry = entry.map_err(|_| Diagnostic::Unavailable)?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|_| Diagnostic::Unavailable)?;
        if kind.is_symlink() {
            return Err(Diagnostic::UnsafePath);
        }
        if !path.starts_with(root) {
            return Err(Diagnostic::UnsafePath);
        }
        if kind.is_dir() {
            if depth == 2 && known_thread.is_none() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|_| Diagnostic::UnsafePath)?;
                let parts: Vec<_> = rel
                    .components()
                    .map(|x| x.as_os_str().to_string_lossy())
                    .collect();
                if parts.len() == 3 {
                    let date = format!("{}-{}-{}T00:00:00Z", parts[0], parts[1], parts[2]);
                    if utc_ms(&date).is_some_and(|at| at.saturating_add(86400000) <= launch_ms) {
                        continue;
                    }
                }
            }
            files(
                root,
                &path,
                out,
                depth + 1,
                launch_ms,
                known_thread,
                searched,
            )?;
        } else if kind.is_file()
            && path.extension().is_some_and(|x| x == "jsonl")
            && path
                .file_name()
                .is_some_and(|x| x.to_string_lossy().starts_with("rollout-"))
        {
            if known_thread.is_some_and(|id| {
                !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(&format!("-{id}.jsonl")))
            }) {
                continue;
            }
            out.push(path);
            if out.len() > MAX_FILES {
                return Err(Diagnostic::LimitExceeded);
            }
        }
    }
    Ok(())
}
fn read(
    path: &Path,
    workdir: &str,
    binding: &Binding,
    budget: &mut u64,
) -> Result<Option<Binding>, Diagnostic> {
    safe_path(path)?;
    let file = std::fs::File::open(path).map_err(|_| Diagnostic::Unavailable)?;
    let size = file.metadata().map_err(|_| Diagnostic::Unavailable)?.len();

    let mut reader = BufReader::new(file.take(MAX_FILE_BYTES + 1));
    let mut line = Vec::new();
    let mut result = None;
    let mut context = None;
    loop {
        line.clear();
        let bytes = reader
            .by_ref()
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| Diagnostic::Unavailable)?;
        if bytes == 0 {
            break;
        }
        if bytes > MAX_LINE_BYTES {
            return Err(Diagnostic::LimitExceeded);
        }
        if line.last() != Some(&b'\n') {
            return Err(Diagnostic::Invalid);
        }
        let record: Record = serde_json::from_slice(&line).map_err(|_| Diagnostic::Invalid)?;
        if record.kind == "session_meta" {
            if result.is_some() {
                return Err(Diagnostic::Invalid);
            }
            let provenance: Provenance =
                serde_json::from_str(record.payload.get()).map_err(|_| Diagnostic::Invalid)?;
            if !provenance.primary() {
                return Ok(None);
            }
            let meta: Session =
                serde_json::from_str(record.payload.get()).map_err(|_| Diagnostic::Invalid)?;
            let created = utc_ms(&meta.timestamp).ok_or(Diagnostic::Invalid)?;
            if meta.source != "cli"
                || meta.thread_source.as_deref() != Some("user")
                || meta.cwd != workdir
            {
                return Ok(None);
            }
            if let Some(known) = &binding.thread_id {
                if &meta.id != known {
                    return Ok(None);
                }
            } else if created < binding.launch_ms {
                return Ok(None);
            }
            if meta.id.is_empty() {
                return Err(Diagnostic::Invalid);
            }
            if size > MAX_FILE_BYTES || size > *budget {
                return Err(Diagnostic::LimitExceeded);
            }
            *budget -= size;
            let mut bound = binding.clone();
            bound.thread_id = Some(meta.id);
            bound.rollout = Some(path.into());
            bound.turn_id = None;
            bound.turn_started_ms = None;
            result = Some(bound);
        } else if record.kind == "event_msg" {
            let event: Lifecycle =
                serde_json::from_str(record.payload.get()).map_err(|_| Diagnostic::Invalid)?;
            if event.kind == "task_started" {
                let at = record
                    .timestamp
                    .as_deref()
                    .and_then(utc_ms)
                    .ok_or(Diagnostic::Invalid)?;
                if at >= binding.launch_ms
                    && let Some(current) = result.as_mut()
                {
                    let turn = event
                        .turn_id
                        .filter(|s| !s.is_empty())
                        .ok_or(Diagnostic::Invalid)?;
                    current.turn_id = Some(turn);
                    current.turn_started_ms = Some(at);
                }
            }
        } else if record.kind == "turn_context" {
            let parsed: Context =
                serde_json::from_str(record.payload.get()).map_err(|_| Diagnostic::Invalid)?;
            context = Some(parsed);
        }
    }
    if let (Some(current), Some(context)) = (&result, context)
        && current.turn_id.as_deref() == Some(context.turn_id.as_str())
        && context.cwd != workdir
    {
        return Err(Diagnostic::Invalid);
    }
    Ok(result.filter(|b| b.turn_id.is_some()))
}
pub(crate) fn refresh(
    root: &Path,
    workdir: &str,
    binding: &Binding,
) -> Result<Binding, Diagnostic> {
    safe_path(root)?;
    if let Some(path) = &binding.rollout {
        if !path.starts_with(root) {
            return Err(Diagnostic::UnsafePath);
        }
        let mut budget = MAX_TOTAL_BYTES;
        return read(path, workdir, binding, &mut budget)?.ok_or(Diagnostic::Missing);
    }
    let mut candidates = Vec::new();
    files(
        root,
        root,
        &mut candidates,
        0,
        binding.launch_ms,
        binding.thread_id.as_deref(),
        &mut 0,
    )?;
    let mut budget = MAX_TOTAL_BYTES;
    let mut matches = Vec::new();
    for path in candidates {
        if let Some(bound) = read(&path, workdir, binding, &mut budget)? {
            matches.push(bound)
        }
    }
    match matches.len() {
        0 => Err(Diagnostic::Missing),
        1 => Ok(matches.remove(0)),
        _ => Err(Diagnostic::Ambiguous),
    }
}
/// Parse the UTC timestamps emitted by rollout metadata, without loading message bodies.
pub(crate) fn utc_ms(text: &str) -> Option<i64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    if date.len() != 10 || !(8..=18).contains(&time.len()) {
        return None;
    }
    let mut dates = date.split('-');
    let year: i64 = dates.next()?.parse().ok()?;
    let month: i64 = dates.next()?.parse().ok()?;
    let day: i64 = dates.next()?.parse().ok()?;
    let mut times = time.split(':');
    let hour: i64 = times.next()?.parse().ok()?;
    let minute: i64 = times.next()?.parse().ok()?;
    let second = times.next()?;
    let (sec, fraction) = second.split_once('.').unwrap_or((second, ""));
    let sec: i64 = sec.parse().ok()?;
    if !(1970..=9999).contains(&year)
        || fraction.len() > 9
        || dates.next().is_some()
        || times.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..24).contains(&hour)
        || !(0..60).contains(&minute)
        || !(0..60).contains(&sec)
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day > max_day {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let fraction = format!("{fraction:0<3}");
    let millis: i64 = fraction.get(..3)?.parse().ok()?;
    days.checked_mul(86400000)?
        .checked_add(hour * 3600000 + minute * 60000 + sec * 1000 + millis)
}
#[cfg(test)]
#[path = "tracker_tests.rs"]
mod tests;
