pub use devbox_core::config::TerminalProfile;
use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Maximum Unicode scalar values stored in a terminal title.
pub const MAX_TITLE_CHARS: usize = 256;

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
#[serde(rename_all = "camelCase")]
pub struct SessionList {
    pub rev: String,
    pub items: Vec<TerminalSession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_after_id: Option<String>,
}
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ListSessions {
    pub project_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub after_id: Option<String>,
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
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AttachSession {
    pub id: String,
    pub cols: u16,
    pub rows: u16,
    pub read_only: bool,
}
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAttached {
    pub stream_id: u32,
    pub cols: u16,
    pub rows: u16,
}
#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResizeStream {
    pub stream_id: u32,
    pub cols: u16,
    pub rows: u16,
}
domain_error! {
    pub enum TerminalError {
        CommandTooLong { limit_bytes: u32 } = "terminal.command_too_long",
        CommandInvalid = "terminal.command_invalid",
        InvalidCursor = "terminal.invalid_cursor",
        ListChanged = "terminal.list_changed",
        TitleTooLong { limit: u16 } = "terminal.title_too_long",
        InvalidSize { cols: u16, rows: u16 } = "terminal.invalid_size",
        StreamUnavailable {} = "terminal.stream_unavailable",
        NotFound { id: String } = "terminal.not_found",
        ProjectNotFound { id: String } = "terminal.project_not_found",
        CwdInvalid { path: String } = "terminal.cwd_invalid",
        AgentOwned = "terminal.agent_owned",
    }
}
