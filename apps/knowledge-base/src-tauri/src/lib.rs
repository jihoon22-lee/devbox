mod applink;
mod commands;
mod core;
mod integration;

use commands::docs::AppState;
use commands::watcher::KnowledgeWatcher;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.knowledgebase";
const CURRENT_IDENTIFIER: &str = "com.devbox.knowledgebase";

fn migrate_local_data() {
    let Some(base_dir) = dirs::data_local_dir() else {
        eprintln!(
            "devbox: local data migration will retry next launch: local data directory unavailable"
        );
        return;
    };
    if let Err(error) = devbox_filesystem::migrate_legacy_identifier_dir(
        base_dir,
        LEGACY_IDENTIFIER,
        CURRENT_IDENTIFIER,
    ) {
        eprintln!("devbox: local data migration will retry next launch: {error}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    migrate_local_data();
    tauri::Builder::default()
        // single-instance는 opener와 setup보다 먼저 등록해 두 번째 프로세스가
        // 중복 초기화되기 전에 argv를 기존 instance로 전달한다.
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
            commands::docs::get_root,
            commands::docs::set_root,
            commands::docs::list_tree,
            commands::docs::read_file,
            commands::docs::open_inbound_note,
            commands::docs::write_file,
            commands::docs::create_file,
            commands::docs::rename_file,
            commands::docs::delete_file,
            commands::docs::search_docs,
            commands::docs::list_tags,
            commands::docs::daily_note,
            commands::markdown::render_markdown,
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
            let conn = core::db::init(&dir.join("data.db"))?;
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                image_cache: Mutex::new(HashMap::new()),
            });
            // watcher 생성 후 루트에 연결 (앱 재시작 시 외부 편집 계속 반영)
            let watcher = KnowledgeWatcher::new(app.handle().clone(), state.clone());
            if let Ok(root) = commands::docs::resolve_root(&state.db.lock().unwrap()) {
                let _ = watcher.set_root(&root);
            }
            // integration snapshot producer (두 번째, §10.1)
            let _ = integration::write_snapshot(&state.db.lock().unwrap());
            app.manage(state);
            app.manage(watcher);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
