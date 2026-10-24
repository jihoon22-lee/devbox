use devbox_protocol::domain_error;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceTarget {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub worktree_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Upstream {
    pub remote: String,
    pub destination_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RemoteExpect {
    pub branch: Option<String>,
    pub head_oid: Option<String>,
    pub upstream: Option<Upstream>,
    pub upstream_oid: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceExpect {
    pub state: RemoteExpect,
    pub index_hash: String,
    pub worktree_hash: String,
}
domain_error! {#[derive(Deserialize)] pub enum SourceGitError {
 ConflictStale { target: SourceTarget, path: String } = "git.conflict_stale",
 ConflictPartial { target: SourceTarget, path: String } = "git.conflict_partial",
 InvalidTarget = "git.invalid_target",
 InvalidSourceInput = "git.source_invalid_input",
 SourceFailed = "git.source_failed",
 SourceLimitExceeded = "git.source_limit_exceeded",
 SourceTimedOut = "git.source_timed_out",
 StateChanged { current: RemoteExpect } = "git.state_changed",
 SourceBusy = "git.source_busy",
 UnsafeSource = "git.unsafe_source",
}}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitTargetQuery {
    pub target: SourceTarget,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffQuery {
    pub target: SourceTarget,
    pub path: String,
    pub staged: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitLogQuery {
    pub target: SourceTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub path: Option<String>,
    pub limit: u32,
    pub skip: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitPathQuery {
    pub target: SourceTarget,
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitFileChange {
    pub target: SourceTarget,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hunk: Option<String>,
    pub expect: crate::hunks::FileExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitParams {
    pub target: SourceTarget,
    pub message: String,
    pub amend: bool,
    pub expect: SourceExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitCheckoutParams {
    pub target: SourceTarget,
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub create: Option<bool>,
    pub expect: SourceExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchDeleteParams {
    pub target: SourceTarget,
    pub branch: String,
    pub oid: String,
    pub force: bool,
    pub expect: SourceExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum GitStashAction {
    Push {
        message: String,
        #[serde(rename = "includeUntracked")]
        include_untracked: bool,
    },
    Apply {
        oid: String,
    },
    Pop {
        oid: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitStashParams {
    pub target: SourceTarget,
    pub payload: GitStashAction,
    pub expect: SourceExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitStashDropParams {
    pub target: SourceTarget,
    pub oid: String,
    pub expect: SourceExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitPullParams {
    pub target: SourceTarget,
    pub rebase: bool,
    pub expect: RemoteExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitPushParams {
    pub target: SourceTarget,
    pub force_with_lease: bool,
    pub expect: RemoteExpect,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitOperationQuery {
    pub target: SourceTarget,
    pub operation_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum GitOperationState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitOperation {
    pub id: String,
    pub target: SourceTarget,
    pub state: GitOperationState,
    pub error: Option<SourceGitError>,
    pub current: Option<RemoteExpect>,
    pub remote_oid: Option<String>,
    pub completion_unknown: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitChanged {
    pub project_id: String,
    pub worktree_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct GitCleanupParams {
    pub target: SourceTarget,
    pub candidates: Vec<crate::cleanup::CleanupCandidate>,
}
