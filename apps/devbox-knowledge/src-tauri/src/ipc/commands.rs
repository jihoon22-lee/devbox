use product_contract::Problem;
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest};
use product_shell_tauri::{admit_request, Reply};
use serde::Deserialize;
use tauri::{Manager, WebviewWindow};
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum QuitCall {
    LifecycleStatus {},
    PendingQuit {},
    DecideQuit { id: String, quit: bool },
}
impl ComponentCall for QuitCall {
    const COMPONENT: &'static str = "knowledge.commands";
    const INSTALLATION_REVIEW: bool = true;
    fn method(&self) -> &'static str {
        match self {
            Self::LifecycleStatus {} => "lifecycle_status",
            Self::PendingQuit {} => "pending_quit",
            Self::DecideQuit { .. } => "decide_quit",
        }
    }
    fn class(&self) -> ExecutionClass {
        ExecutionClass::Control
    }
    fn routes(&self) -> &'static [&'static str] {
        &["notes", "activity"]
    }
}
product_ipc::issue_codes! {
    pub enum QuitIssue {
    Unavailable = "unavailable",
    QuitUnavailable = "quit_unavailable",
    QuitReviewStale = "quit_review_stale",
    }
}
fn classify(error: &str) -> &'static str {
    QuitIssue::from_code(error)
        .unwrap_or(QuitIssue::Unavailable)
        .code()
}
#[tauri::command]
pub async fn commands(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<QuitCall>(&window, request)?;
    Ok(admission.finish(
        match request.call {
            QuitCall::LifecycleStatus {} => {
                crate::lifecycle::collector_status(window.app_handle()).await
            }
            call => crate::lifecycle::quit_dispatch_typed(window.app_handle(), call),
        },
        classify,
    ))
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<QuitCall>()?;
    export.register::<QuitIssue>()?;
    Ok(vec![
        (
            "lifecycle_status",
            export.register::<crate::lifecycle::CollectorStatus>()?,
        ),
        ("pending_quit", export.register::<Option<String>>()?),
        ("decide_quit", export.register::<()>()?),
    ])
}
