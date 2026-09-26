//! Product-owned documents with fixed issue projection and route admission.
use crate::core::documents::{DocumentKind, DocumentStore, Stored, MAX_DOCUMENT_BYTES};
use product_contract::{Problem, ProblemCode};
use product_ipc::{ComponentCall, IncomingRequest};
use product_shell_tauri::{admit_request, Reply};
use serde::Deserialize;
use std::sync::Arc;
use tauri::Manager;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(optional_fields = nullable)]
pub enum StoreCall {
    Load {
        kind: DocumentKind,
    },
    Save {
        kind: DocumentKind,
        body: String,
        expected_revision: Option<u64>,
    },
}
impl ComponentCall for StoreCall {
    const COMPONENT: &'static str = "api-studio.store";
    // Allow bounded oversize inputs to receive the document-specific issue.
    const MAX_ARGUMENT_BYTES: usize = MAX_DOCUMENT_BYTES * 2;
    fn valid_arguments(_method: &str, args: &serde_json::Value) -> bool {
        args.as_object().is_some_and(|args| {
            args.len() <= 3
                && args.iter().all(|(key, value)| match key.as_str() {
                    "kind" => value.as_str().is_some_and(|kind| kind.len() <= 32),
                    "body" => value
                        .as_str()
                        .is_some_and(|body| body.len() <= Self::MAX_ARGUMENT_BYTES),
                    "expectedRevision" => value.is_null() || value.as_u64().is_some(),
                    _ => false,
                })
        })
    }
    fn method(&self) -> &'static str {
        match self {
            Self::Load { .. } => "load",
            Self::Save { .. } => "save",
        }
    }
    fn routes(&self) -> &'static [&'static str] {
        &["requests", "protocols", "webhooks", "transforms", "history"]
    }
}
product_ipc::issue_codes! {
    pub enum StoreIssue {
        StoreUnavailable = "store_unavailable",
        RevisionConflict = "store_revision_conflict",
        TooLarge = "store_document_too_large",
        Invalid = "store_document_invalid",
    }
}
pub fn classify(issue: &str) -> &'static str {
    StoreIssue::from_code(issue)
        .unwrap_or(StoreIssue::StoreUnavailable)
        .code()
}
struct State(Result<(Arc<DocumentStore>, std::path::PathBuf), String>);
pub(crate) fn initialize(app: &tauri::AppHandle) {
    let opened = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "store_unavailable".to_owned())
        .and_then(|root| {
            std::fs::create_dir_all(&root).map_err(|_| "store_unavailable".to_owned())?;
            DocumentStore::open(&root.join("api-store.db")).map(|store| (Arc::new(store), root))
        });
    // Keep the shell available on disk/open failure so migration preserves its source.
    app.manage(State(opened));
}
#[tauri::command]
pub async fn store(
    window: tauri::WebviewWindow,
    request: IncomingRequest,
) -> Result<Reply, Problem> {
    let (admission, request) = admit_request::<StoreCall>(&window, request)?;
    let app = window.app_handle();
    crate::lifecycle::require_open(app).map_err(|_| admission.problem(ProblemCode::Unavailable))?;
    let store = app.state::<State>().0.clone();
    let app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        crate::lifecycle::require_open(&app)?;
        let (store, root) = store?;
        if matches!(
            &request.call,
            StoreCall::Load {
                kind: DocumentKind::Workflows
            } | StoreCall::Save {
                kind: DocumentKind::Workflows,
                ..
            }
        ) {
            store.import_legacy_workflows(&root)?;
        }
        match request.call {
            StoreCall::Load { kind } => serde_json::to_value(store.load(kind)?),
            StoreCall::Save {
                kind,
                body,
                expected_revision,
            } => serde_json::to_value(store.save(kind, &body, expected_revision)?),
        }
        .map_err(|_| "store_unavailable".to_owned())
    })
    .await
    .unwrap_or_else(|_| Err("store_unavailable".into()));
    Ok(admission.finish(result, classify))
}
pub fn result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    export.register::<StoreCall>()?;
    export.register::<StoreIssue>()?;
    Ok(vec![
        ("load", export.register::<Option<Stored>>()?),
        ("save", export.register::<u64>()?),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use product_ipc::ComponentCall;

    #[test]
    fn store_calls_parse_and_are_allowed_from_every_route() {
        let call: StoreCall = serde_json::from_str(r#"{"method":"save","args":{"kind":"collections","body":"{}","expectedRevision":null}}"#).unwrap();
        assert_eq!(call.method(), "save");
        assert_eq!(
            call.routes(),
            &["requests", "protocols", "webhooks", "transforms", "history"]
        );
        assert!(serde_json::from_str::<StoreCall>(
            r#"{"method":"load","args":{"kind":"secrets"}}"#
        )
        .is_err());
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    #[test]
    fn store_admission_rejects_arbitrary_paths_and_projects_only_fixed_issues() {
        assert!(serde_json::from_str::<StoreCall>(
            r#"{"method":"load","args":{"kind":"history","path":"synthetic-private-path"}}"#
        )
        .is_err());
        assert!(!StoreCall::valid_arguments(
            "save",
            &serde_json::json!({"kind":"history","body":"{}","expectedRevision":null,"owner":"foreign"})
        ));
        assert_eq!(
            classify("store_revision_conflict"),
            "store_revision_conflict"
        );
        assert_eq!(
            classify("disk error synthetic-private-path"),
            "store_unavailable"
        );
        assert_eq!(
            StoreCall::Load {
                kind: DocumentKind::History
            }
            .class(),
            product_ipc::ExecutionClass::Normal
        );
    }
}
