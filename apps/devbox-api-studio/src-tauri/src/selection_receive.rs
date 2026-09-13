//! Workspace may offer a selection; only an exact local review may resolve it.
use product_contract::{
    commands::{ContextRequirement, Descriptor, EntityKind, Request, Target},
    transform_selection::Selection,
    transport::Call,
};
use serde::Deserialize;
use serde_json::{json, Value};
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) fn offer(
    app: &tauri::AppHandle,
    id: &str,
    operation: &str,
    revision: &str,
) -> Result<Value> {
    if uuid::Uuid::parse_str(id).is_err() || uuid::Uuid::parse_str(operation).is_err() {
        return Err("selection_invalid");
    }
    let descriptor = Descriptor {
        id: format!("api-studio.selection-{id}"),
        owner: "api-studio".into(),
        component: "api-studio.transforms".into(),
        label: "Workspace 선택 내용 검토".into(),
        revision: revision.into(),
        target: Target::Entity {
            entity: EntityKind::Artifact,
            id: id.into(),
        },
        review_route: Some("transforms".into()),
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
            operation_id: operation.into(),
            command_id: descriptor.id.clone(),
            revision: revision.into(),
            context: None,
            selection_id: None,
        },
    )
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
) -> std::result::Result<Value, String> {
    static READERS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::OnceLock::new();
    let permit = READERS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| "selection_busy")?;
    let input: Input = serde_json::from_value(args).map_err(|_| "selection_invalid")?;
    let target = Target::Entity {
        entity: EntityKind::Artifact,
        id: input.id.clone(),
    };
    crate::suite::require_reviewed(
        app,
        &input.operation_id,
        &input.revision,
        "transforms",
        &target,
    )?;
    let value = crate::suite::remote(
        app,
        "workspace",
        Call::ReadTransformSelection {
            id: input.id.clone(),
        },
        deadline,
    )
    .await?;
    let selection: Selection = serde_json::from_value(value).map_err(|_| "selection_invalid")?;
    if selection.revision()? != input.revision {
        return Err("selection_stale".into());
    }
    crate::suite::require_reviewed(
        app,
        &input.operation_id,
        &input.revision,
        "transforms",
        &target,
    )?;
    // Existing publisher is synchronous and bounded; its one-time store retains
    // claim/cancel/ack semantics and never applies the text itself.
    let app = app.clone();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "selection_expired")?
            .as_millis() as u64;
        if now >= deadline {
            return Err("selection_expired");
        }
        crate::suite::require_reviewed(
            &app,
            &input.operation_id,
            &input.revision,
            "transforms",
            &target,
        )?;
        let state =
            crate::handoff::receive_selection(&app, &input.id, &input.revision, selection, now)?;
        Ok(json!({"state":state}))
    })
    .await
    .map_err(|_| "selection_unavailable")?
    .map_err(str::to_owned)
}
