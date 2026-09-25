use http_client_engine::api::{self, ApiCall};
use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::Deserialize;
use tauri::{Manager, WebviewWindow};
type SelfOwner = StudioApiCall;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum ApiHostCall {
    PickMultipartFile {},
    SendSelectionToToolbox { text: String },
    SendMockDraft(crate::core::mock_draft::MockDraftInput),
}
impl ApiHostCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::PickMultipartFile { .. } => "pick_multipart_file",
            Self::SendSelectionToToolbox { .. } => "send_selection_to_toolbox",
            Self::SendMockDraft(..) => "send_mock_draft",
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum HostApiCall {
    Workspace(super::workspace::WorkspaceCall),
    Knowledge(super::knowledge::KnowledgeCall),
    Navigation(crate::handoff::NavigationCall),
    Extra(ApiHostCall),
}
impl HostApiCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Workspace(call) => call.method(),
            Self::Knowledge(call) => call.method(),
            Self::Navigation(call) => call.method(),
            Self::Extra(call) => call.method(),
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum StudioApiCall {
    Host(HostApiCall),
    Engine(Box<ApiCall>),
}
impl ComponentCall for StudioApiCall {
    const COMPONENT: &'static str = "api-studio.api";
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        match self {
            Self::Host(
                HostApiCall::Knowledge(_)
                | HostApiCall::Extra(
                    ApiHostCall::SendSelectionToToolbox { .. } | ApiHostCall::SendMockDraft(_),
                ),
            ) => &["requests", "history"],
            _ => &["requests", "protocols", "history"],
        }
    }
    fn class(&self) -> ExecutionClass {
        match self {
            Self::Engine(call) => call.class(),
            _ => ExecutionClass::Normal,
        }
    }
}
#[tauri::command]
pub async fn api(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<StudioApiCall>(&window, request)?;
    let app = window.app_handle();
    crate::lifecycle::require_open(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    let result = match request.call {
        StudioApiCall::Engine(call) => api::dispatch(app, *call).await,
        StudioApiCall::Host(call) => {
            host(
                app,
                call,
                admission.provenance().clone(),
                request.header.deadline_ms,
                request.header.context,
            )
            .await
        }
    };
    Ok(admission.finish(result, api::classify))
}
async fn host(
    app: &tauri::AppHandle,
    call: HostApiCall,
    provenance: product_contract::Provenance,
    deadline: u64,
    context: Option<product_contract::ProjectContext>,
) -> Result<serde_json::Value, String> {
    match call {
        HostApiCall::Workspace(call) => {
            crate::api_workspace::dispatch_typed(app, call, context).await
        }
        HostApiCall::Knowledge(call) => {
            crate::knowledge::dispatch_typed(app, SelfOwner::COMPONENT, call, provenance, deadline)
                .await
        }
        HostApiCall::Navigation(call) => crate::handoff::navigation_typed(app, call),
        HostApiCall::Extra(call) => match call {
            ApiHostCall::PickMultipartFile {} => {
                use tauri_plugin_dialog::DialogExt;
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    app.dialog()
                        .file()
                        .set_title("multipart 파일 선택")
                        .blocking_pick_file()
                        .map(|file| {
                            file.into_path()
                                .map(|path| path.to_string_lossy().into_owned())
                                .map_err(|_| "file_selection_unavailable".to_string())
                        })
                        .transpose()
                        .and_then(|v| {
                            serde_json::to_value(v).map_err(|_| "file_selection_unavailable".into())
                        })
                })
                .await
                .unwrap_or_else(|_| Err("file_selection_unavailable".into()))
            }
            ApiHostCall::SendSelectionToToolbox { text } => crate::handoff::send_typed(
                app,
                SelfOwner::COMPONENT,
                crate::handoff::SendCall::Selection { text },
                provenance,
            ),
            ApiHostCall::SendMockDraft(input) => crate::handoff::send_typed(
                app,
                SelfOwner::COMPONENT,
                crate::handoff::SendCall::Mock(input),
                provenance,
            ),
        },
    }
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<StudioApiCall>()?;
    export.register::<api::ApiIssue>()?;
    let mut results = api::result_types(export)?;
    results.extend(super::workspace::result_types(export)?);
    results.extend(super::knowledge::result_types(export)?);
    results.extend([
        ("pick_multipart_file", export.register::<Option<String>>()?),
        (
            "send_selection_to_toolbox",
            export.register::<crate::handoff::HandoffResult>()?,
        ),
        (
            "send_mock_draft",
            export.register::<crate::handoff::HandoffResult>()?,
        ),
        (
            "peek_pending_navigation",
            export.register::<Option<crate::handoff::Navigation>>()?,
        ),
        ("ack_pending_navigation", export.register::<()>()?),
    ]);
    Ok(results)
}
