//! Knowledge keeps UI/vault/selection ownership; background engine state is remote.
use serde_json::{json, Value};
use tauri::Manager;
pub fn installed(app: &tauri::AppHandle) -> Result<bool, String> {
    app.try_state::<agent_client::AgentClient>()
        .map(|client| client.supported())
        .ok_or_else(|| "knowledge_agent_unavailable".into())
}
pub async fn call(
    app: &tauri::AppHandle,
    component: &str,
    method: &str,
    args: Value,
    deadline: u64,
) -> Result<Value, String> {
    if !installed(app)? {
        let raw = json!({"method":method,"args":args});
        return match component {
            "knowledge.activity" => {
                activity_engine::api::dispatch(
                    app,
                    serde_json::from_value(raw).map_err(|_| "component_args_invalid")?,
                )
                .await
            }
            "knowledge.search" => {
                content_index_engine::api::dispatch_search(
                    app,
                    serde_json::from_value(raw).map_err(|_| "component_args_invalid")?,
                )
                .await
            }
            "knowledge.search-settings" => {
                content_index_engine::api::dispatch_settings(
                    app,
                    serde_json::from_value(raw).map_err(|_| "component_args_invalid")?,
                )
                .await
            }
            _ => Err("component_args_invalid".into()),
        };
    }
    let route = match component {
        "knowledge.activity" => "activity",
        "knowledge.search" | "knowledge.search-settings" => "search",
        _ => return Err("component_args_invalid".into()),
    };
    let handshake = product_shell_tauri::native_handshake(app, "knowledge")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "knowledge_agent_unavailable")?
        .as_millis() as u64;
    let request = json!({"header":{
        "protocolVersion":handshake.protocol_version,"installationId":handshake.installation_id,
        "sessionId":handshake.session_id,"requestId":uuid::Uuid::new_v4().to_string(),
        "deadlineMs":deadline.min(now.saturating_add(29000)),"route":route,
    },"method":method,"args":args});
    let client = app
        .try_state::<agent_client::AgentClient>()
        .ok_or("knowledge_agent_unavailable")?;
    let reply = client
        .call(component, request)
        .await
        .map_err(|_| "knowledge_agent_unavailable")?;
    match reply
        .pointer("/operation/outcome/state")
        .and_then(Value::as_str)
    {
        Some("succeeded") => reply
            .get("value")
            .cloned()
            .ok_or_else(|| "component_response_invalid".into()),
        Some("failed") => {
            let issue = reply
                .pointer("/value/issue")
                .and_then(Value::as_str)
                .unwrap_or("unavailable");
            Err(if component == "knowledge.activity" {
                activity_engine::api::classify(issue)
            } else {
                content_index_engine::api::classify(issue)
            }
            .into())
        }
        _ => Err("component_response_invalid".into()),
    }
}
pub async fn root_health(
    app: &tauri::AppHandle,
    deadline: std::time::Instant,
) -> Result<Vec<(String, bool, bool)>, String> {
    if !installed(app)? {
        return Ok(content_index_engine::component::product_root_health(app));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "search_unavailable")?
        .as_millis() as u64;
    let budget = deadline
        .saturating_duration_since(std::time::Instant::now())
        .as_millis() as u64;
    serde_json::from_value(
        call(
            app,
            "knowledge.search",
            "native_root_health",
            json!({}),
            now.saturating_add(budget),
        )
        .await?,
    )
    .map_err(|_| "search_unavailable".into())
}
pub async fn send_draft(
    app: &tauri::AppHandle,
    args: Value,
    deadline: u64,
) -> Result<Value, String> {
    let prepared = call(
        app,
        "knowledge.activity",
        "native_draft_delivery",
        if let Some(id)=args.get("handoffId") { json!({"kind":"regenerate","handoffId":id}) } else { json!({"kind":"prepare","input":args["input"],"regeneratedFrom":args["regeneratedFrom"]}) },
        deadline,
    )
    .await?;
    let request: devbox_applink::OpenRequest = serde_json::from_value(prepared["request"].clone())
        .map_err(|_| "draft_delivery_invalid")?;
    let result = prepared
        .get("result")
        .ok_or("draft_delivery_invalid")?
        .clone();
    let id = result["id"].as_str().ok_or("draft_delivery_invalid")?;
    if !matches!(&request.target, devbox_applink::OpenTarget::Handoff {id: expected, kind} if expected == id && kind == "knowledge-draft/v1")
        || request.from.as_deref() != Some("life-log")
    {
        return Err("draft_delivery_invalid".into());
    }
    let delivery = knowledge_vault_engine::component::offer_product_draft(app, &request);
    let ack = call(
        app,
        "knowledge.activity",
        "native_draft_delivery",
        json!({"kind":"finish","id":id,"delivered":delivery.is_ok()}),
        deadline,
    )
    .await;
    delivery?;
    ack?;
    Ok(result)
}
