//! Shared native adapter for the two products exposing the agent preference.
use product_contract::agent_settings::{AgentAutostartStatus, AgentSettingsCall};
use serde_json::{json, Value};
use tauri::Manager;
pub async fn call(
    app: &tauri::AppHandle,
    product: &str,
    call: AgentSettingsCall,
    deadline: u64,
) -> Result<Value, String> {
    let route = match product {
        "knowledge" => "activity",
        "control-center" => "environment",
        _ => return Err("autostart_owner_invalid".into()),
    };
    let client = app
        .try_state::<agent_client::AgentClient>()
        .ok_or("autostart_unavailable")?;
    if !client.supported() {
        return serde_json::to_value(AgentAutostartStatus {
            supported: false,
            enabled: false,
        })
        .map_err(|_| "autostart_unavailable".into());
    }
    let handshake = crate::native_handshake(app, product)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "autostart_unavailable")?
        .as_millis() as u64;
    let mut request = serde_json::to_value(call).map_err(|_| "autostart_unavailable")?;
    request["header"] = json!({"protocolVersion":handshake.protocol_version,"installationId":handshake.installation_id,"sessionId":handshake.session_id,"requestId":uuid::Uuid::new_v4().to_string(),"deadlineMs":deadline.min(now.saturating_add(29000)),"route":route});
    let reply = client
        .call("agent.settings", request)
        .await
        .map_err(|_| "autostart_unavailable")?;
    if reply
        .pointer("/operation/outcome/state")
        .and_then(Value::as_str)
        == Some("succeeded")
    {
        let status: AgentAutostartStatus =
            serde_json::from_value(reply["value"].clone()).map_err(|_| "autostart_unavailable")?;
        serde_json::to_value(status).map_err(|_| "autostart_unavailable".into())
    } else {
        Err(
            match reply.pointer("/value/issue").and_then(Value::as_str) {
                Some("agent_autostart_conflict") => "autostart_owner_conflict",
                Some("agent_autostart_path_too_long") => "autostart_path_too_long",
                _ => "autostart_unavailable",
            }
            .into(),
        )
    }
}

/// Control Center alone exposes MCP settings; Knowledge's login adapter stays closed.
pub async fn mcp_call(
    app: &tauri::AppHandle,
    product: &str,
    call: product_contract::agent_settings::McpSettingsCall,
    deadline: u64,
) -> Result<Value, String> {
    use product_contract::agent_settings::AgentMcpStatus;
    const ISSUE: &str = "mcp_settings_unavailable";
    if product != "control-center" {
        return Err(ISSUE.into());
    }
    let client = app.try_state::<agent_client::AgentClient>().ok_or(ISSUE)?;
    if !client.supported() {
        return serde_json::to_value(AgentMcpStatus::default()).map_err(|_| ISSUE.into());
    }
    let handshake = crate::native_handshake(app, product)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ISSUE)?
        .as_millis() as u64;
    let mut request = serde_json::to_value(call).map_err(|_| ISSUE)?;
    request["header"] = json!({"protocolVersion":handshake.protocol_version,"installationId":handshake.installation_id,"sessionId":handshake.session_id,"requestId":uuid::Uuid::new_v4().to_string(),"deadlineMs":deadline.min(now.saturating_add(29000)),"route":"environment"});
    let reply = client
        .call("agent.settings", request)
        .await
        .map_err(|_| ISSUE)?;
    if reply
        .pointer("/operation/outcome/state")
        .and_then(Value::as_str)
        != Some("succeeded")
    {
        return Err(ISSUE.into());
    }
    let status: AgentMcpStatus =
        serde_json::from_value(reply["value"].clone()).map_err(|_| ISSUE)?;
    serde_json::to_value(status).map_err(|_| ISSUE.into())
}
