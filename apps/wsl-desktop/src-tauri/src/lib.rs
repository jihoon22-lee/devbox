mod commands;
mod core;

use commands::terminal::SessionState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::Manager;

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.wsldesktop";

/// 구 identifier 디렉터리가 있고 새 디렉터리가 없으면 통째로 옮긴다.
/// 실패해도 앱을 막지 않는다 (로그만 남기고 다음 실행에서 재시도).
fn migrate_local_data(app: &tauri::AppHandle, legacy_id: &str) -> std::io::Result<()> {
    let new_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if new_dir.exists() {
        return Ok(());
    }
    let legacy_dir = new_dir
        .parent()
        .expect("local data dir has a parent")
        .join(legacy_id);
    if !legacy_dir.exists() {
        return Ok(());
    }
    std::fs::rename(&legacy_dir, &new_dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::dashboard::list_distros,
            commands::dashboard::run_wsl_command,
            commands::dashboard::docker_ps,
            commands::dashboard::docker_action,
            commands::dashboard::git_status,
            commands::terminal::start_session,
            commands::terminal::write_session,
            commands::terminal::broadcast,
            commands::terminal::resize_session,
            commands::terminal::close_session,
            commands::terminal::list_sessions,
        ])
        .setup(|app| {
            if let Err(error) = migrate_local_data(app.handle(), LEGACY_IDENTIFIER) {
                eprintln!("devbox: local data migration will retry next launch: {error}");
            }
            let state = Arc::new(SessionState {
                sessions: Mutex::new(HashMap::new()),
            });
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
