//! Thin Tauri commands for persisted LSP settings and live sessions.

use crate::lsp::{
    AppliedDocumentEdits, CompletionResult, DiagnosticResult, DidChange, DidClose, DidOpen,
    DidSave, FeatureResponse, FilteredLocations, LanguageServerLog, LanguageServerStatus,
    LoadedLspConfig, LspConfig, LspManager, LspManagerError, LspPosition, RenameApplyResult,
    RenamePreview, SanitizedHover,
};
use std::path::Path;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn load_lsp_config(
    manager: State<'_, Arc<LspManager>>,
) -> Result<LoadedLspConfig, String> {
    let loaded = manager
        .load_config()
        .map_err(|_| "LSP 설정을 불러오지 못했습니다".to_owned())?;
    Ok(public_loaded_config(loaded))
}

#[tauri::command]
pub async fn save_lsp_config(
    manager: State<'_, Arc<LspManager>>,
    config: LspConfig,
    recover_invalid: bool,
) -> Result<(), String> {
    manager
        .save_config(&config, recover_invalid)
        .map_err(|_| "LSP 설정을 저장하지 못했습니다".to_owned())?;
    manager.stop_all().await.map_err(public_control_error)
}

#[tauri::command]
pub async fn start_language_server(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
) -> Result<(), String> {
    manager
        .start(&language_id)
        .await
        .map_err(public_control_error)
}

#[tauri::command]
pub async fn stop_language_server(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
) -> Result<(), String> {
    manager
        .stop(&language_id)
        .await
        .map_err(public_control_error)
}

#[tauri::command]
pub async fn restart_language_server(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
) -> Result<(), String> {
    manager
        .restart(&language_id)
        .await
        .map_err(public_control_error)
}

#[tauri::command]
pub async fn stop_all_language_servers(manager: State<'_, Arc<LspManager>>) -> Result<(), String> {
    manager.stop_all().await.map_err(public_control_error)
}

#[tauri::command]
pub async fn language_server_statuses(
    manager: State<'_, Arc<LspManager>>,
) -> Result<Vec<LanguageServerStatus>, String> {
    Ok(manager.statuses().await)
}

#[tauri::command]
pub async fn language_server_logs(
    manager: State<'_, Arc<LspManager>>,
) -> Result<Vec<LanguageServerLog>, String> {
    Ok(manager.logs().await)
}

fn public_control_error(error: LspManagerError) -> String {
    match error {
        LspManagerError::Config(_) | LspManagerError::Protocol(_) => {
            "언어 서버 작업을 완료하지 못했습니다".into()
        }
        safe => safe.to_string(),
    }
}

fn public_loaded_config(mut loaded: LoadedLspConfig) -> LoadedLspConfig {
    if loaded.error.is_some() {
        loaded.error = Some("저장된 LSP 설정이 손상되었습니다".into());
    }
    loaded
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_load_lsp_config(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = load_lsp_config(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_save_lsp_config(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        config: LspConfig,
        recover_invalid: bool,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    save_lsp_config(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.config,
        input.recover_invalid,
    )
    .await?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_start_language_server(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    start_language_server(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
    )
    .await?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_stop_language_server(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    stop_language_server(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
    )
    .await?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_restart_language_server(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    restart_language_server(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
    )
    .await?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_stop_all_language_servers(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    stop_all_language_servers(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
    )
    .await?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_language_server_statuses(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = language_server_statuses(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_language_server_logs(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = language_server_logs(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_open_lsp_document(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        path: String,
        text: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = open_lsp_document(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.path,
        input.text,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_change_lsp_document(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
        text: String,
        dirty: bool,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = change_lsp_document(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
        input.text,
        input.dirty,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_reload_lsp_document(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
        text: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = reload_lsp_document(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
        input.text,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_save_lsp_document(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = save_lsp_document(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_close_lsp_document(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = close_lsp_document(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_pull_lsp_diagnostics(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = pull_lsp_diagnostics(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_request_lsp_completion(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
        position: LspPosition,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = request_lsp_completion(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
        input.position,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_request_lsp_hover(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
        position: LspPosition,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = request_lsp_hover(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
        input.position,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_request_lsp_definition(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
        position: LspPosition,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = request_lsp_definition(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
        input.position,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_request_lsp_references(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
        position: LspPosition,
        include_declaration: bool,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = request_lsp_references(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
        input.position,
        input.include_declaration,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_request_lsp_rename(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
        position: LspPosition,
        new_name: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = request_lsp_rename(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
        input.position,
        input.new_name,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_apply_lsp_rename(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        plan_id: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = apply_lsp_rename(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.plan_id,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_cancel_lsp_rename(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        plan_id: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = cancel_lsp_rename(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.plan_id,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_discard_lsp_rename(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        plan_id: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = discard_lsp_rename(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.plan_id,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_request_lsp_formatting(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        language_id: String,
        uri: String,
        tab_size: u32,
        insert_spaces: bool,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = request_lsp_formatting(
        _component_app
            .try_state()
            .ok_or("component_state_unavailable")?,
        input.language_id,
        input.uri,
        input.tab_size,
        input.insert_spaces,
    )
    .await?;
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

#[cfg(test)]
mod tests {
    use super::{public_control_error, public_loaded_config, public_rename_error};
    use crate::lsp::{LoadedLspConfig, LspConfig, LspManagerError};

    #[test]
    fn management_errors_do_not_echo_protocol_paths_or_credentials() {
        let secret = r#"C:\Users\dev\private token=raw-secret"#;
        for error in [
            LspManagerError::Config(secret.into()),
            LspManagerError::Protocol(secret.into()),
        ] {
            let public = public_control_error(error);
            assert_eq!(public, "언어 서버 작업을 완료하지 못했습니다");
            assert!(!public.contains("Users"));
            assert!(!public.contains("raw-secret"));
        }
    }

    #[test]
    fn corrupt_config_detail_is_replaced_before_ipc() {
        let loaded = public_loaded_config(LoadedLspConfig {
            config: LspConfig::empty(),
            persist_allowed: false,
            error: Some(r#"invalid file at C:\Users\dev\lsp.json token=secret"#.into()),
        });
        assert_eq!(
            loaded.error.as_deref(),
            Some("저장된 LSP 설정이 손상되었습니다")
        );
    }

    #[test]
    fn rename_errors_do_not_echo_paths_or_server_details() {
        let secret = r#"C:\Users\dev\workspace\token=raw-secret"#;
        assert_eq!(
            public_rename_error(LspManagerError::Protocol(secret.into())),
            "이름 변경을 준비하거나 적용하지 못했습니다"
        );
        assert_eq!(
            public_rename_error(LspManagerError::NotRunning(secret.into())),
            "이름 변경을 적용할 언어 서버가 실행 중이 아닙니다"
        );
    }
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
pub async fn reload_lsp_document(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
    text: String,
) -> Result<DidChange, String> {
    manager
        .reload_document(&language_id, &uri, text)
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

#[tauri::command]
pub async fn request_lsp_rename(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
    position: LspPosition,
    new_name: String,
) -> Result<RenamePreview, String> {
    manager
        .rename(&language_id, &uri, position, new_name)
        .await
        .map_err(public_rename_error)
}

#[tauri::command]
pub async fn apply_lsp_rename(
    manager: State<'_, Arc<LspManager>>,
    plan_id: String,
) -> Result<RenameApplyResult, String> {
    manager
        .apply_rename(&plan_id)
        .await
        .map_err(public_rename_error)
}

#[tauri::command]
pub async fn cancel_lsp_rename(
    manager: State<'_, Arc<LspManager>>,
    plan_id: String,
) -> Result<bool, String> {
    Ok(manager.cancel_rename(&plan_id).await)
}

#[tauri::command]
pub async fn discard_lsp_rename(
    manager: State<'_, Arc<LspManager>>,
    plan_id: String,
) -> Result<bool, String> {
    Ok(manager.discard_rename(&plan_id).await)
}

/// Rename plans can contain filesystem and protocol details that must stay on
/// the native side.  Keep the IPC error intentionally categorical; the
/// structured preview/apply result carries only workspace-relative paths and
/// safe per-file statuses.
fn public_rename_error(error: LspManagerError) -> String {
    match error {
        LspManagerError::NotRunning(_) => {
            "이름 변경을 적용할 언어 서버가 실행 중이 아닙니다".into()
        }
        LspManagerError::UnsupportedFeature { .. } => {
            "언어 서버가 이름 변경을 지원하지 않습니다".into()
        }
        LspManagerError::Disabled => "LSP가 비활성화되어 있습니다".into(),
        _ => "이름 변경을 준비하거나 적용하지 못했습니다".into(),
    }
}

#[tauri::command]
pub async fn request_lsp_formatting(
    manager: State<'_, Arc<LspManager>>,
    language_id: String,
    uri: String,
    tab_size: u32,
    insert_spaces: bool,
) -> Result<AppliedDocumentEdits, String> {
    manager
        .formatting(&language_id, &uri, tab_size, insert_spaces)
        .await
        .map_err(|error| error.to_string())
}
