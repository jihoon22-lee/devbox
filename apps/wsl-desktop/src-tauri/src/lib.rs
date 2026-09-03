mod applink;
mod commands;
mod core;
mod integration;
mod log_lens;
mod quick_summon;
mod runtime_snapshot;

use commands::shell_integration::ShellIntegrationState;
use commands::terminal::SessionState;
use std::sync::Arc;
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
            quick_summon::reveal_main_window(app);
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::dashboard::list_distros,
            commands::dashboard::dashboard_snapshot,
            commands::dashboard::run_wsl_command,
            commands::dashboard::docker_ps,
            commands::dashboard::docker_action,
            commands::multiplexer::detect_multiplexers,
            commands::shell_integration::inspect_shell_integration,
            commands::shell_integration::update_shell_integration,
            commands::workspace::list_workspace_profiles,
            commands::workspace::save_workspace_profile,
            commands::workspace::delete_workspace_profile,
            commands::terminal::start_session,
            commands::terminal::attach_session,
            commands::terminal::windows_build_number,
            commands::terminal::write_session,
            commands::terminal::broadcast,
            commands::terminal::resize_session,
            commands::terminal::close_session,
            commands::terminal::list_sessions,
            log_lens::open_wsl_file_in_log_lens,
            log_lens::open_wsl_journal_in_log_lens,
            quick_summon::configure_quick_summon,
        ])
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(req)) => app.state::<applink::PendingOpen>().set(req),
                Ok(None) => {}
                Err(e) => eprintln!("applink: {e}"),
            }
            let state = Arc::new(SessionState::new());
            app.manage(Arc::clone(&state));
            app.manage(ShellIntegrationState::default());
            app.manage(quick_summon::QuickSummonState::default());
            match app.handle().plugin(quick_summon::plugin()) {
                Ok(()) => app
                    .state::<quick_summon::QuickSummonState>()
                    .set_shortcut_backend_available(true),
                Err(error) => {
                    // Quick Summon is optional: a platform hotkey initialization
                    // failure must not prevent the terminal itself from starting.
                    eprintln!("quick summon: global shortcut backend unavailable: {error}");
                }
            }
            runtime_snapshot::spawn_snapshot_writer(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window
                    .state::<quick_summon::QuickSummonState>()
                    .close_to_tray()
                {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
