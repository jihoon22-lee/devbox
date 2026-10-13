pub use devbox_core::config::TerminalProfile;
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    Shell,
    Agent,
}
devbox_core::sql_text_enum!(SessionKind { Shell = "shell", Agent = "agent" });
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSession {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub cwd: String,
    pub kind: SessionKind,
    pub alive: bool,
    pub command: String,
    pub created_ms: i64,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SessionList {
    pub rev: String,
    pub items: Vec<TerminalSession>,
}
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ListSessions {
    pub project_id: Option<String>,
}
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateSession {
    pub project_id: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub command: Option<String>,
    pub attach_zellij: Option<String>,
}
#[derive(Debug, Deserialize, Serialize, TS)]
pub struct RenameSession {
    pub id: String,
    pub title: String,
}
#[derive(Debug, Deserialize, Serialize, TS)]
pub struct CloseSession {
    pub id: String,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ZellijSessions {
    pub items: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TerminalProfiles {
    pub items: Vec<TerminalProfile>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct TerminalChanged {
    pub rev: String,
}
domain_error! {
    pub enum TerminalError {
        NotFound { id: String } = "terminal.not_found",
        ProjectNotFound { id: String } = "terminal.project_not_found",
        CwdInvalid { path: String } = "terminal.cwd_invalid",
        Tmux { reason: String } = "terminal.tmux_failed",
    }
}
