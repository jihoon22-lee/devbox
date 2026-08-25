mod applink;
mod commands;
mod core;

use commands::terminal::SessionState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.wsldesktop";
const CURRENT_IDENTIFIER: &str = "com.devbox.wsldesktop";

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
        // single-instance는 반드시 첫 플러그인이어야 한다: 이후 플러그인·setup이
        // 두 번째 프로세스에서 중복 초기화되기 전에 중복 실행을 종료한다.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match devbox_applink::parse_argv(&args) {
                Ok(Some(req)) => {
                    app.state::<applink::PendingOpen>().set(req.clone());
                    let _ = app.emit("devbox://open", req);
                }
                Ok(None) => {}
                Err(e) => eprintln!("applink: {e}"),
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::dashboard::list_distros,
            commands::dashboard::run_wsl_command,
            commands::dashboard::docker_ps,
            commands::dashboard::docker_action,
            commands::terminal::start_session,
            commands::terminal::attach_session,
            commands::terminal::windows_build_number,
            commands::terminal::write_session,
            commands::terminal::broadcast,
            commands::terminal::resize_session,
            commands::terminal::close_session,
            commands::terminal::list_sessions,
        ])
        .setup(|app| {
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(req)) => app.state::<applink::PendingOpen>().set(req),
                Ok(None) => {}
                Err(e) => eprintln!("applink: {e}"),
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
