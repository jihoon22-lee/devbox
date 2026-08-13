pub mod commands;
pub mod core;
pub mod lsp;
pub mod watcher;

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.codepad";

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
    let shutdown_started = std::sync::Arc::new(AtomicBool::new(false));
    let exit_authorized = std::sync::Arc::new(AtomicBool::new(false));
    let shutdown_started_for_run = std::sync::Arc::clone(&shutdown_started);
    let exit_authorized_for_run = std::sync::Arc::clone(&exit_authorized);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if let Err(error) = migrate_local_data(app.handle(), LEGACY_IDENTIFIER) {
                eprintln!("devbox: local data migration will retry next launch: {error}");
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
            commands::file::open_file,
            commands::file::save_file,
            commands::file::validate_encoding,
            commands::folder::list_workspace_files,
            commands::folder::canonicalize_workspace,
            commands::installer::lsp_catalog,
            commands::installer::lsp_installed,
            commands::installer::lsp_recover_installed,
            commands::installer::lsp_install,
            commands::installer::lsp_uninstall,
            commands::preview::render_preview,
            commands::session::load_session,
            commands::session::save_session,
            commands::watch::watch_file,
            commands::watch::unwatch_file,
            commands::lsp::load_lsp_config,
            commands::lsp::save_lsp_config,
            commands::lsp::start_language_server,
            commands::lsp::stop_language_server,
            commands::lsp::restart_language_server,
            commands::lsp::stop_all_language_servers,
            commands::lsp::language_server_statuses,
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
            commands::lsp::request_lsp_formatting,
        ])
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
