//! Saved result -> existing Notes claim/preview flow, with source-owned revision.
use product_contract::{
    commands::{ContextRequirement, Descriptor, EntityKind, Request, Target},
    knowledge_draft::{self, Draft},
    transport::Call,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct Pending {
    component: String,
    operation: String,
    revision: String,
    created: Instant,
}
fn pending() -> &'static Mutex<HashMap<String, Pending>> {
    static PENDING: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    PENDING.get_or_init(Mutex::default)
}
pub(crate) fn offer(
    app: &tauri::AppHandle,
    component: &str,
    id: &str,
    revision: &str,
    operation_id: &str,
) -> Result<Value, &'static str> {
    if !knowledge_draft::COMPONENTS.contains(&component)
        || uuid::Uuid::parse_str(id).is_err()
        || uuid::Uuid::parse_str(operation_id).is_err()
    {
        return Err("draft_invalid");
    }
    crate::startup::require_active(app).map_err(|_| "draft_unavailable")?;
    let mut pending = pending().lock().map_err(|_| "draft_busy")?;
    pending.retain(|_, entry| entry.created.elapsed() < Duration::from_secs(120));
    if pending.len() >= 32 && !pending.contains_key(id) {
        return Err("draft_limit");
    }
    let descriptor = Descriptor {
        id: format!("knowledge.result-{id}"),
        owner: "knowledge".into(),
        component: "knowledge.notes".into(),
        label: format!(
            "API Studio · {} 결과 초안",
            if component == "api-studio.api" {
                "Requests"
            } else {
                "Transforms"
            }
        ),
        revision: revision.into(),
        target: Target::Entity {
            entity: EntityKind::Artifact,
            id: id.into(),
        },
        review_route: Some("notes".into()),
        required_context: ContextRequirement::None,
        context: None,
        destructive: false,
        requires_review: true,
        disabled_reason: None,
    };
    let request = Request {
        operation_id: operation_id.into(),
        command_id: descriptor.id.clone(),
        revision: revision.into(),
        context: None,
        selection_id: None,
    };
    let receipt = crate::suite::enqueue_review(app, &descriptor, &request)?;
    pending.insert(
        id.into(),
        Pending {
            component: component.into(),
            operation: operation_id.into(),
            revision: revision.into(),
            created: Instant::now(),
        },
    );
    Ok(receipt)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    id: String,
    operation_id: String,
    revision: String,
}
pub(crate) async fn open(
    app: &tauri::AppHandle,
    args: Value,
    deadline: u64,
) -> Result<Value, String> {
    let permit = crate::session_receive::reserve(app)?;
    let input: Input = serde_json::from_value(args).map_err(|_| "draft_invalid")?;
    let component = {
        let pending = pending().lock().map_err(|_| "draft_busy")?;
        pending
            .get(&input.id)
            .filter(|entry| {
                entry.operation == input.operation_id
                    && entry.revision == input.revision
                    && entry.created.elapsed() < Duration::from_secs(120)
            })
            .map(|entry| entry.component.clone())
            .ok_or("draft_stale")?
    };
    let target = Target::Entity {
        entity: EntityKind::Artifact,
        id: input.id.clone(),
    };
    crate::suite::require_reviewed(app, &input.operation_id, &input.revision, "notes", &target)?;
    let value = crate::suite::remote(
        app,
        "api-studio",
        Call::ReadKnowledgeDraft {
            component: component.clone(),
            id: input.id.clone(),
        },
        deadline,
    )
    .await?;
    let draft: Draft = serde_json::from_value(value).map_err(|_| "draft_invalid")?;
    draft.validate_for(&component)?;
    if draft.artifact.id != input.id || draft.revision()? != input.revision {
        return Err("draft_stale".into());
    }
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "draft_expired")?
            .as_millis()
            >= u128::from(deadline)
        {
            return Err("draft_expired");
        }
        crate::suite::require_reviewed(
            &app,
            &input.operation_id,
            &input.revision,
            "notes",
            &target,
        )?;
        let value = crate::session_receive::publish(
            &app,
            knowledge_draft::KIND,
            knowledge_draft::PRODUCER,
            &format!("{component}:{}", input.id),
            &input.revision,
            &serde_json::to_value(draft).map_err(|_| "draft_invalid")?,
            true,
        )?;
        Ok(json!({"state":value["state"]}))
    })
    .await
    .map_err(|_| "draft_unavailable")?
    .map_err(str::to_owned)
}
