use crate::core::knowledge::Store;
use product_contract::Provenance;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::Manager;

pub const COMMANDS: &[&str] = &[
    "save_knowledge_draft",
    "send_knowledge_draft",
    "list_knowledge_drafts",
    "get_knowledge_draft",
    "delete_knowledge_draft",
];
pub fn is_command(component: &str, method: &str) -> bool {
    matches!(component, "api-studio.api" | "api-studio.transforms") && COMMANDS.contains(&method)
}
pub async fn dispatch(
    app: &tauri::AppHandle,
    component: &str,
    method: &str,
    args: Value,
    provenance: Provenance,
    deadline: u64,
) -> Result<Value, String> {
    if method == "send_knowledge_draft" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            id: String,
        }
        let input: Input = serde_json::from_value(args).map_err(|_| "knowledge_draft_invalid")?;
        let draft = crate::federation::read(app, component, &input.id).await?;
        return crate::suite::remote(
            app,
            "knowledge",
            product_contract::transport::Call::DeliverKnowledgeDraft {
                component: component.into(),
                id: input.id,
                revision: draft.revision()?,
                operation_id: provenance.request_id,
            },
            deadline,
        )
        .await
        .map_err(str::to_owned);
    }
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "knowledge_storage_unavailable")?;
    let component = component.to_string();
    let method = method.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open(&root, &component)?;
        match method.as_str() {
            "save_knowledge_draft" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    output: String,
                    source: Option<transforms_core::core::export_policy::OutputSource>,
                }
                let Input { output, source } =
                    serde_json::from_value(args).map_err(|_| "knowledge_draft_invalid")?;
                let output = zeroize::Zeroizing::new(output);
                if component == "api-studio.transforms" {
                    source
                        .ok_or("transform_export_denied")?
                        .require_exportable()?;
                } else if source.is_some() {
                    return Err("knowledge_owner_invalid".into());
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|v| u64::try_from(v.as_millis()).ok())
                    .ok_or("knowledge_storage_unavailable")?;
                let draft = store.save(&output, provenance, now)?;
                // Source persistence and explicit receiver delivery are separate actions.
                Ok(json!({ "delivery": "stored", "draft": draft }))
            }
            "list_knowledge_drafts" if args.as_object().is_some_and(|v| v.is_empty()) => {
                serde_json::to_value(store.list()?)
                    .map_err(|_| "knowledge_storage_unavailable".into())
            }
            "get_knowledge_draft" | "delete_knowledge_draft" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    id: String,
                }
                let Input { id } =
                    serde_json::from_value(args).map_err(|_| "knowledge_draft_invalid")?;
                if method == "get_knowledge_draft" {
                    serde_json::to_value(store.get(&id)?)
                        .map_err(|_| "knowledge_storage_unavailable".into())
                } else {
                    store.delete(&id)?;
                    Ok(Value::Null)
                }
            }
            _ => Err("knowledge_draft_invalid".into()),
        }
    })
    .await
    .map_err(|_| "knowledge_storage_unavailable".to_string())?
}
