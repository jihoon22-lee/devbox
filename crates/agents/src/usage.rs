//! Bounded, metadata-only usage from the task's exact session.
use crate::{
    model::{AgentTask, AgentUsage, Tokens},
    profile::Hooks,
    tracker::Binding,
};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};
const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_LINE: usize = 4 * 1024 * 1024;
#[derive(Default, Deserialize)]
struct Counts {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default, alias = "cached_input_tokens")]
    cache_read_input_tokens: Option<u64>,
    #[serde(default, alias = "cache_write_input_tokens")]
    cache_creation_input_tokens: Option<u64>,
}
impl Counts {
    fn tokens(self) -> Option<Tokens> {
        if self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cache_read_input_tokens.is_none()
            && self.cache_creation_input_tokens.is_none()
        {
            return None;
        }
        Some(Tokens {
            input: self.input_tokens.unwrap_or(0),
            output: self.output_tokens.unwrap_or(0),
            cache_read: self.cache_read_input_tokens.unwrap_or(0),
            cache_write: self.cache_creation_input_tokens.unwrap_or(0),
        })
    }
}
#[derive(Deserialize)]
struct ClaudeRecord {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    cwd: Option<String>,
    message: Option<Box<serde_json::value::RawValue>>,
}
#[derive(Deserialize)]
struct ClaudeMessage {
    id: String,
    usage: Option<Counts>,
}
#[derive(Deserialize)]
struct CodexRecord {
    #[serde(rename = "type")]
    kind: String,
    payload: Box<serde_json::value::RawValue>,
}
#[derive(Deserialize)]
struct Session {
    id: String,
    cwd: String,
    source: Box<serde_json::value::RawValue>,
    thread_source: Option<Box<serde_json::value::RawValue>>,
}
#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    info: Option<Box<serde_json::value::RawValue>>,
}
#[derive(Deserialize)]
struct Info {
    total_token_usage: Option<Counts>,
}
fn empty(truncated: bool) -> AgentUsage {
    AgentUsage {
        tokens: None,
        truncated,
    }
}
// fill_buf bounds allocation even for a huge unterminated record. Malformed records
// and incomplete tails are reported, never silently advertised as complete usage.
fn records(mut reader: impl BufRead, mut visit: impl FnMut(&[u8]) -> bool) -> bool {
    let mut budget = MAX_BYTES;
    let mut line = Vec::new();
    let mut truncated = false;
    loop {
        let buffer = match reader.fill_buf() {
            Ok(b) => b,
            Err(_) => return true,
        };
        if buffer.is_empty() {
            if !line.is_empty() {
                truncated = true;
            }
            return truncated;
        }
        if budget == 0 {
            return true;
        }
        let end = buffer.iter().position(|b| *b == b'\n').map(|n| n + 1);
        let count = end.unwrap_or(buffer.len()).min(budget);
        if line.len().saturating_add(count) > MAX_LINE {
            return true;
        }
        line.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        budget -= count;
        if end == Some(count) {
            if !line.iter().all(u8::is_ascii_whitespace) && !visit(&line) {
                truncated = true;
            }
            line.clear();
        }
    }
}
fn claude_read(reader: impl BufRead, session: &str, cwd: &str) -> AgentUsage {
    let mut messages: HashMap<String, Tokens> = HashMap::new();
    let mut wrong_owner = false;
    let mut truncated = records(reader, |line| {
        let Ok(record) = serde_json::from_slice::<ClaudeRecord>(line) else {
            return false;
        };
        if record.kind != "assistant" {
            return true;
        }
        if record.session_id.as_deref() != Some(session) || record.cwd.as_deref() != Some(cwd) {
            wrong_owner = true;
            return true;
        }
        let Some(raw) = record.message else {
            return false;
        };
        let Ok(message) = serde_json::from_str::<ClaudeMessage>(raw.get()) else {
            return false;
        };
        if message.id.is_empty() {
            return false;
        }
        let Some(usage) = message.usage else {
            return true;
        };
        let Some(next) = usage.tokens() else {
            return true;
        };
        let prev = messages.entry(message.id).or_default();
        prev.input = prev.input.max(next.input);
        prev.output = prev.output.max(next.output);
        prev.cache_read = prev.cache_read.max(next.cache_read);
        prev.cache_write = prev.cache_write.max(next.cache_write);
        true
    });
    if wrong_owner {
        return empty(true);
    }
    let tokens = (!messages.is_empty()).then(|| {
        messages.values().fold(Tokens::default(), |mut sum, t| {
            for (target, value) in [
                (&mut sum.input, t.input),
                (&mut sum.output, t.output),
                (&mut sum.cache_read, t.cache_read),
                (&mut sum.cache_write, t.cache_write),
            ] {
                if let Some(total) = target.checked_add(value) {
                    *target = total;
                } else {
                    *target = u64::MAX;
                    truncated = true;
                }
            }
            sum
        })
    });
    AgentUsage { tokens, truncated }
}
fn codex_read(reader: impl BufRead, session: &str, cwd: &str) -> AgentUsage {
    let mut verified = false;
    let mut invalid = false;
    let mut last = None;
    let truncated = records(reader, |line| {
        let Ok(record) = serde_json::from_slice::<CodexRecord>(line) else {
            return false;
        };
        if record.kind == "session_meta" {
            let Ok(meta) = serde_json::from_str::<Session>(record.payload.get()) else {
                invalid = true;
                return false;
            };
            let source = serde_json::from_str::<String>(meta.source.get()).ok();
            let origin = meta
                .thread_source
                .and_then(|s| serde_json::from_str::<String>(s.get()).ok());
            if verified
                || meta.id != session
                || meta.cwd != cwd
                || source.as_deref() != Some("cli")
                || origin.as_deref() != Some("user")
            {
                invalid = true;
            } else {
                verified = true;
            }
        } else if record.kind == "event_msg" {
            let Ok(event) = serde_json::from_str::<Event>(record.payload.get()) else {
                return false;
            };
            if event.kind == "token_count" {
                if !verified {
                    invalid = true;
                }
                let info = match event.info {
                    Some(raw) => match serde_json::from_str::<Info>(raw.get()) {
                        Ok(i) => Some(i),
                        Err(_) => return false,
                    },
                    None => None,
                };
                if let Some(total) = info.and_then(|i| i.total_token_usage) {
                    if let Some(tokens) = total.tokens() {
                        last = Some(tokens);
                    }
                }
            }
        }
        true
    });
    if invalid || !verified {
        empty(true)
    } else {
        AgentUsage {
            tokens: last,
            truncated,
        }
    }
}
fn safe_file(path: &Path) -> bool {
    path.ancestors()
        .all(|p| std::fs::symlink_metadata(p).is_ok_and(|m| !m.file_type().is_symlink()))
}
fn claude_project_dir(home: &Path, cwd: &str) -> PathBuf {
    let name: String = cwd
        .chars()
        .map(|b| if b.is_ascii_alphanumeric() { b } else { '-' })
        .collect();
    home.join(".claude/projects").join(name)
}
// The CLI's directory escaping has changed across versions. Probe only the
// known session filename in immediate project directories, never newest files.
fn claude_lookup(home: &Path, session: &str, cwd: &str) -> AgentUsage {
    let root = home.join(".claude/projects");
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return empty(false),
        Err(_) => return empty(true),
    };
    if !safe_file(&root) {
        return empty(true);
    }
    let preferred = claude_project_dir(home, cwd).join(format!("{session}.jsonl"));
    let mut paths = Vec::new();
    let mut bytes = 0u64;
    for (index, entry) in entries.enumerate() {
        if index >= 2048 {
            return empty(true);
        }
        let Ok(entry) = entry else { return empty(true) };
        let Ok(kind) = entry.file_type() else {
            return empty(true);
        };
        if !kind.is_dir() {
            continue;
        }
        let path = entry.path().join(format!("{session}.jsonl"));
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return empty(true),
            Ok(meta) => {
                if !meta.is_file() || !safe_file(&path) {
                    return empty(true);
                }
                bytes = bytes.saturating_add(meta.len());
                if bytes > MAX_BYTES as u64 {
                    return empty(true);
                }
                paths.push(path);
            }
        }
    }
    paths.sort_by_key(|p| p != &preferred);
    let mut found = None;
    let mut truncated = false;
    for path in paths {
        let Ok(file) = File::open(path) else {
            return empty(true);
        };
        let report = claude_read(BufReader::new(file), session, cwd);
        truncated |= report.truncated;
        if report.tokens.is_some() {
            if found.is_some() {
                return empty(true);
            }
            found = report.tokens;
        }
    }
    AgentUsage {
        tokens: found,
        truncated,
    }
}
pub(crate) fn tokens_for(
    home: &Path,
    task: &AgentTask,
    hooks: Hooks,
    binding: Option<&Binding>,
) -> AgentUsage {
    let (path, session) = match hooks {
        Hooks::None => return empty(false),
        Hooks::Claude => {
            let Some(id) = task.session_id.as_deref().filter(|id| {
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            }) else {
                return empty(false);
            };
            return claude_lookup(home, id, &task.workdir);
        }
        Hooks::Codex => {
            let Some(binding) = binding else {
                return empty(false);
            };
            let (Some(id), Some(path)) = (binding.thread_id.as_deref(), binding.rollout.as_ref())
            else {
                return empty(false);
            };
            if task.session_id.as_deref().is_some_and(|s| s != id)
                || !path.starts_with(home.join(".codex/sessions"))
                || path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return empty(true);
            }
            (path.clone(), id)
        }
    };
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return empty(false),
        Err(_) => return empty(true),
        Ok(m) if !m.is_file() => return empty(true),
        Ok(_) => {}
    }
    if !safe_file(&path) {
        return empty(true);
    }
    let file = match File::open(&path) {
        Ok(f) => f,
        Err(_) => return empty(true),
    };
    match hooks {
        Hooks::Claude => claude_read(BufReader::new(file), session, &task.workdir),
        Hooks::Codex => codex_read(BufReader::new(file), session, &task.workdir),
        Hooks::None => empty(false),
    }
}
#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
