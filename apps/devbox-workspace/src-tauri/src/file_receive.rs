//! Exact Knowledge file references require a destination review before native
//! file grants. The existing Files owner and dirty editor flow perform the open.
use product_contract::{
    commands::{ContextRequirement, Descriptor, EntityKind, Request, Target},
    file_reference::Proof,
    transport::Call,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
struct Pending {
    operation: String,
    revision: String,
    created: Instant,
}
fn pending() -> &'static Mutex<HashMap<String, Pending>> {
    static OWNER: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    OWNER.get_or_init(Mutex::default)
}
pub(crate) fn offer(
    app: &tauri::AppHandle,
    reference: &str,
    operation_id: &str,
    revision: &str,
) -> Result<Value, &'static str> {
    if uuid::Uuid::parse_str(reference).is_err() || uuid::Uuid::parse_str(operation_id).is_err() {
        return Err("file_reference_invalid");
    }
    let mut pending = pending().lock().map_err(|_| "file_reference_busy")?;
    pending.retain(|_, entry| entry.created.elapsed() < Duration::from_secs(120));
    if pending.len() >= 32 && !pending.contains_key(reference) {
        return Err("file_reference_limit");
    }
    let descriptor = Descriptor {
        id: format!("workspace.received-file-{reference}"),
        owner: "workspace".into(),
        component: "workspace.files".into(),
        label: "Knowledge에서 선택한 파일".into(),
        revision: revision.into(),
        target: Target::Entity {
            entity: EntityKind::File,
            id: reference.into(),
        },
        review_route: Some("files".into()),
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
        reference.into(),
        Pending {
            operation: operation_id.into(),
            revision: revision.into(),
            created: Instant::now(),
        },
    );
    Ok(receipt)
}
pub(crate) async fn open(
    app: &tauri::AppHandle,
    reference: &str,
    deadline: u64,
) -> Result<Value, &'static str> {
    let (operation, revision) = {
        let pending = pending().lock().map_err(|_| "file_reference_busy")?;
        let entry = pending
            .get(reference)
            .filter(|entry| entry.created.elapsed() < Duration::from_secs(120))
            .ok_or("file_reference_stale")?;
        (entry.operation.clone(), entry.revision.clone())
    };
    let target = Target::Entity {
        entity: EntityKind::File,
        id: reference.into(),
    };
    crate::suite::require_reviewed(app, &operation, &revision, "files", &target)?;
    let value = crate::suite::remote(
        app,
        "knowledge",
        Call::ReadFileReference {
            reference: reference.into(),
        },
        deadline,
    )
    .await?;
    let proof: Proof = serde_json::from_value(value).map_err(|_| "file_reference_invalid")?;
    if proof.reference != reference || proof.revision()? != revision {
        return Err("file_reference_stale");
    }
    crate::suite::require_reviewed(app, &operation, &revision, "files", &target)?;
    crate::component::approve_received_file(app, proof, deadline).await
}
