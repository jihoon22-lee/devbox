pub mod applink;
pub mod commands;
pub mod core;
pub mod lsp;
pub mod watcher;

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.codepad";
const CURRENT_IDENTIFIER: &str = "com.devbox.codepad";

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
    let shutdown_started = std::sync::Arc::new(AtomicBool::new(false));
    let exit_authorized = std::sync::Arc::new(AtomicBool::new(false));
    let shutdown_started_for_run = std::sync::Arc::clone(&shutdown_started);
    let exit_authorized_for_run = std::sync::Arc::clone(&exit_authorized);
    let app = tauri::Builder::default()
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(req)) => app.state::<applink::PendingOpen>().set(req),
                Ok(None) => {}
                Err(e) => eprintln!("applink: {e}"),
            }
            app.manage(watcher::WatcherManager::new(app.handle().clone()));
            let app_local_data_dir = app.path().app_local_data_dir()?;
            let installer = std::sync::Arc::new(lsp::ManagedInstaller::new(&app_local_data_dir)?);
            let manager = std::sync::Arc::new(lsp::LspManager::with_installer(
                app_local_data_dir,
                env!("CARGO_PKG_VERSION"),
                std::sync::Arc::clone(&installer),
            ));
            let mut events = manager.subscribe_events();
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let event = match events.recv().await {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    match event {
                        lsp::LspEvent::Diagnostics(payload) => {
                            let _ = app_handle.emit("lsp/diagnostics", payload);
                        }
                        lsp::LspEvent::Status(payload) => {
                            let _ = app_handle.emit("lsp/status", payload);
                        }
                    }
                }
            });
            app.manage(manager);
            app.manage(installer);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::file::open_file,
            commands::file::save_file,
            commands::file::rename_file_action,
            commands::file::delete_file_action,
            commands::file::reveal_file_action,
            commands::file::validate_encoding,
            commands::folder::list_workspace_files,
            commands::folder::canonicalize_workspace,
            commands::folder::workspace_capabilities,
            commands::installer::lsp_catalog,
            commands::installer::lsp_installed,
            commands::installer::lsp_recover_installed,
            commands::installer::lsp_install,
            commands::installer::lsp_import_archive,
            commands::installer::lsp_uninstall,
            commands::preview::render_preview,
            commands::session::load_session,
            commands::session::save_session,
            commands::recovery::save_recovery,
            commands::recovery::load_recovery,
            commands::recovery::discard_recovery,
            commands::recovery::apply_recovery,
            commands::watch::watch_file,
            commands::watch::unwatch_file,
            commands::lsp::load_lsp_config,
            commands::lsp::save_lsp_config,
            commands::lsp::start_language_server,
            commands::lsp::stop_language_server,
            commands::lsp::restart_language_server,
            commands::lsp::stop_all_language_servers,
            commands::lsp::language_server_statuses,
            commands::lsp::language_server_logs,
            commands::lsp::open_lsp_document,
            commands::lsp::change_lsp_document,
            commands::lsp::reload_lsp_document,
            commands::lsp::save_lsp_document,
            commands::lsp::close_lsp_document,
            commands::lsp::pull_lsp_diagnostics,
            commands::lsp::request_lsp_completion,
            commands::lsp::request_lsp_hover,
            commands::lsp::request_lsp_definition,
            commands::lsp::request_lsp_references,
            commands::lsp::request_lsp_rename,
            commands::lsp::apply_lsp_rename,
            commands::lsp::cancel_lsp_rename,
            commands::lsp::discard_lsp_rename,
            commands::lsp::request_lsp_formatting,
        ])
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .build(tauri::generate_context!())
        .expect("error while building Code Pad");

    app.run(move |handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if exit_authorized_for_run.load(Ordering::Acquire) {
                return;
            }
            api.prevent_exit();
            if shutdown_started_for_run
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let manager = handle
                    .state::<std::sync::Arc<lsp::LspManager>>()
                    .inner()
                    .clone();
                let exit_authorized = std::sync::Arc::clone(&exit_authorized_for_run);
                let shutdown_started = std::sync::Arc::clone(&shutdown_started_for_run);
                let app_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if manager.shutdown_for_exit().await.is_ok() {
                        exit_authorized.store(true, Ordering::Release);
                        if let Some(window) = app_handle.get_webview_window("main") {
                            devbox_window_state_tauri::save_main_webview_window(&window);
                        }
                        app_handle.exit(0);
                    } else {
                        // Fail closed: keep the app alive while an owned child
                        // is not termination-confirmed. A later exit request
                        // retries the same idempotent shutdown boundary.
                        shutdown_started.store(false, Ordering::Release);
                    }
                });
            }
        }
    });
}
