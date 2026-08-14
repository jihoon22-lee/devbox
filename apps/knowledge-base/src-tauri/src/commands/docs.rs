use crate::core::db;
use crate::core::store;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tauri::Manager;

/// 앱 전역 상태
pub struct AppState {
    pub db: Mutex<Connection>,
    /// 렌더 프리뷰용 이미지 인라인 캐시: (경로, mtime)이 같으면 base64 재인코딩을
    /// 건너뛴다. 항목 32개를 넘기면 통째로 비운다(LRU까지 갈 필요 없음).
    pub image_cache: Mutex<HashMap<PathBuf, (SystemTime, String)>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeEntry {
    pub path: String,
    pub is_dir: bool,
}

/// KnowledgeRoot 경로를 반환한다. 미설정이면 Documents/Knowledge로 초기화.
///
/// `commands::markdown`도 이미지·링크 해석을 위해 루트 경로가 필요해 `pub(crate)`로
/// 공개한다.
pub(crate) fn resolve_root(conn: &Connection) -> Result<PathBuf, String> {
    if let Some(root) = db::get_setting(conn, "root").map_err(|e| e.to_string())? {
        return Ok(PathBuf::from(root));
    }
    let default = dirs::document_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Knowledge");
    store::ensure_layout(&default)?;
    db::set_setting(conn, "root", &default.to_string_lossy()).map_err(|e| e.to_string())?;
    Ok(default)
}

#[tauri::command]
pub fn get_root(state: tauri::State<'_, Arc<AppState>>) -> Result<String, String> {
    let conn = state.db.lock().unwrap();
    Ok(resolve_root(&conn)?.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn set_root(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    store::ensure_layout(Path::new(&path))?;
    db::set_setting(&conn, "root", &path).map_err(|e| e.to_string())?;
    drop(conn);
    // watcher를 새 루트로 재시작
    let watcher = app.state::<Arc<crate::commands::watcher::KnowledgeWatcher>>();
    watcher.set_root(Path::new(&path))
}

#[tauri::command]
pub fn list_tree(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<TreeEntry>, String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let entries = store::tree(&root)?;
    Ok(entries
        .into_iter()
        .map(|(path, is_dir)| TreeEntry { path, is_dir })
        .collect())
}

#[tauri::command]
pub fn read_file(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<String, String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = store::safe_join(&root, &rel)?;
    store::read_file(&path)
}

#[tauri::command]
pub fn write_file(
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
    content: String,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = store::safe_join(&root, &rel)?;
    store::write_file(&path, &content)?;
    db::index_doc(&conn, &rel, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_file(
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
    content: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = store::safe_join(&root, &rel)?;
    if path.exists() {
        return Err("파일이 이미 존재합니다".into());
    }
    let content = content.unwrap_or_default();
    store::write_file(&path, &content)?;
    db::index_doc(&conn, &rel, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_file(
    state: tauri::State<'_, Arc<AppState>>,
    from: String,
    to: String,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let src = store::safe_join(&root, &from)?;
    let dst = store::safe_join(&root, &to)?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&src, &dst).map_err(|e| e.to_string())?;
    db::remove_doc(&conn, &from).map_err(|e| e.to_string())?;
    if let Ok(content) = store::read_file(&dst) {
        let _ = db::index_doc(&conn, &to, &content);
    }
    Ok(())
}

#[tauri::command]
pub fn delete_file(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = store::safe_join(&root, &rel)?;
    store::delete_file(&path)?;
    db::remove_doc(&conn, &rel).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn search_docs(
    state: tauri::State<'_, Arc<AppState>>,
    query: String,
) -> Result<Vec<(String, String)>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let conn = state.db.lock().unwrap();
    db::search(&conn, q, 100).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_tags(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<String>, String> {
    let conn = state.db.lock().unwrap();
    db::list_tags(&conn).map_err(|e| e.to_string())
}

/// 오늘 날짜 데일리 노트를 생성/열기. (경로, 내용)을 반환한다.
#[tauri::command]
pub fn daily_note(state: tauri::State<'_, Arc<AppState>>) -> Result<(String, String), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let rel = format!("Journal/{}.md", today_str());
    let path = store::safe_join(&root, &rel)?;
    if !path.exists() {
        let content = format!("---\ntags: [daily]\n---\n\n# {}\n\n", today_str());
        store::write_file(&path, &content)?;
        db::index_doc(&conn, &rel, &content).map_err(|e| e.to_string())?;
        return Ok((rel, content));
    }
    let content = store::read_file(&path)?;
    Ok((rel, content))
}

fn today_str() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant의 civil_from_days 알고리즘 (epoch 1970-01-01)
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as i64;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as i64;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_for_epoch() {
        // 1970-01-01
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn civil_date_known() {
        // 2026-08-11 = epoch days
        let days = 20_676;
        assert_eq!(civil_from_days(days), (2026, 8, 11));
    }
}
