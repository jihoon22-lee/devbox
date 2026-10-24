//! One native observation, as the Windows app sends it (Task 9 builds them).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ObservationKind {
    /// The foreground window or its title changed (or input resumed).
    Foreground,
    /// Input heartbeat: extends the open session and settles waiting intervals.
    Input,
    /// Idle threshold passed: everything after `lastInputMs` is not activity.
    Idle,
    /// Collection paused: open sessions end at `atMs`.
    Pause,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    /// Per `runId` observation number, from 1.
    pub seq: u64,
    pub at_ms: i64,
    pub kind: ObservationKind,
    pub app: Option<String>,
    /// Already filtered by the app's privacy rules; the daemon applies them again.
    pub title: Option<String>,
    /// Epoch ms of the latest user input.
    pub last_input_ms: i64,
}
