use product_ipc::workspace::{Lane, DEFAULT_BUDGET_MS};
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
pub enum ProblemsCall {
    Snapshot {},
    Resolve {
        id: String,
        revision: String,
        #[serde(default)]
        #[ts(as = "Option<bool>", optional)]
        log: bool,
    },
}
pub const METHODS: &[&str] = &["resolve", "snapshot"];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "resolve" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "problems",
            "runtime",
            "source",
            "tasks",
            "terminal",
        ],
        "snapshot" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "problems",
            "runtime",
            "source",
            "tasks",
            "terminal",
        ],
        _ => &[],
    }
}
impl ComponentCall for ProblemsCall {
    const COMPONENT: &'static str = "workspace.problems";
    const MAX_ARGUMENT_BYTES: usize = 65536;
    const SHARED_REQUEST_LIMIT: bool = false;
    fn valid_arguments(_method: &str, args: &serde_json::Value) -> bool {
        super::bounded_arguments(args, Self::MAX_ARGUMENT_BYTES)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::Snapshot { .. } => "snapshot",
            Self::Resolve { .. } => "resolve",
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
impl ProblemsCall {
    pub fn lane(&self) -> Lane {
        Lane::Metadata
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
#[tauri::command]
pub(crate) async fn problems(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<ProblemsCall>(window, runtime, request).await
}
impl super::WorkspaceCall for ProblemsCall {
    fn into_call(self) -> super::Call {
        super::Call::Problems(self)
    }
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    use super::results::*;
    export.register::<ProblemsCall>()?;
    let mut results = Vec::new();
    results.retain(|(method, _)| METHODS.contains(method));
    results.retain(|(method, _)| *method != "snapshot");
    results.push(("snapshot", export.register::<ProblemsReply>()?));
    results.retain(|(method, _)| *method != "resolve");
    results.push(("resolve", export.register::<ProblemResolution>()?));
    results.sort_by_key(|(method, _)| *method);
    Ok(results)
}
pub const fn deadline_budget_for(_method: &str) -> u64 {
    DEFAULT_BUDGET_MS
}
