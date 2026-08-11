use crate::core::db::{
    add_root as db_add_root, clear_all, clear_root, list_roots as db_list_roots,
    remove_root as db_remove_root, total_files, upsert_content, upsert_file,
};
use crate::core::indexer::{collect, is_text_ext, MAX_CONTENT_BYTES};
use crate::core::models::IndexStatus;
use rusqlite::Connection;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

/// 한 트랜잭션에 담아 커밋할 파일 개수. 이 단위로 락을 반납해 `index_status`가
/// 인덱싱 도중에도 응답할 수 있게 한다.
const BATCH_SIZE: usize = 500;

/// 앱 전역 상태
pub struct AppState {
    pub db: Mutex<Connection>,
    pub indexing: AtomicBool,
    pub indexed: AtomicI64,
}

/// 인덱스 루트를 추가하고 인덱싱을 시작한다.
#[tauri::command]
pub fn add_root(
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
    index_content: bool,
) -> Result<(), String> {
    let conn = state.db.lock().unwrap();
    db_add_root(&conn, &path, index_content).map_err(|e| e.to_string())?;
    let path_for_index = path.clone();
    let st = state.inner().clone();
    drop(conn);
    spawn_index(st, vec![path_for_index]);
    Ok(())
}

#[tauri::command]
pub fn remove_root(state: tauri::State<'_, Arc<AppState>>, path: String) -> Result<(), String> {
    db_remove_root(&state.db.lock().unwrap(), &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_roots(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<crate::core::models::RootInfo>, String> {
    db_list_roots(&state.db.lock().unwrap()).map_err(|e| e.to_string())
}

/// 전체 루트를 다시 인덱싱한다 (백그라운드 실행).
#[tauri::command]
pub fn index_now(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    let st = state.inner().clone();
    spawn_index(st, Vec::new());
    Ok(())
}

#[tauri::command]
pub fn index_status(state: tauri::State<'_, Arc<AppState>>) -> Result<IndexStatus, String> {
    let conn = state.db.lock().unwrap();
    let roots = db_list_roots(&conn).map_err(|e| e.to_string())?;
    let total = total_files(&conn).map_err(|e| e.to_string())?;
    Ok(IndexStatus {
        indexing: state.indexing.load(Ordering::SeqCst),
        total_files: total,
        indexed_files: state.indexed.load(Ordering::SeqCst),
        roots: roots.len(),
        last_indexed_at: None,
    })
}

/// `only_roots`가 비어 있으면 전체 재인덱싱, 아니면 해당 루트만 부분 재인덱싱한다.
fn spawn_index(state: Arc<AppState>, only_roots: Vec<String>) {
    if state.indexing.swap(true, Ordering::SeqCst) {
        return; // 이미 인덱싱 중
    }
    state.indexed.store(0, Ordering::SeqCst);
    std::thread::spawn(move || {
        if let Err(e) = run_index(&state, &only_roots) {
            eprintln!("indexing error: {e}");
        }
        state.indexing.store(false, Ordering::SeqCst);
    });
}

/// 실제 인덱싱을 수행한다. 대상 루트 조회와 기존 인덱스 삭제만 락을 잡고,
/// 가장 오래 걸리는 파일 순회(`collect`)는 락 밖에서 수행한다. 삽입은
/// `BATCH_SIZE` 단위로 트랜잭션을 끊어 커밋할 때마다 락을 반납해 `index_status`가
/// 인덱싱 도중에도 응답할 수 있게 한다.
fn run_index(state: &Arc<AppState>, only_roots: &[String]) -> rusqlite::Result<()> {
    let full_reindex = only_roots.is_empty();
    let targets: Vec<_> = {
        let conn = state.db.lock().unwrap();
        let roots = db_list_roots(&conn)?;
        if full_reindex {
            clear_all(&conn)?;
            roots
        } else {
            let targets: Vec<_> = roots
                .into_iter()
                .filter(|r| only_roots.contains(&r.path))
                .collect();
            for root in &targets {
                clear_root(&conn, &root.path)?;
            }
            targets
        }
    };

    for root in &targets {
        let files = collect(Path::new(&root.path));
        for chunk in files.chunks(BATCH_SIZE) {
            let conn = state.db.lock().unwrap();
            conn.execute("BEGIN TRANSACTION", [])?;
            for f in chunk {
                let path_str = f.path.to_string_lossy().into_owned();
                let file_id = upsert_file(&conn, &path_str, f.size, f.modified_ts, 0)?;
                if root.content
                    && is_text_ext(&ext_of(&path_str))
                    && f.size <= MAX_CONTENT_BYTES as i64
                {
                    if let Ok(content) = std::fs::read_to_string(&f.path) {
                        let _ = upsert_content(&conn, file_id, &content);
                    }
                }
            }
            conn.execute("COMMIT", [])?;
            state
                .indexed
                .fetch_add(chunk.len() as i64, Ordering::SeqCst);
            // conn(락)은 여기서 스코프를 벗어나며 반납된다.
        }
    }
    Ok(())
}

fn ext_of(path: &str) -> String {
    path.rsplit('.')
        .next()
        .filter(|e| *e != path && e.len() <= 10)
        .unwrap_or("")
        .to_lowercase()
}
