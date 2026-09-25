use content_index_engine::api::{self, SearchCall, SearchSettingsCall};
use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, ExecutionClass, IncomingRequest, TypeExporter};
use product_shell_tauri::{admit_request, Reply};
use serde::{Deserialize, Serialize};
use tauri::{Manager, WebviewWindow};
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum HostSearchCall {
    SourceQuery {
        source: String,
        query: String,
        mode: String,
        limit: Option<i64>,
        #[serde(default)]
        filter: content_index_engine::component::query::SearchFilter,
    },
    SourcePoll {
        generation: String,
    },
    SourceCancel {
        generation: String,
    },
    SourceReference {
        reference: String,
    },
    SourceSavedReference {
        id: String,
        revision: String,
    },
}
impl HostSearchCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::SourceQuery { .. } => "source_query",
            Self::SourcePoll { .. } => "source_poll",
            Self::SourceCancel { .. } => "source_cancel",
            Self::SourceReference { .. } => "source_reference",
            Self::SourceSavedReference { .. } => "source_saved_reference",
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(untagged)]
#[ts(optional_fields = nullable)]
pub enum KnowledgeSearchCall {
    Host(HostSearchCall),
    Engine(SearchCall),
}
impl ComponentCall for KnowledgeSearchCall {
    const COMPONENT: &'static str = "knowledge.search";
    fn method(&self) -> &'static str {
        match self {
            Self::Host(call) => call.method(),
            Self::Engine(call) => call.method(),
        }
    }
    fn class(&self) -> ExecutionClass {
        match self {
            Self::Host(HostSearchCall::SourceCancel { .. }) => ExecutionClass::Control,
            _ => ExecutionClass::Normal,
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        match self {
            Self::Engine(SearchCall::SearchFiles { .. } | SearchCall::SearchContent { .. }) => &[],
            _ => &["search"],
        }
    }
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(optional_fields = nullable)]
pub struct KnowledgeSearchSettingsCall(pub SearchSettingsCall);
impl ComponentCall for KnowledgeSearchSettingsCall {
    const COMPONENT: &'static str = "knowledge.search-settings";
    fn method(&self) -> &'static str {
        self.0.method()
    }
    fn class(&self) -> ExecutionClass {
        self.0.class()
    }
    fn routes(&self) -> &'static [&'static str] {
        &["search"]
    }
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct SourceReference {
    pub reference: String,
    pub source: String,
    pub name: String,
    pub path: std::path::PathBuf,
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum KnowledgeOpenerCall {
    OpenFile { reference: String },
    RevealFile { reference: String },
    OpenTargets {},
    OpenIn { app_id: String, reference: String },
}
impl ComponentCall for KnowledgeOpenerCall {
    const COMPONENT: &'static str = "knowledge.opener";
    fn method(&self) -> &'static str {
        match self {
            Self::OpenFile { .. } => "open_file",
            Self::RevealFile { .. } => "reveal_file",
            Self::OpenTargets {} => "open_targets",
            Self::OpenIn { .. } => "open_in",
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["search"]
    }
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
pub struct OpenedSource {
    pub owner: String,
}
#[tauri::command]
pub async fn search(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<KnowledgeSearchCall>(&window, request)?;
    let app = window.app_handle();
    crate::startup::require_active(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    if matches!(&request.call,KnowledgeSearchCall::Host(HostSearchCall::SourceQuery {source,..}) if source=="current_project")
    {
        crate::project_provider::refresh(app, request.header.deadline_ms).await;
    }
    let method = request.call.method();
    let result = match request.call {
        KnowledgeSearchCall::Host(HostSearchCall::SourceSavedReference { id, revision }) => {
            crate::federation::saved_reference_typed(app, id, revision).await
        }
        KnowledgeSearchCall::Host(call) => crate::search::dispatch_typed(app, call),
        KnowledgeSearchCall::Engine(call) => api::dispatch_search(app, call).await,
    };
    let result = result.and_then(|value| match method {
        "source_reference" => super::typed::<SourceReference>(value),
        _ => Ok(value),
    });
    Ok(admission.finish(result, api::classify))
}
#[tauri::command]
pub async fn search_settings(
    window: WebviewWindow,
    request: IncomingRequest,
) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<KnowledgeSearchSettingsCall>(&window, request)?;
    let app = window.app_handle();
    crate::startup::require_active(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    Ok(admission.finish(
        api::dispatch_settings(app, request.call.0).await,
        api::classify,
    ))
}
#[tauri::command]
pub async fn opener(window: WebviewWindow, request: IncomingRequest) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<KnowledgeOpenerCall>(&window, request)?;
    let app = window.app_handle();
    crate::startup::require_active(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    crate::project_provider::refresh(app, request.header.deadline_ms).await;
    let result = match request.call {
        KnowledgeOpenerCall::OpenFile { reference } => {
            crate::search::open_typed(app, crate::search::OpenKind::Open, reference).await
        }
        KnowledgeOpenerCall::RevealFile { reference } => {
            crate::search::open_typed(app, crate::search::OpenKind::Reveal, reference).await
        }
        KnowledgeOpenerCall::OpenTargets {} => {
            Ok(serde_json::json!([super::notes::OpenTargetChoice {
                id: "devbox-workspace".into(),
                display_name: if crate::suite::installed_products(app).contains("workspace") {
                    "Workspace Editor"
                } else {
                    "Workspace Editor · 설치·제품 연결 필요"
                }
                .into()
            }]))
        }
        KnowledgeOpenerCall::OpenIn { app_id, reference } => {
            crate::file_send::send_typed(
                app,
                app_id,
                reference,
                &request.header.request_id,
                request.header.deadline_ms,
            )
            .await
        }
    };
    Ok(admission.finish(result, api::classify))
}
pub fn result_types(export: &mut TypeExporter<'_>) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<KnowledgeSearchCall>()?;
    export.register::<api::SearchIssue>()?;
    let mut results = api::search_result_types(export)?;
    results.extend([
        (
            "source_query",
            export.register::<crate::core::source_search::Snapshot>()?,
        ),
        (
            "source_poll",
            export.register::<crate::core::source_search::Snapshot>()?,
        ),
        ("source_cancel", export.register::<()>()?),
        ("source_reference", export.register::<SourceReference>()?),
        (
            "source_saved_reference",
            export.register::<content_index_engine::api::SavedQuery>()?,
        ),
    ]);
    Ok(results)
}
pub fn settings_result_types(
    export: &mut TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<KnowledgeSearchSettingsCall>()?;
    api::settings_result_types(export)
}
pub fn opener_result_types(
    export: &mut TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<KnowledgeOpenerCall>()?;
    Ok(vec![
        ("open_file", export.register::<OpenedSource>()?),
        ("reveal_file", export.register::<OpenedSource>()?),
        (
            "open_targets",
            export.register::<Vec<super::notes::OpenTargetChoice>>()?,
        ),
        (
            "open_in",
            export.register::<product_contract::navigation::Receipt>()?,
        ),
    ])
}
