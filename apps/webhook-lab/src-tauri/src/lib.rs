// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

use commands::server_state;
use tauri::Manager;

mod commands;
mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            app.manage(server_state());
            Ok(())
        })
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::server_status,
            commands::start_server,
            commands::stop_server,
            commands::list_history,
            commands::clear_history,
            commands::copy_masked_history,
            commands::copy_raw_history,
            commands::copy_history_headers,
            commands::delete_history,
            commands::replay_history,
            commands::list_fixtures,
            commands::save_fixture,
            commands::delete_fixture,
            commands::clear_fixtures,
            commands::fixture_to_rule,
            commands::replay_fixture,
            commands::send_history_to_api,
            commands::send_fixture_to_api,
            commands::list_rules,
            commands::set_rule,
            commands::delete_rule,
            commands::reset_rule_sequence,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
