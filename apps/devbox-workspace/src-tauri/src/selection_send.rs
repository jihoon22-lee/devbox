//! Explicit editor selections stay source-owned until the receiver rechecks them.
use product_contract::{
    transform_selection::{selected_utf16, Selection},
    transport::Call,
    ProjectContext,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Editor {
    pub path: String,
    pub native_revision: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    path: String,
    native_revision: String,
    text: String,
    from: usize,
    to: usize,
}
#[derive(Clone)]
struct Entry {
    source: Source,
    context: Option<ProjectContext>,
    hash: [u8; 32],
    value: Selection,
    created: Instant,
}
#[derive(Clone)]
enum Source {
    Editor(Editor),
    Logs(crate::selection_logs::Proof),
}
fn entries() -> &'static Mutex<HashMap<String, Entry>> {
    static OWNER: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    OWNER.get_or_init(Mutex::default)
}
pub(crate) async fn send(app: &tauri::AppHandle, args: Value, deadline: u64) -> Result<Value> {
    let input: Input = serde_json::from_value(args).map_err(|_| "selection_invalid")?;
    if input.text.len() > 32 * 1024 * 1024 {
        return Err("selection_limit");
    }
    let editor = Editor {
        path: input.path,
        native_revision: input.native_revision,
    };
    let (context, hash) =
        crate::component::editor_selection_proof(app, editor.clone(), deadline).await?;
    if hash != <[u8; 32]>::from(Sha256::digest(input.text.as_bytes())) {
        return Err("selection_stale");
    }
    let value = Selection::prepare(
        "code-pad",
        selected_utf16(&input.text, input.from, input.to)?,
    )?;
    let revision = value.revision()?;
    let redacted = value.redacted;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut entries = entries().lock().map_err(|_| "selection_busy")?;
        entries.retain(|_, entry| entry.created.elapsed() < Duration::from_secs(120));
        if entries.len() >= 16 {
            return Err("selection_limit");
        }
        entries.insert(
            id.clone(),
            Entry {
                source: Source::Editor(editor),
                context,
                hash,
                value,
                created: Instant::now(),
            },
        );
    }
    let receipt = crate::suite::remote(
        app,
        "api-studio",
        Call::DeliverTransformSelection {
            id: id.clone(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            revision,
        },
        deadline,
    )
    .await?;
    Ok(json!({"handoffId":id,"redacted":redacted,"receipt":receipt}))
}
pub(crate) async fn read(app: &tauri::AppHandle, id: &str, deadline: u64) -> Result<Value> {
    let entry = entries()
        .lock()
        .map_err(|_| "selection_busy")?
        .get(id)
        .filter(|entry| entry.created.elapsed() < Duration::from_secs(120))
        .cloned()
        .ok_or("selection_expired")?;
    match entry.source {
        Source::Editor(editor) => {
            let (context, hash) =
                crate::component::editor_selection_proof(app, editor, deadline).await?;
            if context != entry.context || hash != entry.hash {
                return Err("selection_stale");
            }
        }
        Source::Logs(proof) => {
            crate::component::validate_log_selection(app, entry.context.as_ref(), &proof, deadline)
                .await?;
        }
    }
    serde_json::to_value(entry.value).map_err(|_| "selection_invalid")
}

pub(crate) async fn send_logs(
    app: &tauri::AppHandle,
    args: Value,
    context: Option<&ProjectContext>,
    deadline: u64,
) -> Result<Value> {
    let (proof, value) = crate::selection_logs::prepare(args, context)?;
    crate::selection_logs::revalidate(app, &proof, deadline).await?;
    let revision = value.revision()?;
    let redacted = value.redacted;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut entries = entries().lock().map_err(|_| "selection_busy")?;
        entries.retain(|_, entry| entry.created.elapsed() < Duration::from_secs(120));
        if entries.len() >= 16 {
            return Err("selection_limit");
        }
        entries.insert(
            id.clone(),
            Entry {
                source: Source::Logs(proof),
                context: context.cloned(),
                hash: [0; 32],
                value,
                created: Instant::now(),
            },
        );
    }
    crate::suite::remote(
        app,
        "api-studio",
        Call::DeliverTransformSelection {
            id: id.clone(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            revision,
        },
        deadline,
    )
    .await?;
    Ok(json!({"handoffId":id,"redacted":redacted}))
}
