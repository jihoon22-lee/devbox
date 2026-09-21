//! Verified API Studio -> explicit Suite review -> read-only Logs source.
use product_contract::{
    commands::{ContextRequirement, Descriptor, EntityKind, Request, Target},
    transport::Call,
    webhook_log::Artifact,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};
type Result<T> = std::result::Result<T, &'static str>;
struct Received {
    artifact: Artifact,
    revision: String,
    operation: String,
    acknowledged: bool,
}
fn received() -> &'static Mutex<BTreeMap<String, Received>> {
    static RECEIVED: OnceLock<Mutex<BTreeMap<String, Received>>> = OnceLock::new();
    RECEIVED.get_or_init(Mutex::default)
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub(crate) fn offer(
    app: &tauri::AppHandle,
    id: String,
    revision: String,
    operation: String,
) -> Result<Value> {
    let descriptor = Descriptor {
        id: format!("workspace.webhook-log-{id}"),
        owner: "workspace".into(),
        component: "workspace.logs".into(),
        label: "API Studio · Webhook 로그 확인".into(),
        revision: revision.clone(),
        target: Target::Entity {
            entity: EntityKind::Artifact,
            id,
        },
        review_route: Some("logs".into()),
        required_context: ContextRequirement::None,
        context: None,
        destructive: false,
        requires_review: true,
        disabled_reason: None,
    };
    crate::suite::enqueue_review(
        app,
        &descriptor,
        &Request {
            operation_id: operation,
            command_id: descriptor.id.clone(),
            revision,
            context: None,
            selection_id: None,
        },
    )
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    id: String,
    revision: String,
    operation_id: String,
}
pub(crate) async fn open(app: &tauri::AppHandle, args: Value, deadline: u64) -> Result<Value> {
    static READERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let _permit = READERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| "webhook_log_busy")?;
    let input: Input = serde_json::from_value(args).map_err(|_| "webhook_log_invalid")?;
    let target = Target::Entity {
        entity: EntityKind::Artifact,
        id: input.id.clone(),
    };
    crate::suite::require_reviewed(app, &input.operation_id, &input.revision, "logs", &target)?;
    let cached = {
        let mut entries = received().lock().map_err(|_| "webhook_log_busy")?;
        entries.retain(|_, entry| entry.artifact.expires_at_ms > now());
        if entries.len() >= 32 && !entries.contains_key(&input.id) {
            return Err("webhook_log_limit");
        }
        if let Some(entry) = entries.get(&input.id) {
            if entry.operation != input.operation_id || entry.revision != input.revision {
                return Err("webhook_log_stale");
            }
            Some((entry.artifact.clone(), entry.acknowledged))
        } else {
            None
        }
    };
    let (artifact, acknowledged) = if let Some(cached) = cached {
        cached
    } else {
        let value = crate::suite::remote(
            app,
            "api-studio",
            Call::ClaimWebhookLog {
                id: input.id.clone(),
                revision: input.revision.clone(),
                operation_id: input.operation_id.clone(),
            },
            deadline,
        )
        .await?;
        let artifact: Artifact =
            serde_json::from_value(value).map_err(|_| "webhook_log_invalid")?;
        artifact.require(&input.id, &input.revision, now())?;
        crate::suite::require_reviewed(app, &input.operation_id, &input.revision, "logs", &target)?;
        received().lock().map_err(|_| "webhook_log_busy")?.insert(
            input.id.clone(),
            Received {
                artifact: artifact.clone(),
                revision: input.revision.clone(),
                operation: input.operation_id.clone(),
                acknowledged: false,
            },
        );
        (artifact, false)
    };
    artifact.require(&input.id, &input.revision, now())?;
    if now() >= deadline {
        return Err("webhook_log_expired");
    }
    crate::suite::require_reviewed(app, &input.operation_id, &input.revision, "logs", &target)?;
    if !acknowledged {
        crate::suite::remote(
            app,
            "api-studio",
            Call::AcknowledgeWebhookLog {
                id: input.id.clone(),
                revision: input.revision.clone(),
                operation_id: input.operation_id.clone(),
            },
            deadline,
        )
        .await?;
        received()
            .lock()
            .map_err(|_| "webhook_log_busy")?
            .get_mut(&input.id)
            .ok_or("webhook_log_stale")?
            .acknowledged = true;
    }
    // Duplicate delivery of the same accepted operation returns the same source
    // identity; the Logs consumer deduplicates by the operation ID and payload.
    crate::suite::require_reviewed(app, &input.operation_id, &input.revision, "logs", &target)?;
    Ok(json!({"kind":"webhookCapture","capture":artifact.payload}))
}
