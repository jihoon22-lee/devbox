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
