use crate::core::db;
use crate::core::store;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

use crate::core::entry_actions::{
    canonical_existing_entry, prepare_open_request, select_open_targets, validated_new_entry,
    KnowledgeOpenTarget,
};

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InboundNote {
    pub path: String,
    pub content: String,
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

/// schema v1 최초 실행에만 원문 Markdown에서 정확한 source line을 재구축한다.
/// 읽을 수 없거나 root 밖으로 canonicalize되는 항목은 index 대상에서 제외한다.
pub(crate) fn rebuild_wikilink_index_if_needed(
    conn: &Connection,
    root: &Path,
) -> Result<(), String> {
    if !db::wikilink_index_needs_rebuild(conn)
        .map_err(|_| "위키링크 인덱스를 준비할 수 없습니다".to_string())?
    {
        return Ok(());
    }
    let entries =
        store::tree(root).map_err(|_| "위키링크 인덱스를 준비할 수 없습니다".to_string())?;
    let docs = entries
        .into_iter()
        .filter(|(path, is_dir)| {
            !*is_dir
                && Path::new(path)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        })
        .filter_map(|(path, _)| crate::core::inbound::read_note(root, &path).ok())
        .collect::<Vec<_>>();
    db::rebuild_wikilink_index(conn, &docs)
        .map_err(|_| "위키링크 인덱스를 준비할 수 없습니다".to_string())
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

/// Untrusted applink `Path`를 현재 Knowledge root 안의 실제 Markdown note로
/// 해석하고 같은 canonical target에서 bounded read까지 수행한다. 실패 메시지는
/// 요청 경로나 OS 오류를 반향하지 않는다.
#[tauri::command]
pub fn open_inbound_note(
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
) -> Result<InboundNote, String> {
    let root = {
        let conn = state.db.lock().unwrap();
        resolve_root(&conn).map_err(|_| "요청한 노트를 열 수 없습니다".to_string())?
    };
    let (path, content) = crate::core::inbound::read_note(&root, &path).map_err(str::to_string)?;
    Ok(InboundNote { path, content })
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
    db::index_doc(&conn, &rel, &content).map_err(|e| e.to_string())?;
    drop(conn);
    // integration snapshot 갱신 (best-effort — 실패해도 저장은 유지)
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    Ok(())
}

#[tauri::command]
pub fn create_file(
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
    content: Option<String>,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = validated_new_entry(&root, &rel).map_err(str::to_string)?;
    if path.exists() {
        return Err("파일이 이미 존재합니다".into());
    }
    let content = content.unwrap_or_default();
    store::write_file(&path, &content)?;
    db::index_doc(&conn, &rel, &content).map_err(|e| e.to_string())?;
    drop(conn);
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    Ok(())
}

#[tauri::command]
pub fn create_directory(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = validated_new_entry(&root, &rel).map_err(str::to_string)?;
    if path.exists() {
        return Err("폴더가 이미 존재합니다".into());
    }
    std::fs::create_dir_all(path).map_err(|_| "폴더를 만들 수 없습니다".to_string())
}

#[tauri::command]
pub fn rename_file(
    state: tauri::State<'_, Arc<AppState>>,
    from: String,
    to: String,
) -> Result<(), String> {
    let mut conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let src = canonical_existing_entry(&root, &from).map_err(str::to_string)?;
    let dst = validated_new_entry(&root, &to).map_err(str::to_string)?;
    if dst.exists() {
        return Err("같은 이름의 항목이 이미 존재합니다".into());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "항목 이름을 바꿀 수 없습니다".to_string())?;
    }
    std::fs::rename(&src, &dst).map_err(|_| "항목 이름을 바꿀 수 없습니다".to_string())?;
    if replace_indexed_path(&mut conn, &root, &from, &to).is_err() {
        // 파일이 source of truth다. DB transaction이 실패하면 가능한 경우 filesystem
        // rename도 되돌려 호출자가 성공으로 오인하지 않게 한다.
        let _ = std::fs::rename(&dst, &src);
        return Err("검색 인덱스를 갱신할 수 없습니다".to_string());
    }
    drop(conn);
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    Ok(())
}

fn replace_indexed_path(
    conn: &mut Connection,
    root: &Path,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let transaction = conn
        .transaction()
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    db::remove_docs_under(&transaction, from)
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    let renamed = canonical_existing_entry(root, to).map_err(str::to_string)?;
    if renamed.is_dir() {
        let entries =
            store::tree(&renamed).map_err(|_| "검색할 폴더 내용을 읽을 수 없습니다".to_string())?;
        for (child, is_dir) in entries {
            if is_dir {
                continue;
            }
            let child_rel = format!("{}/{}", to.trim_end_matches('/'), child);
            let Ok(child_path) = canonical_existing_entry(root, &child_rel) else {
                continue;
            };
            let Ok(content) = store::read_file(&child_path) else {
                continue;
            };
            db::index_doc_in_transaction(&transaction, &child_rel, &content)
                .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
        }
    } else if let Ok(content) = store::read_file(&renamed) {
        db::index_doc_in_transaction(&transaction, to, &content)
            .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    }
    transaction
        .commit()
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())
}

#[tauri::command]
pub fn delete_file(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<(), String> {
    let mut conn = state.db.lock().unwrap();
    let root = resolve_root(&conn)?;
    let path = canonical_existing_entry(&root, &rel).map_err(str::to_string)?;
    let transaction = conn
        .transaction()
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    db::remove_docs_under(&transaction, &rel)
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    store::delete_file(&path).map_err(|_| "항목을 삭제할 수 없습니다".to_string())?;
    transaction
        .commit()
        .map_err(|_| "검색 인덱스를 갱신할 수 없습니다".to_string())?;
    drop(conn);
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    Ok(())
}

fn available_open_targets() -> Vec<KnowledgeOpenTarget> {
    select_open_targets("knowledge-base", devbox_launch::installed_targets("path"))
}

fn resolve_entry_for_action(
    state: &tauri::State<'_, Arc<AppState>>,
    rel: &str,
) -> Result<PathBuf, String> {
    let conn = state.db.lock().unwrap();
    let root = resolve_root(&conn).map_err(|_| "Knowledge 루트를 열 수 없습니다".to_string())?;
    canonical_existing_entry(&root, rel).map_err(str::to_string)
}

/// 사용자가 명시적으로 Copy path를 선택했을 때만 absolute path를 frontend에 반환한다.
#[tauri::command]
pub fn entry_path(state: tauri::State<'_, Arc<AppState>>, rel: String) -> Result<String, String> {
    let path = resolve_entry_for_action(&state, &rel)?;
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| "Knowledge 항목 경로가 올바르지 않습니다".to_string())
}

#[tauri::command]
pub fn reveal_entry(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    rel: String,
) -> Result<(), String> {
    let path = resolve_entry_for_action(&state, &rel)?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|_| "탐색기에서 항목을 표시할 수 없습니다".to_string())
}

/// Catalog capability와 실제 설치 executable의 교집합만 공개한다. executable
/// 경로는 frontend에 보내지 않는다.
#[tauri::command]
pub fn open_targets() -> Vec<KnowledgeOpenTarget> {
    available_open_targets()
}

#[tauri::command]
pub fn open_in(
    state: tauri::State<'_, Arc<AppState>>,
    app_id: String,
    rel: String,
) -> Result<(), String> {
    let entry = resolve_entry_for_action(&state, &rel)?;
    let targets = available_open_targets();
    let (target_id, request) =
        prepare_open_request(&targets, &app_id, &entry).map_err(str::to_string)?;
    devbox_launch::launch_open(&target_id, &request).map(|_| ())
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
        drop(conn);
        let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
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

    #[test]
    fn folder_rename_reindexes_descendants_and_delete_removes_them() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("Old/nested")).unwrap();
        std::fs::write(root.path().join("Old/a.md"), "alpha unique").unwrap();
        std::fs::write(root.path().join("Old/nested/b.md"), "beta unique").unwrap();
        let mut conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        db::index_doc(&conn, "Old/a.md", "alpha unique").unwrap();
        db::index_doc(&conn, "Old/nested/b.md", "beta unique").unwrap();

        std::fs::rename(root.path().join("Old"), root.path().join("New")).unwrap();
        replace_indexed_path(&mut conn, root.path(), "Old", "New").unwrap();

        assert_eq!(db::search(&conn, "alpha", 10).unwrap()[0].0, "New/a.md");
        assert_eq!(
            db::search(&conn, "beta", 10).unwrap()[0].0,
            "New/nested/b.md"
        );
        store::delete_file(&root.path().join("New")).unwrap();
        db::remove_docs_under(&conn, "New").unwrap();
        assert!(db::search(&conn, "unique", 10).unwrap().is_empty());
    }
}
