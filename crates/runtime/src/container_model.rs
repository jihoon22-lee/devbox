use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ContainersQuery {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ContainerUnavailableReason {
    CliMissing,
    DaemonUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContainersResult {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<ContainerUnavailableReason>,
    pub items: Vec<ContainerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub ports: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compose_project: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ContainerActionParams {
    pub id: String,
    pub action: ContainerAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ContainerActionResult {
    pub id: String,
    pub action: ContainerAction,
    // A successful stop offers start; this is not an external-state transaction.
    pub undo: Option<ContainerActionParams>,
}

domain_error! {
    pub enum ContainerError {
        InvalidId = "runtime.container_invalid_id",
        Unavailable = "runtime.container_unavailable",
        Failed = "runtime.container_failed",
        Timeout = "runtime.container_timeout",
        OutputLimit = "runtime.container_output_limit",
        InvalidOutput = "runtime.container_invalid_output",
        Cancelled = "runtime.container_cancelled",
    }
}
