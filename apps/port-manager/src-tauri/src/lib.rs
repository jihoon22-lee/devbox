mod commands;
mod core;

use tauri::Manager;

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.portmanager";
const CURRENT_IDENTIFIER: &str = "com.devbox.portmanager";

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
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::correlation::list_port_observations,
            commands::correlation::open_port_owner,
            commands::correlation::open_port_log,
            commands::ports::list_ports,
            commands::ports::kill_listener,
            commands::ports::handoff_container_stop,
            commands::ports::get_process_info,
            commands::ports::reveal_process,
            commands::ports::open_browser,
            commands::preferences::load_port_manager_preferences,
            commands::preferences::save_port_manager_preferences,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
