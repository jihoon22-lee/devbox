//! Thin Tauri commands for persisted LSP settings and live sessions.

use crate::lsp::{
    CompletionResult, DiagnosticResult, DidChange, DidClose, DidOpen, DidSave, FeatureResponse,
    FilteredLocations, LanguageServerStatus, LoadedLspConfig, LspConfig, LspManager, LspPosition,
    SanitizedHover,
};
use std::path::Path;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn load_lsp_config(
    manager: State<'_, Arc<LspManager>>,
) -> Result<LoadedLspConfig, String> {
    manager.load_config().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_lsp_config(
    manager: State<'_, Arc<LspManager>>,
    config: LspConfig,
    recover_invalid: bool,
) -> Result<(), String> {
    manager
        .save_config(&config, recover_invalid)
        .map_err(|error| error.to_string())?;
    manager.stop_all().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn start_language_server(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
) -> Result<(), String> {
    manager
        .start(&language_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_language_server(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
) -> Result<(), String> {
    manager
        .stop(&language_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_all_language_servers(manager: State<'_, Arc<LspManager>>) -> Result<(), String> {
    manager.stop_all().await.map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn language_server_statuses(
    manager: State<'_, Arc<LspManager>>,
) -> Result<Vec<LanguageServerStatus>, String> {
    Ok(manager.statuses().await)
}

#[tauri::command]
pub async fn open_lsp_document(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    path: String,
    text: String,
) -> Result<DidOpen, String> {
    manager
        .open_document(&language_id, Path::new(&path), text)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn change_lsp_document(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
    text: String,
    dirty: bool,
) -> Result<DidChange, String> {
    manager
        .change_document(&language_id, &uri, text, dirty)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_lsp_document(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
) -> Result<DidSave, String> {
    manager
        .save_document(&language_id, &uri)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn close_lsp_document(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
) -> Result<DidClose, String> {
    manager
        .close_document(&language_id, &uri)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn pull_lsp_diagnostics(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
) -> Result<FeatureResponse<DiagnosticResult>, String> {
    manager
        .pull_diagnostics(&language_id, &uri)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn request_lsp_completion(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
    position: LspPosition,
) -> Result<FeatureResponse<CompletionResult>, String> {
    manager
        .completion(&language_id, &uri, position)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn request_lsp_hover(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
    position: LspPosition,
) -> Result<FeatureResponse<Option<SanitizedHover>>, String> {
    manager
        .hover(&language_id, &uri, position)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn request_lsp_definition(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
    position: LspPosition,
) -> Result<FeatureResponse<FilteredLocations>, String> {
    manager
        .definition(&language_id, &uri, position)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn request_lsp_references(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
    position: LspPosition,
    include_declaration: bool,
) -> Result<FeatureResponse<FilteredLocations>, String> {
    manager
        .references(&language_id, &uri, position, include_declaration)
        .await
        .map_err(|error| error.to_string())
}
