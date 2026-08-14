//! recovery 파일 명령. 세션 파일과 분리된 `recovery.json`을 읽고 쓴다.
//! 정상 저장·닫기 시 프론트가 해당 항목을 discard해 recovery를 제거한다.

use crate::core::recovery::{RecoveryEntry, RecoveryFile};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

pub const RECOVERY_FILE_NAME: &str = "recovery.json";

fn recovery_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
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
