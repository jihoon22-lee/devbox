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
