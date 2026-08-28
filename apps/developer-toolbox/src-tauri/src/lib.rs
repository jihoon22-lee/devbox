mod commands;
mod core;

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.developertoolbox";
const CURRENT_IDENTIFIER: &str = "com.devbox.developertoolbox";

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
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::tools::hash,
            commands::tools::hmac_generate,
            commands::tools::hmac_verify,
            commands::tools::generate_uuid,
            commands::tools::generate_ids,
            commands::tools::regex_test,
            commands::tools::diff,
            commands::tools::jwt_verify,
            commands::qr::generate_qr,
            commands::workflows::load_workflow_metadata,
            commands::workflows::save_workflow_metadata,
            commands::handoff::create_api_request_handoff,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
