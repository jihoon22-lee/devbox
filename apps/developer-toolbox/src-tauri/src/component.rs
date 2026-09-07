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
        .join("transforms");
    if !app.manage(ComponentRoot(root)) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(crate::applink::PendingOpen::new()) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(crate::commands::text_handoff::PendingToolboxText::new()) {
        return Err("component_state_conflict".into());
    }
    Ok(())
}

pub const COMMANDS: &[&str] = &[
    "take_pending_open",
    "hash",
    "hmac_generate",
    "hmac_verify",
    "generate_uuid",
    "generate_ids",
    "regex_test",
    "diff",
    "jwt_verify",
    "generate_qr",
    "load_workflow_metadata",
    "save_workflow_metadata",
];

/// No dynamic command loading or legacy application startup occurs here.
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "take_pending_open" => crate::applink::__component_take_pending_open(app, args).await,
        "hash" => crate::commands::tools::__component_hash(app, args).await,
        "hmac_generate" => crate::commands::tools::__component_hmac_generate(app, args).await,
        "hmac_verify" => crate::commands::tools::__component_hmac_verify(app, args).await,
        "generate_uuid" => crate::commands::tools::__component_generate_uuid(app, args).await,
        "generate_ids" => crate::commands::tools::__component_generate_ids(app, args).await,
        "regex_test" => crate::commands::tools::__component_regex_test(app, args).await,
        "diff" => crate::commands::tools::__component_diff(app, args).await,
        "jwt_verify" => crate::commands::tools::__component_jwt_verify(app, args).await,
        "generate_qr" => crate::commands::qr::__component_generate_qr(app, args).await,
        "load_workflow_metadata" => {
            crate::commands::workflows::__component_load_workflow_metadata(app, args).await
        }
        "save_workflow_metadata" => {
            crate::commands::workflows::__component_save_workflow_metadata(app, args).await
        }
        _ => Err("component_method_unavailable".into()),
    }
}
