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

/// Native-only delivery. A pending user action cannot be overwritten.
pub fn deliver(app: &tauri::AppHandle, request: devbox_applink::OpenRequest) -> Result<(), String> {
    use tauri::Emitter as _;
    app.state::<crate::applink::PendingOpen>()
        .try_set(request)?;
    // The slot owns the delivery. Wake-up failure leaves it for a cold pull.
    let _ = app.emit_to("main", "api-studio://api-open", ());
    Ok(())
}

/// Native product workers reuse the MCP stdio process-tree ownership primitive.
/// Windows callers must create the root suspended before assignment.
pub struct OwnedProcessTree(process_tree::ProcessTree);
impl OwnedProcessTree {
    pub fn assign(child: &tokio::process::Child) -> Result<Self, String> {
        process_tree::ProcessTree::assign(child)
            .map(Self)
            .map_err(|_| "owned_worker_assignment_failed".into())
    }
    pub async fn terminate(&mut self, child: &mut tokio::process::Child) -> bool {
        self.0.terminate(child).await
    }
}

/// Product-owned saved OpenAPI projections reuse the actual request wire type.
/// This only normalizes a draft; it never resolves variables or sends a request.
pub fn normalize_openapi_request(value: serde_json::Value) -> Result<serde_json::Value, String> {
    const ERROR: &str = "openapi_definition_invalid";
    let request: crate::commands::request::RequestTemplate =
        serde_json::from_value(value).map_err(|_| ERROR)?;
    if !matches!(
        request.method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" | "TRACE"
    ) || request.url.len() > 16 * 1024
        || request.url.chars().any(char::is_control)
        || !(request.url.starts_with("http://") || request.url.starts_with("https://"))
        || request.headers.len() > 100
        || request.cookies.len() > 100
        || request.params.len() > 100
        || request.multipart.len() > 100
        || request.body.len() > 512 * 1024
        || request.timeout_ms == 0
        || request.timeout_ms > 300_000
        || request.graphql.is_some()
        || !matches!(
            request.body_kind.as_str(),
            "none" | "json" | "raw" | "form" | "multipart"
        )
    {
        return Err(ERROR.into());
    }
    crate::commands::request::validate_multipart_rows(&request).map_err(|_| ERROR)?;
    let mut value = serde_json::to_value(request).map_err(|_| ERROR)?;
    value["requiresSecretReview"] = true.into();
    Ok(value)
}

pub fn sanitize_openapi_request(value: serde_json::Value) -> Result<serde_json::Value, String> {
    let normalized = normalize_openapi_request(value)?;
    let request = serde_json::from_value(normalized).map_err(|_| "openapi_definition_invalid")?;
    crate::commands::request::sanitize_openapi_template(request)
}

/// Reuse the current persistence sanitizer for product-owned OpenAPI definitions.
pub fn sanitize_saved_json(serialized: String, environment: &str) -> Result<String, String> {
    crate::commands::saved_environment::sanitize(serialized, environment)
}
