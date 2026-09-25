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

/// Native-only delivery. A pending user action cannot be overwritten.
pub fn deliver(app: &tauri::AppHandle, request: devbox_applink::OpenRequest) -> Result<(), String> {
    use tauri::Emitter as _;
    app.state::<crate::applink::PendingOpen>()
        .try_set(request)?;
    // The slot owns the delivery. Wake-up failure leaves it for a cold pull.
    let _ = app.emit_to("main", "api-studio://transforms-open", ());
    Ok(())
}
