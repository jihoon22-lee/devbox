//! Pure MCP protocol and tool contracts; native hosts own all effects.
pub mod protocol;
pub mod tools;
pub use protocol::Server;
pub use tools::ToolCall;
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpSettings {
    pub enabled: bool,
    pub allow_note_capture: bool,
    pub allow_task_run: bool,
}
#[derive(Debug, Clone)]
pub struct HostError {
    pub code: String,
    pub message: String,
}
pub trait ToolHost {
    fn settings(&mut self) -> Result<McpSettings, HostError>;
    fn call(&mut self, tool: &ToolCall) -> Result<serde_json::Value, HostError>;
}
