// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

use commands::server_state;
use tauri::Manager;

mod commands;
mod core;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let service_profile_id = match core::service_profile::parse_service_profile_argv(
        &std::env::args().collect::<Vec<_>>(),
    ) {
        Ok(profile) => profile,
        Err(error) => {
            eprintln!("webhook-lab: {error}");
            return;
        }
    };
    let service_mode = service_profile_id.is_some();
    let state = server_state();
    let mut builder = tauri::Builder::default();
    if service_profile_id.is_none() {
        // single-instance must remain the first plugin in interactive mode so
        // a duplicate process exits before any later plugin or setup work.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));
    }
    builder = builder.plugin(tauri_plugin_opener::init());
    builder
        .setup(move |app| {
            app.manage(std::sync::Arc::clone(&state));
            if let Some(profile_id) = service_profile_id.as_deref() {
                let data_root = app.path().app_local_data_dir().map_err(|_| {
                    std::io::Error::other(core::service_profile::SERVICE_PROFILE_LOAD_ERROR)
                })?;
                let profile = core::service_profile::load_profile(&data_root, profile_id)
                    .map_err(std::io::Error::other)?;
                *state.rules.lock().map_err(|_| {
                    std::io::Error::other(core::service_profile::SERVICE_PROFILE_LOAD_ERROR)
                })? = core::service_profile::rules_map(&profile);
                *state.sequence_cursors.lock().map_err(|_| {
                    std::io::Error::other(core::service_profile::SERVICE_PROFILE_LOAD_ERROR)
                })? = core::rules::ResponseSequenceState::default();
                commands::start_server_inner(&state, Some(profile.bind), profile.port, Some(false))
                    .map_err(std::io::Error::other)?;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                    let _ = window.set_skip_taskbar(true);
                }
            } else {
                devbox_window_state_tauri::restore_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(move |window, event| {
            // The hidden service process shares the application's local-data
            // root. It must never overwrite the user's persistent main-window
            // geometry with its non-interactive default window state.
            if !service_mode {
                devbox_window_state_tauri::handle_window_event(window, event);
            }
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
            commands::send_history_to_log_lens,
            commands::send_fixture_to_log_lens,
            commands::list_rules,
            commands::preview_rule_conflicts,
            commands::set_rule,
            commands::delete_rule,
            commands::reset_rule_sequence,
            commands::export_run_service_definition,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
