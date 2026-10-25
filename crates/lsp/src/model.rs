use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    Rust,
    #[serde(rename = "typescript")]
    TypeScript,
    Python,
    Go,
    Bash,
    Yaml,
    Toml,
    Json,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenLsp {
    pub project_id: String,
    pub language: Language,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallLsp {
    pub project_id: String,
    pub language: Language,
    // Must match the displayed command; never execute caller-supplied shell text.
    pub expected_command: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServersQuery {
    pub project_id: String,
}
/// One newline-delimited packet per stream message. `json` is an LSP JSON object,
/// without Content-Length. didOpen/didChange/didSave carry the live document lease.
/// didChange sends a full text replacement; old owner/version messages are refused.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelayMessage {
    pub json: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lease_id: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ServerState {
    Missing,
    Starting,
    Ready,
    Failed,
    IdleStopped,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub language: Language,
    pub command: String,
    pub args: Vec<String>,
    pub install: Option<String>,
    pub state: ServerState,
    pub generation: u64,
    pub pid: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub clients: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InotifyInfo {
    pub max_user_instances: Option<u64>,
    pub max_user_watches: Option<u64>,
    // Counts only this daemon and its owned language servers, not all user processes.
    pub owned_instances: u64,
    pub owned_watches: u64,
    pub usage_complete: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ServersResult {
    pub items: Vec<ServerInfo>,
    pub inotify: InotifyInfo,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Restarted {
    pub project_id: String,
    pub language: Language,
    pub failed: bool,
    pub generation: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub terminal_id: String,
}
domain_error! {
 pub enum LspError {
  InvalidInput = "lsp.invalid_input",
  ProjectNotFound = "lsp.project_not_found",
  MissingServer = "lsp.missing_server",
  Failed = "lsp.failed",
  LimitExceeded = "lsp.limit_exceeded",
  LeaseLost = "lsp.lease_lost",
  OutsideProject = "lsp.outside_project",
  Protocol = "lsp.protocol",
  StreamUnavailable = "lsp.stream_unavailable",
  InstallUnavailable = "lsp.install_unavailable",
 }
}

/// Daemon-created request provenance, never accepted from the language server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceBaseline {
    pub uri: String,
    pub disk_rev: Option<devbox_documents::DiskRev>,
    pub text_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lease_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceProof {
    pub token: String,
    pub request_id: String,
    pub root_uri: String,
    pub generation: String,
    pub targets: Vec<WorkspaceBaseline>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceIdentity {
    pub token: String,
    pub request_id: String,
    pub root_uri: String,
    pub generation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceValidation {
    #[serde(flatten)]
    #[ts(flatten)]
    pub identity: WorkspaceIdentity,
    pub targets: Vec<WorkspaceBaseline>,
}
