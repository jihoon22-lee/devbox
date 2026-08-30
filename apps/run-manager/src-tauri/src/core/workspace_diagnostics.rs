//! Bounded projection of explicit workspace-task problem matchers.

use crate::core::workspace_tasks::WorkspaceProblemMatcher;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const MAX_DIAGNOSTICS: usize = 500;
pub const MAX_DIAGNOSTIC_LINES_PER_STREAM: usize = 50_000;
const MAX_LOG_LINE_BYTES: usize = 16 * 1024;
const MAX_FILE_BYTES: usize = 1_024;
const MAX_MESSAGE_CHARS: usize = 1_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskDiagnostic {
    pub index: u32,
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    pub message: String,
    pub severity: Option<String>,
    pub stream: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTaskDiagnostics {
    pub run_id: String,
    pub items: Vec<WorkspaceTaskDiagnostic>,
    pub truncated: bool,
}

pub fn match_workspace_diagnostics(
    run_id: &str,
    matcher: &WorkspaceProblemMatcher,
    streams: &[(&str, &[u8], bool)],
) -> WorkspaceTaskDiagnostics {
    let Ok(regexp) = regex::Regex::new(&matcher.regexp) else {
        return WorkspaceTaskDiagnostics {
            run_id: run_id.to_owned(),
            items: Vec::new(),
            truncated: true,
        };
    };
    let mut items = Vec::new();
    let mut truncated = false;
    for (stream, bytes, stream_truncated) in streams {
        truncated |= *stream_truncated;
        let mut line_count = 0usize;
        for raw_line in bytes.split(|byte| *byte == b'\n') {
            line_count = line_count.saturating_add(1);
            if line_count > MAX_DIAGNOSTIC_LINES_PER_STREAM {
                truncated = true;
                break;
            }
            if raw_line.len() > MAX_LOG_LINE_BYTES {
                truncated = true;
                continue;
            }
            let raw_line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
            let text = String::from_utf8_lossy(raw_line);
            let Some(captures) = regexp.captures(&text) else {
                continue;
            };
            let Some(file) = capture(&captures, matcher.file).and_then(normalize_relative_file)
            else {
                continue;
            };
            let Some(line) = capture(&captures, matcher.line).and_then(parse_position) else {
                continue;
            };
            let Some(message) = capture(&captures, matcher.message).and_then(sanitize_message)
            else {
                continue;
            };
            let column = matcher
                .column
                .and_then(|index| capture(&captures, index))
                .and_then(parse_position);
            let severity = matcher
                .severity
                .and_then(|index| capture(&captures, index))
                .and_then(normalize_severity);
            let Ok(index) = u32::try_from(items.len()) else {
                truncated = true;
                break;
            };
            items.push(WorkspaceTaskDiagnostic {
                index,
                file,
                line,
                column,
                message,
                severity,
                stream: (*stream).to_owned(),
            });
            if items.len() >= MAX_DIAGNOSTICS {
                truncated = true;
                break;
            }
        }
        if items.len() >= MAX_DIAGNOSTICS {
            break;
        }
    }
    WorkspaceTaskDiagnostics {
        run_id: run_id.to_owned(),
        items,
        truncated,
    }
}

pub fn resolve_workspace_diagnostic_path(root: &str, file: &str) -> Result<PathBuf, &'static str> {
    let relative = normalize_relative_file(file).ok_or("workspace-task-diagnostic-path-invalid")?;
    let root = Path::new(root)
        .canonicalize()
        .map_err(|_| "workspace-task-diagnostic-path-unavailable")?;
    if !root.is_dir() {
        return Err("workspace-task-diagnostic-path-unavailable");
    }
    let candidate = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    devbox_filesystem::ensure_no_links(&candidate)
        .map_err(|_| "workspace-task-diagnostic-path-unsafe")?;
    let candidate = candidate
        .canonicalize()
        .map_err(|_| "workspace-task-diagnostic-path-unavailable")?;
    if !candidate.starts_with(&root) || !candidate.is_file() {
        return Err("workspace-task-diagnostic-path-unsafe");
    }
    Ok(candidate)
}

fn capture<'a>(captures: &'a regex::Captures<'a>, index: u32) -> Option<&'a str> {
    captures
        .get(usize::try_from(index).ok()?)
        .map(|value| value.as_str())
}

fn parse_position(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok().filter(|value| *value > 0)
}

fn normalize_relative_file(value: &str) -> Option<String> {
    if value.is_empty()
        || value.len() > MAX_FILE_BYTES
        || value.starts_with('/')
        || value.starts_with('\\')
        || value.contains(':')
        || value.chars().any(char::is_control)
    {
        return None;
    }
    let normalized = value.replace('\\', "/");
    let segments = normalized.split('/').collect::<Vec<_>>();
    if segments.is_empty()
        || segments
            .iter()
            .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return None;
    }
    Some(segments.join("/"))
}

fn sanitize_message(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.chars().take(MAX_MESSAGE_CHARS).collect())
}

fn normalize_severity(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Some("error".to_owned()),
        "warning" | "warn" => Some("warning".to_owned()),
        "info" | "information" => Some("info".to_owned()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher() -> WorkspaceProblemMatcher {
        WorkspaceProblemMatcher {
            regexp: r"^(.+):(\d+):(\d+): (error|warning): (.+)$".to_owned(),
            file: 1,
            line: 2,
            column: Some(3),
            severity: Some(4),
            message: 5,
        }
    }

    #[test]
    fn projects_only_safe_relative_one_based_diagnostics() {
        let bytes = b"src/main.rs:12:3: warning: unused\n../secret:1:1: error: nope\nC:\\x:2:1: error: nope\n";
        let result = match_workspace_diagnostics("run", &matcher(), &[("stderr", bytes, false)]);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].file, "src/main.rs");
        assert_eq!(result.items[0].line, 12);
        assert_eq!(result.items[0].column, Some(3));
        assert_eq!(result.items[0].severity.as_deref(), Some("warning"));
    }

    #[test]
    fn output_count_and_stream_truncation_are_explicit() {
        let input = "a.rs:1:1: error: x\n".repeat(MAX_DIAGNOSTICS + 1);
        let result =
            match_workspace_diagnostics("run", &matcher(), &[("stdout", input.as_bytes(), true)]);
        assert_eq!(result.items.len(), MAX_DIAGNOSTICS);
        assert!(result.truncated);
    }

    #[test]
    fn control_characters_are_not_projected_into_diagnostic_messages() {
        let input = b"src/main.rs:1:1: error: unsafe\tterminal text\n";
        let result = match_workspace_diagnostics("run", &matcher(), &[("stderr", input, false)]);
        assert!(result.items.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn resolution_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        symlink(outside.path(), root.path().join("linked.rs")).unwrap();
        assert_eq!(
            resolve_workspace_diagnostic_path(root.path().to_str().unwrap(), "linked.rs"),
            Err("workspace-task-diagnostic-path-unsafe")
        );
    }
}
