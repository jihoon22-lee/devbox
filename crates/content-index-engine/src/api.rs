//! Typed index operations. Host admission still forbids unowned raw file queries.
use serde::Deserialize;
use tauri::Manager as _;
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SearchCall {
    TakePendingOpen {},
    WatcherStatuses {},
    SearchFiles {
        query: String,
        limit: Option<i64>,
        filter: Option<crate::core::models::SearchFilter>,
    },
    SearchContent {
        query: String,
        limit: Option<i64>,
        filter: Option<crate::core::models::SearchFilter>,
    },
    ListRoots {},
    IndexStatus {},
    ListSavedQueries {},
}
pub const SEARCH_METHODS: &[&str] = &[
    "take_pending_open",
    "watcher_statuses",
    "search_files",
    "search_content",
    "list_roots",
    "index_status",
    "list_saved_queries",
];
impl SearchCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::TakePendingOpen { .. } => "take_pending_open",
            Self::WatcherStatuses { .. } => "watcher_statuses",
            Self::SearchFiles { .. } => "search_files",
            Self::SearchContent { .. } => "search_content",
            Self::ListRoots { .. } => "list_roots",
            Self::IndexStatus { .. } => "index_status",
            Self::ListSavedQueries { .. } => "list_saved_queries",
        }
    }
}
pub async fn dispatch_search(
    component_app: &tauri::AppHandle,
    call: SearchCall,
) -> Result<serde_json::Value, String> {
    match call {
        SearchCall::TakePendingOpen {} => {
            use crate::applink::*;
            let value = take_pending_open(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        SearchCall::WatcherStatuses {} => {
            use crate::commands::watcher::*;
            let value = watcher_statuses(component_app.state());
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        SearchCall::SearchFiles {
            query,
            limit,
            filter,
        } => {
            use crate::commands::search::*;
            let value = search_files(component_app.state(), query, limit, filter)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        SearchCall::SearchContent {
            query,
            limit,
            filter,
        } => {
            use crate::commands::search::*;
            let value = search_content(component_app.state(), query, limit, filter)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        SearchCall::ListRoots {} => {
            use crate::commands::indexing::*;
            let value = list_roots(component_app.state())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        SearchCall::IndexStatus {} => {
            use crate::commands::indexing::*;
            let value = index_status(component_app.state())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        SearchCall::ListSavedQueries {} => {
            use crate::commands::saved_queries::*;
            let value = list_saved_queries(component_app.state())?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
    }
}
pub fn search_result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        (
            "take_pending_open",
            export.register::<Option<devbox_applink::OpenRequest>>()?,
        ),
        (
            "watcher_statuses",
            export.register::<Vec<crate::core::models::RootStatus>>()?,
        ),
        (
            "search_files",
            export.register::<Vec<crate::core::models::FileEntry>>()?,
        ),
        (
            "search_content",
            export.register::<Vec<crate::core::models::ContentResult>>()?,
        ),
        (
            "list_roots",
            export.register::<Vec<crate::core::models::RootInfo>>()?,
        ),
        (
            "index_status",
            export.register::<crate::core::models::IndexStatus>()?,
        ),
        (
            "list_saved_queries",
            export.register::<Vec<crate::core::models::SavedQuery>>()?,
        ),
    ])
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SearchSettingsCall {
    AddRoot {
        path: String,
        index_content: bool,
    },
    RemoveRoot {
        path: String,
    },
    IndexNow {},
    CancelIndex {},
    SaveSavedQuery {
        request: crate::commands::saved_queries::SaveSavedQueryRequest,
    },
    DeleteSavedQuery {
        id: i64,
    },
}
pub const SETTINGS_METHODS: &[&str] = &[
    "add_root",
    "remove_root",
    "index_now",
    "cancel_index",
    "save_saved_query",
    "delete_saved_query",
];
impl SearchSettingsCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::AddRoot { .. } => "add_root",
            Self::RemoveRoot { .. } => "remove_root",
            Self::IndexNow { .. } => "index_now",
            Self::CancelIndex { .. } => "cancel_index",
            Self::SaveSavedQuery { .. } => "save_saved_query",
            Self::DeleteSavedQuery { .. } => "delete_saved_query",
        }
    }
}
pub async fn dispatch_settings(
    component_app: &tauri::AppHandle,
    call: SearchSettingsCall,
) -> Result<serde_json::Value, String> {
    match call {
        SearchSettingsCall::AddRoot {
            path,
            index_content,
        } => {
            use crate::commands::indexing::*;
            add_root(
                component_app.clone(),
                component_app.state(),
                path,
                index_content,
            )?;
            Ok(serde_json::Value::Null)
        }
        SearchSettingsCall::RemoveRoot { path } => {
            use crate::commands::indexing::*;
            remove_root(component_app.clone(), component_app.state(), path)?;
            Ok(serde_json::Value::Null)
        }
        SearchSettingsCall::IndexNow {} => {
            use crate::commands::indexing::*;
            index_now(component_app.state())?;
            Ok(serde_json::Value::Null)
        }
        SearchSettingsCall::CancelIndex {} => {
            use crate::commands::indexing::*;
            cancel_index(component_app.state())?;
            Ok(serde_json::Value::Null)
        }
        SearchSettingsCall::SaveSavedQuery { request } => {
            use crate::commands::saved_queries::*;
            let value = save_saved_query(component_app.state(), request)?;
            serde_json::to_value(value).map_err(|_| "component_response_invalid".to_owned())
        }
        SearchSettingsCall::DeleteSavedQuery { id } => {
            use crate::commands::saved_queries::*;
            delete_saved_query(component_app.state(), id)?;
            Ok(serde_json::Value::Null)
        }
    }
}
pub fn settings_result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        ("add_root", export.register::<()>()?),
        ("remove_root", export.register::<()>()?),
        ("index_now", export.register::<()>()?),
        ("cancel_index", export.register::<()>()?),
        (
            "save_saved_query",
            export.register::<crate::core::models::SavedQuery>()?,
        ),
        ("delete_saved_query", export.register::<()>()?),
    ])
}
#[derive(Deserialize, ts_rs::TS)]
#[serde(
    tag = "method",
    content = "args",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum OpenerCall {
    OpenFile { path: String },
    RevealFile { path: String },
}
pub const OPENER_METHODS: &[&str] = &["open_file", "reveal_file"];
impl OpenerCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::OpenFile { .. } => "open_file",
            Self::RevealFile { .. } => "reveal_file",
        }
    }
}
pub async fn dispatch_opener(
    component_app: &tauri::AppHandle,
    call: OpenerCall,
) -> Result<serde_json::Value, String> {
    match call {
        OpenerCall::OpenFile { path } => {
            use crate::commands::actions::*;
            open_file(component_app.clone(), component_app.state(), path).await?;
            Ok(serde_json::Value::Null)
        }
        OpenerCall::RevealFile { path } => {
            use crate::commands::actions::*;
            reveal_file(component_app.clone(), component_app.state(), path).await?;
            Ok(serde_json::Value::Null)
        }
    }
}
pub fn opener_result_types(
    export: &mut product_ipc::TypeExporter<'_>,
) -> Result<Vec<(&'static str, String)>, String> {
    Ok(vec![
        ("open_file", export.register::<()>()?),
        ("reveal_file", export.register::<()>()?),
    ])
}
product_ipc::issue_codes! {pub enum SearchIssue {
ComponentArgsInvalid = "component_args_invalid",
ComponentStateConflict = "component_state_conflict",
FileReferenceInvalid = "file_reference_invalid",
KnowledgeCommandInvalid = "knowledge_command_invalid",
KnowledgeCommandUnavailable = "knowledge_command_unavailable",
KnowledgeFileStale = "knowledge_file_stale",
KnowledgeQueryModeUnavailable = "knowledge_query_mode_unavailable",
KnowledgeReferenceInvalid = "knowledge_reference_invalid",
KnowledgeReferenceStale = "knowledge_reference_stale",
KnowledgeSourceBusy = "knowledge_source_busy",
KnowledgeSourceDenied = "knowledge_source_denied",
KnowledgeSourceInvalid = "knowledge_source_invalid",
KnowledgeSourceStale = "knowledge_source_stale",
KnowledgeSourceUnavailable = "knowledge_source_unavailable",
MigrationBusy = "migration_busy",
MigrationUnavailable = "migration_unavailable",
ProviderUnavailable = "provider_unavailable",
QueryCancelled = "query_cancelled",
SearchBusy = "search_busy",
SearchLimit = "search_limit",
SearchSourceDenied = "search_source_denied",
SearchStale = "search_stale",
SearchUnavailable = "search_unavailable",
SetupRequired = "setup_required",ComponentArgsInvalid="component_args_invalid",ComponentResponseInvalid="component_response_invalid",ComponentStateConflict="component_state_conflict",ComponentStateUnavailable="component_state_unavailable",ComponentStorageUnavailable="component_storage_unavailable",ComponentStoreExists="component_store_exists",ImportRowInvalid="import_row_invalid",ProviderUnavailable="provider_unavailable",RootUnavailable="root_unavailable",SearchBusy="search_busy",SearchStale="search_stale",SearchUnavailable="search_unavailable",Unavailable="unavailable",}}
pub fn classify(error: &str) -> &'static str {
    SearchIssue::from_code(error)
        .unwrap_or(SearchIssue::Unavailable)
        .code()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_settings_and_opener_calls_stay_separate() {
        let root = r#"{"method":"add_root","args":{"path":"C:/fixture","indexContent":false}}"#;
        assert!(serde_json::from_str::<SearchCall>(root).is_err());
        assert!(serde_json::from_str::<SearchSettingsCall>(root).is_ok());
        assert!(serde_json::from_str::<OpenerCall>(
            r#"{"method":"reveal_file","args":{"path":"C:/fixture"}}"#
        )
        .is_ok());
        assert_eq!(
            SEARCH_METHODS.len() + SETTINGS_METHODS.len() + OPENER_METHODS.len(),
            15
        );
        assert_eq!(classify("credential: private"), "unavailable");
    }
}

pub use crate::core::models::SavedQuery;
