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
        match call {
            Call::ReadKnowledgeDraft { component, id } => {
                serde_json::to_value(read(&app, &component, &id).await?)
                    .map_err(|_| "draft_invalid")
            }
            _ => Err("api_source_unavailable"),
        }
    })
}
