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
        .manage(std::sync::Arc::new(commands::mcp::McpHttpState::default()))
        .manage(std::sync::Arc::new(
            commands::mcp_oauth::McpOAuthState::default(),
        ))
        .manage(std::sync::Arc::new(
            commands::mcp_stdio::McpStdioState::default(),
        ))
        .manage(std::sync::Arc::new(commands::grpc::GrpcState::default()))
        .manage(std::sync::Arc::new(
            commands::grpc_selection::GrpcSelectionState::default(),
        ))
        .manage(std::sync::Arc::new(
            commands::grpc_credentials::GrpcCredentialState::default(),
        ))
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
            commands::mcp::connect_mcp_http,
            commands::mcp::invoke_mcp_http,
            commands::mcp::cancel_mcp_http,
            commands::mcp::disconnect_mcp_http,
            commands::mcp_oauth::authorize_mcp_http,
            commands::mcp_oauth::cancel_mcp_oauth,
            commands::mcp_oauth::list_mcp_oauth_grants,
            commands::mcp_oauth::revoke_mcp_oauth_grant,
            commands::mcp_stdio::pick_mcp_stdio_executable,
            commands::mcp_stdio::pick_mcp_stdio_cwd,
            commands::mcp_stdio::connect_mcp_stdio,
            commands::mcp_stdio::invoke_mcp_stdio,
            commands::mcp_stdio::cancel_mcp_stdio,
            commands::mcp_stdio::disconnect_mcp_stdio,
            commands::grpc::pick_grpc_proto,
            commands::grpc::pick_grpc_import_root,
            commands::grpc::connect_grpc,
            commands::grpc::invoke_grpc,
            commands::grpc::cancel_grpc,
            commands::grpc::disconnect_grpc,
            commands::grpc::export_grpc_summary,
            commands::grpc_credentials::pick_grpc_ca,
            commands::grpc_credentials::pick_grpc_client_certificate,
            commands::grpc_credentials::pick_grpc_client_key,
            commands::grpc_credentials::import_grpc_tls_credential,
            commands::grpc_credentials::list_grpc_tls_credentials,
            commands::grpc_credentials::delete_grpc_tls_credential,
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
