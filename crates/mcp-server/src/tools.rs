use crate::McpSettings;
use serde_json::{json, Value};
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCall {
    Projects,
    NoteRead { path: String },
    TaskRun { job_id: String },
}
impl ToolCall {
    pub fn parse(name: &str, arguments: &Value) -> Result<Self, String> {
        let args = arguments.as_object().ok_or("arguments must be an object")?;
        let text = |key: &str| {
            args.get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must be a nonempty string"))
        };
        match name {
            "devbox_projects" if args.is_empty() => Ok(Self::Projects),
            "devbox_note_read" if args.len() == 1 => Ok(Self::NoteRead {
                path: text("path")?,
            }),
            "devbox_task_run" if args.len() == 1 => Ok(Self::TaskRun {
                job_id: text("jobId")?,
            }),
            _ => Err("Unknown tool name or arguments".into()),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Projects => "devbox_projects",
            Self::NoteRead { .. } => "devbox_note_read",
            Self::TaskRun { .. } => "devbox_task_run",
        }
    }
    pub fn is_write(&self) -> bool {
        matches!(self, Self::TaskRun { .. })
    }
    pub fn allowed(&self, settings: &McpSettings) -> bool {
        settings.enabled && (!self.is_write() || settings.allow_task_run)
    }
}
pub fn catalog(settings: &McpSettings) -> Vec<Value> {
    if !settings.enabled {
        return vec![];
    }
    let mut tools = vec![
        json!({"name":"devbox_projects","description":"List the user's registered Devbox projects.","inputSchema":{"type":"object","properties":{},"additionalProperties":false}}),
        json!({"name":"devbox_note_read","description":"Read a Markdown note relative to the current vault.","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}}),
    ];
    if settings.allow_task_run {
        tools.push(json!({"name":"devbox_task_run","description":"Start a task already trusted in Workspace.","inputSchema":{"type":"object","properties":{"jobId":{"type":"string"}},"required":["jobId"],"additionalProperties":false}}));
    }
    tools
}
