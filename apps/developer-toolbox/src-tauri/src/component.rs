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

pub fn initialize(
    app: &tauri::AppHandle,
    handoff_store: devbox_applink::HandoffStore,
) -> Result<(), String> {
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
    if !app.manage(crate::commands::text_handoff::PendingToolboxText::with_store(handoff_store)) {
        return Err("component_state_conflict".into());
    }
    Ok(())
}

pub const COMMANDS: &[&str] = &[
    "preview_toolbox_text",
    "accept_toolbox_text",
    "discard_toolbox_text",
    "renew_toolbox_text",
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
        "preview_toolbox_text" => {
            crate::commands::text_handoff::__component_preview_toolbox_text(app, args).await
        }
        "accept_toolbox_text" => {
            crate::commands::text_handoff::__component_accept_toolbox_text(app, args).await
        }
        "discard_toolbox_text" => {
            crate::commands::text_handoff::__component_discard_toolbox_text(app, args).await
        }
        "renew_toolbox_text" => {
            crate::commands::text_handoff::__component_renew_toolbox_text(app, args).await
        }

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

/// Native-only delivery. A pending user action cannot be overwritten.
pub fn deliver(app: &tauri::AppHandle, request: devbox_applink::OpenRequest) -> Result<(), String> {
    use tauri::Emitter as _;
    app.state::<crate::applink::PendingOpen>()
        .try_set(request)?;
    // The slot owns the delivery. Wake-up failure leaves it for a cold pull.
    let _ = app.emit_to("main", "api-studio://transforms-open", ());
    Ok(())
}
