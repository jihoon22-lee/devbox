use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulesQuery {
    pub project_id: String,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct SchedulesResult {
    pub items: Vec<ScheduleStatus>,
}
#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ScheduleStatus {
    pub id: String,
    pub cron: String,
    // UTC RFC3339 timestamp, absent when no future occurrence exists.
    pub next: Option<String>,
    pub last: Option<ScheduleLast>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ScheduleLast {
    pub at: String,
    pub result: ScheduleOutcome,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScheduleOutcome {
    Claimed,
    Running,
    Succeeded,
    Failed,
    Stopped,
    SkippedMissed,
    SkippedTrust,
}

impl ScheduleOutcome {
    pub(crate) fn from_run_state(state: crate::RunState) -> Self {
        match state {
            crate::RunState::Starting | crate::RunState::Running | crate::RunState::Restarting => {
                Self::Running
            }
            crate::RunState::Exited => Self::Succeeded,
            crate::RunState::Failed => Self::Failed,
            crate::RunState::Stopped => Self::Stopped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ScheduleClaim {
    pub project_id: String,
    pub schedule_id: String,
    pub task: String,
    pub at: i64,
}
