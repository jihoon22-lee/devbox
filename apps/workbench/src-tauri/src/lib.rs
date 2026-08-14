mod commands;
mod core;

use commands::workspace::run_registry;
use tauri::Manager;

// TODO(0.5.0): 신규 앱 — identifier 변경 이전 데이터가 없어 마이그레이션 없음.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(run_registry());
            // life-log의 프로젝트 경로를 흡수 (read-only, §10.2)
            let mut store = crate::core::profile::ProfileStore::load(
                &app.handle()
                    .path()
                    .app_local_data_dir()
                    .ok()
                    .and_then(|d| std::fs::read_to_string(d.join("project-profiles.json")).ok())
                    .unwrap_or_default(),
            );
            if let Ok(n) = commands::workspace::absorb_life_log_projects(&mut store) {
                if n > 0 {
                    if let Ok(dir) = app.handle().path().app_local_data_dir() {
                        let path = dir.join("project-profiles.json");
                        if let Ok(json) = store.to_json() {
                            let _ = std::fs::write(&path, json);
                        }
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace::list_profiles,
            commands::workspace::create_profile,
            commands::workspace::update_profile,
            commands::workspace::delete_profile,
            commands::workspace::git_status,
            commands::workspace::project_health,
            commands::workspace::start_workspace,
            commands::workspace::stop_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
