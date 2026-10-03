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
pub enum CommandsCall {
    PrepareClose {},
    ConfirmClose { nonce: String },
    CancelClose { nonce: String },
}
pub const METHODS: &[&str] = &["prepare_close", "confirm_close", "cancel_close"];
pub fn routes_for(method: &str) -> &'static [&'static str] {
    if METHODS.contains(&method) {
        &[
            "overview",
            "files",
            "source",
            "dependencies",
            "tasks",
            "runtime",
            "logs",
            "terminal",
            "agents",
            "problems",
        ]
    } else {
        &[]
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
        match self {
            Self::PrepareClose {} => "prepare_close",
            Self::ConfirmClose { .. } => "confirm_close",
            Self::CancelClose { .. } => "cancel_close",
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
    let mut results = vec![
        (
            "prepare_close",
            export.register::<crate::core::close_review::CloseRequest>()?,
        ),
        ("confirm_close", "null".into()),
        ("cancel_close", "null".into()),
    ];
    results.retain(|(method, _)| METHODS.contains(method));
    results.sort_by_key(|(method, _)| *method);
    Ok(results)
}
pub fn deadline_budget_for(method: &str) -> u64 {
    super::deadlines::budget("workspace.commands", method)
}

pub(crate) fn dispatch(
    window: &tauri::WebviewWindow,
    runtime: &crate::component::Runtime,
    call: CommandsCall,
) -> Result<serde_json::Value, &'static str> {
    use tauri::Manager;
    if window.label() != "main" {
        return Err("close_review_unavailable");
    }
    let context = product_shell_tauri::workspace_context(window)?;
    let mut review = runtime
        .close_review
        .lock()
        .map_err(|_| "close_review_unavailable")?;
    match call {
        CommandsCall::PrepareClose {} => {
            serde_json::to_value(review.request(context)).map_err(|_| "close_review_unavailable")
        }
        CommandsCall::CancelClose { nonce } => {
            review.cancel(&nonce)?;
            Ok(serde_json::Value::Null)
        }
        CommandsCall::ConfirmClose { nonce } => {
            review.confirm(&nonce, context.as_ref())?;
            drop(review);
            window.app_handle().exit(0);
            Ok(serde_json::Value::Null)
        }
    }
}
