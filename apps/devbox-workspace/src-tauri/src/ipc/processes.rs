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
pub enum ProcessesCallHost {
    PreviewLegacyRuntimeSettings {},
    ApplyLegacyRuntimeSettings {},
    OpenPortOwner {
        action_key: String,
        stream: Option<String>,
    },
    OpenPortLog {
        action_key: String,
        stream: Option<String>,
    },
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum ProcessesCall {
    Host(ProcessesCallHost),
    Engine(Box<ports_engine::api::PortsCall>),
}
pub const METHODS: &[&str] = &[
    "apply_legacy_runtime_settings",
    "get_process_info",
    "handoff_container_stop",
    "list_port_observations",
    "list_ports",
    "load_port_manager_preferences",
    "open_browser",
    "open_port_log",
    "open_port_owner",
    "preview_legacy_runtime_settings",
    "reveal_process",
    "save_port_manager_preferences",
];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "apply_legacy_runtime_settings" => &["runtime"],
        "get_process_info" => &["runtime"],
        "handoff_container_stop" => &["runtime"],
        "list_port_observations" => &["runtime"],
        "list_ports" => &["runtime"],
        "load_port_manager_preferences" => &["runtime"],
        "open_browser" => &["runtime"],
        "open_port_log" => &["runtime"],
        "open_port_owner" => &["runtime"],
        "preview_legacy_runtime_settings" => &["runtime"],
        "reveal_process" => &["runtime"],
        "save_port_manager_preferences" => &["runtime"],
        _ => &[],
    }
}
impl ProcessesCallHost {
    fn method(&self) -> &'static str {
        match self {
            Self::PreviewLegacyRuntimeSettings { .. } => "preview_legacy_runtime_settings",
            Self::ApplyLegacyRuntimeSettings { .. } => "apply_legacy_runtime_settings",
            Self::OpenPortOwner { .. } => "open_port_owner",
            Self::OpenPortLog { .. } => "open_port_log",
        }
    }
}
impl ComponentCall for ProcessesCall {
    const COMPONENT: &'static str = "workspace.processes";
    const MAX_ARGUMENT_BYTES: usize = 65536;
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
impl ProcessesCall {
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
pub(crate) async fn processes(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<ProcessesCall>(window, runtime, request).await
}
impl super::WorkspaceCall for ProcessesCall {
    fn into_call(self) -> super::Call {
        super::Call::Processes(self)
    }
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    use super::results::*;
    export.register::<ProcessesCall>()?;
    let mut results = ports_engine::api::result_types(export)?;
    results.retain(|(method, _)| METHODS.contains(method));
    results.retain(|(method, _)| *method != "open_port_owner");
    results.push(("open_port_owner", export.register::<()>()?));
    results.retain(|(method, _)| *method != "open_port_log");
    results.push(("open_port_log", export.register::<PortLogReply>()?));
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
