//! Product-owned adapters around the existing native implementation.
use std::path::PathBuf;
use tauri::Manager;

struct ComponentRoot(PathBuf);

pub(crate) fn data_root(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    if let Some(root) = app.try_state::<ComponentRoot>() {
        return Ok(root.0.clone());
    }
    app.path().app_local_data_dir()
}

pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "component_storage_unavailable")?
        .join("webhooks");
    if !app.manage(ComponentRoot(root)) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(crate::commands::server_state()) {
        return Err("component_state_conflict".into());
    }
    Ok(())
}

pub const COMMANDS: &[&str] = &[
    "server_status",
    "start_server",
    "stop_server",
    "list_history",
    "clear_history",
    "copy_masked_history",
    "copy_raw_history",
    "copy_history_headers",
    "delete_history",
    "replay_history",
    "list_fixtures",
    "save_fixture",
    "delete_fixture",
    "clear_fixtures",
    "fixture_to_rule",
    "replay_fixture",
    "list_rules",
    "preview_rule_conflicts",
    "set_rule",
    "delete_rule",
    "reset_rule_sequence",
];

/// No dynamic command loading or legacy application startup occurs here.
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "server_status" => crate::commands::__component_server_status(app, args).await,
        "start_server" => crate::commands::__component_start_server(app, args).await,
        "stop_server" => crate::commands::__component_stop_server(app, args).await,
        "list_history" => crate::commands::__component_list_history(app, args).await,
        "clear_history" => crate::commands::__component_clear_history(app, args).await,
        "copy_masked_history" => crate::commands::__component_copy_masked_history(app, args).await,
        "copy_raw_history" => crate::commands::__component_copy_raw_history(app, args).await,
        "copy_history_headers" => {
            crate::commands::__component_copy_history_headers(app, args).await
        }
        "delete_history" => crate::commands::__component_delete_history(app, args).await,
        "replay_history" => crate::commands::__component_replay_history(app, args).await,
        "list_fixtures" => crate::commands::__component_list_fixtures(app, args).await,
        "save_fixture" => crate::commands::__component_save_fixture(app, args).await,
        "delete_fixture" => crate::commands::__component_delete_fixture(app, args).await,
        "clear_fixtures" => crate::commands::__component_clear_fixtures(app, args).await,
        "fixture_to_rule" => crate::commands::__component_fixture_to_rule(app, args).await,
        "replay_fixture" => crate::commands::__component_replay_fixture(app, args).await,
        "list_rules" => crate::commands::__component_list_rules(app, args).await,
        "preview_rule_conflicts" => {
            crate::commands::__component_preview_rule_conflicts(app, args).await
        }
        "set_rule" => crate::commands::__component_set_rule(app, args).await,
        "delete_rule" => crate::commands::__component_delete_rule(app, args).await,
        "reset_rule_sequence" => crate::commands::__component_reset_rule_sequence(app, args).await,
        _ => Err("component_method_unavailable".into()),
    }
}
