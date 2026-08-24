mod commands;
mod platform;

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.apiplayground";
const CURRENT_IDENTIFIER: &str = "com.devbox.apiplayground";

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
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::request::send_request,
            commands::request::build_revealed_curl,
            commands::request::sanitize_persisted_json,
            commands::secrets::seal_secret,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
