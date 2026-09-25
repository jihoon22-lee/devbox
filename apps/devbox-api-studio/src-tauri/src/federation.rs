//! Native read of an explicitly saved result, only for authenticated Knowledge.
use product_contract::{query::Cancellation, transport::Call};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use tauri::Manager;
pub(crate) async fn read(
    app: &tauri::AppHandle,
    component: &str,
    id: &str,
) -> Result<product_contract::knowledge_draft::Draft, &'static str> {
    static READERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let permit = READERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| "draft_busy")?;
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "draft_unavailable")?;
    let component = component.to_owned();
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        crate::core::knowledge::Store::open(&root, &component)
            .and_then(|store| store.get(&id))
            .map_err(|_| "draft_stale")
    })
    .await
    .map_err(|_| "draft_unavailable")?
}
pub(crate) fn handle(
    app: tauri::AppHandle,
    call: Call,
    _deadline: u64,
    _cancel: Option<Cancellation>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, &'static str>> + Send>> {
    Box::pin(async move {
        if matches!(&call, Call::ReadMigrationStatus {}) {
            static READERS: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> =
                std::sync::OnceLock::new();
            let permit = READERS
                .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(1)))
                .clone()
                .try_acquire_owned()
                .map_err(|_| "migration_busy")?;
            return tokio::task::spawn_blocking(move || {
                let _permit = permit;
                status_summary()
            })
            .await
            .map_err(|_| "migration_unavailable")?;
        }

        if matches!(
            &call,
            Call::ReadOperations {} | Call::ReviewOperation { .. }
        ) {
            return crate::suite::project_operations(&app, &call, {
                let mut rows = Vec::new();
                rows.extend(crate::lifecycle::operation_rows(&app)?);
                rows
            });
        }
        match call {
            call @ (Call::ClaimWebhookLog { .. } | Call::AcknowledgeWebhookLog { .. }) => {
                crate::webhook_logs::handle(call)
            }
            Call::DeliverTransformSelection {
                id,
                operation_id,
                revision,
            } => crate::selection_receive::offer(&app, &id, &operation_id, &revision),
            Call::ReadKnowledgeDraft { component, id } => {
                serde_json::to_value(read(&app, &component, &id).await?)
                    .map_err(|_| "draft_invalid")
            }
            _ => Err("api_source_unavailable"),
        }
    })
}

pub(crate) fn status_summary() -> Result<Value, &'static str> {
    let summary = product_contract::migration_status::Summary::new(
        "api-studio",
        env!("CARGO_PKG_VERSION"),
        false,
        true,
        false,
        b"api-studio-store-ready",
    )?;
    serde_json::to_value(summary).map_err(|_| "migration_unavailable")
}
#[cfg(test)]
mod store_readiness_tests {
    #[test]
    fn api_studio_reports_a_ready_store_without_imports() {
        let value = super::status_summary().unwrap();
        assert_eq!(value["setupSelected"], true);
        assert_eq!(value["reviewRequired"], false);
        assert_eq!(value["busy"], false);
        assert!(value.get("mappings").is_none_or(serde_json::Value::is_null));
    }
}
