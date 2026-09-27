use mcp_server::{tools::LogStream, HostError, ToolCall};
#[cfg(windows)]
use mcp_server::{McpSettings, ToolHost};
use serde_json::{json, Value};
#[cfg(windows)]
pub struct AgentToolHost {
    root: std::path::PathBuf,
    root_identity: devbox_filesystem::FilesystemIdentity,
    session: String,
    runtime: tokio::runtime::Runtime,
}
pub fn host_error(code: &str) -> HostError {
    let message = match code {
        "note_missing" => "Note not found.",
        "note_unavailable" => "Could not read or write the selected note.",
        "note_capture_limit" => "Too many notes share this title and timestamp.",
        "note_path_invalid" => "Choose a Markdown path inside the current notes folder.",
        "vault_unavailable" => "Open Devbox Knowledge once to choose a notes folder.",
        "search_unavailable" => "Open Devbox Knowledge once to set up search.",
        "task_not_trusted" => "Review and trust this task in Devbox Workspace before running it.",
        "task_missing" => "Task not found. Refresh the task list.",
        "task_execution_failed" => {
            "The task could not be started. Check its state in Devbox Workspace."
        }
        "tool_not_allowed" => "Enable this tool in Devbox Control Center > Settings > MCP.",
        "mcp_disabled" => "Turn on Devbox MCP in Devbox Control Center > Settings > MCP.",
        _ => "Devbox agent is not available. Open Control Center and check the installation.",
    };
    HostError {
        code: if matches!(
            code,
            "note_unavailable"
                | "note_capture_limit"
                | "note_missing"
                | "note_path_invalid"
                | "vault_unavailable"
                | "search_unavailable"
                | "task_not_trusted"
                | "task_missing"
                | "task_execution_failed"
                | "tool_not_allowed"
                | "mcp_disabled"
        ) {
            code
        } else {
            "agent_unavailable"
        }
        .into(),
        message: message.into(),
    }
}
pub fn arguments(tool: &ToolCall) -> Value {
    match tool {
        ToolCall::Projects => json!({}),
        ToolCall::Tasks { root } => match root {
            Some(root) => json!({"root":root}),
            None => json!({}),
        },
        ToolCall::Runs { job_id, limit } => match job_id {
            Some(job_id) => json!({"jobId":job_id,"limit":limit}),
            None => json!({"limit":limit}),
        },
        ToolCall::RunLog {
            run_id,
            stream,
            max_bytes,
        } => {
            json!({"runId":run_id,"stream":match stream { LogStream::Stdout => "stdout", LogStream::Stderr => "stderr" },"maxBytes":max_bytes})
        }
        ToolCall::Search { query, limit } => json!({"query":query,"limit":limit}),
        ToolCall::NoteRead { path } => json!({"path":path}),
        ToolCall::NoteCapture { title, body } => json!({"title":title,"body":body}),
        ToolCall::TaskRun { job_id } => json!({"jobId":job_id}),
    }
}
#[cfg(windows)]
impl AgentToolHost {
    pub fn new(root: std::path::PathBuf) -> Result<Self, HostError> {
        let root_identity = devbox_filesystem::filesystem_identity(&root, true)
            .map_err(|_| host_error("agent_unavailable"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| host_error("agent_unavailable"))?;
        Ok(Self {
            root,
            root_identity,
            session: uuid::Uuid::new_v4().to_string(),
            runtime,
        })
    }
    fn invoke(&self, method: &str, args: Value) -> Result<Value, HostError> {
        if devbox_filesystem::filesystem_identity(&self.root, true).ok() != Some(self.root_identity)
        {
            return Err(host_error("agent_unavailable"));
        }
        let response = self.runtime.block_on(async {
            let (client, installation, generation) = suite_runtime::mcp_client(&self.root).map_err(|_| host_error("agent_unavailable"))?;
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| host_error("agent_unavailable"))?.as_millis() as u64;
            let result = client.call("agent.mcp", json!({"header":{"protocolVersion":1,"installationId":installation,"sessionId":self.session,"requestId":uuid::Uuid::new_v4().to_string(),"deadlineMs":now + 29_000,"route":"mcp","context":null},"generation":generation,"method":method,"args":args})).await;
            client.disconnect().await;
            drop(client);
            result.map_err(|_| host_error("agent_unavailable"))
        })?;
        if response
            .pointer("/operation/outcome/state")
            .and_then(Value::as_str)
            != Some("succeeded")
        {
            return Err(host_error(
                response
                    .pointer("/value/issue")
                    .and_then(Value::as_str)
                    .unwrap_or("agent_unavailable"),
            ));
        }
        response
            .get("value")
            .cloned()
            .ok_or_else(|| host_error("agent_unavailable"))
    }
}
#[cfg(windows)]
impl ToolHost for AgentToolHost {
    fn settings(&mut self) -> Result<McpSettings, HostError> {
        serde_json::from_value(self.invoke("settings", json!({}))?)
            .map_err(|_| host_error("agent_unavailable"))
    }
    fn call(&mut self, tool: &ToolCall) -> Result<Value, HostError> {
        self.invoke(tool.name().trim_start_matches("devbox_"), arguments(tool))
    }
}
