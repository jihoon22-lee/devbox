mod commands;
mod core;

use commands::docs::AppState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::docs::get_root,
            commands::docs::set_root,
            commands::docs::list_tree,
            commands::docs::read_file,
            commands::docs::write_file,
            commands::docs::create_file,
            commands::docs::rename_file,
            commands::docs::delete_file,
            commands::docs::search_docs,
            commands::docs::list_tags,
            commands::docs::daily_note,
            commands::markdown::render_markdown,
        ])
        .setup(|app| {
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let conn = core::db::init(&dir.join("data.db"))?;
            let state = Arc::new(AppState {
                db: Mutex::new(conn),
                image_cache: Mutex::new(HashMap::new()),
            });
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
