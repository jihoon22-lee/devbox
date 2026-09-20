mod applink;
mod commands;
pub mod component;
mod core;

#[cfg(feature = "standalone")]
use tauri::{Emitter, Manager};

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
#[cfg(feature = "standalone")]
const LEGACY_IDENTIFIER: &str = "com.workbench.everythingplus";
#[cfg(feature = "standalone")]
const CURRENT_IDENTIFIER: &str = "com.devbox.everythingplus";

#[cfg(feature = "standalone")]
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

#[cfg(feature = "standalone")]
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
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
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
            commands::saved_queries::list_saved_queries,
            commands::saved_queries::save_saved_query,
            commands::saved_queries::delete_saved_query,
            commands::watcher::watcher_statuses,
            commands::actions::open_file,
            commands::actions::reveal_file,
            commands::actions::open_targets,
            commands::actions::open_in,
        ])
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(request)) => app.state::<applink::PendingOpen>().set(request),
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            let dir = app.path().app_local_data_dir()?;
            component::initialize(app.handle(), &dir, None)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
