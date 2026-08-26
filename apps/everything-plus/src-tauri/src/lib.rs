mod applink;
mod commands;
mod core;

use commands::indexing::AppState;
use commands::watcher::WatcherManager;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.everythingplus";
const CURRENT_IDENTIFIER: &str = "com.devbox.everythingplus";

fn migrate_local_data() {
    let Some(base_dir) = dirs::data_local_dir() else {
        eprintln!(
            "devbox: local data migration will retry next launch: local data directory unavailable"
        );
        return;
    };
    if let Err(error) =
        filesystem::migrate_legacy_identifier_dir(base_dir, LEGACY_IDENTIFIER, CURRENT_IDENTIFIER)
    {
        eprintln!("devbox: local data migration will retry next launch: {error}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    migrate_local_data();
    tauri::Builder::default()
        // 두 번째 process가 index DB와 watcher를 다시 초기화하기 전에 기존
        // instance로 argv를 전달하도록 첫 plugin으로 등록한다.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match devbox_applink::parse_argv(&args) {
                Ok(Some(request)) => {
                    app.state::<applink::PendingOpen>().set(request.clone());
                    let _ = app.emit("devbox://open", request);
                }
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::indexing::add_root,
            commands::indexing::remove_root,
            commands::indexing::list_roots,
            commands::indexing::index_now,
            commands::indexing::cancel_index,
            commands::indexing::index_status,
            commands::search::search_files,
            commands::search::search_content,
            commands::watcher::watcher_statuses,
            commands::actions::open_file,
            commands::actions::reveal_file,
            commands::actions::open_targets,
            commands::actions::open_in,
        ])
        .setup(|app| {
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(request)) => app.state::<applink::PendingOpen>().set(request),
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let (conn, index_cleared) = core::db::init(&dir.join("data.db"))?;
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                lifecycle: Mutex::new(()),
                indexing: AtomicBool::new(false),
                cancel_requested: AtomicBool::new(false),
                restart_requested: AtomicBool::new(false),
                indexed: AtomicI64::new(0),
                total: AtomicI64::new(0),
                content_indexed: AtomicI64::new(0),
                content_truncated: AtomicI64::new(0),
                content_failed: AtomicI64::new(0),
                last_indexed_at: AtomicI64::new(0),
                last_error: Mutex::new(None),
            });
            // watcher는 DB 초기화 뒤, 상태 관리 전에 생성한다 (restore_all이 db를 읽는다)
            let watcher = WatcherManager::new(app.handle().clone(), state.clone());
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
            app.manage(watcher.clone());
            // 앱 재시작 시 등록된 루트의 watcher를 복원한다
            watcher.restore_all();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
