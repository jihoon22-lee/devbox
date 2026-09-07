//! Product-owned adapters around the existing native implementation.
use std::path::PathBuf;
use tauri::Manager;

#[cfg_attr(not(windows), allow(dead_code))]
struct ComponentRoot(PathBuf);

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn data_root(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    if let Some(root) = app.try_state::<ComponentRoot>() {
        return Ok(root.0.clone());
    }
    app.path().app_local_data_dir()
}

pub fn initialize(
    app: &tauri::AppHandle,
    handoff_store: devbox_applink::HandoffStore,
) -> Result<(), String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "component_storage_unavailable")?
        .join("api");
    if !app.manage(ComponentRoot(root)) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(crate::commands::request::ResponseHeaderVault::default()) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(crate::commands::request::RequestCancellation::default()) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(std::sync::Arc::new(
        crate::commands::mcp::McpHttpState::default(),
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(std::sync::Arc::new(
        crate::commands::mcp_oauth::McpOAuthState::default(),
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(std::sync::Arc::new(
        crate::commands::mcp_stdio::McpStdioState::default(),
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(std::sync::Arc::new(
        crate::commands::grpc::GrpcState::default(),
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(std::sync::Arc::new(
        crate::commands::grpc_selection::GrpcSelectionState::default(),
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(std::sync::Arc::new(
        crate::commands::grpc_credentials::GrpcCredentialState::default(),
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(std::sync::Arc::new(
        crate::commands::sse::SseState::default(),
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(std::sync::Arc::new(
        crate::commands::websocket::WebSocketState::default(),
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(crate::commands::handoff::ApiHandoffState::with_store(
        handoff_store,
    )) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(crate::applink::PendingOpen::new()) {
        return Err("component_state_conflict".into());
    }
    Ok(())
}

pub const COMMANDS: &[&str] = &[
    "claim_api_request",
    "renew_api_request",
    "ack_api_request",
    "restore_api_request",
    "take_pending_open",
    "fetch_openapi_source",
    "send_request",
    "cancel_request",
    "discard_current_response",
    "build_revealed_curl",
    "copy_raw_response_headers",
    "copy_raw_response_cookies",
    "save_response_binary",
    "sanitize_persisted_json",
    "read_json_file",
    "save_json_file",
    "seal_secret",
    "connect_mcp_http",
    "invoke_mcp_http",
    "cancel_mcp_http",
    "disconnect_mcp_http",
    "authorize_mcp_http",
    "cancel_mcp_oauth",
    "list_mcp_oauth_grants",
    "revoke_mcp_oauth_grant",
    "pick_mcp_stdio_executable",
    "pick_mcp_stdio_cwd",
    "connect_mcp_stdio",
    "invoke_mcp_stdio",
    "cancel_mcp_stdio",
    "disconnect_mcp_stdio",
    "pick_grpc_proto",
    "pick_grpc_import_root",
    "connect_grpc",
    "invoke_grpc",
    "cancel_grpc",
    "disconnect_grpc",
    "export_grpc_summary",
    "pick_grpc_ca",
    "pick_grpc_client_certificate",
    "pick_grpc_client_key",
    "import_grpc_tls_credential",
    "list_grpc_tls_credentials",
    "delete_grpc_tls_credential",
    "start_sse_stream",
    "stop_sse_stream",
    "start_websocket",
    "send_websocket_message",
    "ping_websocket",
    "close_websocket",
    "disconnect_websocket",
    "save_websocket_binary",
];

/// No dynamic command loading or legacy application startup occurs here.
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "claim_api_request" => {
            crate::commands::handoff::__component_claim_api_request(app, args).await
        }
        "renew_api_request" => {
            crate::commands::handoff::__component_renew_api_request(app, args).await
        }
        "ack_api_request" => crate::commands::handoff::__component_ack_api_request(app, args).await,
        "restore_api_request" => {
            crate::commands::handoff::__component_restore_api_request(app, args).await
        }

        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "fetch_openapi_source" => {
            crate::commands::openapi::__component_fetch_openapi_source(app, args).await
        }
        "send_request" => crate::commands::request::__component_send_request(app, args).await,
        "cancel_request" => crate::commands::request::__component_cancel_request(app, args).await,
        "discard_current_response" => {
            crate::commands::request::__component_discard_current_response(app, args).await
        }
        "build_revealed_curl" => {
            crate::commands::request::__component_build_revealed_curl(app, args).await
        }
        "copy_raw_response_headers" => {
            crate::commands::request::__component_copy_raw_response_headers(app, args).await
        }
        "copy_raw_response_cookies" => {
            crate::commands::request::__component_copy_raw_response_cookies(app, args).await
        }
        "save_response_binary" => {
            crate::commands::request::__component_save_response_binary(app, args).await
        }
        "sanitize_persisted_json" => {
            crate::commands::request::__component_sanitize_persisted_json(app, args).await
        }
        "read_json_file" => crate::commands::transfer::__component_read_json_file(app, args).await,
        "save_json_file" => crate::commands::transfer::__component_save_json_file(app, args).await,
        "seal_secret" => crate::commands::secrets::__component_seal_secret(app, args).await,
        "connect_mcp_http" => crate::commands::mcp::__component_connect_mcp_http(app, args).await,
        "invoke_mcp_http" => crate::commands::mcp::__component_invoke_mcp_http(app, args).await,
        "cancel_mcp_http" => crate::commands::mcp::__component_cancel_mcp_http(app, args).await,
        "disconnect_mcp_http" => {
            crate::commands::mcp::__component_disconnect_mcp_http(app, args).await
        }
        "authorize_mcp_http" => {
            crate::commands::mcp_oauth::__component_authorize_mcp_http(app, args).await
        }
        "cancel_mcp_oauth" => {
            crate::commands::mcp_oauth::__component_cancel_mcp_oauth(app, args).await
        }
        "list_mcp_oauth_grants" => {
            crate::commands::mcp_oauth::__component_list_mcp_oauth_grants(app, args).await
        }
        "revoke_mcp_oauth_grant" => {
            crate::commands::mcp_oauth::__component_revoke_mcp_oauth_grant(app, args).await
        }
        "pick_mcp_stdio_executable" => {
            crate::commands::mcp_stdio::__component_pick_mcp_stdio_executable(app, args).await
        }
        "pick_mcp_stdio_cwd" => {
            crate::commands::mcp_stdio::__component_pick_mcp_stdio_cwd(app, args).await
        }
        "connect_mcp_stdio" => {
            crate::commands::mcp_stdio::__component_connect_mcp_stdio(app, args).await
        }
        "invoke_mcp_stdio" => {
            crate::commands::mcp_stdio::__component_invoke_mcp_stdio(app, args).await
        }
        "cancel_mcp_stdio" => {
            crate::commands::mcp_stdio::__component_cancel_mcp_stdio(app, args).await
        }
        "disconnect_mcp_stdio" => {
            crate::commands::mcp_stdio::__component_disconnect_mcp_stdio(app, args).await
        }
        "pick_grpc_proto" => crate::commands::grpc::__component_pick_grpc_proto(app, args).await,
        "pick_grpc_import_root" => {
            crate::commands::grpc::__component_pick_grpc_import_root(app, args).await
        }
        "connect_grpc" => crate::commands::grpc::__component_connect_grpc(app, args).await,
        "invoke_grpc" => crate::commands::grpc::__component_invoke_grpc(app, args).await,
        "cancel_grpc" => crate::commands::grpc::__component_cancel_grpc(app, args).await,
        "disconnect_grpc" => crate::commands::grpc::__component_disconnect_grpc(app, args).await,
        "export_grpc_summary" => {
            crate::commands::grpc::__component_export_grpc_summary(app, args).await
        }
        "pick_grpc_ca" => {
            crate::commands::grpc_credentials::__component_pick_grpc_ca(app, args).await
        }
        "pick_grpc_client_certificate" => {
            crate::commands::grpc_credentials::__component_pick_grpc_client_certificate(app, args)
                .await
        }
        "pick_grpc_client_key" => {
            crate::commands::grpc_credentials::__component_pick_grpc_client_key(app, args).await
        }
        "import_grpc_tls_credential" => {
            crate::commands::grpc_credentials::__component_import_grpc_tls_credential(app, args)
                .await
        }
        "list_grpc_tls_credentials" => {
            crate::commands::grpc_credentials::__component_list_grpc_tls_credentials(app, args)
                .await
        }
        "delete_grpc_tls_credential" => {
            crate::commands::grpc_credentials::__component_delete_grpc_tls_credential(app, args)
                .await
        }
        "start_sse_stream" => crate::commands::sse::__component_start_sse_stream(app, args).await,
        "stop_sse_stream" => crate::commands::sse::__component_stop_sse_stream(app, args).await,
        "start_websocket" => {
            crate::commands::websocket::__component_start_websocket(app, args).await
        }
        "send_websocket_message" => {
            crate::commands::websocket::__component_send_websocket_message(app, args).await
        }
        "ping_websocket" => crate::commands::websocket::__component_ping_websocket(app, args).await,
        "close_websocket" => {
            crate::commands::websocket::__component_close_websocket(app, args).await
        }
        "disconnect_websocket" => {
            crate::commands::websocket::__component_disconnect_websocket(app, args).await
        }
        "save_websocket_binary" => {
            crate::commands::websocket::__component_save_websocket_binary(app, args).await
        }
        _ => Err("component_method_unavailable".into()),
    }
}

/// Native-only delivery. A pending user action cannot be overwritten.
pub fn deliver(app: &tauri::AppHandle, request: devbox_applink::OpenRequest) -> Result<(), String> {
    use tauri::Emitter as _;
    app.state::<crate::applink::PendingOpen>()
        .try_set(request)?;
    // The slot owns the delivery. Wake-up failure leaves it for a cold pull.
    let _ = app.emit_to("main", "api-studio://api-open", ());
    Ok(())
}

/// Importer-only native API ownership; this is not a renderer command registry.
pub fn prepare_legacy_environment(raw: &str) -> Result<(String, usize), String> {
    crate::commands::migration::prepare_environment(raw)
}
pub fn sanitize_legacy_json(serialized: String, environment: &str) -> Result<String, String> {
    crate::commands::migration::sanitize(serialized, environment)
}

/// Native product workers reuse the MCP stdio process-tree ownership primitive.
/// Windows callers must create the root suspended before assignment.
pub struct OwnedProcessTree(crate::commands::process_tree::ProcessTree);
impl OwnedProcessTree {
    pub fn assign(child: &tokio::process::Child) -> Result<Self, String> {
        crate::commands::process_tree::ProcessTree::assign(child)
            .map(Self)
            .map_err(|_| "owned_worker_assignment_failed".into())
    }
    pub async fn terminate(&mut self, child: &mut tokio::process::Child) -> bool {
        self.0.terminate(child).await
    }
}

pub fn prepare_legacy_native_store(
    kind: &str,
    bytes: &[u8],
) -> Result<(serde_json::Value, Vec<String>), String> {
    match kind {
        "oauth" => crate::commands::mcp_oauth::prepare_legacy_store(bytes),
        "grpc-tls" => crate::commands::grpc_credentials::prepare_legacy_store(bytes),
        _ => Err("legacy_api_store_invalid".into()),
    }
}
pub fn validate_migration_native_store(kind: &str, bytes: &[u8]) -> Result<(), String> {
    match kind {
        "oauth" => crate::commands::mcp_oauth::validate_migration_store(bytes),
        "grpc-tls" => crate::commands::grpc_credentials::validate_migration_store(bytes),
        _ => Err("legacy_api_store_invalid".into()),
    }
}
