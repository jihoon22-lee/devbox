//! Fixed agent settings calls. Renderer hosts expose only their own typed adapters.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, Serialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AgentSettingsCall {
    AutostartStatus {},
    SetAutostart { enabled: bool },
}
impl AgentSettingsCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::AutostartStatus {} => "autostart_status",
            Self::SetAutostart { .. } => "set_autostart",
        }
    }
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentAutostartStatus {
    pub supported: bool,
    pub enabled: bool,
}

/// MCP grants are distinct from the two products' shared login preference.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMcpSettings {
    pub enabled: bool,
    pub allow_note_capture: bool,
    pub allow_task_run: bool,
}
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentMcpStatus {
    pub settings: AgentMcpSettings,
    pub launcher_path: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum McpSettingsCall {
    McpSettings {},
    SetMcpSettings { settings: AgentMcpSettings },
}
impl McpSettingsCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::McpSettings {} => "mcp_settings",
            Self::SetMcpSettings { .. } => "set_mcp_settings",
        }
    }
}

#[cfg(test)]
mod mcp_tests {
    use super::*;
    #[test]
    fn mcp_settings_have_closed_inputs_and_default_off_permissions() {
        assert_eq!(
            serde_json::to_value(AgentMcpSettings::default()).unwrap(),
            serde_json::json!({"enabled":false,"allowNoteCapture":false,"allowTaskRun":false})
        );
        let call: McpSettingsCall = serde_json::from_value(serde_json::json!({"method":"set_mcp_settings","args":{"settings":{"enabled":true,"allowNoteCapture":false,"allowTaskRun":false}}})).unwrap();
        assert_eq!(call.method(), "set_mcp_settings");
        assert!(serde_json::from_value::<McpSettingsCall>(
            serde_json::json!({"method":"mcp_settings","args":{"root":"foreign"}})
        )
        .is_err());
    }
}
