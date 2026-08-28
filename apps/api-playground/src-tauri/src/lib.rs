mod applink;
mod commands;
mod core;
mod platform;

use tauri::{Emitter, Manager};

// TODO(0.5.0): v0.4.x 이전 사용자를 위한 1회성 마이그레이션. 두 릴리스 뒤 제거한다.
const LEGACY_IDENTIFIER: &str = "com.workbench.apiplayground";
const CURRENT_IDENTIFIER: &str = "com.devbox.apiplayground";

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
        // Store before emitting so cold and hot delivery share one pending
        // slot and the renderer can always re-read the current request.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            match devbox_applink::parse_argv(&args) {
                Ok(Some(request)) => {
                    app.state::<applink::PendingOpen>().set(request.clone());
                    let _ = app.emit("devbox://open", request);
                }
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .manage(commands::request::ResponseHeaderVault::default())
        .manage(commands::request::RequestCancellation::default())
        .manage(std::sync::Arc::new(commands::sse::SseState::default()))
        .manage(std::sync::Arc::new(
            commands::websocket::WebSocketState::default(),
        ))
        .manage(commands::handoff::ApiHandoffState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            devbox_window_state_tauri::handle_window_event(window, event);
        })
        .setup(|app| {
            devbox_window_state_tauri::restore_main_window(app.handle());
            app.manage(applink::PendingOpen::new());
            match devbox_applink::parse_argv(&std::env::args().collect::<Vec<_>>()) {
                Ok(Some(request)) => app.state::<applink::PendingOpen>().set(request),
                Ok(None) => {}
                Err(_) => eprintln!("applink: invalid request"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            applink::take_pending_open,
            commands::openapi::fetch_openapi_source,
            commands::request::send_request,
            commands::request::cancel_request,
            commands::request::discard_current_response,
            commands::request::build_revealed_curl,
            commands::request::copy_raw_response_headers,
            commands::request::copy_raw_response_cookies,
            commands::request::save_response_binary,
            commands::request::sanitize_persisted_json,
            commands::transfer::read_json_file,
            commands::transfer::save_json_file,
            commands::secrets::seal_secret,
            commands::handoff::claim_api_request,
            commands::handoff::renew_api_request,
            commands::handoff::ack_api_request,
            commands::handoff::restore_api_request,
            commands::sse::start_sse_stream,
            commands::sse::stop_sse_stream,
            commands::websocket::start_websocket,
            commands::websocket::send_websocket_message,
            commands::websocket::ping_websocket,
            commands::websocket::close_websocket,
            commands::websocket::disconnect_websocket,
            commands::websocket::save_websocket_binary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
