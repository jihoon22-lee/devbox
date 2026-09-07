//! Typed bridge into the existing implementation. The product is responsible
//! for creating its own managed states after migration and enforcing native
//! caller/owner/session checks before dispatch. This module starts no legacy app.

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
        integration_root: Some(integration_root),
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
    crate::integration::spawn_snapshot_writer(state);
    crate::commands::tracking::spawn_poller(app);
    Ok(())
}

pub const COMMANDS: &[&str] = &[
    "get_digest",
    "cancel_digest",
    "save_digest",
    "send_digest_to_knowledge",
    "knowledge_draft_history",
    "export_life_log",
    "save_life_log",
    "set_projects",
    "get_projects",
    "probe_project",
    "get_day",
    "get_range",
    "start_tracking",
    "stop_tracking",
    "is_tracking",
    "set_idle_threshold",
    "get_idle_threshold",
    "get_privacy_rules",
    "set_privacy_rules",
    "redact_existing",
    "autostart_status",
    "set_autostart",
    "integration_sources",
    "project_attribution",
    "timeline",
    "app_stats",
];

pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "get_digest" => crate::commands::digest::__component_get_digest(app, args).await,
        "cancel_digest" => crate::commands::digest::__component_cancel_digest(app, args).await,
        "save_digest" => crate::commands::digest::__component_save_digest(app, args).await,
        "send_digest_to_knowledge" => {
            crate::commands::handoff::__component_send_digest_to_knowledge(app, args).await
        }
        "knowledge_draft_history" => {
            crate::commands::handoff::__component_knowledge_draft_history(app, args).await
        }
        "export_life_log" => crate::commands::export::__component_export_life_log(app, args).await,
        "save_life_log" => crate::commands::export::__component_save_life_log(app, args).await,
        "set_projects" => crate::commands::life::__component_set_projects(app, args).await,
        "get_projects" => crate::commands::life::__component_get_projects(app, args).await,
        "probe_project" => crate::commands::life::__component_probe_project(app, args).await,
        "get_day" => crate::commands::life::__component_get_day(app, args).await,
        "get_range" => crate::commands::life::__component_get_range(app, args).await,
        "start_tracking" => crate::commands::tracking::__component_start_tracking(app, args).await,
        "stop_tracking" => crate::commands::tracking::__component_stop_tracking(app, args).await,
        "is_tracking" => crate::commands::tracking::__component_is_tracking(app, args).await,
        "set_idle_threshold" => {
            crate::commands::tracking::__component_set_idle_threshold(app, args).await
        }
        "get_idle_threshold" => {
            crate::commands::tracking::__component_get_idle_threshold(app, args).await
        }
        "get_privacy_rules" => {
            crate::commands::privacy::__component_get_privacy_rules(app, args).await
        }
        "set_privacy_rules" => {
            crate::commands::privacy::__component_set_privacy_rules(app, args).await
        }
        "redact_existing" => crate::commands::privacy::__component_redact_existing(app, args).await,
        "autostart_status" => {
            crate::commands::autostart::__component_autostart_status(app, args).await
        }
        "set_autostart" => crate::commands::autostart::__component_set_autostart(app, args).await,
        "integration_sources" => {
            crate::commands::life::__component_integration_sources(app, args).await
        }
        "project_attribution" => {
            crate::commands::life::__component_project_attribution(app, args).await
        }
        "timeline" => crate::commands::queries::__component_timeline(app, args).await,
        "app_stats" => crate::commands::queries::__component_app_stats(app, args).await,
        _ => Err("component_method_unavailable".into()),
    }
}
