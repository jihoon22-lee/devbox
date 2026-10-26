use crate::exit_record::ExitRecord;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RunKind {
    Task,
    Service,
    Scheduled,
    AgentTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Starting,
    Running,
    Restarting,
    Exited,
    Failed,
    Stopped,
}
impl RunState {
    pub fn is_live(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Restarting)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WindowsRun {
    pub directory: String,
    pub notice: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub project_id: String,
    pub def_id: String,
    pub kind: RunKind,
    pub state: RunState,
    pub exit: Option<ExitRecord>,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub restarts: u32,
    pub unit: String,
    pub ready: bool,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub windows: Option<WindowsRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartRun {
    pub project_id: String,
    pub id: String,
    #[serde(default)]
    pub worktree: Option<String>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunId {
    pub run_id: String,
}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunsQuery {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}
fn default_limit() -> u32 {
    100
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct RunsResult {
    pub rev: String,
    pub items: Vec<Run>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct RunDetail {
    pub rev: String,
    pub run: Run,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RunSummaryQuery {}
#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub rev: String,
    pub active: u32,
    pub active_services: u32,
}
