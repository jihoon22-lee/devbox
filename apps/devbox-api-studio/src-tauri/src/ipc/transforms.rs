use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::Deserialize;
use tauri::{Manager, WebviewWindow};
use toolbox_engine::api::{self, ToolboxCall};
type SelfOwner = StudioTransformCall;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TransformHostCall {
    OpenWorkspaceSelection {
        id: String,
        operation_id: String,
        revision: String,
    },
    ReadClipboardText {},
    SendMockDraft(crate::core::mock_draft::MockDraftInput),
    CreateApiRequestHandoff {
        source: transforms_core::core::export_policy::OutputSource,
        output: String,
    },
}
impl TransformHostCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::OpenWorkspaceSelection { .. } => "open_workspace_selection",
            Self::ReadClipboardText { .. } => "read_clipboard_text",
            Self::SendMockDraft(..) => "send_mock_draft",
            Self::CreateApiRequestHandoff { .. } => "create_api_request_handoff",
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum HostTransformCall {
    Knowledge(super::knowledge::KnowledgeCall),
    Extra(TransformHostCall),
}
impl HostTransformCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Knowledge(call) => call.method(),
            Self::Extra(call) => call.method(),
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
pub enum StudioTransformCall {
    Host(HostTransformCall),
    Engine(ToolboxCall),
}
impl ComponentCall for StudioTransformCall {
    const COMPONENT: &'static str = "api-studio.transforms";
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["transforms"]
    }
    fn class(&self) -> ExecutionClass {
        match self {
            Self::Engine(call) => call.class(),
            _ => ExecutionClass::Normal,
        }
    }
}
#[derive(serde::Serialize, ts_rs::TS)]
pub struct SelectionOpened {
    pub state: String,
}
#[tauri::command]
pub async fn transforms(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<StudioTransformCall>(&window, request)?;
    let app = window.app_handle();
    crate::lifecycle::require_open(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    let result = match request.call {
        StudioTransformCall::Engine(call) => api::dispatch(app, call).await,
        StudioTransformCall::Host(call) => {
            host(
                app,
                call,
                admission.provenance().clone(),
                request.header.deadline_ms,
            )
            .await
        }
    };
    Ok(admission.finish(result, api::classify))
}
async fn host(
    app: &tauri::AppHandle,
    call: HostTransformCall,
    provenance: product_contract::Provenance,
    deadline: u64,
) -> Result<serde_json::Value, String> {
    match call {
        HostTransformCall::Knowledge(call) => {
            crate::knowledge::dispatch_typed(app, SelfOwner::COMPONENT, call, provenance, deadline)
                .await
        }
        HostTransformCall::Extra(call) => match call {
            TransformHostCall::OpenWorkspaceSelection {
                id,
                operation_id,
                revision,
            } => {
                crate::selection_receive::open_typed(app, id, operation_id, revision, deadline)
                    .await
            }
            TransformHostCall::ReadClipboardText {} => {
                use tauri_plugin_clipboard_manager::ClipboardExt;
                app.clipboard()
                    .read_text()
                    .map(serde_json::Value::String)
                    .map_err(|_| "clipboard_unavailable".into())
            }
            TransformHostCall::SendMockDraft(input) => crate::handoff::send_typed(
                app,
                SelfOwner::COMPONENT,
                crate::handoff::SendCall::Mock(input),
                provenance,
            ),
            TransformHostCall::CreateApiRequestHandoff { source, output } => {
                crate::handoff::send_typed(
                    app,
                    SelfOwner::COMPONENT,
                    crate::handoff::SendCall::TransformRequest { source, output },
                    provenance,
                )
            }
        },
    }
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<StudioTransformCall>()?;
    export.register::<api::ToolboxIssue>()?;
    let mut results = api::result_types(export)?;
    results.extend(super::knowledge::result_types(export)?);
    results.extend([
        (
            "open_workspace_selection",
            export.register::<SelectionOpened>()?,
        ),
        ("read_clipboard_text", export.register::<String>()?),
        (
            "send_mock_draft",
            export.register::<crate::handoff::HandoffResult>()?,
        ),
        (
            "create_api_request_handoff",
            export.register::<crate::handoff::HandoffResult>()?,
        ),
    ]);
    Ok(results)
}
