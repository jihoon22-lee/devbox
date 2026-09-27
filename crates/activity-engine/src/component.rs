//! Typed bridge into the existing implementation. The product is responsible
//! for creating its own managed states after migration and enforcing native
//! caller/owner/session checks before dispatch. This module starts no legacy app.

struct BackgroundTasks(std::sync::Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>);

/// Initialize only this product's database and snapshot namespace. In particular,
/// this does not invoke legacy identifier migration or absorb activity-timeline.
pub fn initialize(
    app: &tauri::AppHandle,
    data_root: &std::path::Path,
    integration_root: std::path::PathBuf,
) -> Result<(), String> {
    use crate::commands::tracking::{AppState, DigestOperationState};
    use std::sync::{atomic::AtomicBool, Arc, Mutex};
    use tauri::Manager;
    if app.try_state::<Arc<AppState>>().is_some() {
        return Err("component_state_conflict".into());
    }
    std::fs::create_dir_all(data_root).map_err(|_| "component_storage_unavailable")?;
    let conn = crate::core::db::init(&data_root.join("data.db"))
        .map_err(|_| "component_storage_unavailable")?;
    let consent = crate::commands::tracking::product_consent(&conn);
    let state = Arc::new(AppState {
        shutdown: tokio::sync::watch::channel(false).0,
        integration_root: Some(integration_root),
        privacy: crate::commands::privacy::PrivacyState::load(&conn),
        db: Mutex::new(conn),
        sessionizer: Mutex::new(crate::core::sessionizer::Sessionizer::new()),
        tracking: AtomicBool::new(consent),
        tracking_control: Mutex::new(()),
        persist_tracking_consent: true,
        snapshot_writer: Mutex::new(()),
        digest_operations: Arc::new(DigestOperationState::default()),
        digest_handles: crate::core::digest::DigestHandleStore::default(),
    });
    if !app.manage(state.clone()) {
        return Err("component_state_conflict".into());
    }
    let snapshot = crate::integration::spawn_snapshot_writer(state);
    let poller = crate::commands::tracking::spawn_poller(app);
    app.manage(BackgroundTasks(std::sync::Mutex::new(vec![
        snapshot, poller,
    ])));
    Ok(())
}

/// End the in-memory session on process exit without revoking saved consent.
pub fn shutdown(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(state) = app.try_state::<std::sync::Arc<crate::commands::tracking::AppState>>() {
        state.shutdown.send_replace(true);
        state.digest_operations.cancel_generation();
        crate::commands::tracking::stop_tracking_runtime(&state, false)?;
        drop(
            state
                .snapshot_writer
                .lock()
                .map_err(|_| "activity_stopping")?,
        );
    }
    Ok(())
}

pub async fn shutdown_agent(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let result = shutdown(app);
    let tasks = if let Some(tasks) = app.try_state::<BackgroundTasks>() {
        std::mem::take(&mut *tasks.0.lock().map_err(|_| "activity_stopping")?)
    } else {
        Vec::new()
    };
    for task in tasks {
        task.await.map_err(|_| "activity_stopping")?;
    }
    result
}

/// Create only a new product-owned database; never initialize a legacy source.
pub fn create_empty_store(path: &std::path::Path) -> Result<(), String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "component_store_exists")?;
    crate::core::db::init(path).map_err(|_| "component_storage_unavailable")?;
    Ok(())
}

/// Use the same bounded metadata validator as the actual history reader.
pub fn validate_import_history(connection: &rusqlite::Connection) -> Result<(), String> {
    crate::core::draft_history::list(connection)
        .map(|_| ())
        .map_err(|_| "import_row_invalid".into())
}

/// Product-owned delivery preserves the producer's validation, cancellation and
/// one-time history without requiring a standalone Knowledge installation.
pub async fn send_product_draft_typed<F>(
    app: &tauri::AppHandle,
    input: crate::core::digest::DigestInput,
    regenerated_from: Option<String>,
    deliver: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(&devbox_applink::OpenRequest) -> Result<(), String> + Send,
{
    use tauri::Manager;
    let result = crate::commands::handoff::send_with_delivery(
        app.state(),
        input,
        regenerated_from,
        false,
        false,
        deliver,
    )
    .await?;
    serde_json::to_value(result).map_err(|_| "component_response_invalid".into())
}

/// Native-only protocol between the Activity producer and its Knowledge UI.
#[derive(serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DraftDelivery {
    Prepare {
        input: crate::core::digest::DigestInput,
        regenerated_from: Option<String>,
    },
    Finish {
        id: String,
        delivered: bool,
    },
}
pub async fn draft_delivery(
    app: &tauri::AppHandle,
    call: DraftDelivery,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    match call {
        DraftDelivery::Prepare {
            input,
            regenerated_from,
        } => {
            crate::commands::handoff::prepare_product_draft(app.state(), input, regenerated_from)
                .await
        }
        DraftDelivery::Finish { id, delivered } => {
            let state = app.state::<std::sync::Arc<crate::commands::tracking::AppState>>();
            crate::commands::handoff::finish_product_draft(&state, &id, delivered)?;
            Ok(serde_json::json!({"acknowledged":true}))
        }
    }
}

/// Cached native status for the agent tray; this never starts collection.
pub fn tracking_status(app: &tauri::AppHandle) -> Option<bool> {
    use tauri::Manager;
    app.try_state::<std::sync::Arc<crate::commands::tracking::AppState>>()
        .map(|state| state.tracking.load(std::sync::atomic::Ordering::Acquire))
}
