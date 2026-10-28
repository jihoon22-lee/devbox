//! pi's token usage, read from the task's own session file and nothing else.
use crate::{
    model::{AgentTask, AgentUsage, Tokens},
    usage::{empty, records, safe_file},
};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{
    ffi::OsStr,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// How long one read may take, the folder listing included.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// How many entries of the session folder are looked at.
const MAX_ENTRIES: usize = 4096;

/// The first line of a session file.
#[derive(Deserialize)]
struct Header {
    #[serde(rename = "type")]
    kind: String,
    id: String,
    cwd: String,
}

/// What usage reads of the other lines; everything else in them is ignored.
#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: String,
    usage: Option<Box<RawValue>>,
    message: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
struct Message {
    role: String,
    usage: Option<Box<RawValue>>,
}

/// pi 1.3.0 counts reasoning in the output already, and its cost is not summed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

/// A path variable the way pi reads it: `~` is the home folder, and a relative path is one in
/// pi's working folder, the task's.
fn expanded(home: &Path, workdir: &str, value: &OsStr) -> PathBuf {
    let path = Path::new(value);
    if path == Path::new("~") {
        return home.to_path_buf();
    }
    match path.strip_prefix("~") {
        Ok(rest) => home.join(rest),
        Err(_) => Path::new(workdir).join(path),
    }
}

/// The folder pi 1.3.0 keeps `workdir`'s sessions in: `PI_CODING_AGENT_SESSION_DIR`, which holds
/// every project's sessions, or else `sessions/--<workdir>--` in `PI_CODING_AGENT_DIR` or in
/// `~/.pi/agent`. pi reads an empty variable as unset.
pub(crate) fn pi_session_dir(
    home: &Path,
    agent_dir: Option<&OsStr>,
    session_dir: Option<&OsStr>,
    workdir: &str,
) -> PathBuf {
    if let Some(dir) = session_dir.filter(|dir| !dir.is_empty()) {
        return expanded(home, workdir, dir);
    }
    let agent = match agent_dir.filter(|dir| !dir.is_empty()) {
        Some(dir) => expanded(home, workdir, dir),
        None => home.join(".pi/agent"),
    };
    let project = workdir
        .strip_prefix(['/', '\\'])
        .unwrap_or(workdir)
        .replace(['/', '\\', ':'], "-");
    agent.join("sessions").join(format!("--{project}--"))
}

/// An id the way pi accepts them, so it can only name a file in the session folder.
pub(crate) fn pi_session_id(id: &str) -> bool {
    let edge = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    id.len() <= 128
        && edge(id.chars().next())
        && edge(id.chars().last())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The usage of the task's pi session, from its own file in the folder pi keeps the task's
/// sessions in. `agent_dir` and `session_dir` are pi's variables, as the task's panes get them.
pub(crate) fn pi_usage(
    home: &Path,
    agent_dir: Option<&OsStr>,
    session_dir: Option<&OsStr>,
    task: &AgentTask,
) -> AgentUsage {
    let Some(session) = task.session_id.as_deref().filter(|id| pi_session_id(id)) else {
        return empty(false);
    };
    let dir = pi_session_dir(home, agent_dir, session_dir, &task.workdir);
    usage_in(&dir, session, &task.workdir, Instant::now() + READ_TIMEOUT)
}

/// Whether pi has written the task's session file yet: `Some(false)` only when its folder shows
/// none, and `None` when the folder cannot be read.
pub(crate) fn pi_session_written(
    home: &Path,
    agent_dir: Option<&OsStr>,
    session_dir: Option<&OsStr>,
    task: &AgentTask,
) -> Option<bool> {
    let session = task.session_id.as_deref().filter(|id| pi_session_id(id))?;
    let dir = pi_session_dir(home, agent_dir, session_dir, &task.workdir);
    match session_file(&dir, session, Instant::now() + READ_TIMEOUT) {
        SessionFile::Missing => Some(false),
        SessionFile::Found(_) => Some(true),
        SessionFile::Unreadable => None,
    }
}

/// What the session folder holds for one session.
enum SessionFile {
    /// No folder yet, or no file for the session in it.
    Missing,
    Found(PathBuf),
    /// The folder or the file could not be read safely, or two files name the session.
    Unreadable,
}

/// Finds `<dir>/<timestamp>_<session>.jsonl`, the one name pi gives the session. No other file is
/// looked at, so another session, or another project's, is never read.
fn session_file(dir: &Path, session: &str, until: Instant) -> SessionFile {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return SessionFile::Missing,
        Err(_) => return SessionFile::Unreadable,
    };
    if !safe_file(dir) {
        return SessionFile::Unreadable;
    }
    let name = format!("{session}.jsonl");
    let mut found = None;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_ENTRIES || Instant::now() >= until {
            return SessionFile::Unreadable;
        }
        let Ok(entry) = entry else {
            return SessionFile::Unreadable;
        };
        // pi's timestamp has no `_`, and the id follows the first one.
        let file_name = entry.file_name();
        let named = file_name
            .to_str()
            .and_then(|n| n.split_once('_'))
            .is_some_and(|(_, rest)| rest == name);
        if !named {
            continue;
        }
        if found.is_some() {
            return SessionFile::Unreadable;
        }
        found = Some(entry.path());
    }
    let Some(path) = found else {
        return SessionFile::Missing;
    };
    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) || !safe_file(&path) {
        return SessionFile::Unreadable;
    }
    SessionFile::Found(path)
}

/// The usage in the session's own file in `dir`.
fn usage_in(dir: &Path, session: &str, cwd: &str, until: Instant) -> AgentUsage {
    match session_file(dir, session, until) {
        SessionFile::Missing => empty(false),
        SessionFile::Unreadable => empty(true),
        SessionFile::Found(path) => match File::open(&path) {
            Ok(file) => pi_read(BufReader::new(file), session, cwd, until),
            Err(_) => empty(true),
        },
    }
}

/// A reader that fails once `until` has passed, which `records` reports as truncated.
struct Deadline<R> {
    inner: R,
    until: Instant,
}

impl<R> Deadline<R> {
    fn check(&self) -> std::io::Result<()> {
        if Instant::now() >= self.until {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        Ok(())
    }
}

impl<R: Read> Read for Deadline<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.check()?;
        self.inner.read(buf)
    }
}

impl<R: BufRead> BufRead for Deadline<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.check()?;
        self.inner.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.inner.consume(amount);
    }
}

/// The usage pi bills in a session file: assistant answers, tool results that carry usage,
/// `usage` entries, and the summaries of compactions and branches. A file that is not this
/// session's, or a billed entry that cannot be read, is reported rather than read as complete.
fn pi_read(reader: impl BufRead, session: &str, cwd: &str, until: Instant) -> AgentUsage {
    let mut header = None;
    let mut sum: Option<Tokens> = None;
    let mut overflow = false;
    let truncated = records(
        Deadline {
            inner: reader,
            until,
        },
        |line| {
            let Some(owned) = header else {
                let ours = serde_json::from_slice::<Header>(line)
                    .is_ok_and(|h| h.kind == "session" && h.id == session && h.cwd == cwd);
                header = Some(ours);
                return true;
            };
            if !owned {
                return true;
            }
            let Ok(entry) = serde_json::from_slice::<Entry>(line) else {
                return false;
            };
            let usage = match entry.kind.as_str() {
                "usage" => match entry.usage {
                    Some(usage) => usage,
                    None => return false,
                },
                "compaction" | "branch_summary" => match entry.usage {
                    Some(usage) => usage,
                    None => return true,
                },
                "message" => {
                    let Some(message) = entry
                        .message
                        .and_then(|raw| serde_json::from_str::<Message>(raw.get()).ok())
                    else {
                        return false;
                    };
                    match (message.role.as_str(), message.usage) {
                        ("assistant" | "toolResult", Some(usage)) => usage,
                        ("assistant", None) => return false,
                        _ => return true,
                    }
                }
                _ => return true,
            };
            let Ok(usage) = serde_json::from_str::<Usage>(usage.get()) else {
                return false;
            };
            let total = sum.get_or_insert_with(Tokens::default);
            for (target, value) in [
                (&mut total.input, usage.input),
                (&mut total.output, usage.output),
                (&mut total.cache_read, usage.cache_read),
                (&mut total.cache_write, usage.cache_write),
            ] {
                *target = target.checked_add(value).unwrap_or_else(|| {
                    overflow = true;
                    u64::MAX
                });
            }
            true
        },
    );
    if header != Some(true) {
        return empty(true);
    }
    AgentUsage {
        tokens: sum,
        truncated: truncated || overflow,
    }
}

#[cfg(test)]
#[path = "usage_pi_tests.rs"]
mod tests;
