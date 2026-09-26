use product_ipc::workspace::{Lane, LONG_BUDGET_MS};
use product_ipc::{ComponentCall, ExecutionClass};
use serde::Deserialize;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum WorkspaceLogsCallHost {
    PreviewLegacyRuntimeSettings {},
    ApplyLegacyRuntimeSettings {},
    ReconnectRuntimeSources {
        sources: Vec<logs_engine::core::SourceSpec>,
        filter: logs_engine::core::FilterSpec,
    },
    OpenWebhookLog {
        id: String,
        revision: String,
        operation_id: String,
    },
    SendSelectionToToolbox {
        generation: u64,
        keys: Vec<LogSelectionKey>,
    },
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum WorkspaceLogsCall {
    Host(WorkspaceLogsCallHost),
    Engine(Box<logs_engine::api::LogsCall>),
}
pub const METHODS: &[&str] = &[
    "accept_log_source",
    "apply_legacy_runtime_settings",
    "cancel_read",
    "delete_saved_view",
    "discard_log_source",
    "export_log_records",
    "filter_log_records",
    "fixed_adapter",
    "list_saved_views",
    "open_webhook_log",
    "preview_legacy_runtime_settings",
    "preview_log_source",
    "read_source",
    "read_sources",
    "receive_log_source",
    "reconnect_runtime_sources",
    "renew_log_source",
    "save_saved_view",
    "send_selection_to_toolbox",
    "summarize_source",
    "take_pending_open",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "accept_log_source" => &["logs"],
        "apply_legacy_runtime_settings" => &["logs"],
        "cancel_read" => &["logs"],
        "delete_saved_view" => &["logs"],
        "discard_log_source" => &["logs"],
        "export_log_records" => &["logs"],
        "filter_log_records" => &["logs"],
        "fixed_adapter" => &["logs"],
        "list_saved_views" => &["logs"],
        "open_webhook_log" => &["logs"],
        "preview_legacy_runtime_settings" => &["logs"],
        "preview_log_source" => &["logs"],
        "read_source" => &["logs"],
        "read_sources" => &["logs"],
        "receive_log_source" => &["logs"],
        "reconnect_runtime_sources" => &["logs"],
        "renew_log_source" => &["logs"],
        "save_saved_view" => &["logs"],
        "send_selection_to_toolbox" => &["logs"],
        "summarize_source" => &["logs"],
        "take_pending_open" => &["logs"],
        _ => &[],
    }
}
impl WorkspaceLogsCallHost {
    fn method(&self) -> &'static str {
        match self {
            Self::PreviewLegacyRuntimeSettings { .. } => "preview_legacy_runtime_settings",
            Self::ApplyLegacyRuntimeSettings { .. } => "apply_legacy_runtime_settings",
            Self::ReconnectRuntimeSources { .. } => "reconnect_runtime_sources",
            Self::OpenWebhookLog { .. } => "open_webhook_log",
            Self::SendSelectionToToolbox { .. } => "send_selection_to_toolbox",
        }
    }
}
impl ComponentCall for WorkspaceLogsCall {
    const COMPONENT: &'static str = "workspace.logs";
    const MAX_ARGUMENT_BYTES: usize = 67108864;
    const SHARED_REQUEST_LIMIT: bool = false;
    fn valid_arguments(_method: &str, args: &serde_json::Value) -> bool {
        super::bounded_arguments(args, Self::MAX_ARGUMENT_BYTES)
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
    fn class(&self) -> ExecutionClass {
        if matches!(self.lane(), Lane::EngineStop | Lane::TerminalStop) {
            ExecutionClass::Control
        } else {
            ExecutionClass::Normal
        }
    }
}
impl WorkspaceLogsCall {
    pub fn lane(&self) -> Lane {
        match self {
            Self::Engine(call) => call.lane(),
            Self::Host(_) => Lane::Engine,
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
#[tauri::command]
pub(crate) async fn logs(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<WorkspaceLogsCall>(window, runtime, request).await
}
impl super::WorkspaceCall for WorkspaceLogsCall {
    fn into_call(self) -> super::Call {
        super::Call::Logs(self)
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LogSelectionKey {
    pub source_id: String,
    pub sequence: u64,
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    use super::results::*;
    export.register::<WorkspaceLogsCall>()?;
    let mut results = logs_engine::api::result_types(export)?;
    results.retain(|(method, _)| METHODS.contains(method));
    results.retain(|(method, _)| *method != "reconnect_runtime_sources");
    results.push((
        "reconnect_runtime_sources",
        export.register::<ReconnectedLogs>()?,
    ));
    results.retain(|(method, _)| *method != "open_webhook_log");
    results.push(("open_webhook_log", export.register::<serde_json::Value>()?));
    results.retain(|(method, _)| *method != "send_selection_to_toolbox");
    results.push((
        "send_selection_to_toolbox",
        export.register::<SelectionReply>()?,
    ));
    results.retain(|(method, _)| *method != "preview_legacy_runtime_settings");
    results.push((
        "preview_legacy_runtime_settings",
        export.register::<serde_json::Value>()?,
    ));
    results.retain(|(method, _)| *method != "apply_legacy_runtime_settings");
    results.push((
        "apply_legacy_runtime_settings",
        export.register::<serde_json::Value>()?,
    ));
    results.sort_by_key(|(method, _)| *method);
    Ok(results)
}
pub const fn deadline_budget_for(_method: &str) -> u64 {
    LONG_BUDGET_MS
}
