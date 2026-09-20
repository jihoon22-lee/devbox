//! recovery 파일 명령. 세션 파일과 분리된 `recovery.json`을 읽고 쓴다.
//! 정상 저장·닫기 시 프론트가 해당 항목을 discard해 recovery를 제거한다.

use crate::core::recovery::{RecoveryEntry, RecoveryFile};
use std::path::PathBuf;
use tauri::AppHandle;

pub const RECOVERY_FILE_NAME: &str = "recovery.json";

fn recovery_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = crate::component::data_root(app).map_err(|e| e.to_string())?;
    Ok(dir.join(RECOVERY_FILE_NAME))
}

fn read_current(app: &AppHandle) -> RecoveryFile {
    recovery_path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| RecoveryFile::load(&text))
        .unwrap_or_default()
}

fn write_atomic(path: &PathBuf, json: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// 미저장 버퍼 스냅샷을 저장한다 (bounded).
#[tauri::command]
pub fn save_recovery(app: AppHandle, entries: Vec<RecoveryEntry>) -> Result<(), String> {
    let path = recovery_path(&app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = read_current(&app);
    for entry in entries {
        file.upsert(entry);
    }
    let json = file.to_json().map_err(|e| e.to_string())?;
    write_atomic(&path, &json)
}

/// 저장된 recovery 항목 목록.
#[tauri::command]
pub fn load_recovery(app: AppHandle) -> Vec<RecoveryEntry> {
    read_current(&app).entries
}

/// recovery를 폐기한다. path가 없으면 전체를 비운다.
#[tauri::command]
pub fn discard_recovery(app: AppHandle, path: Option<String>) -> Result<(), String> {
    let recovery_path = recovery_path(&app)?;
    let mut file = read_current(&app);
    match path {
        Some(p) => file.remove(&p),
        None => file = RecoveryFile::empty(),
    }
    let json = file.to_json().map_err(|e| e.to_string())?;
    write_atomic(&recovery_path, &json)
}

/// 사용자가 승인한 recovery를 파일에 적용한다 (복구 = 덮어쓰기 승인).
#[tauri::command]
pub fn apply_recovery(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_save_recovery(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        entries: Vec<RecoveryEntry>,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    save_recovery(_component_app.clone(), input.entries)?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_load_recovery(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {}
    let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    let value = load_recovery(_component_app.clone());
    serde_json::to_value(value).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_discard_recovery(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        path: Option<String>,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    discard_recovery(_component_app.clone(), input.path)?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}

/// Typed product adapter; the native host owns caller/session/owner admission.
pub(crate) async fn __component_apply_recovery(
    _component_app: &tauri::AppHandle,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Input {
        path: String,
        content: String,
    }
    let input: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
    apply_recovery(input.path, input.content)?;
    serde_json::to_value(()).map_err(|_| "component_response_invalid".into())
}
