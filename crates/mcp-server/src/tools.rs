use crate::McpSettings;
use serde_json::{json, Map, Value};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCall {
    Projects,
    Tasks {
        root: Option<String>,
    },
    Runs {
        job_id: Option<String>,
        limit: u32,
    },
    RunLog {
        run_id: String,
        stream: LogStream,
        max_bytes: u32,
    },
    Search {
        query: String,
        limit: u32,
    },
    NoteRead {
        path: String,
    },
    NoteCapture {
        title: String,
        body: String,
    },
    TaskRun {
        job_id: String,
    },
}
fn fields(args: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    if args.keys().any(|key| !allowed.contains(&key.as_str())) {
        Err("arguments contains an unknown field".into())
    } else {
        Ok(())
    }
}
fn text(args: &Map<String, Value>, key: &str, max: usize) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && value.chars().count() <= max)
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} must be a nonempty string of at most {max} characters"))
}
fn optional_text(
    args: &Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<String>, String> {
    args.contains_key(key)
        .then(|| text(args, key, max))
        .transpose()
}
fn integer(args: &Map<String, Value>, key: &str, default: u32, max: u32) -> Result<u32, String> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=u64::from(max)).contains(n))
            .map(|n| n as u32)
            .ok_or_else(|| format!("{key} must be an integer from 1 to {max}")),
    }
}
fn absolute_root(value: &str) -> bool {
    !value.chars().any(char::is_control)
        && !value
            .split(['/', '\\'])
            .any(|part| matches!(part, "." | ".."))
        && (value.starts_with('/')
            || (value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
                && value.as_bytes().get(1) == Some(&b':')
                && matches!(value.as_bytes().get(2), Some(b'/' | b'\\')))
            || (value.starts_with("\\\\")
                && value.split('\\').filter(|part| !part.is_empty()).count() >= 2))
}
impl ToolCall {
    pub fn parse(name: &str, arguments: &Value) -> Result<Self, String> {
        let args = arguments.as_object().ok_or("arguments must be an object")?;
        match name {
            "devbox_projects" => {
                fields(args, &[])?;
                Ok(Self::Projects)
            }
            "devbox_tasks" => {
                fields(args, &["root"])?;
                let root = optional_text(args, "root", 4096)?;
                if root.as_deref().is_some_and(|root| !absolute_root(root)) {
                    return Err("root must be an absolute path without parent traversal".into());
                }
                Ok(Self::Tasks { root })
            }
            "devbox_runs" => {
                fields(args, &["jobId", "limit"])?;
                Ok(Self::Runs {
                    job_id: optional_text(args, "jobId", 256)?,
                    limit: integer(args, "limit", 20, 50)?,
                })
            }
            "devbox_run_log" => {
                fields(args, &["runId", "stream", "maxBytes"])?;
                let stream = match args.get("stream") {
                    None => LogStream::Stdout,
                    Some(Value::String(value)) if value == "stdout" => LogStream::Stdout,
                    Some(Value::String(value)) if value == "stderr" => LogStream::Stderr,
                    _ => return Err("stream must be stdout or stderr".into()),
                };
                Ok(Self::RunLog {
                    run_id: text(args, "runId", 256)?,
                    stream,
                    max_bytes: integer(args, "maxBytes", 16384, 65536)?,
                })
            }
            "devbox_search" => {
                fields(args, &["query", "limit"])?;
                Ok(Self::Search {
                    query: text(args, "query", 256)?,
                    limit: integer(args, "limit", 10, 20)?,
                })
            }
            "devbox_note_read" => {
                fields(args, &["path"])?;
                Ok(Self::NoteRead {
                    path: text(args, "path", 1024)?,
                })
            }
            "devbox_note_capture" => {
                fields(args, &["title", "body"])?;
                let body = args
                    .get("body")
                    .and_then(Value::as_str)
                    .filter(|body| body.len() <= 65536)
                    .ok_or("body must be a string of at most 65536 UTF-8 bytes")?;
                Ok(Self::NoteCapture {
                    title: text(args, "title", 120)?,
                    body: body.into(),
                })
            }
            "devbox_task_run" => {
                fields(args, &["jobId"])?;
                Ok(Self::TaskRun {
                    job_id: text(args, "jobId", 256)?,
                })
            }
            _ => Err("Unknown tool name".into()),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Projects => "devbox_projects",
            Self::Tasks { .. } => "devbox_tasks",
            Self::Runs { .. } => "devbox_runs",
            Self::RunLog { .. } => "devbox_run_log",
            Self::Search { .. } => "devbox_search",
            Self::NoteRead { .. } => "devbox_note_read",
            Self::NoteCapture { .. } => "devbox_note_capture",
            Self::TaskRun { .. } => "devbox_task_run",
        }
    }
    pub fn is_write(&self) -> bool {
        matches!(self, Self::NoteCapture { .. } | Self::TaskRun { .. })
    }
    pub fn allowed(&self, settings: &McpSettings) -> bool {
        settings.enabled
            && match self {
                Self::NoteCapture { .. } => settings.allow_note_capture,
                Self::TaskRun { .. } => settings.allow_task_run,
                _ => true,
            }
    }
}
fn string(max: u32) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn tool(name: &str, description: &str, properties: Value, required: &[&str], write: bool) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":!write,"destructiveHint":write,"idempotentHint":!write,"openWorldHint":true}})
}
pub fn catalog(settings: &McpSettings) -> Vec<Value> {
    if !settings.enabled {
        return vec![];
    }
    let mut tools = vec![
        tool("devbox_projects", "List the user's registered Devbox projects and worktrees (up to 50).", json!({}), &[], false),
        tool("devbox_tasks", "List Devbox tasks, optionally restricted to an absolute project root and its children.", json!({"root":string(4096)}), &[], false),
        tool("devbox_runs", "List recent task runs; use jobId to filter a particular task.", json!({"jobId":string(256),"limit":{"type":"integer","minimum":1,"maximum":50,"default":20}}), &[], false),
        tool("devbox_run_log", "Read the tail of a task run's stdout or stderr. Use devbox_runs to find run ids.", json!({"runId":string(256),"stream":{"type":"string","enum":["stdout","stderr"],"default":"stdout"},"maxBytes":{"type":"integer","minimum":1,"maximum":65536,"default":16384}}), &["runId"], false),
        tool("devbox_search", "Search the user's indexed Devbox notes and content before reading individual notes.", json!({"query":string(256),"limit":{"type":"integer","minimum":1,"maximum":20,"default":10}}), &["query"], false),
        tool("devbox_note_read", "Read a Markdown note relative to the current vault, limited to 256 KiB.", json!({"path":string(1024)}), &["path"], false),
    ];
    if settings.allow_note_capture {
        tools.push(tool("devbox_note_capture", "Create a new Inbox Markdown note without overwriting existing notes. Body is limited to 65536 UTF-8 bytes.", json!({"title":string(120),"body":{"type":"string","maxLength":65536}}), &["title","body"], true));
    }
    if settings.allow_task_run {
        tools.push(tool("devbox_task_run", "Start a Devbox task that the user has already trusted in Workspace. Returns the run operation.", json!({"jobId":string(256)}), &["jobId"], true));
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn independent_write_flags_and_byte_limits_are_enforced() {
        let note =
            ToolCall::parse("devbox_note_capture", &json!({"title":"Note","body":""})).unwrap();
        let task = ToolCall::parse("devbox_task_run", &json!({"jobId":"job"})).unwrap();
        let settings = McpSettings {
            enabled: true,
            allow_note_capture: true,
            allow_task_run: false,
        };
        assert!(note.allowed(&settings));
        assert!(!task.allowed(&settings));
        assert!(!note.allowed(&McpSettings {
            enabled: false,
            ..settings
        }));
        assert!(ToolCall::parse(
            "devbox_note_capture",
            &json!({"title":"t","body":"가".repeat(22_000)})
        )
        .is_err());
        for limit in [json!(0), json!(-1), json!(1.5), json!(null), json!("1")] {
            assert!(ToolCall::parse("devbox_runs", &json!({"limit":limit})).is_err());
        }
        for root in [
            "/home/fixture",
            r"C:\Work\fixture",
            r"\\server\share\fixture",
        ] {
            assert!(
                ToolCall::parse("devbox_tasks", &json!({"root":root})).is_ok(),
                "{root}"
            );
        }
        assert!(ToolCall::parse("devbox_tasks", &json!({"root":"/home/../other"})).is_err());
    }

    #[test]
    fn arguments_get_defaults_and_bounds() {
        assert_eq!(
            ToolCall::parse("devbox_runs", &json!({})).unwrap(),
            ToolCall::Runs {
                job_id: None,
                limit: 20
            }
        );
        assert!(ToolCall::parse("devbox_runs", &json!({"limit": 51})).is_err());
        assert_eq!(
            ToolCall::parse("devbox_run_log", &json!({"runId": "r1"})).unwrap(),
            ToolCall::RunLog {
                run_id: "r1".into(),
                stream: LogStream::Stdout,
                max_bytes: 16_384
            }
        );
        assert!(ToolCall::parse("devbox_search", &json!({"query": ""})).is_err());
        assert!(ToolCall::parse("devbox_search", &json!({"query": "x", "extra": true})).is_err());
        assert!(ToolCall::parse(
            "devbox_note_capture",
            &json!({"title": "t", "body": "x".repeat(65_537)})
        )
        .is_err());
        assert!(ToolCall::parse("devbox_tasks", &json!({"root": "relative"})).is_err());
    }

    #[test]
    fn catalog_schemas_are_objects_and_hide_disallowed_writes() {
        let all = catalog(&McpSettings {
            enabled: true,
            allow_note_capture: true,
            allow_task_run: true,
        });
        assert_eq!(all.len(), 8);
        assert!(all
            .iter()
            .all(|tool| tool["inputSchema"]["type"] == "object"
                && tool["description"].as_str().is_some_and(|d| !d.is_empty())));
        let read_only = catalog(&McpSettings {
            enabled: true,
            ..McpSettings::default()
        });
        assert_eq!(read_only.len(), 6);
        assert!(ToolCall::parse("devbox_task_run", &json!({"jobId": "j"}))
            .unwrap()
            .is_write());
    }
}
