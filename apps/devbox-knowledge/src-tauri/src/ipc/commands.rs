use product_contract::Problem;
use product_ipc::{ComponentCall, IncomingRequest};
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
pub enum QuitCall {
    PendingQuit {},
    DecideQuit { id: String, quit: bool },
}
impl ComponentCall for QuitCall {
    const COMPONENT: &'static str = "knowledge.commands";
    const INSTALLATION_REVIEW: bool = true;
    fn method(&self) -> &'static str {
        match self {
            Self::PendingQuit {} => "pending_quit",
            Self::DecideQuit { .. } => "decide_quit",
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["notes"]
    }
}
product_ipc::issue_codes! {pub enum QuitIssue {Unavailable="unavailable",QuitUnavailable="quit_unavailable",QuitReviewStale="quit_review_stale"}}
fn classify(error: &str) -> &'static str {
    QuitIssue::from_code(error)
        .unwrap_or(QuitIssue::Unavailable)
        .code()
}
#[tauri::command]
pub async fn commands(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<QuitCall>(&window, request)?;
    Ok(admission.finish(
        crate::lifecycle::quit_dispatch_typed(window.app_handle(), request.call),
        classify,
    ))
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<QuitCall>()?;
    export.register::<QuitIssue>()?;
    Ok(vec![
        ("pending_quit", export.register::<Option<String>>()?),
        ("decide_quit", export.register::<()>()?),
    ])
}
