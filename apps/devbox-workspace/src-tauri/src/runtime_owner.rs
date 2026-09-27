//! Native runtime owner selection and bounded agent response projection.
use serde_json::{json, Value};
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OwnerKind {
    Agent,
    Local,
}
fn decide_owner(availability: &str) -> Result<OwnerKind> {
    match availability {
        "connected" => Ok(OwnerKind::Agent),
        "unsupported" => Ok(OwnerKind::Local),
        _ => Err("runtime_agent_unavailable"),
    }
}
/// Installed ownership is immutable even while the agent is disconnected.
pub(crate) fn installed(app: &tauri::AppHandle) -> Result<bool> {
    app.try_state::<agent_client::AgentClient>()
        .map(|client| client.supported())
        .ok_or("runtime_agent_unavailable")
}
pub(crate) async fn owner(app: &tauri::AppHandle) -> Result<OwnerKind> {
    let client = app
        .try_state::<agent_client::AgentClient>()
        .ok_or("runtime_agent_unavailable")?;
    if !client.supported() {
        return decide_owner("unsupported");
    }
    let handshake = product_shell_tauri::native_handshake(app, "workspace")?;
    client
        .connect(&handshake.session_id)
        .await
        .map_err(|_| "runtime_agent_unavailable")?;
    decide_owner(client.status())
}
pub(crate) async fn call(
    app: &tauri::AppHandle,
    component: &str,
    method: &str,
    args: Value,
    route: &str,
    context: Option<&product_contract::ProjectContext>,
) -> Result<Value> {
    call_until(app, component, method, args, route, context, u64::MAX).await
}
pub(crate) async fn call_until(
    app: &tauri::AppHandle,
    component: &str,
    method: &str,
    args: Value,
    route: &str,
    context: Option<&product_contract::ProjectContext>,
    deadline: u64,
) -> Result<Value> {
    let handshake = product_shell_tauri::native_handshake(app, "workspace")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "request_expired")?
        .as_millis() as u64;
    let request = json!({"header":{
        "protocolVersion":handshake.protocol_version,"installationId":handshake.installation_id,
        "sessionId":handshake.session_id,"requestId":uuid::Uuid::new_v4().to_string(),
        "deadlineMs":deadline.min(now.saturating_add(29000)),"route":route,"context":context,
    },"method":method,"args":args});
    let client = app
        .try_state::<agent_client::AgentClient>()
        .ok_or("runtime_agent_unavailable")?;
    let reply = client
        .call(component, request)
        .await
        .map_err(|_| "runtime_agent_unavailable")?;
    response(reply)
}
pub(crate) async fn session<T: serde::de::DeserializeOwned>(
    app: &tauri::AppHandle,
    call: workspace_core::session_rpc::Call,
) -> Result<T> {
    call.validate()?;
    let value = self::call(
        app,
        "workspace.runtime",
        "session_owner",
        serde_json::to_value(&call).map_err(|_| "session_runtime_invalid")?,
        "sessions",
        call.live_context(),
    )
    .await?;
    serde_json::from_value(value).map_err(|_| "session_runtime_invalid")
}
fn response(value: Value) -> Result<Value> {
    match value
        .pointer("/operation/outcome/state")
        .and_then(Value::as_str)
    {
        Some("succeeded") => value
            .get("value")
            .cloned()
            .ok_or("runtime_agent_unavailable"),
        Some("failed") => Err(
            match value.pointer("/value/issue").and_then(Value::as_str) {
                Some("session_runtime_stale") => "session_runtime_stale",
                Some("session_runtime_conflict") => "session_runtime_conflict",
                Some("session_runtime_limit") => "session_runtime_limit",
                Some("session_runtime_cancelled") => "session_runtime_cancelled",
                Some("session_runtime_pending") => "session_runtime_pending",
                Some("session_runtime_borrowed") => "session_runtime_borrowed",
                Some("session_runtime_definition_changed") => "session_runtime_definition_changed",
                Some("session_runtime_source_changed") => "session_runtime_source_changed",
                Some("session_runtime_changed") => "session_runtime_changed",
                Some("session_runtime_invalid") => "session_runtime_invalid",
                Some("session_runtime_unavailable") => "session_runtime_unavailable",
                Some(issue) => workspace_core::runtime_policy::issue(issue.into()),
                None => "runtime_agent_unavailable",
            },
        ),
        _ => Err("runtime_agent_unavailable"),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn ownership_follows_agent_availability_without_installed_local_fallback() {
        assert_eq!(decide_owner("connected"), Ok(OwnerKind::Agent));
        assert_eq!(decide_owner("unsupported"), Ok(OwnerKind::Local));
        for status in ["unavailable", "starting", "unknown"] {
            assert_eq!(decide_owner(status), Err("runtime_agent_unavailable"));
        }
    }
    #[test]
    fn reply_projection_requires_success_and_never_returns_private_errors() {
        assert_eq!(
            response(json!({"operation":{"outcome":{"state":"succeeded"}},"value":{"ready":true}})),
            Ok(json!({"ready":true}))
        );
        assert_eq!(
            response(
                json!({"operation":{"outcome":{"state":"failed"}},"value":{"issue":"session_runtime_stale"}})
            ),
            Err("session_runtime_stale")
        );
        assert_eq!(
            response(
                json!({"operation":{"outcome":{"state":"failed"}},"value":{"issue":"private/path?secret=value"}})
            ),
            Err("runtime_operation_unavailable")
        );
        assert!(response(json!({"value":true})).is_err());
    }
}

pub(crate) async fn query<T: serde::de::DeserializeOwned>(
    app: &tauri::AppHandle,
    host: &workspace_core::Host,
    definitions: &std::sync::Mutex<workspace_core::definitions::Definitions>,
    query: workspace_core::runtime_queries::Call,
    deadline: u64,
) -> Result<T> {
    query.validate()?;
    workspace_core::current_deadline(deadline)?;
    let value = if installed(app)? {
        call_until(
            app,
            "workspace.runtime",
            "native_query",
            serde_json::to_value(&query).map_err(|_| "invalid_request")?,
            "sessions",
            query.context(),
            deadline,
        )
        .await?
    } else {
        workspace_core::runtime_queries::execute(app, host, definitions, query, deadline).await?
    };
    workspace_core::current_deadline(deadline)?;
    serde_json::from_value(value).map_err(|_| "invalid_response")
}

pub(crate) async fn diagnostic_identity(
    app: &tauri::AppHandle,
    host: &workspace_core::Host,
    context: &product_contract::ProjectContext,
    run: &str,
) -> Result<Option<(String, i64)>> {
    query(
        app,
        host,
        &std::sync::Mutex::new(workspace_core::definitions::Definitions::default()),
        workspace_core::runtime_queries::Call::DiagnosticIdentity {
            context: context.clone(),
            run_id: run.into(),
        },
        u64::MAX,
    )
    .await
}
pub(crate) fn diagnostic_matches(
    app: &tauri::AppHandle,
    host: &workspace_core::Host,
    context: &product_contract::ProjectContext,
    run: &str,
) -> bool {
    tauri::async_runtime::block_on(diagnostic_identity(app, host, context, run))
        .is_ok_and(|value| value.is_some())
}
pub(crate) async fn log_revision(
    app: &tauri::AppHandle,
    host: &workspace_core::Host,
    run: &str,
) -> Result<String> {
    query(
        app,
        host,
        &std::sync::Mutex::new(workspace_core::definitions::Definitions::default()),
        workspace_core::runtime_queries::Call::LogRevision { run_id: run.into() },
        u64::MAX,
    )
    .await
}
pub(crate) async fn engine(
    app: &tauri::AppHandle,
    method: &str,
    args: Value,
    context: Option<&product_contract::ProjectContext>,
    deadline: u64,
) -> Result<Value> {
    if installed(app)? {
        return call_until(
            app,
            "workspace.runtime",
            method,
            args,
            "tasks",
            context,
            deadline,
        )
        .await;
    }
    let call = serde_json::from_value(json!({"method":method,"args":args}))
        .map_err(|_| "invalid_request")?;
    runtime_engine::api::dispatch(app, call)
        .await
        .map_err(workspace_core::runtime_policy::issue)
}
