use knowledge_vault_engine::api::{self, DailyCall, NotesCall};
use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Manager, WebviewWindow};
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HostNotesCall {
    ReadClipboardText {},
    OpenExternalUrl {
        url: String,
    },
    SetRoot {
        path: String,
    },
    OpenTargets {},
    OpenIn {
        app_id: String,
        rel: String,
    },
    PreviewSessionSummary {
        source_id: String,
        operation_id: String,
        revision: String,
    },
    OpenSessionSummary {
        source_id: String,
        operation_id: String,
        revision: String,
    },
    OpenResultDraft {
        id: String,
        operation_id: String,
        revision: String,
    },
}
impl HostNotesCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::ReadClipboardText { .. } => "read_clipboard_text",
            Self::OpenExternalUrl { .. } => "open_external_url",
            Self::SetRoot { .. } => "set_root",
            Self::OpenTargets { .. } => "open_targets",
            Self::OpenIn { .. } => "open_in",
            Self::PreviewSessionSummary { .. } => "preview_session_summary",
            Self::OpenSessionSummary { .. } => "open_session_summary",
            Self::OpenResultDraft { .. } => "open_result_draft",
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum KnowledgeNotesCall {
    Host(HostNotesCall),
    Daily(DailyCall),
    Engine(NotesCall),
}
impl ComponentCall for KnowledgeNotesCall {
    const COMPONENT: &'static str = "knowledge.notes";
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Daily(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["notes", "daily"]
    }
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenTargetChoice {
    pub id: String,
    pub display_name: String,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum DraftPublicationState {
    Saved,
    Prepared,
    PreviewPending,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct SessionSummaryReply {
    pub state: DraftPublicationState,
    pub draft: product_contract::session_summary::Draft,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct ResultDraftReply {
    pub state: DraftPublicationState,
}
#[tauri::command]
pub async fn notes(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<KnowledgeNotesCall>(&window, request)?;
    let app = window.app_handle();
    crate::startup::require_active(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    let result = match request.call {
        KnowledgeNotesCall::Host(call) => host(app, call, request.header.deadline_ms).await,
        call => {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || match call {
                KnowledgeNotesCall::Engine(call) => {
                    tauri::async_runtime::block_on(api::dispatch(&app, call))
                }
                KnowledgeNotesCall::Daily(call) => api::daily_dispatch(&app, call),
                KnowledgeNotesCall::Host(_) => unreachable!("host calls handled before worker"),
            })
            .await
            .unwrap_or_else(|_| Err("component_worker_unavailable".into()))
        }
    };
    Ok(admission.finish(result, api::classify))
}
async fn host(app: &tauri::AppHandle, call: HostNotesCall, deadline: u64) -> Result<Value, String> {
    match call {
        HostNotesCall::ReadClipboardText {} => {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            app.clipboard()
                .read_text()
                .map(Value::String)
                .map_err(|_| "clipboard_unavailable".into())
        }
        HostNotesCall::OpenExternalUrl { url } => {
            use tauri_plugin_opener::OpenerExt;
            if url.len() > 8192
                || url.chars().any(char::is_control)
                || !(url.starts_with("https://") || url.starts_with("http://"))
            {
                return Err("component_args_invalid".into());
            }
            app.opener()
                .open_url(url, None::<&str>)
                .map(|_| Value::Null)
                .map_err(|_| "opener_unavailable".into())
        }
        HostNotesCall::SetRoot { .. } => Err("vault_binding_unavailable".into()),
        HostNotesCall::OpenTargets {} => Ok(serde_json::json!([])),
        HostNotesCall::OpenIn { .. } => Err("provider_unavailable".into()),
        HostNotesCall::PreviewSessionSummary {
            source_id,
            operation_id,
            revision,
        } => crate::session_receive::dispatch_typed(
            app,
            false,
            source_id,
            operation_id,
            revision,
            deadline,
        )
        .await
        .and_then(super::typed::<SessionSummaryReply>),
        HostNotesCall::OpenSessionSummary {
            source_id,
            operation_id,
            revision,
        } => crate::session_receive::dispatch_typed(
            app,
            true,
            source_id,
            operation_id,
            revision,
            deadline,
        )
        .await
        .and_then(super::typed::<SessionSummaryReply>),
        HostNotesCall::OpenResultDraft {
            id,
            operation_id,
            revision,
        } => crate::result_receive::open_typed(app, id, operation_id, revision, deadline)
            .await
            .and_then(super::typed::<ResultDraftReply>),
    }
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<KnowledgeNotesCall>()?;
    export.register::<api::NotesIssue>()?;
    let mut results = api::result_types(export)?;
    results.extend([
        ("read_clipboard_text", export.register::<String>()?),
        ("open_external_url", export.register::<()>()?),
        ("set_root", export.register::<()>()?),
        ("open_targets", export.register::<Vec<OpenTargetChoice>>()?),
        ("open_in", export.register::<()>()?),
        (
            "preview_session_summary",
            export.register::<SessionSummaryReply>()?,
        ),
        (
            "open_session_summary",
            export.register::<SessionSummaryReply>()?,
        ),
        ("open_result_draft", export.register::<ResultDraftReply>()?),
    ]);
    Ok(results)
}
