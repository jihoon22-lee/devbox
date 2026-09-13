//! Session metadata enters the existing Notes claim/preview/cancel/save path only
//! after native Workspace authentication and an exact destination route review.
use devbox_applink::{CreateHandoff, HandoffDescriptor, HandoffStatus, HandoffStore, OpenRequest};
use product_contract::{
    commands::{ContextRequirement, Descriptor, EntityKind, Request, Target},
    session_summary::{self, Draft, Metadata},
    transport::Call,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
struct Owner {
    lock: Mutex<()>,
    slots: std::sync::Arc<tokio::sync::Semaphore>,
}
impl Default for Owner {
    fn default() -> Self {
        Self {
            lock: Mutex::new(()),
            slots: std::sync::Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    revision: String,
    descriptor: HandoffDescriptor,
    expires: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Ledger {
    schema: u32,
    entries: BTreeMap<String, Entry>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    source_id: String,
    operation_id: String,
    revision: String,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
fn revision(metadata: &Metadata) -> Result<String> {
    Ok(
        Sha256::digest(serde_json::to_vec(metadata).map_err(|_| "summary_invalid")?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
pub(crate) fn offer(
    app: &tauri::AppHandle,
    source_id: &str,
    operation_id: &str,
    revision: &str,
) -> Result<Value> {
    if uuid::Uuid::parse_str(source_id).is_err() || uuid::Uuid::parse_str(operation_id).is_err() {
        return Err("summary_invalid");
    }
    crate::startup::require_active(app).map_err(|_| "summary_unavailable")?;
    let descriptor = Descriptor {
        id: format!("knowledge.session-summary-{source_id}"),
        owner: "knowledge".into(),
        component: "knowledge.notes".into(),
        label: "개발 세션 요약 검토".into(),
        revision: revision.into(),
        target: Target::Entity {
            entity: EntityKind::Artifact,
            id: source_id.into(),
        },
        review_route: Some("daily".into()),
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
    crate::suite::enqueue_review(app, &descriptor, &request)
}
fn read_ledger(path: &Path) -> Result<Ledger> {
    if !path.try_exists().map_err(|_| "summary_store_unavailable")? {
        return Ok(Ledger {
            schema: 1,
            entries: BTreeMap::new(),
        });
    }
    devbox_filesystem::ensure_no_links(path).map_err(|_| "summary_store_invalid")?;
    let (file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "summary_store_unavailable")?;
    let mut bytes = Vec::new();
    file.take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "summary_store_unavailable")?;
    if bytes.len() > 256 * 1024
        || devbox_filesystem::filesystem_identity(path, false).ok() != Some(identity)
    {
        return Err("summary_store_invalid");
    }
    let ledger: Ledger = serde_json::from_slice(&bytes).map_err(|_| "summary_store_invalid")?;
    if ledger.schema != 1
        || ledger.entries.len() > 256
        || ledger.entries.iter().any(|(id, entry)| {
            uuid::Uuid::parse_str(id).is_err()
                || !product_contract::commands::revision(&entry.revision)
                || entry.descriptor.kind != session_summary::KIND
                || entry.descriptor.id.len() != 32
                || !entry
                    .descriptor
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    {
        return Err("summary_store_invalid");
    }
    Ok(ledger)
}
fn prepare(
    app: &tauri::AppHandle,
    source_id: &str,
    revision: &str,
    draft: &Draft,
    open: bool,
) -> Result<Value> {
    let owner = app.state::<Owner>();
    let _guard = owner.lock.lock().map_err(|_| "summary_busy")?;
    let root = crate::startup::integration_root(app).map_err(|_| "summary_unavailable")?;
    #[cfg(windows)]
    let _parent = crate::suite::platform::component_scope::pin_directories(
        root.parent().ok_or("summary_store_invalid")?,
    )?;
    if !root.try_exists().map_err(|_| "summary_store_unavailable")? {
        std::fs::create_dir(&root).map_err(|_| "summary_store_unavailable")?;
    }
    devbox_filesystem::ensure_no_links(&root).map_err(|_| "summary_store_invalid")?;
    #[cfg(windows)]
    let _pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let path = root.join("workspace-summary-receipts-v1.json");
    let mut ledger = read_ledger(&path)?;
    let store = HandoffStore::new(devbox_applink::handoff_root_in(&root));
    if !ledger.entries.contains_key(source_id) {
        if ledger.entries.len() == 256 {
            return Err("summary_limit");
        }
        let timestamp = now();
        let descriptor = store
            .create(
                CreateHandoff {
                    kind: session_summary::KIND.into(),
                    source_app: session_summary::PRODUCER.into(),
                    target_app: Some("knowledge-base".into()),
                    payload: serde_json::to_value(draft).map_err(|_| "summary_invalid")?,
                },
                timestamp,
            )
            .map_err(|_| "summary_store_unavailable")?;
        ledger.entries.insert(
            source_id.into(),
            Entry {
                revision: revision.into(),
                descriptor,
                expires: timestamp.saturating_add(devbox_applink::DEFAULT_HANDOFF_TTL_MS),
            },
        );
        // Persist identity before offering the envelope. A crash before this write
        // can leave only an unoffered expiring orphan, never a second saved note.
        devbox_filesystem::atomic_write(
            &path,
            &serde_json::to_vec(&ledger).map_err(|_| "summary_invalid")?,
        )
        .map_err(|_| "summary_store_unavailable")?;
    }
    let entry = &ledger.entries[source_id];
    if entry.revision != revision {
        return Err("summary_stale");
    }
    if store
        .read_status(&entry.descriptor.id)
        .map_err(|_| "summary_store_unavailable")?
        .is_some_and(|status| status.status == HandoffStatus::Consumed)
    {
        return Ok(json!({"state":"saved","draft":draft}));
    }
    if entry.expires <= now() {
        return Err("summary_expired");
    }
    if open {
        knowledge_base_lib::component::offer_product_draft(
            app,
            &OpenRequest {
                target: entry.descriptor.clone().into(),
                from: Some(session_summary::PRODUCER.into()),
            },
        )
        .map_err(|_| "summary_preview_busy")?;
    }
    Ok(json!({"state":if open {"previewPending"} else {"prepared"},"draft":draft}))
}
pub(crate) async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: Value,
    deadline: u64,
) -> std::result::Result<Value, String> {
    let permit = app
        .state::<Owner>()
        .slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| "summary_busy")?;
    let input: Input = serde_json::from_value(args).map_err(|_| "summary_invalid")?;
    crate::suite::require_reviewed(
        app,
        &input.operation_id,
        &input.revision,
        "daily",
        &Target::Entity {
            entity: EntityKind::Artifact,
            id: input.source_id.clone(),
        },
    )?;
    let value = crate::suite::remote(
        app,
        "workspace",
        Call::ReadSessionSummary {
            source_id: input.source_id.clone(),
        },
        deadline,
    )
    .await?;
    let metadata: Metadata = serde_json::from_value(value).map_err(|_| "summary_invalid")?;
    if revision(&metadata)? != input.revision {
        return Err("summary_stale".into());
    }
    let draft = session_summary::prepare(
        &serde_json::to_vec(&metadata).map_err(|_| "summary_invalid")?,
        &metadata.binding,
    )?;
    let app = app.clone();
    let open = method == "open_session_summary";
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if now() >= deadline {
            return Err("summary_expired");
        }
        crate::suite::require_reviewed(
            &app,
            &input.operation_id,
            &input.revision,
            "daily",
            &Target::Entity {
                entity: EntityKind::Artifact,
                id: input.source_id.clone(),
            },
        )?;
        prepare(&app, &input.source_id, &input.revision, &draft, open)
    })
    .await
    .map_err(|_| "summary_unavailable")?
    .map_err(str::to_owned)
}
pub(crate) fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("knowledge-session-receiver")
        .setup(|app, _| {
            app.manage(Owner::default());
            Ok(())
        })
        .build()
}
