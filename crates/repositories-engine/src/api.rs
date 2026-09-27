//! Typed source and dependency calls; execution still requires native access.
use product_ipc::workspace::{Lane, LONG_BUDGET_MS};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum SourceCall {
    RepoFileHunks {
        request: crate::commands::FileHunksRequest,
    },
    RepoHunksApply {
        request: crate::commands::HunksApplyRequest,
    },
    RepoLastCommit {
        request: crate::commands::PathRequest,
    },
    RepoBlame {
        request: crate::commands::BlameRequest,
    },

    RepoBranches {
        request: crate::commands::PathRequest,
    },
    RepoBranchCreate {
        request: crate::commands::BranchCreateRequest,
    },
    RepoSwitch {
        request: crate::commands::SwitchRequest,
    },
    RepoBranchRename {
        request: crate::commands::BranchRenameRequest,
    },
    RepoBranchDelete {
        request: crate::commands::BranchDeleteRequest,
    },
    RepoStashList {
        request: crate::commands::PathRequest,
    },
    RepoStashPush {
        request: crate::commands::StashPushRequest,
    },
    RepoStashApply {
        request: crate::commands::StashApplyRequest,
    },
    RepoStashDrop {
        request: crate::commands::StashDropRequest,
    },
    RepoStashStore {
        request: crate::commands::StashStoreRequest,
    },

    InspectAgentWorktree {
        request: crate::commands::InspectAgentWorktreeRequest,
    },
    RepoMerge {
        request: crate::commands::MergeRequest,
    },
    RemoveAgentWorktree {
        request: crate::commands::RemoveAgentWorktreeRequest,
    },
    CreateWorktree {},
    RepoStatus {
        path: String,
    },
    Worktrees {
        path: String,
    },
    WorktreeClean {
        path: String,
    },
    RepoPreflight {
        request: crate::commands::RepoPreflightRequest,
    },
    RepoHistory {
        request: crate::commands::HistoryRequest,
    },
    RepoCommitDetail {
        request: crate::commands::CommitDetailRequest,
    },
    RepoDiff {
        request: crate::commands::DiffRequest,
    },
    RepoChanges {
        request: crate::commands::RepoChangesRequest,
    },
    RepoStage {
        request: crate::commands::StagePathsRequest,
    },
    RepoUnstage {
        request: crate::commands::UnstagePathsRequest,
    },
    RepoCommitPreview {
        request: crate::commands::RepoChangesRequest,
    },
    RepoCommit {
        request: crate::commands::CommitRequest,
    },
    RepoLocalCancel {
        request: crate::commands::RemoteCancelRequest,
    },
    RepoRemoteStatus {
        request: crate::commands::RemoteSyncRequest,
    },
    RepoFetch {
        request: crate::commands::RemoteOperationRequest,
    },
    RepoPull {
        request: crate::commands::RemoteOperationRequest,
    },
    RepoPush {
        request: crate::commands::RemoteOperationRequest,
    },
    RepoRemoteCancel {
        request: crate::commands::RemoteCancelRequest,
    },
    RepoCleanupPreview {
        request: crate::commands::CleanupPreviewRequest,
    },
    RepoCleanup {
        request: crate::commands::CleanupRequest,
    },
    RepoCleanupCancel {
        request: crate::commands::RemoteCancelRequest,
    },
}
impl SourceCall {
    pub const METHODS: &'static [&'static str] = &[
        "repo_file_hunks",
        "repo_hunks_apply",
        "repo_last_commit",
        "repo_blame",
        "repo_branches",
        "repo_branch_create",
        "repo_switch",
        "repo_branch_rename",
        "repo_branch_delete",
        "repo_stash_list",
        "repo_stash_push",
        "repo_stash_apply",
        "repo_stash_drop",
        "repo_stash_store",
        "inspect_agent_worktree",
        "repo_merge",
        "remove_agent_worktree",
        "create_worktree",
        "repo_status",
        "worktrees",
        "worktree_clean",
        "repo_preflight",
        "repo_history",
        "repo_commit_detail",
        "repo_diff",
        "repo_changes",
        "repo_stage",
        "repo_unstage",
        "repo_commit_preview",
        "repo_commit",
        "repo_local_cancel",
        "repo_remote_status",
        "repo_fetch",
        "repo_pull",
        "repo_push",
        "repo_remote_cancel",
        "repo_cleanup_preview",
        "repo_cleanup",
        "repo_cleanup_cancel",
    ];
    pub fn method(&self) -> &'static str {
        match self {
            Self::RepoFileHunks { .. } => "repo_file_hunks",
            Self::RepoHunksApply { .. } => "repo_hunks_apply",
            Self::RepoLastCommit { .. } => "repo_last_commit",
            Self::RepoBlame { .. } => "repo_blame",

            Self::RepoBranches { .. } => "repo_branches",
            Self::RepoBranchCreate { .. } => "repo_branch_create",
            Self::RepoSwitch { .. } => "repo_switch",
            Self::RepoBranchRename { .. } => "repo_branch_rename",
            Self::RepoBranchDelete { .. } => "repo_branch_delete",
            Self::RepoStashList { .. } => "repo_stash_list",
            Self::RepoStashPush { .. } => "repo_stash_push",
            Self::RepoStashApply { .. } => "repo_stash_apply",
            Self::RepoStashDrop { .. } => "repo_stash_drop",
            Self::RepoStashStore { .. } => "repo_stash_store",

            Self::InspectAgentWorktree { .. } => "inspect_agent_worktree",
            Self::RepoMerge { .. } => "repo_merge",
            Self::RemoveAgentWorktree { .. } => "remove_agent_worktree",
            Self::CreateWorktree { .. } => "create_worktree",
            Self::RepoStatus { .. } => "repo_status",
            Self::Worktrees { .. } => "worktrees",
            Self::WorktreeClean { .. } => "worktree_clean",
            Self::RepoPreflight { .. } => "repo_preflight",
            Self::RepoHistory { .. } => "repo_history",
            Self::RepoCommitDetail { .. } => "repo_commit_detail",
            Self::RepoDiff { .. } => "repo_diff",
            Self::RepoChanges { .. } => "repo_changes",
            Self::RepoStage { .. } => "repo_stage",
            Self::RepoUnstage { .. } => "repo_unstage",
            Self::RepoCommitPreview { .. } => "repo_commit_preview",
            Self::RepoCommit { .. } => "repo_commit",
            Self::RepoLocalCancel { .. } => "repo_local_cancel",
            Self::RepoRemoteStatus { .. } => "repo_remote_status",
            Self::RepoFetch { .. } => "repo_fetch",
            Self::RepoPull { .. } => "repo_pull",
            Self::RepoPush { .. } => "repo_push",
            Self::RepoRemoteCancel { .. } => "repo_remote_cancel",
            Self::RepoCleanupPreview { .. } => "repo_cleanup_preview",
            Self::RepoCleanup { .. } => "repo_cleanup",
            Self::RepoCleanupCancel { .. } => "repo_cleanup_cancel",
        }
    }
    pub fn lane(&self) -> Lane {
        Lane::Source
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        if matches!(
            self,
            Self::RepoBranches { .. }
                | Self::RepoStashList { .. }
                | Self::RepoFileHunks { .. }
                | Self::RepoLastCommit { .. }
                | Self::RepoBlame { .. }
        ) {
            return product_ipc::workspace::DEFAULT_BUDGET_MS;
        }
        LONG_BUDGET_MS
    }
}
impl SourceCall {
    pub fn is_cancel(&self) -> bool {
        matches!(
            self,
            Self::RepoLocalCancel { .. }
                | Self::RepoRemoteCancel { .. }
                | Self::RepoCleanupCancel { .. }
        )
    }
    pub(crate) fn path(&self) -> Option<&str> {
        match self {
            Self::RepoFileHunks { request } => Some(&request.path),
            Self::RepoHunksApply { request } => Some(&request.path),
            Self::RepoLastCommit { request } => Some(&request.path),
            Self::RepoBlame { request } => Some(&request.path),

            Self::RepoBranches { request } => Some(&request.path),
            Self::RepoBranchCreate { request } => Some(&request.path),
            Self::RepoSwitch { request } => Some(&request.path),
            Self::RepoBranchRename { request } => Some(&request.path),
            Self::RepoBranchDelete { request } => Some(&request.path),
            Self::RepoStashList { request } => Some(&request.path),
            Self::RepoStashPush { request } => Some(&request.path),
            Self::RepoStashApply { request } => Some(&request.path),
            Self::RepoStashDrop { request } => Some(&request.path),
            Self::RepoStashStore { request } => Some(&request.path),

            Self::InspectAgentWorktree { request } => Some(&request.path),
            Self::RepoMerge { request } => Some(&request.path),
            Self::RemoveAgentWorktree { request } => Some(&request.path),
            Self::CreateWorktree {} => None,
            Self::RepoStatus { path } => Some(path),
            Self::Worktrees { path } => Some(path),
            Self::WorktreeClean { path } => Some(path),
            Self::RepoPreflight { request } => Some(&request.path),
            Self::RepoHistory { request } => Some(&request.path),
            Self::RepoCommitDetail { request } => Some(&request.path),
            Self::RepoDiff { request } => Some(&request.path),
            Self::RepoChanges { request } => Some(&request.path),
            Self::RepoStage { request } => Some(&request.path),
            Self::RepoUnstage { request } => Some(&request.path),
            Self::RepoCommitPreview { request } => Some(&request.path),
            Self::RepoCommit { request } => Some(&request.path),
            Self::RepoLocalCancel { request } => {
                let _ = request;
                None
            }
            Self::RepoRemoteStatus { request } => Some(&request.path),
            Self::RepoFetch { request } => Some(&request.path),
            Self::RepoPull { request } => Some(&request.path),
            Self::RepoPush { request } => Some(&request.path),
            Self::RepoRemoteCancel { request } => {
                let _ = request;
                None
            }
            Self::RepoCleanupPreview { request } => Some(&request.path),
            Self::RepoCleanup { request } => Some(&request.path),
            Self::RepoCleanupCancel { request } => {
                let _ = request;
                None
            }
        }
    }
    pub(crate) fn operation_id_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::RepoHunksApply { request } => Some(&mut request.operation_id),

            Self::RepoBranchCreate { request } => Some(&mut request.operation_id),
            Self::RepoSwitch { request } => Some(&mut request.operation_id),
            Self::RepoBranchRename { request } => Some(&mut request.operation_id),
            Self::RepoBranchDelete { request } => Some(&mut request.operation_id),
            Self::RepoStashPush { request } => Some(&mut request.operation_id),
            Self::RepoStashApply { request } => Some(&mut request.operation_id),
            Self::RepoStashDrop { request } => Some(&mut request.operation_id),
            Self::RepoStashStore { request } => Some(&mut request.operation_id),

            Self::RepoMerge { request } => Some(&mut request.operation_id),
            Self::RemoveAgentWorktree { request } => Some(&mut request.operation_id),
            Self::RepoStage { request } => Some(&mut request.operation_id),
            Self::RepoUnstage { request } => Some(&mut request.operation_id),
            Self::RepoCommit { request } => Some(&mut request.operation_id),
            Self::RepoLocalCancel { request } => Some(&mut request.operation_id),
            Self::RepoFetch { request } => Some(&mut request.operation_id),
            Self::RepoPull { request } => Some(&mut request.operation_id),
            Self::RepoPush { request } => Some(&mut request.operation_id),
            Self::RepoRemoteCancel { request } => Some(&mut request.operation_id),
            Self::RepoCleanupPreview { request } => Some(&mut request.operation_id),
            Self::RepoCleanup { request } => Some(&mut request.operation_id),
            Self::RepoCleanupCancel { request } => Some(&mut request.operation_id),
            _ => None,
        }
    }
}
pub(crate) async fn execute_source(call: SourceCall) -> Result<serde_json::Value, String> {
    match call {
        SourceCall::RepoFileHunks { request } => {
            serde_json::to_value(crate::commands::repo_file_hunks(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoHunksApply { request } => {
            serde_json::to_value(crate::commands::repo_hunks_apply(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoLastCommit { request } => {
            serde_json::to_value(crate::commands::repo_last_commit(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoBlame { request } => {
            serde_json::to_value(crate::commands::repo_blame(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }

        SourceCall::RepoBranches { request } => {
            serde_json::to_value(crate::commands::repo_branches(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoBranchCreate { request } => {
            serde_json::to_value(crate::commands::repo_branch_create(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoSwitch { request } => {
            serde_json::to_value(crate::commands::repo_switch(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoBranchRename { request } => {
            serde_json::to_value(crate::commands::repo_branch_rename(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoBranchDelete { request } => {
            serde_json::to_value(crate::commands::repo_branch_delete(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoStashList { request } => {
            serde_json::to_value(crate::commands::repo_stash_list(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoStashPush { request } => {
            serde_json::to_value(crate::commands::repo_stash_push(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoStashApply { request } => {
            serde_json::to_value(crate::commands::repo_stash_apply(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoStashDrop { request } => {
            serde_json::to_value(crate::commands::repo_stash_drop(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoStashStore { request } => {
            serde_json::to_value(crate::commands::repo_stash_store(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }

        SourceCall::InspectAgentWorktree { request } => {
            serde_json::to_value(crate::commands::inspect_agent_worktree(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoMerge { request } => {
            serde_json::to_value(crate::commands::repo_merge(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RemoveAgentWorktree { request } => {
            crate::commands::remove_agent_worktree(request).await?;
            Ok(serde_json::Value::Null)
        }
        SourceCall::CreateWorktree {} => Err("worktree_review_required".into()),
        SourceCall::RepoStatus { path } => {
            use crate::commands::*;
            let value = repo_status(path).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::Worktrees { path } => {
            use crate::commands::*;
            let value = worktrees(path).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::WorktreeClean { path } => {
            use crate::commands::*;
            let value = worktree_clean(path).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoPreflight { request } => {
            use crate::commands::*;
            let value = repo_preflight(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoHistory { request } => {
            use crate::commands::*;
            let value = repo_history(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoCommitDetail { request } => {
            use crate::commands::*;
            let value = repo_commit_detail(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoDiff { request } => {
            use crate::commands::*;
            let value = repo_diff(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoChanges { request } => {
            use crate::commands::*;
            let value = repo_changes(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoStage { request } => {
            use crate::commands::*;
            repo_stage(request).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoUnstage { request } => {
            use crate::commands::*;
            repo_unstage(request).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoCommitPreview { request } => {
            use crate::commands::*;
            serde_json::to_value(repo_commit_preview(request).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoCommit { request } => {
            use crate::commands::*;
            repo_commit(request).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoLocalCancel { request } => {
            use crate::commands::*;
            let value = repo_local_cancel(request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoRemoteStatus { request } => {
            use crate::commands::*;
            let value = repo_remote_status(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoFetch { request } => {
            use crate::commands::*;
            repo_fetch(request).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoPull { request } => {
            use crate::commands::*;
            repo_pull(request).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoPush { request } => {
            use crate::commands::*;
            repo_push(request).await?;
            serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoRemoteCancel { request } => {
            use crate::commands::*;
            let value = repo_remote_cancel(request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoCleanupPreview { request } => {
            use crate::commands::*;
            let value = repo_cleanup_preview(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoCleanup { request } => {
            use crate::commands::*;
            let value = repo_cleanup(request).await?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
        SourceCall::RepoCleanupCancel { request } => {
            use crate::commands::*;
            let value = repo_cleanup_cancel(request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
        }
    }
}
pub fn source_result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<SourceCall>()?;
    Ok(vec![
        (
            "repo_file_hunks",
            export.register::<crate::commands::FileHunks>()?,
        ),
        ("repo_hunks_apply", export.register::<()>()?),
        (
            "repo_last_commit",
            export.register::<crate::commands::LastCommit>()?,
        ),
        ("repo_blame", export.register::<crate::commands::Blame>()?),
        (
            "repo_branches",
            export.register::<crate::commands::BranchList>()?,
        ),
        ("repo_branch_create", export.register::<()>()?),
        ("repo_switch", export.register::<()>()?),
        ("repo_branch_rename", export.register::<()>()?),
        (
            "repo_branch_delete",
            export.register::<crate::commands::DeletedBranch>()?,
        ),
        (
            "repo_stash_list",
            export.register::<Vec<crate::commands::StashEntry>>()?,
        ),
        ("repo_stash_push", export.register::<()>()?),
        (
            "repo_stash_apply",
            export.register::<crate::commands::StashApplyResult>()?,
        ),
        (
            "repo_stash_drop",
            export.register::<crate::commands::DroppedStash>()?,
        ),
        ("repo_stash_store", export.register::<()>()?),
        (
            "repo_merge",
            export.register::<crate::commands::MergeResult>()?,
        ),
        (
            "inspect_agent_worktree",
            export.register::<crate::commands::AgentWorktreePresence>()?,
        ),
        ("remove_agent_worktree", export.register::<()>()?),
        (
            "create_worktree",
            export.register::<crate::commands::WorktreeCreate>()?,
        ),
        (
            "repo_status",
            export.register::<crate::core::git::RepoSnapshot>()?,
        ),
        ("worktrees", export.register::<Vec<String>>()?),
        ("worktree_clean", export.register::<bool>()?),
        (
            "repo_preflight",
            export.register::<crate::core::git_safety::GitSafetySnapshot>()?,
        ),
        (
            "repo_history",
            export.register::<crate::core::history_diff::HistoryResult>()?,
        ),
        (
            "repo_commit_detail",
            export.register::<crate::core::history_diff::CommitDetail>()?,
        ),
        (
            "repo_diff",
            export.register::<crate::core::history_diff::DiffResult>()?,
        ),
        (
            "repo_changes",
            export.register::<Vec<crate::core::stage_commit::ChangeEntry>>()?,
        ),
        ("repo_stage", export.register::<()>()?),
        ("repo_unstage", export.register::<()>()?),
        (
            "repo_commit_preview",
            export.register::<crate::commands::commit_review::Review>()?,
        ),
        ("repo_commit", export.register::<()>()?),
        ("repo_local_cancel", export.register::<bool>()?),
        (
            "repo_remote_status",
            export.register::<crate::core::remote_sync::RemoteState>()?,
        ),
        ("repo_fetch", export.register::<()>()?),
        ("repo_pull", export.register::<()>()?),
        ("repo_push", export.register::<()>()?),
        ("repo_remote_cancel", export.register::<bool>()?),
        (
            "repo_cleanup_preview",
            export.register::<crate::core::cleanup::CleanupPreview>()?,
        ),
        (
            "repo_cleanup",
            export.register::<crate::core::cleanup::CleanupResult>()?,
        ),
        ("repo_cleanup_cancel", export.register::<bool>()?),
    ])
}
pub async fn dispatch_source(
    access: crate::component::SourceAccess,
    call: SourceCall,
) -> Result<serde_json::Value, String> {
    if cfg!(feature = "desktop") && !crate::component::is_product() {
        return Err("component_method_invalid".into());
    }
    crate::component::dispatch_source_typed_native(access, call).await
}
pub async fn dispatch_source_cancel(
    key: &str,
    call: SourceCall,
) -> Result<serde_json::Value, String> {
    crate::component::dispatch_source_cancel_typed_native(key, call).await
}

#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DependenciesCall {
    DependencyInventory {
        request: crate::commands::DependencyInventoryRequest,
    },
    DependencyEnrichmentPreview {
        request: crate::commands::dependency_enrichment::DependencyEnrichmentPreviewRequest,
    },
    DependencyEnrichmentExecute {
        request: crate::commands::dependency_enrichment::DependencyEnrichmentExecuteRequest,
    },
    DependencyEnrichmentCancel {
        request: crate::commands::dependency_enrichment::DependencyEnrichmentExecuteRequest,
    },
}
impl DependenciesCall {
    pub const METHODS: &'static [&'static str] = &[
        "dependency_inventory",
        "dependency_enrichment_preview",
        "dependency_enrichment_execute",
        "dependency_enrichment_cancel",
    ];
    pub fn method(&self) -> &'static str {
        match self {
            Self::DependencyInventory { .. } => "dependency_inventory",
            Self::DependencyEnrichmentPreview { .. } => "dependency_enrichment_preview",
            Self::DependencyEnrichmentExecute { .. } => "dependency_enrichment_execute",
            Self::DependencyEnrichmentCancel { .. } => "dependency_enrichment_cancel",
        }
    }
    pub fn lane(&self) -> Lane {
        Lane::Probes
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        LONG_BUDGET_MS
    }
}
#[derive(serde::Serialize, ts_rs::TS)]
pub struct DependencyCancelled {}
pub async fn dispatch_dependencies(
    access: crate::component::DependencyAccess,
    call: DependenciesCall,
) -> Result<serde_json::Value, String> {
    use crate::commands::dependency_enrichment as remote;
    let path = match &call {
        DependenciesCall::DependencyInventory { request } => &request.path,
        DependenciesCall::DependencyEnrichmentPreview { request } => &request.path,
        DependenciesCall::DependencyEnrichmentExecute { request }
        | DependenciesCall::DependencyEnrichmentCancel { request } => &request.path,
    };
    if *path != access.root.to_string_lossy() {
        return Err("dependency_context_changed".into());
    }
    match call {
        DependenciesCall::DependencyInventory { .. } => {
            serde_json::to_value(crate::commands::dependency_inventory_with_access(access).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        DependenciesCall::DependencyEnrichmentPreview { request } => serde_json::to_value(
            remote::preview_with_access(access, request.services, request.force_refresh).await?,
        )
        .map_err(|_| "component_response_invalid".into()),
        DependenciesCall::DependencyEnrichmentExecute { request } => {
            serde_json::to_value(remote::execute_with_access(access, request.preview_token).await?)
                .map_err(|_| "component_response_invalid".into())
        }
        DependenciesCall::DependencyEnrichmentCancel { request } => {
            crate::runtime::spawn_blocking(move || {
                remote::cancel_with_access(&access, &request.preview_token)
            })
            .await
            .map_err(|_| "component_worker_unavailable")??;
            serde_json::to_value(DependencyCancelled {})
                .map_err(|_| "component_response_invalid".into())
        }
    }
}
pub fn dependencies_result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<DependenciesCall>()?;
    Ok(vec![
        (
            "dependency_inventory",
            export.register::<crate::core::dependency_lens::DependencyReport>()?,
        ),
        (
            "dependency_enrichment_preview",
            export.register::<crate::core::dependency_enrichment::DependencyEnrichmentPreview>()?,
        ),
        (
            "dependency_enrichment_execute",
            export.register::<crate::core::dependency_enrichment::DependencyEnrichmentReport>()?,
        ),
        (
            "dependency_enrichment_cancel",
            export.register::<DependencyCancelled>()?,
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::workspace::Lane;
    #[test]
    fn source_calls_exclude_retired_root_scanning_and_keep_cancellation() {
        let status: SourceCall =
            serde_json::from_str(r#"{"method":"repo_status","args":{"path":"fixture"}}"#).unwrap();
        assert_eq!(status.lane(), Lane::Source);
        assert_eq!(status.deadline_budget_ms(), 29_000);
        assert!(serde_json::from_str::<SourceCall>(
            r#"{"method":"scan_root","args":{"root":"fixture"}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<SourceCall>(r#"{"method":"create_worktree","args":{"repoPath":"fixture","branch":"unsafe","targetDir":"target"}}"#).is_err());
    }
}

#[cfg(test)]
mod agent_worktree_tests {
    use super::*;
    #[test]
    fn agent_worktree_mutations_keep_native_path_operation_and_budget() {
        for (method, extra) in [
            ("repo_merge", serde_json::json!({})),
            (
                "remove_agent_worktree",
                serde_json::json!({"worktree":"/repo-task","force":false}),
            ),
        ] {
            let mut request =
                serde_json::json!({"path":"/repo","branch":"agent/task","operationId":"operation"});
            request
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let mut call: SourceCall = serde_json::from_value(
                serde_json::json!({"method":method,"args":{"request":request}}),
            )
            .unwrap();
            assert_eq!(call.method(), method);
            assert_eq!(call.path(), Some("/repo"));
            assert_eq!(
                call.operation_id_mut().map(|s| s.as_str()),
                Some("operation")
            );
            assert_eq!(call.lane(), Lane::Source);
            assert_eq!(call.deadline_budget_ms(), LONG_BUDGET_MS);
        }
    }
}

#[cfg(test)]
mod branch_stash_tests {
    use super::*;
    #[test]
    fn branch_stash_calls_bind_paths_operations_and_budgets() {
        for (method, request, mutation) in [
            ("repo_branches", serde_json::json!({"path": "/repo"}), false),
            (
                "repo_branch_create",
                serde_json::json!({"name": "x", "startPoint": null, "checkout": true, "operationId": "op", "path": "/repo"}),
                true,
            ),
            (
                "repo_switch",
                serde_json::json!({"branch": "x", "operationId": "op", "path": "/repo"}),
                true,
            ),
            (
                "repo_branch_rename",
                serde_json::json!({"from": "a", "to": "b", "operationId": "op", "path": "/repo"}),
                true,
            ),
            (
                "repo_branch_delete",
                serde_json::json!({"name": "x", "operationId": "op", "path": "/repo"}),
                true,
            ),
            (
                "repo_stash_list",
                serde_json::json!({"path": "/repo"}),
                false,
            ),
            (
                "repo_stash_push",
                serde_json::json!({"message": null, "includeUntracked": false, "operationId": "op", "path": "/repo"}),
                true,
            ),
            (
                "repo_stash_apply",
                serde_json::json!({"index": 0, "pop": true, "operationId": "op", "path": "/repo"}),
                true,
            ),
            (
                "repo_stash_drop",
                serde_json::json!({"index": 0, "operationId": "op", "path": "/repo"}),
                true,
            ),
            (
                "repo_stash_store",
                serde_json::json!({"commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "message": "message", "operationId": "op", "path": "/repo"}),
                true,
            ),
        ] {
            let mut call: SourceCall = serde_json::from_value(
                serde_json::json!({"method":method,"args":{"request":request}}),
            )
            .unwrap();
            assert_eq!(call.method(), method);
            assert!(SourceCall::METHODS.contains(&method));
            assert_eq!(call.path(), Some("/repo"));
            assert_eq!(call.lane(), Lane::Source);
            assert_eq!(call.operation_id_mut().is_some(), mutation);
            assert_eq!(
                call.deadline_budget_ms(),
                if mutation {
                    LONG_BUDGET_MS
                } else {
                    product_ipc::workspace::DEFAULT_BUDGET_MS
                }
            );
        }
    }
}

#[cfg(test)]
mod hunk_blame_tests {
    use super::*;
    #[test]
    fn hunk_blame_calls_keep_native_scope_and_do_not_accept_patch_text() {
        for (method, mut request, mutation) in [
            (
                "repo_file_hunks",
                serde_json::json!({"file": "a.txt", "staged": false, "path": "/repo"}),
                false,
            ),
            (
                "repo_hunks_apply",
                serde_json::json!({"file": "a.txt", "staged": false, "action": "stage", "hunkIds": ["native-hunk"], "revision": "native-revision", "operationId": "op", "path": "/repo"}),
                true,
            ),
            (
                "repo_last_commit",
                serde_json::json!({"path": "/repo"}),
                false,
            ),
            (
                "repo_blame",
                serde_json::json!({"file": "a.txt", "commitId": null, "path": "/repo"}),
                false,
            ),
        ] {
            let mut call: SourceCall = serde_json::from_value(
                serde_json::json!({"method":method,"args":{"request":request}}),
            )
            .unwrap();
            assert_eq!(call.method(), method);
            assert!(SourceCall::METHODS.contains(&method));
            assert_eq!(call.path(), Some("/repo"));
            assert_eq!(call.lane(), Lane::Source);
            assert_eq!(call.operation_id_mut().is_some(), mutation);
            assert_eq!(
                call.deadline_budget_ms(),
                if mutation {
                    LONG_BUDGET_MS
                } else {
                    product_ipc::workspace::DEFAULT_BUDGET_MS
                }
            );
            request["patch"] = serde_json::json!("renderer patch");
            assert!(serde_json::from_value::<SourceCall>(
                serde_json::json!({"method":method,"args":{"request":request}})
            )
            .is_err());
        }
    }
}
