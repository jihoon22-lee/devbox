mod commands;

use commands::terminal::SessionState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::terminal::list_distros,
            commands::terminal::start_session,
            commands::terminal::write_session,
            commands::terminal::broadcast,
            commands::terminal::resize_session,
            commands::terminal::close_session,
            commands::terminal::list_sessions,
        ])
        .setup(|app| {
            let state = Arc::new(SessionState {
                sessions: Mutex::new(HashMap::new()),
            });
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
