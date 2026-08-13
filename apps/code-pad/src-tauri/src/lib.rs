pub mod commands;
pub mod core;
pub mod lsp;
pub mod watcher;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(watcher::WatcherManager::new(app.handle().clone()));
            let app_local_data_dir = app.path().app_local_data_dir()?;
            let installer = std::sync::Arc::new(lsp::ManagedInstaller::new(&app_local_data_dir)?);
            let manager = std::sync::Arc::new(lsp::LspManager::with_installer(
                app_local_data_dir,
                env!("CARGO_PKG_VERSION"),
                std::sync::Arc::clone(&installer),
            ));
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
