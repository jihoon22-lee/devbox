use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum UserUnitKind {
    Service,
    Timer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum UnitsUnavailableReason {
    CliMissing,
    ManagerUnavailable,
}

/// A service or timer of the systemd user manager that the user wrote. Read-only: Devbox
/// neither starts nor stops these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UserUnitInfo {
    pub name: String,
    pub description: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub kind: UserUnitKind,
    /// A timer's next run. One that counts from boot or from its last run is converted to
    /// wall-clock time with the daemon's clocks. Absent when nothing is scheduled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_elapse_ms: Option<u64>,
    /// The unit a timer starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub triggers: Option<String>,
    /// The registered project whose folder holds a service's `WorkingDirectory`. A timer has
    /// the project of the listed unit it triggers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UnitsResult {
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<UnitsUnavailableReason>,
    pub items: Vec<UserUnitInfo>,
    /// More units were loaded than one query shows.
    pub truncated: bool,
}
