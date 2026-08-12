mod commands;
mod core;

use commands::indexing::AppState;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::sync::{Arc, Mutex};
use tauri::Manager;

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
