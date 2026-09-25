use crate::core::knowledge::Store;
use product_contract::Provenance;
use serde_json::Value;
use tauri::Manager;

pub async fn dispatch_typed(
    app: &tauri::AppHandle,
    component: &'static str,
    call: crate::ipc::knowledge::KnowledgeCall,
    provenance: Provenance,
    deadline: u64,
) -> Result<Value, String> {
    use crate::ipc::knowledge::KnowledgeCall;
    if let KnowledgeCall::SendKnowledgeDraft { id } = call {
        let draft = crate::federation::read(app, component, &id).await?;
        return crate::suite::remote(
            app,
            "knowledge",
            product_contract::transport::Call::DeliverKnowledgeDraft {
                component: component.into(),
                id,
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
    tauri::async_runtime::spawn_blocking(move || {
        let store = Store::open(&root, component)?;
        match call {
            KnowledgeCall::SaveKnowledgeDraft { output, source } => {
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
                serde_json::to_value(crate::ipc::knowledge::SavedKnowledgeDraft {
                    delivery: crate::ipc::knowledge::DraftDelivery::Stored,
                    draft,
                })
                .map_err(|_| "knowledge_storage_unavailable".into())
            }
            KnowledgeCall::ListKnowledgeDrafts {} => serde_json::to_value(store.list()?)
                .map_err(|_| "knowledge_storage_unavailable".into()),
            KnowledgeCall::GetKnowledgeDraft { id } => serde_json::to_value(store.get(&id)?)
                .map_err(|_| "knowledge_storage_unavailable".into()),
            KnowledgeCall::DeleteKnowledgeDraft { id } => {
                store.delete(&id)?;
                Ok(Value::Null)
            }
            KnowledgeCall::SendKnowledgeDraft { .. } => {
                unreachable!("remote delivery handled before local worker")
            }
        }
    })
    .await
    .map_err(|_| "knowledge_storage_unavailable".to_string())?
}
