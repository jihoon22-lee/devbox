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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::file::open_file,
            commands::file::save_file,
            commands::file::validate_encoding,
            commands::folder::list_workspace_files,
            commands::folder::canonicalize_workspace,
            commands::preview::render_preview,
            commands::session::load_session,
            commands::session::save_session,
            commands::watch::watch_file,
            commands::watch::unwatch_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
