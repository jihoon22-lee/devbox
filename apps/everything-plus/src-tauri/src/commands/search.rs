use crate::commands::indexing::AppState;
use crate::core::db::search;
use crate::core::models::{ContentResult, FileEntry};
use std::sync::Arc;

/// 파일명 FTS5 검색.
#[tauri::command]
pub fn search_files(
    state: tauri::State<'_, Arc<AppState>>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<FileEntry>, String> {
    let limit = limit.unwrap_or(200);
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    search(&state.db.lock().unwrap(), q, limit).map_err(|e| e.to_string())
}

/// 파일 내용 FTS5 검색.
#[tauri::command]
pub fn search_content(
    state: tauri::State<'_, Arc<AppState>>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<ContentResult>, String> {
    let limit = limit.unwrap_or(200);
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    crate::core::db::search_content(&state.db.lock().unwrap(), q, limit).map_err(|e| e.to_string())
}
