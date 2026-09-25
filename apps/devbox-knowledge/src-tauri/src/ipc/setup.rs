use product_contract::Problem;
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::{Deserialize, Serialize};
use tauri::{Manager, WebviewWindow};
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum StartupCall {
    Status {},
    StartEmpty {},
    ContinueExisting {},
}
impl StartupCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Status { .. } => "status",
            Self::StartEmpty { .. } => "start_empty",
            Self::ContinueExisting { .. } => "continue_existing",
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
#[ts(optional_fields = nullable)]
pub enum VaultCall {
    VaultChangeStatus {},
    ScheduleVaultChange { path: String },
    CancelVaultChange {},
    DiscardVaultPreview { id: String },
    VaultChangeJob { id: String },
    PrepareVaultChange {},
    ApplyVaultChange { id: String },
}
impl VaultCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::VaultChangeStatus { .. } => "vault_change_status",
            Self::ScheduleVaultChange { .. } => "schedule_vault_change",
            Self::CancelVaultChange { .. } => "cancel_vault_change",
            Self::DiscardVaultPreview { .. } => "discard_vault_preview",
            Self::VaultChangeJob { .. } => "vault_change_job",
            Self::PrepareVaultChange { .. } => "prepare_vault_change",
            Self::ApplyVaultChange { .. } => "apply_vault_change",
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum SetupCall {
    Startup(StartupCall),
    Vault(VaultCall),
}
impl ComponentCall for SetupCall {
    const COMPONENT: &'static str = "knowledge.setup";
    fn method(&self) -> &'static str {
        match self {
            Self::Startup(call) => call.method(),
            Self::Vault(call) => call.method(),
        }
    }
    fn class(&self) -> ExecutionClass {
        match self {
            Self::Vault(VaultCall::CancelVaultChange {}) => ExecutionClass::Control,
            _ => ExecutionClass::Normal,
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["notes"]
    }
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepared: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_existing: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_unavailable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_change: Option<bool>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct VaultScheduleView {
    pub schedule: Option<crate::core::vault_binding::Schedule>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct VaultPreview {
    pub preview_id: String,
    pub target: String,
    pub previous_root: String,
    pub already_applied: bool,
    pub expires_in_seconds: u64,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct VaultJobStarted {
    pub job_id: String,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct VaultActivated {
    pub active: bool,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum VaultJobValue {
    Preview(VaultPreview),
    Activated(VaultActivated),
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum VaultJob {
    Running {
        committed: bool,
    },
    Succeeded {
        value: VaultJobValue,
    },
    Failed {
        issue: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        committed: Option<bool>,
    },
}
product_ipc::issue_codes! {
    pub enum SetupIssue {
    Cancelled = "cancelled",
    ComponentArgsInvalid = "component_args_invalid",
    ComponentInitializationFailed = "component_initialization_failed",
    ComponentResponseInvalid = "component_response_invalid",
    ComponentStateConflict = "component_state_conflict",
    FutureSchema = "future_schema",
    ImportRestartRequired = "import_restart_required",
    RestartRequired = "restart_required",
    SetupRequired = "setup_required",
    StoreBusy = "store_busy",
    StoreFutureSchema = "store_future_schema",
    StoreInvalid = "store_invalid",
    StoreManifestInvalid = "store_manifest_invalid",
    StorePathInvalid = "store_path_invalid",
    StoreUnavailable = "store_unavailable",
    Unavailable = "unavailable",
    VaultBindingInvalid = "vault_binding_invalid",
    VaultBindingUnavailable = "vault_binding_unavailable",
    VaultChangeCancelled = "vault_change_cancelled",
    VaultChangeConflict = "vault_change_conflict",
    VaultChangeFuture = "vault_change_future",
    VaultChangeInvalid = "vault_change_invalid",
    VaultChangeSame = "vault_change_same",
    VaultChangeSaveFailed = "vault_change_save_failed",
    VaultChangeStale = "vault_change_stale",
    VaultChangeTimeout = "vault_change_timeout",
    VaultOwnerBusy = "vault_owner_busy",
    VaultOwnerUnavailable = "vault_owner_unavailable",
    }
}
pub fn classify(error: &str) -> &'static str {
    SetupIssue::from_code(error)
        .unwrap_or(SetupIssue::Unavailable)
        .code()
}
#[tauri::command]
pub async fn setup(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<SetupCall>(&window, request)?;
    let method = request.call.method();
    let value = match request.call {
        SetupCall::Startup(call) => crate::startup::dispatch_typed(window.app_handle(), call),
        SetupCall::Vault(call) => crate::vault_binding::dispatch_typed(window.app_handle(), call),
    }
    .and_then(|value| validate(method, value));
    Ok(admission.finish(value, classify))
}
fn validate(method: &str, value: serde_json::Value) -> Result<serde_json::Value, String> {
    match method {
        "status" | "start_empty" | "continue_existing" => super::typed::<StartupStatus>(value),
        "vault_change_status" | "schedule_vault_change" | "cancel_vault_change" => {
            super::typed::<VaultScheduleView>(value)
        }
        "prepare_vault_change" | "apply_vault_change" => super::typed::<VaultJobStarted>(value),
        "vault_change_job" => super::typed::<VaultJob>(value),
        _ => Ok(value),
    }
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<SetupCall>()?;
    export.register::<SetupIssue>()?;
    Ok(vec![
        ("status", export.register::<StartupStatus>()?),
        ("start_empty", export.register::<StartupStatus>()?),
        ("continue_existing", export.register::<StartupStatus>()?),
        (
            "vault_change_status",
            export.register::<VaultScheduleView>()?,
        ),
        (
            "schedule_vault_change",
            export.register::<VaultScheduleView>()?,
        ),
        (
            "cancel_vault_change",
            export.register::<VaultScheduleView>()?,
        ),
        ("discard_vault_preview", export.register::<()>()?),
        ("vault_change_job", export.register::<VaultJob>()?),
        (
            "prepare_vault_change",
            export.register::<VaultJobStarted>()?,
        ),
        ("apply_vault_change", export.register::<VaultJobStarted>()?),
    ])
}
