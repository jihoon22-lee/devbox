use product_ipc::workspace::Lane;
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
pub enum CommandsCall {}
pub const METHODS: &[&str] = &[];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        _ => &[],
    }
}
impl ComponentCall for CommandsCall {
    const COMPONENT: &'static str = "workspace.commands";
    const MAX_ARGUMENT_BYTES: usize = 65536;
    const SHARED_REQUEST_LIMIT: bool = false;
    fn valid_arguments(_method: &str, args: &serde_json::Value) -> bool {
        super::bounded_arguments(args, Self::MAX_ARGUMENT_BYTES)
    }
    fn method(&self) -> &'static str {
        match *self {}
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
impl CommandsCall {
    pub fn lane(&self) -> Lane {
        Lane::Metadata
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
#[tauri::command]
pub(crate) async fn commands(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<CommandsCall>(window, runtime, request).await
}
impl super::WorkspaceCall for CommandsCall {
    fn into_call(self) -> super::Call {
        super::Call::Commands(self)
    }
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<CommandsCall>()?;
    let mut results = Vec::new();
    results.retain(|(method, _)| METHODS.contains(method));
    results.sort_by_key(|(method, _)| *method);
    Ok(results)
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.commands", method)
}
