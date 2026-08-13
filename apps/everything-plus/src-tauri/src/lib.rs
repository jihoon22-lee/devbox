mod commands;
mod core;

use commands::indexing::AppState;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::sync::{Arc, Mutex};
use tauri::Manager;

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.everythingplus";

/// 구 identifier 디렉터리가 있고 새 디렉터리가 없으면 통째로 옮긴다.
/// 실패해도 앱을 막지 않는다 (로그만 남기고 다음 실행에서 재시도).
fn migrate_local_data(app: &tauri::AppHandle, legacy_id: &str) -> std::io::Result<()> {
    let new_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if new_dir.exists() {
        return Ok(());
    }
    let legacy_dir = new_dir
        .parent()
        .expect("local data dir has a parent")
        .join(legacy_id);
    if !legacy_dir.exists() {
        return Ok(());
    }
    std::fs::rename(&legacy_dir, &new_dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::indexing::add_root,
            commands::indexing::remove_root,
            commands::indexing::list_roots,
            commands::indexing::index_now,
            commands::indexing::index_status,
            commands::search::search_files,
            commands::search::search_content,
        ])
        .setup(|app| {
            if let Err(error) = migrate_local_data(app.handle(), LEGACY_IDENTIFIER) {
                eprintln!("devbox: local data migration will retry next launch: {error}");
            }
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let (conn, index_cleared) = core::db::init(&dir.join("data.db"))?;
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                indexing: AtomicBool::new(false),
                indexed: AtomicI64::new(0),
            });
            // 스키마 버전이 올라가 migrate()가 인덱스를 비웠다면, 등록된
            // 루트가 있는 한 사용자가 빈 검색 결과만 보지 않도록 전체
            // 재인덱싱을 자동으로 걸어준다.
            if index_cleared {
                let roots = core::db::list_roots(&state.db.lock().unwrap()).unwrap_or_default();
                if !roots.is_empty() {
                    commands::indexing::spawn_index(state.clone(), Vec::new());
                }
            }
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
