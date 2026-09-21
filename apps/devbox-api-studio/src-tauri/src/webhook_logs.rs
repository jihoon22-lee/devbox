use product_contract::{transport::Call, webhook_log::Store};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(Mutex::default)
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub(crate) async fn send(
    app: &tauri::AppHandle,
    args: Value,
    saved: bool,
    operation: String,
    deadline: u64,
) -> Result<Value, String> {
    let payload = webhook_lab_lib::component::prepare_log_handoff(app, args, saved)?;
    let artifact = store().lock().map_err(|_| "webhook_log_busy")?.publish(
        uuid::Uuid::new_v4().simple().to_string(),
        payload,
        now(),
    )?;
    let revision = artifact.revision()?;
    crate::suite::remote(
        app,
        "workspace",
        Call::DeliverWebhookLog {
            id: artifact.id.clone(),
            revision,
            operation_id: operation,
        },
        deadline,
    )
    .await
    .map_err(|_| {
        "Workspace Logs에 연결하지 못했습니다. 원본 요청과 fixture는 유지됩니다.".to_owned()
    })?;
    Ok(
        json!({"handoffId":artifact.id,"producerId":"devbox-api-studio","consumerId":"devbox-workspace","createdAtMs":artifact.created_at_ms,"expiresAtMs":artifact.expires_at_ms}),
    )
}
pub(crate) fn handle(call: Call) -> Result<Value, &'static str> {
    let mut store = store().lock().map_err(|_| "webhook_log_busy")?;
    match call {
        Call::ClaimWebhookLog {
            id,
            revision,
            operation_id,
        } => serde_json::to_value(store.claim(&id, &revision, &operation_id, now())?)
            .map_err(|_| "webhook_log_invalid"),
        Call::AcknowledgeWebhookLog {
            id,
            revision,
            operation_id,
        } => {
            store.acknowledge(&id, &revision, &operation_id, now())?;
            Ok(Value::Null)
        }
        _ => Err("webhook_log_invalid"),
    }
}
