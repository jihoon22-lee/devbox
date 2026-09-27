//! Installed listener calls never fall back to a second local owner.
use serde_json::{json, Value};
use tauri::Manager;
pub fn installed(app: &tauri::AppHandle) -> Result<bool, String> {
    app.try_state::<agent_client::AgentClient>()
        .map(|client| client.supported())
        .ok_or_else(|| "webhook_agent_unavailable".into())
}
pub async fn call(
    app: &tauri::AppHandle,
    method: &str,
    args: Value,
    deadline: u64,
) -> Result<Value, String> {
    if !installed(app)? {
        let call = serde_json::from_value(json!({"method":method,"args":args}))
            .map_err(|_| "component_args_invalid")?;
        let value = webhook_host::api::dispatch(app, call).await?;
        crate::lifecycle::observe(app, &value);
        return Ok(value);
    }
    let handshake = product_shell_tauri::native_handshake(app, "api-studio")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "webhook_agent_unavailable")?
        .as_millis() as u64;
    let request = json!({"header":{
        "protocolVersion":handshake.protocol_version,"installationId":handshake.installation_id,
        "sessionId":handshake.session_id,"requestId":uuid::Uuid::new_v4().to_string(),
        "deadlineMs":deadline.min(now.saturating_add(29000)),"route":"webhooks",
    },"method":method,"args":args});
    let client = app
        .try_state::<agent_client::AgentClient>()
        .ok_or("webhook_agent_unavailable")?;
    let reply = client
        .call("api-studio.webhooks", request)
        .await
        .map_err(|_| "webhook_agent_unavailable")?;
    match reply
        .pointer("/operation/outcome/state")
        .and_then(Value::as_str)
    {
        Some("succeeded") => {
            let value = reply
                .get("value")
                .cloned()
                .ok_or("component_response_invalid")?;
            crate::lifecycle::observe(app, &value);
            Ok(value)
        }
        Some("failed") => Err(webhook_host::api::classify(
            reply
                .pointer("/value/issue")
                .and_then(Value::as_str)
                .unwrap_or("unavailable"),
        )
        .into()),
        _ => Err("component_response_invalid".into()),
    }
}
pub async fn project(
    app: &tauri::AppHandle,
    projection: webhook_host::component::Projection,
    deadline: u64,
) -> Result<Value, String> {
    project_from_owner(
        installed(app)?,
        || async {
            call(
                app,
                "native_projection",
                serde_json::to_value(projection.clone()).map_err(|_| "component_args_invalid")?,
                deadline,
            )
            .await
        },
        || webhook_host::component::project(app, projection.clone()),
    )
    .await
}
async fn project_from_owner<R, F, L>(installed: bool, remote: R, local: L) -> Result<Value, String>
where
    R: FnOnce() -> F,
    F: std::future::Future<Output = Result<Value, String>>,
    L: FnOnce() -> Result<Value, String>,
{
    if installed {
        remote().await
    } else {
        local()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn disconnected_installed_owner_never_reads_or_creates_local_listener_state() {
        assert_eq!(
            project_from_owner(
                true,
                || async { Err("webhook_agent_unavailable".into()) },
                || panic!("installed cannot fall back")
            )
            .await
            .unwrap_err(),
            "webhook_agent_unavailable"
        );
        assert_eq!(
            project_from_owner(
                false,
                || async { panic!("portable cannot launch agent") },
                || Ok(json!({"portable":true}))
            )
            .await
            .unwrap(),
            json!({"portable":true})
        );
    }
}
