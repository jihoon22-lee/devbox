mod applink;
mod commands;
pub mod core;
mod handoff;

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Ok(Some(request)) = devbox_applink::parse_argv(&args) {
                if applink::is_log_source_request(&request) {
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
        .manage(commands::AppState::default())
        .manage(applink::PendingOpen::new())
        .manage(handoff::PendingLogSource::new())
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::summarize_source,
            commands::receive_log_source,
            commands::fixed_adapter,
            commands::read_source,
            commands::read_sources,
            commands::cancel_read,
            commands::filter_log_records,
            commands::export_log_records,
            handoff::preview_log_source,
            handoff::accept_log_source,
            handoff::discard_log_source,
            handoff::renew_log_source,
        ])
        .setup(|app| {
            if let Ok(Some(request)) =
                devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>())
            {
                if applink::is_log_source_request(&request) {
                    app.state::<applink::PendingOpen>().set(request);
                }
            }
            Ok(())
        });
    app.run(tauri::generate_context!())
        .expect("error while running Log Lens");
}
