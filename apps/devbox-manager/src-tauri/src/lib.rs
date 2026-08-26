mod commands;
mod core;

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.devboxmanager";
const CURRENT_IDENTIFIER: &str = "com.devbox.devboxmanager";

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
        .setup(|app| {
            // 중단된 다운로드의 .partial 정리 (재시도/안전 정리)
            commands::manager::cleanup_partials(app.handle());
            if let Err(error) = commands::manager::sync_runtime_metadata(app.handle()) {
                eprintln!("devbox: runtime metadata sync will retry next launch: {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::manager::catalog,
            commands::manager::available,
            commands::manager::installed,
            commands::manager::install_path,
            commands::manager::preview_install_root,
            commands::manager::apply_install_root,
            commands::manager::install,
            commands::manager::install_many,
            commands::manager::launch,
            commands::manager::current,
            commands::manager::rollback,
            commands::manager::open_install_folder,
            commands::manager::remove_portable_app,
            commands::doctor::run_diagnosis,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
