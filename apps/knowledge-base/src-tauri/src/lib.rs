mod applink;
mod commands;
mod core;
mod integration;
mod platform;

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
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::assets::save_image_asset,
            applink::take_pending_open,
            commands::docs::get_root,
            commands::docs::set_root,
            commands::docs::list_tree,
            commands::docs::read_file,
            commands::docs::open_inbound_note,
            commands::docs::write_file,
            commands::docs::create_file,
            commands::docs::create_directory,
            commands::rename::preview_rename,
            commands::rename::apply_rename,
            commands::rename::discard_rename_preview,
            commands::docs::delete_file,
            commands::docs::entry_path,
            commands::docs::reveal_entry,
            commands::docs::open_targets,
            commands::docs::open_in,
            commands::docs::preview_quick_capture,
            commands::docs::save_quick_capture,
            commands::docs::discard_quick_capture_preview,
            commands::templates::list_templates,
            commands::templates::create_template,
            commands::templates::update_template,
            commands::templates::delete_template,
            commands::templates::preview_template,
            commands::templates::save_template,
            commands::templates::discard_template_preview,
            commands::docs::search_docs,
            commands::docs::list_tags,
            commands::docs::daily_note,
            platform::shortcut_status,
            commands::handoff::preview_knowledge_draft,
            commands::handoff::save_knowledge_draft,
            commands::handoff::discard_knowledge_draft,
            commands::handoff::renew_knowledge_draft,
            commands::markdown::render_markdown,
            commands::wikilinks::analyze_wikilinks,
            commands::wikilinks::wikilink_candidates,
            commands::wikilinks::backlinks,
        ])
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            app.manage(applink::PendingOpen::new());
            app.manage(commands::handoff::PendingKnowledgeDraft::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(request)) => app.state::<applink::PendingOpen>().set(request),
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = core::db::init(&dir.join("data.db"))?;
            if let Ok(root) = commands::docs::resolve_root(&conn) {
                if commands::docs::rebuild_wikilink_index_if_needed(&conn, &root).is_err() {
                    eprintln!("wikilink index rebuild will retry next launch");
                }
            }
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                rename_plans: Mutex::new(core::rename::RenamePlanStore::default()),
                quick_capture_previews: Mutex::new(
                    commands::docs::QuickCapturePreviewStore::default(),
                ),
                template_previews: Mutex::new(commands::templates::TemplatePreviewStore::default()),
                image_cache: Mutex::new(HashMap::new()),
            });
            // watcher 생성 후 루트에 연결 (앱 재시작 시 외부 편집 계속 반영)
            let watcher = KnowledgeWatcher::new(app.handle().clone(), state.clone());
            if let Ok(root) = commands::docs::resolve_root(&state.db.lock().unwrap()) {
                let _ = watcher.set_root(&root);
            }
            // integration snapshot producer (두 번째, §10.1)
            let _ = integration::write_snapshot(&state.db.lock().unwrap());
            let shortcut_state = Arc::new(platform::QuickCaptureShortcutState::default());
            app.manage(shortcut_state.clone());
            platform::install(app.handle().clone(), shortcut_state);
            app.manage(state);
            app.manage(watcher);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
