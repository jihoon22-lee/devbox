use product_ipc::{workspace::Lane, ComponentCall};
#[derive(serde::Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum SourceHostCall {
    CreateWorktree {
        preview_id: String,
        operation_id: String,
    },
    PreviewWorktree {
        branch: String,
        target_dir: String,
    },
    PreviewCleanupScope {
        worktree_ids: Vec<String>,
    },
    TrustStatus {},
    PreviewTrust {},
    RevokeTrust {},
    CleanupScopeStatus {},
    RevokeCleanupScope {},
    CancelWorktree {
        preview_id: String,
    },
    CancelTrust {
        preview_id: String,
    },
    ApproveTrust {
        preview_id: String,
    },
    ApproveCleanupScope {
        preview_id: String,
    },
    CancelCleanupScope {
        preview_id: String,
    },
}
impl SourceHostCall {
    fn method(&self) -> &'static str {
        match self {
            Self::CreateWorktree { .. } => "create_worktree",
            Self::PreviewWorktree { .. } => "preview_worktree",
            Self::PreviewCleanupScope { .. } => "preview_cleanup_scope",
            Self::TrustStatus { .. } => "trust_status",
            Self::PreviewTrust { .. } => "preview_trust",
            Self::RevokeTrust { .. } => "revoke_trust",
            Self::CleanupScopeStatus { .. } => "cleanup_scope_status",
            Self::RevokeCleanupScope { .. } => "revoke_cleanup_scope",
            Self::CancelWorktree { .. } => "cancel_worktree",
            Self::CancelTrust { .. } => "cancel_trust",
            Self::ApproveTrust { .. } => "approve_trust",
            Self::ApproveCleanupScope { .. } => "approve_cleanup_scope",
            Self::CancelCleanupScope { .. } => "cancel_cleanup_scope",
        }
    }
}
#[derive(ts_rs::TS)]
#[ts(untagged)]
pub enum WorkspaceSourceCall {
    Host(SourceHostCall),
    Engine(Box<repositories_engine::api::SourceCall>),
}
impl<'de> serde::Deserialize<'de> for WorkspaceSourceCall {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        product_ipc::decode_host_first(
            d,
            &[
                "approve_cleanup_scope",
                "approve_trust",
                "cancel_cleanup_scope",
                "cancel_trust",
                "cancel_worktree",
                "cleanup_scope_status",
                "create_worktree",
                "preview_cleanup_scope",
                "preview_trust",
                "preview_worktree",
                "revoke_cleanup_scope",
                "revoke_trust",
                "trust_status",
            ],
            Self::Host,
            Self::Engine,
        )
    }
}
pub const METHODS: &[&str] = &[
    "approve_cleanup_scope",
    "approve_trust",
    "cancel_cleanup_scope",
    "cancel_trust",
    "cancel_worktree",
    "cleanup_scope_status",
    "create_worktree",
    "preview_cleanup_scope",
    "preview_trust",
    "preview_worktree",
    "repo_changes",
    "repo_cleanup",
    "repo_cleanup_cancel",
    "repo_cleanup_preview",
    "repo_commit",
    "repo_commit_detail",
    "repo_commit_preview",
    "repo_diff",
    "repo_fetch",
    "repo_history",
    "repo_local_cancel",
    "repo_preflight",
    "repo_pull",
    "repo_push",
    "repo_remote_cancel",
    "repo_remote_status",
    "repo_stage",
    "repo_status",
    "repo_unstage",
    "revoke_cleanup_scope",
    "revoke_trust",
    "trust_status",
    "worktree_clean",
    "worktrees",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "approve_cleanup_scope" => &["source"],
        "approve_trust" => &["source"],
        "cancel_cleanup_scope" => &["source"],
        "cancel_trust" => &["source"],
        "cancel_worktree" => &["source"],
        "cleanup_scope_status" => &["source"],
        "create_worktree" => &["source"],
        "preview_cleanup_scope" => &["source"],
        "preview_trust" => &["source"],
        "preview_worktree" => &["source"],
        "repo_changes" => &["source"],
        "repo_cleanup" => &["source"],
        "repo_cleanup_cancel" => &["source"],
        "repo_cleanup_preview" => &["source"],
        "repo_commit" => &["source"],
        "repo_commit_detail" => &["source"],
        "repo_commit_preview" => &["source"],
        "repo_diff" => &["source"],
        "repo_fetch" => &["source"],
        "repo_history" => &["source"],
        "repo_local_cancel" => &["source"],
        "repo_preflight" => &["source"],
        "repo_pull" => &["source"],
        "repo_push" => &["source"],
        "repo_remote_cancel" => &["source"],
        "repo_remote_status" => &["source"],
        "repo_stage" => &["source"],
        "repo_status" => &["source"],
        "repo_unstage" => &["source"],
        "revoke_cleanup_scope" => &["source"],
        "revoke_trust" => &["source"],
        "trust_status" => &["source"],
        "worktree_clean" => &["source"],
        "worktrees" => &["source"],
        _ => &[],
    }
}
impl ComponentCall for WorkspaceSourceCall {
    const COMPONENT: &'static str = "workspace.source";
    const IMPORT_PHASE: bool = false;
    const SHARED_REQUEST_LIMIT: bool = false;
    const MAX_ARGUMENT_BYTES: usize = 65536;
    fn valid_arguments(method: &str, args: &serde_json::Value) -> bool {
        let _ = method;
        let limit = Self::MAX_ARGUMENT_BYTES;
        super::bounded_arguments(args, limit)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
}
impl WorkspaceSourceCall {
    pub fn lane(&self) -> Lane {
        Lane::Source
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.source", method)
}
#[tauri::command]
pub(crate) async fn source(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<WorkspaceSourceCall>(window, runtime, request).await
}
impl super::WorkspaceCall for WorkspaceSourceCall {
    fn into_call(self) -> super::Call {
        super::Call::Source(self)
    }
}

use super::Request;
use crate::component::Runtime;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::{Manager, WebviewWindow};

pub(crate) async fn execute_source(
    window: &WebviewWindow,
    runtime: &Runtime,
    request: Request,
    context_permit: crate::core::context_activity::ContextPermit,
) -> Result<Value, &'static str> {
    let context = request.header.context.ok_or("project_selection_required")?;
    let deadline = request.header.deadline_ms;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "request_expired")?
        .as_millis() as u64;
    let span = deadline
        .checked_sub(now)
        .ok_or("request_expired")?
        .min(product_contract::MAX_DEADLINE_MS);
    let budget = crate::source_host::Budget {
        deadline_ms: deadline,
        expires: std::time::Instant::now() + Duration::from_millis(span),
    };
    let remaining = || -> Result<Duration, &'static str> {
        budget.check()?;
        budget
            .expires
            .checked_duration_since(std::time::Instant::now())
            .ok_or("request_expired")
    };
    let app = window.app_handle().clone();
    let operation_key = serde_json::to_string(&context).map_err(|_| "invalid_context")?;
    if repositories_engine::component::source_cancel(&request.method) {
        let super::Call::Source(WorkspaceSourceCall::Engine(call)) = request.typed else {
            return Err("invalid_request");
        };
        use repositories_engine::api::SourceCall as C;
        let id = match &*call {
            C::RepoLocalCancel { request }
            | C::RepoRemoteCancel { request }
            | C::RepoCleanupCancel { request } => &request.operation_id,
            _ => return Err("invalid_request"),
        };
        let admitted = runtime.source_operations.cancel(&operation_key, id)?;
        let running = repositories_engine::api::dispatch_source_cancel(&operation_key, *call)
            .await
            .map_err(|error| crate::source_host::issue(&error))?;
        return Ok(json!(admitted || running.as_bool() == Some(true)));
    }
    let filesystem = if matches!(
        request.method.as_str(),
        "cancel_trust"
            | "revoke_trust"
            | "cancel_worktree"
            | "cancel_cleanup_scope"
            | "revoke_cleanup_scope"
            | "cleanup_scope_status"
    ) {
        None
    } else {
        Some(
            runtime
                .filesystem_activity
                .enter(source_mutation(&request.method))?,
        )
    };
    let queued = runtime.lanes.try_enter(Lane::Source)?;
    let operation_id = if request.method == "create_worktree" {
        request.args.get("operationId")
    } else {
        request
            .args
            .get("request")
            .and_then(|value| value.get("operationId"))
    };
    let operation_id = operation_id
        .map(|value| value.as_str().ok_or("invalid_request"))
        .transpose()?;
    let admitted = runtime
        .source_operations
        .register(&operation_key, operation_id)?;
    let _cancel_on_drop = admitted.cancel_on_drop();
    let worker_slot = tokio::time::timeout(
        remaining()?,
        admitted.until_cancelled(runtime.lanes.workers(Lane::Source).acquire_owned()),
    )
    .await
    .map_err(|_| "request_expired")??
    .map_err(|_| "source_owner_unavailable")?;
    let host = runtime.host()?;
    let source = runtime.source.clone();
    let definitions = runtime.definitions.clone();
    let worker_app = app.clone();
    let method = request.method;
    let args = request.args;
    if crate::source_host::management(&method) {
        let worker_admission = admitted.clone();
        let worker = tauri::async_runtime::spawn_blocking(move || {
            let _retained = (queued, worker_slot, context_permit, filesystem);
            worker_admission.check()?;
            crate::files_host::current_deadline(deadline)?;
            let mut source = source.lock().map_err(|_| "source_owner_unavailable")?;
            let mut definitions = definitions.lock().map_err(|_| "definition_owner_busy")?;
            crate::files_host::current_deadline(deadline)?;
            source.initialize(&worker_app, &host)?;
            let result = source.manage(&host, &mut definitions, &context, &method, args, budget)?;
            worker_admission.check()?;
            Ok(result)
        });
        return tokio::time::timeout(remaining()?, admitted.until_cancelled(worker))
            .await
            .map_err(|_| "request_expired")??
            .unwrap_or(Err("worker_unavailable"))
            .map_err(crate::source_host::issue);
    }
    let worker_admission = admitted.clone();
    let worker_method = method.clone();
    let files = runtime.files.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        worker_admission.check()?;
        let retained = (
            queued,
            worker_slot,
            context_permit,
            filesystem,
            worker_admission.clone(),
        );
        crate::files_host::current_deadline(deadline)?;
        let prepared = {
            let mut source = source.lock().map_err(|_| "source_owner_unavailable")?;
            let mut definitions = definitions.lock().map_err(|_| "definition_owner_busy")?;
            crate::files_host::current_deadline(deadline)?;
            source.initialize(&worker_app, &host)?;
            source.access(
                host,
                &mut definitions,
                crate::source_host::Invocation {
                    files,
                    context,
                    method: worker_method,
                    args,
                    budget,
                    admitted: worker_admission,
                },
                retained,
            )?
        };
        prepared.finish_on_worker()
    });
    let prepared = tokio::time::timeout(remaining()?, admitted.until_cancelled(worker))
        .await
        .map_err(|_| "request_expired")??
        .unwrap_or(Err("worker_unavailable"))
        .map_err(crate::source_host::issue)?;
    let (access, args) = match prepared {
        crate::source_host::ReadySource::Native(access) => *access,
        #[cfg(windows)]
        crate::source_host::ReadySource::Complete(value) => return Ok(value),
    };
    match tokio::time::timeout(
        remaining()?,
        repositories_engine::api::dispatch_source(
            access,
            serde_json::from_value(json!({"method":method,"args":args}))
                .map_err(|_| "invalid_request")?,
        ),
    )
    .await
    {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(crate::source_host::issue(&error)),
        Err(_) => Err("request_expired"),
    }
}

pub(crate) fn source_mutation(method: &str) -> bool {
    matches!(
        method,
        "repo_stage"
            | "repo_unstage"
            | "repo_commit"
            | "repo_fetch"
            | "repo_pull"
            | "repo_push"
            | "repo_cleanup"
            | "create_worktree"
    )
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceTrustStatus {
    pub approved: bool,
    pub has_approval: bool,
    pub review: workspace_wsl::git_trust::GitReview,
    pub changed_evidence: Vec<String>,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceTrustPreview {
    pub preview_id: String,
    pub status: SourceTrustStatus,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceApproved {
    pub approved: bool,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceWorktreePreview {
    pub preview_id: String,
    pub branch: String,
    pub target_dir: String,
    pub root: String,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupMember {
    pub id: String,
    pub root: String,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupScopeStatus {
    pub available: Vec<CleanupMember>,
    pub has_approval: bool,
    pub selected_ids: Vec<String>,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReviewedMember {
    pub id: String,
    pub root: String,
    pub review: workspace_wsl::git_trust::GitReview,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupScopePreview {
    pub preview_id: String,
    pub members: Vec<CleanupReviewedMember>,
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupApproved {
    pub has_approval: bool,
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<WorkspaceSourceCall>()?;
    let mut result = repositories_engine::api::source_result_types(export)?;
    result.retain(|(method, _)| METHODS.contains(method));
    result.retain(|(method, _)| *method != "preview_worktree");
    result.push((
        "preview_worktree",
        export.register::<SourceWorktreePreview>()?,
    ));
    result.retain(|(method, _)| *method != "create_worktree");
    result.push((
        "create_worktree",
        export.register::<repositories_engine::commands::WorktreeCreate>()?,
    ));
    result.retain(|(method, _)| *method != "cancel_worktree");
    result.push(("cancel_worktree", export.register::<()>()?));
    result.retain(|(method, _)| *method != "trust_status");
    result.push(("trust_status", export.register::<SourceTrustStatus>()?));
    result.retain(|(method, _)| *method != "preview_trust");
    result.push(("preview_trust", export.register::<SourceTrustPreview>()?));
    result.retain(|(method, _)| *method != "approve_trust");
    result.push(("approve_trust", export.register::<SourceApproved>()?));
    result.retain(|(method, _)| *method != "revoke_trust");
    result.push(("revoke_trust", export.register::<SourceApproved>()?));
    result.retain(|(method, _)| *method != "cancel_trust");
    result.push(("cancel_trust", export.register::<()>()?));
    result.retain(|(method, _)| *method != "cleanup_scope_status");
    result.push((
        "cleanup_scope_status",
        export.register::<CleanupScopeStatus>()?,
    ));
    result.retain(|(method, _)| *method != "preview_cleanup_scope");
    result.push((
        "preview_cleanup_scope",
        export.register::<CleanupScopePreview>()?,
    ));
    result.retain(|(method, _)| *method != "approve_cleanup_scope");
    result.push((
        "approve_cleanup_scope",
        export.register::<CleanupApproved>()?,
    ));
    result.retain(|(method, _)| *method != "revoke_cleanup_scope");
    result.push((
        "revoke_cleanup_scope",
        export.register::<CleanupApproved>()?,
    ));
    result.retain(|(method, _)| *method != "cancel_cleanup_scope");
    result.push(("cancel_cleanup_scope", export.register::<()>()?));
    result.sort_by_key(|(method, _)| *method);
    Ok(result)
}
