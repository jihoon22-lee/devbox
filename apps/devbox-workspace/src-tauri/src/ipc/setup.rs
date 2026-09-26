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
pub enum SetupCall {
    Status {},
    StartEmpty {},
}
pub const METHODS: &[&str] = &["start_empty", "status"];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    match method {
        "start_empty" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        "status" => &[
            "dependencies",
            "files",
            "logs",
            "overview",
            "runtime",
            "source",
            "tasks",
        ],
        _ => &[],
    }
}
impl ComponentCall for SetupCall {
    const COMPONENT: &'static str = "workspace.setup";
    const IMPORT_PHASE: bool = true;
    const SHARED_REQUEST_LIMIT: bool = false;
    const MAX_ARGUMENT_BYTES: usize = 65536;
    fn valid_arguments(method: &str, args: &serde_json::Value) -> bool {
        let _ = method;
        let limit = Self::MAX_ARGUMENT_BYTES;
        super::bounded_arguments(args, limit)
    }
    fn method(&self) -> &'static str {
        match self {
            Self::Status { .. } => "status",
            Self::StartEmpty { .. } => "start_empty",
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        routes_for(self.method())
    }
}
impl SetupCall {
    pub fn lane(&self) -> Lane {
        match self {
            Self::Status {} => Lane::Metadata,
            Self::StartEmpty {} => Lane::EngineBackground,
        }
    }
    pub fn deadline_budget_ms(&self) -> u64 {
        deadline_budget_for(self.method())
    }
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.setup", method)
}
#[tauri::command]
pub(crate) async fn setup(
    window: tauri::WebviewWindow,
    runtime: tauri::State<'_, crate::component::Runtime>,
    request: product_ipc::IncomingRequest,
) -> Result<product_shell_tauri::Reply, product_contract::Problem> {
    super::execute::<SetupCall>(window, runtime, request).await
}
impl super::WorkspaceCall for SetupCall {
    fn into_call(self) -> super::Call {
        super::Call::Setup(self)
    }
}

#[derive(serde::Serialize, ts_rs::TS)]
#[serde(tag = "phase", rename_all = "lowercase")]
pub enum SetupStatus {
    Loading,
    Setup,
    Selected,
    Failed { issue: String },
}
#[derive(serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SetupStarted {
    pub selected: bool,
    pub generation: Option<String>,
}

pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<SetupCall>()?;
    let mut result = Vec::new();
    result.retain(|(method, _)| METHODS.contains(method));
    result.retain(|(method, _)| *method != "status");
    result.push(("status", export.register::<SetupStatus>()?));
    result.retain(|(method, _)| *method != "start_empty");
    result.push(("start_empty", export.register::<SetupStarted>()?));
    result.sort_by_key(|(method, _)| *method);
    Ok(result)
}
