mod applink;
mod commands;
mod core;

use tauri::{Emitter, Manager};

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
        // Launcher's install handoff must focus the existing Manager instance;
        // arbitrary argv targets are intentionally ignored here.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Ok(Some(request)) = devbox_applink::parse_argv(&args) {
                if applink::is_install_request(&request) {
                    app.state::<applink::PendingOpen>().set(request.clone());
                    let _ = app.emit("devbox://open", request);
                }
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            app.manage(applink::PendingOpen::new());
            app.manage(commands::diagnostics::DiagnosticsState::default());
            if let Ok(Some(request)) =
                devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>())
            {
                if applink::is_install_request(&request) {
                    app.state::<applink::PendingOpen>().set(request);
                }
            }
            // 중단된 다운로드의 .partial 정리 (재시도/안전 정리)
            commands::manager::cleanup_partials(app.handle());
            if let Err(error) = commands::manager::sync_runtime_metadata(app.handle()) {
                eprintln!("devbox: runtime metadata sync will retry next launch: {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
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
            commands::manager::preview_remove_app,
            commands::manager::remove_portable_app,
            commands::doctor::run_diagnosis,
            commands::diagnostics::inspect_data_databases,
            commands::diagnostics::preview_data_query,
            commands::diagnostics::cancel_data_diagnostics,
            commands::diagnostics::export_data_preview,
            commands::diagnostics::preview_support_bundle,
            commands::diagnostics::cancel_support_bundle,
            commands::diagnostics::export_support_bundle,
            commands::related_tools::related_tools,
            commands::related_tools::dev_setup_audit,
            commands::related_tools::install_related_tool,
            commands::related_tools::launch_related_tool,
        ])
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
