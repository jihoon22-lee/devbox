use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LogSource {
    Run { run_id: String },
    File { path: String },
    Container { id: String },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct LogLine {
    pub src: LogSource,
    pub ts: String,
    pub stream: LogStream,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum LogFrom {
    Tail(Tail),
    Offset { offset: u64 },
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Tail {
    Tail,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LogFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub streams: Option<Vec<LogStream>>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct OpenLogs {
    pub sources: Vec<LogSource>,
    pub from: LogFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub filter: Option<LogFilter>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceCursor {
    pub source: LogSource,
    pub identity: String,
    pub stream: LogStream,
    pub offset: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SearchCursor {
    pub sources: Vec<SourceCursor>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SearchLogs {
    pub sources: Vec<LogSource>,
    pub query: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub before: Option<SearchCursor>,
    pub limit: u32,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub items: Vec<LogLine>,
    pub next_cursor: Option<SearchCursor>,
}
domain_error! {
    pub enum ObserveError {
        InvalidInput = "observe.invalid_input",
        StateChanged = "observe.state_changed",
        PermissionDenied = "observe.permission_denied",
        ProjectNotFound = "observe.project_not_found",
        InvalidRegex = "observe.invalid_regex",
        FileOutsideRoots = "observe.file_outside_roots",
        SourceUnavailable = "observe.source_unavailable",
        RunNotFound = "observe.run_not_found",
        InvalidCursor = "observe.invalid_cursor",
        LimitExceeded = "observe.limit_exceeded",
        StreamUnsupported = "observe.stream_unsupported",
    }
}
