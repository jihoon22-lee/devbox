use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Manager, WebviewWindow};
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum UpdateCall {
    CheckSuiteUpdate {},
    SuiteUpdateStatus { id: String },
    DownloadSuiteUpdate { id: String },
    CancelSuiteUpdate { id: String },
    LaunchSuiteUpdate { id: String },
}
impl UpdateCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::CheckSuiteUpdate { .. } => "check_suite_update",
            Self::SuiteUpdateStatus { .. } => "suite_update_status",
            Self::DownloadSuiteUpdate { .. } => "download_suite_update",
            Self::CancelSuiteUpdate { .. } => "cancel_suite_update",
            Self::LaunchSuiteUpdate { .. } => "launch_suite_update",
        }
    }
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::CheckSuiteUpdate {} => None,
            Self::SuiteUpdateStatus { id }
            | Self::DownloadSuiteUpdate { id }
            | Self::CancelSuiteUpdate { id }
            | Self::LaunchSuiteUpdate { id } => Some(id),
        }
    }
}
#[derive(Clone, Copy, Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum RestoreAction {
    Snapshot,
    ActivateClean,
    CommitClean,
    CommitReinstall,
    Restore,
    Resume,
    Commit,
    Rollback,
    UpdateResume,
    UpdateCommit,
    UpdateRollback,
}
impl RestoreAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::ActivateClean => "activateClean",
            Self::CommitClean => "commitClean",
            Self::CommitReinstall => "commitReinstall",
            Self::Restore => "restore",
            Self::Resume => "resume",
            Self::Commit => "commit",
            Self::Rollback => "rollback",
            Self::UpdateResume => "updateResume",
            Self::UpdateCommit => "updateCommit",
            Self::UpdateRollback => "updateRollback",
        }
    }
    fn valid_id(self, id: &str) -> bool {
        match self {
            Self::Snapshot | Self::ActivateClean | Self::CommitClean | Self::CommitReinstall => {
                id.is_empty()
            }
            _ => uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id),
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DeliveryAction {
    SuiteInventory {},
    OpenInstallationFolder {},
    SuiteRecovery {},
    RestoreInventory {},
    RestoreAction { action: RestoreAction, id: String },
    RecordSuiteHealth { product: String },
}
impl DeliveryAction {
    pub fn method(&self) -> &'static str {
        match self {
            Self::SuiteInventory { .. } => "suite_inventory",
            Self::OpenInstallationFolder { .. } => "open_installation_folder",
            Self::SuiteRecovery { .. } => "suite_recovery",
            Self::RestoreInventory { .. } => "restore_inventory",
            Self::RestoreAction { .. } => "restore_action",
            Self::RecordSuiteHealth { .. } => "record_suite_health",
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum DeliveryCall {
    Update(UpdateCall),
    Action(DeliveryAction),
}
impl ComponentCall for DeliveryCall {
    const COMPONENT: &'static str = "control-center.delivery";
    const MAX_ARGUMENT_BYTES: usize = 512 * 1024;
    fn method(&self) -> &'static str {
        match self {
            Self::Update(call) => call.method(),
            Self::Action(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        match self {
            Self::Update(_) => &["updates"],
            Self::Action(
                DeliveryAction::RestoreInventory {} | DeliveryAction::RestoreAction { .. },
            ) => &["updates", "recovery", "products"],
            Self::Action(DeliveryAction::RecordSuiteHealth { .. }) => &["updates", "recovery"],
            Self::Action(DeliveryAction::OpenInstallationFolder {}) => &["products"],
            _ => &["products", "updates", "components", "recovery"],
        }
    }
    fn class(&self) -> ExecutionClass {
        if matches!(self, Self::Update(UpdateCall::CancelSuiteUpdate { .. })) {
            ExecutionClass::Control
        } else {
            ExecutionClass::Normal
        }
    }
}
impl DeliveryCall {
    fn valid(&self) -> bool {
        match self {
            Self::Update(call) => call.id().is_none_or(product_contract::commands::revision),
            Self::Action(DeliveryAction::RestoreAction { action, id }) => action.valid_id(id),
            Self::Action(DeliveryAction::RecordSuiteHealth { product }) => {
                product_contract::installation::PRODUCTS.contains(&product.as_str())
            }
            _ => true,
        }
    }
}
#[tauri::command]
pub async fn delivery(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<DeliveryCall>(&window, request)?;
    if !request.call.valid() {
        return Err(admission.problem(ProblemCode::Unauthorized));
    }
    let result = dispatch(
        window.app_handle(),
        request.call,
        request.header.deadline_ms,
    )
    .await;
    Ok(admission.finish(result, classify))
}
async fn dispatch(
    app: &tauri::AppHandle,
    call: DeliveryCall,
    deadline: u64,
) -> Result<Value, String> {
    match call {
        DeliveryCall::Update(call) => {
            #[cfg(windows)]
            let result = crate::updates::execute_typed(app.clone(), call, deadline).await;
            #[cfg(not(windows))]
            let result: Result<Value, &'static str> = {
                let _ = (call, deadline);
                Err("suite_windows_required")
            };
            result.map_err(str::to_owned)
        }
        DeliveryCall::Action(DeliveryAction::RestoreInventory {}) => {
            #[cfg(windows)]
            let result =
                tauri::async_runtime::spawn_blocking(crate::bootstrap::interactive::inventory)
                    .await
                    .map_err(|_| "restore_worker_unavailable")
                    .and_then(|v| v);
            #[cfg(not(windows))]
            let result: Result<Value, &'static str> = Err("suite_windows_required");
            result.map_err(str::to_owned)
        }
        DeliveryCall::Action(DeliveryAction::RestoreAction { action, id }) => {
            #[cfg(windows)]
            let result = {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    crate::bootstrap::interactive::launch(
                        &app,
                        crate::bootstrap::interactive::Request {
                            action: action.as_str().into(),
                            id,
                        },
                    )
                })
                .await
                .map_err(|_| "restore_worker_unavailable")
                .and_then(|v| v)
            };
            #[cfg(not(windows))]
            let result: Result<Value, &'static str> = {
                let _ = (action, id);
                Err("suite_windows_required")
            };
            result.map_err(str::to_owned)
        }
        DeliveryCall::Action(DeliveryAction::RecordSuiteHealth { product }) => {
            #[cfg(windows)]
            let result = crate::owner_evidence::record(app.clone(), product, deadline).await;
            #[cfg(not(windows))]
            let result: Result<Value, &'static str> = {
                let _ = product;
                Err("suite_windows_required")
            };
            result.map_err(str::to_owned)
        }
        DeliveryCall::Action(DeliveryAction::SuiteRecovery {}) => {
            let root = app
                .path()
                .app_local_data_dir()
                .map_err(|_| "suite_store_unavailable")?;
            tauri::async_runtime::spawn_blocking(move || {
                let journal = crate::core::delivery_store::Store::inspect(&root)?;
                let result = match journal {
                    None => RecoveryStatus::None,
                    Some((journal, _)) => RecoveryStatus::Recorded {
                        recovery: journal.recovery()?,
                        phase: journal.phase,
                        committed: journal.committed,
                        backup_count: journal.backup.len(),
                        data_checkpoint_count: journal.data_checkpoints.len(),
                        import_count: journal.imports.len(),
                        recorded_owner_count: journal.owner_evidence.len(),
                        health_check_count: journal.health_checks.len(),
                        cleanup_pending: journal.cleanup_pending.len(),
                        failure: journal.failure,
                    },
                };
                serde_json::to_value(result).map_err(|_| "suite_store_unavailable")
            })
            .await
            .map_err(|_| "suite_store_unavailable".to_string())
            .and_then(|v| v.map_err(str::to_owned))
        }
        DeliveryCall::Action(DeliveryAction::OpenInstallationFolder {}) => {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                #[cfg(windows)]
                {
                    use tauri_plugin_opener::OpenerExt;
                    let scope =
                        crate::suite::capture_own("control-center", env!("CARGO_PKG_VERSION"))?;
                    scope.revalidate()?;
                    app.opener()
                        .open_path(scope.review_root(), None::<&str>)
                        .map_err(|_| "installation_folder_unavailable")?;
                    scope.revalidate()?;
                    serde_json::to_value(InstallationOpened { opened: true })
                        .map_err(|_| "installation_folder_unavailable")
                }
                #[cfg(not(windows))]
                {
                    let _ = app;
                    Err("suite_windows_required")
                }
            })
            .await
            .map_err(|_| "installation_folder_unavailable".to_string())
            .and_then(|v: Result<Value, &'static str>| v.map_err(str::to_owned))
        }
        DeliveryCall::Action(DeliveryAction::SuiteInventory {}) => {
            tauri::async_runtime::spawn_blocking(|| {
                #[cfg(windows)]
                let scope =
                    crate::suite::capture_own("control-center", env!("CARGO_PKG_VERSION")).ok();
                #[cfg(windows)]
                let captured = scope.as_ref().map(|scope| {
                    (
                        &scope.manifest,
                        scope.installation_key.as_str(),
                        &scope.issues,
                    )
                });
                #[cfg(not(windows))]
                let captured = None;
                crate::core::inventory::observe(captured)
                    .map_err(str::to_owned)
                    .and_then(|v| {
                        serde_json::to_value(v).map_err(|_| "inventory_unavailable".into())
                    })
            })
            .await
            .map_err(|_| "inventory_worker_unavailable".to_string())
            .and_then(|v| v)
        }
    }
}
#[derive(Serialize, ts_rs::TS)]
pub struct InstallationOpened {
    pub opened: bool,
}
#[derive(Serialize, ts_rs::TS)]
pub struct DeliveryAccepted {
    pub accepted: bool,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RecoveryStatus {
    None,
    Recorded {
        phase: crate::core::delivery::Phase,
        committed: bool,
        recovery: crate::core::delivery::Recovery,
        backup_count: usize,
        data_checkpoint_count: usize,
        import_count: usize,
        recorded_owner_count: usize,
        health_check_count: usize,
        cleanup_pending: usize,
        failure: Option<String>,
    },
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReview {
    pub available: bool,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOperation {
    pub id: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared_ms: Option<u64>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOperation {
    pub id: String,
    pub state: String,
    pub payload_revision: String,
    pub previous_version: String,
    pub version: String,
    pub checkpoint_id: String,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InstallationState {
    pub phase: crate::core::delivery::Phase,
    pub committed: bool,
    pub recorded_owners: usize,
    pub clean: bool,
    pub reinstall: bool,
    pub fresh_health: bool,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RestoreInventory {
    pub checkpoints: Vec<crate::core::data_checkpoint::Receipt>,
    pub operations: Vec<RestoreOperation>,
    pub active_operation: Option<String>,
    pub installation: InstallationState,
    pub update: Option<UpdateOperation>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RecordedHealth {
    pub owner: String,
    pub recorded: bool,
    pub revision: u64,
    pub activation_ready: bool,
    pub native_store_ready: bool,
    pub report: product_contract::health::Report,
}
product_ipc::issue_codes! {
    pub enum DeliveryIssue {
    BootstrapArgumentsInvalid = "bootstrap_arguments_invalid",
    BootstrapClockInvalid = "bootstrap_clock_invalid",
    BootstrapDataUnavailable = "bootstrap_data_unavailable",
    BootstrapDataUnsafe = "bootstrap_data_unsafe",
    BootstrapDispatcherUntrusted = "bootstrap_dispatcher_untrusted",
    BootstrapGateChanged = "bootstrap_gate_changed",
    BootstrapGateUnavailable = "bootstrap_gate_unavailable",
    BootstrapGateUnsafe = "bootstrap_gate_unsafe",
    BootstrapHelperBusy = "bootstrap_helper_busy",
    BootstrapIdentityUnavailable = "bootstrap_identity_unavailable",
    BootstrapJournalMissing = "bootstrap_journal_missing",
    BootstrapLaunchFailed = "bootstrap_launch_failed",
    BootstrapOwnerChanged = "bootstrap_owner_changed",
    BootstrapOwnerInvalid = "bootstrap_owner_invalid",
    BootstrapPayloadChanged = "bootstrap_payload_changed",
    BootstrapPayloadIncomplete = "bootstrap_payload_incomplete",
    BootstrapRestoreRetentionReviewRequired = "bootstrap_restore_retention_review_required",
    BootstrapRootChanged = "bootstrap_root_changed",
    BootstrapRootUnsafe = "bootstrap_root_unsafe",
    BootstrapStorePreparationRequired = "bootstrap_store_preparation_required",
    BootstrapUpdatePending = "bootstrap_update_pending",
    CheckpointMissing = "checkpoint_missing",
    DowngradeRequiresBackupExport = "downgrade_requires_backup_export",
    InstallationFolderUnavailable = "installation_folder_unavailable",
    InventoryUnavailable = "inventory_unavailable",
    InventoryWorkerUnavailable = "inventory_worker_unavailable",
    ManagerToolsUnavailable = "manager_tools_unavailable",
    RestoreActionAlreadyStarted = "restore_action_already_started",
    RestoreActionInvalid = "restore_action_invalid",
    RestoreOperationConflict = "restore_operation_conflict",
    RestoreOperationInvalid = "restore_operation_invalid",
    RestoreOperationUnsafe = "restore_operation_unsafe",
    RestorePlanChanged = "restore_plan_changed",
    RestorePlanInvalid = "restore_plan_invalid",
    RestoreRecordInvalid = "restore_record_invalid",
    RestoreRecordUnavailable = "restore_record_unavailable",
    RestoreWorkerUnavailable = "restore_worker_unavailable",
    SuiteHealthInvalid = "suite_health_invalid",
    SuiteHealthRequired = "suite_health_required",
    SuiteJournalChanged = "suite_journal_changed",
    SuiteJournalMissing = "suite_journal_missing",
    SuiteJournalStale = "suite_journal_stale",
    SuiteOwnerChanged = "suite_owner_changed",
    SuiteOwnerExpired = "suite_owner_expired",
    SuiteOwnerInvalid = "suite_owner_invalid",
    SuiteOwnerPhaseInvalid = "suite_owner_phase_invalid",
    SuiteSpaceInsufficient = "suite_space_insufficient",
    SuiteStoreUnavailable = "suite_store_unavailable",
    SuiteWindowsRequired = "suite_windows_required",
    SuiteWritersMustClose = "suite_writers_must_close",
    Unavailable = "unavailable",
    UpdateActivationInvalid = "update_activation_invalid",
    UpdateAlreadyInstalled = "update_already_installed",
    UpdateAlreadyLaunched = "update_already_launched",
    UpdateAssetChanged = "update_asset_changed",
    UpdateAssetMissing = "update_asset_missing",
    UpdateAssetTopologyInvalid = "update_asset_topology_invalid",
    UpdateBusy = "update_busy",
    UpdateCacheChanged = "update_cache_changed",
    UpdateCacheConflict = "update_cache_conflict",
    UpdateCacheUnavailable = "update_cache_unavailable",
    UpdateCacheUnsafe = "update_cache_unsafe",
    UpdateCallerUntrusted = "update_caller_untrusted",
    UpdateClaimChanged = "update_claim_changed",
    UpdateClaimInvalid = "update_claim_invalid",
    UpdateClockInvalid = "update_clock_invalid",
    UpdateCommitStarted = "update_commit_started",
    UpdateCommittedInstallationRequired = "update_committed_installation_required",
    UpdateDownloadCancelled = "update_download_cancelled",
    UpdateDownloadFailed = "update_download_failed",
    UpdateDownloadLarge = "update_download_large",
    UpdateDownloadRequired = "update_download_required",
    UpdateInstalledSuiteRequired = "update_installed_suite_required",
    UpdateJournalChanged = "update_journal_changed",
    UpdateLaunchFailed = "update_launch_failed",
    UpdateManifestChanged = "update_manifest_changed",
    UpdateManifestDigestMissing = "update_manifest_digest_missing",
    UpdateManifestMissing = "update_manifest_missing",
    UpdateNetworkTimeout = "update_network_timeout",
    UpdateNetworkUnavailable = "update_network_unavailable",
    UpdateOperationConflict = "update_operation_conflict",
    UpdateOperationInvalid = "update_operation_invalid",
    UpdateOtherGenerationPending = "update_other_generation_pending",
    UpdateOtherRecoveryPending = "update_other_recovery_pending",
    UpdateOwnerChanged = "update_owner_changed",
    UpdateOwnerInvalid = "update_owner_invalid",
    UpdatePlanChanged = "update_plan_changed",
    UpdatePlanInvalid = "update_plan_invalid",
    UpdatePreviousOperationIncomplete = "update_previous_operation_incomplete",
    UpdateProgressInvalid = "update_progress_invalid",
    UpdateRecordInvalid = "update_record_invalid",
    UpdateRecordWriteFailed = "update_record_write_failed",
    UpdateRecordsChanged = "update_records_changed",
    UpdateReleaseInvalid = "update_release_invalid",
    UpdateRequestInvalid = "update_request_invalid",
    UpdateResumeSelectedAction = "update_resume_selected_action",
    UpdateRetentionReviewRequired = "update_retention_review_required",
    UpdateReviewExpired = "update_review_expired",
    UpdateReviewInvalid = "update_review_invalid",
    UpdateRevisionExhausted = "update_revision_exhausted",
    UpdateRootChanged = "update_root_changed",
    UpdateStoreUnavailable = "update_store_unavailable",
    UpdateVersionInvalid = "update_version_invalid",
    }
}
pub fn classify(error: &str) -> &'static str {
    DeliveryIssue::from_code(error)
        .unwrap_or(DeliveryIssue::Unavailable)
        .code()
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<DeliveryCall>()?;
    export.register::<DeliveryIssue>()?;
    Ok(vec![
        (
            "suite_inventory",
            export.register::<crate::core::inventory::Inventory>()?,
        ),
        (
            "open_installation_folder",
            export.register::<InstallationOpened>()?,
        ),
        ("suite_recovery", export.register::<RecoveryStatus>()?),
        ("restore_inventory", export.register::<RestoreInventory>()?),
        ("restore_action", export.register::<DeliveryAccepted>()?),
        ("record_suite_health", export.register::<RecordedHealth>()?),
        ("check_suite_update", export.register::<UpdateReview>()?),
        ("suite_update_status", export.register::<UpdateReview>()?),
        ("download_suite_update", export.register::<UpdateReview>()?),
        ("cancel_suite_update", export.register::<UpdateReview>()?),
        (
            "launch_suite_update",
            export.register::<DeliveryAccepted>()?,
        ),
    ])
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    #[test]
    fn restore_requests_cannot_substitute_paths_or_noncanonical_ids() {
        for (action, id, accepted) in [
            ("snapshot", "", true),
            ("snapshot", "a", false),
            ("restore", "../private", false),
            ("restore", "a7d270b0-2ac5-4fbb-b71e-c4706ff41b29", true),
            ("restore", "A7D270B0-2AC5-4FBB-B71E-C4706FF41B29", false),
        ] {
            let call: DeliveryCall = serde_json::from_value(
                serde_json::json!({"method":"restore_action","args":{"action":action,"id":id}}),
            )
            .unwrap();
            assert_eq!(call.valid(), accepted);
        }
        let unknown =
            serde_json::json!({"method":"restore_action","args":{"action":"erase","id":""}});
        assert!(serde_json::from_value::<DeliveryCall>(unknown).is_err());
    }
}
