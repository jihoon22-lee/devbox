//! Native Workspace registry projection for the Knowledge consumer only.
//! Paths stay inside authenticated product IPC, never Launcher metadata/history.
use crate::ProjectContext;
use serde::{Deserialize, Serialize};
pub const MAX_BYTES: usize = 64 * 1024;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Offline,
    Missing,
    Unverified,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub context: ProjectContext,
    pub root: String,
    pub availability: Availability,
    pub activity_paths: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub current: Option<ProjectContext>,
    pub projects: Vec<Project>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delivery {
    pub epoch: String,
    pub snapshot: Snapshot,
}
