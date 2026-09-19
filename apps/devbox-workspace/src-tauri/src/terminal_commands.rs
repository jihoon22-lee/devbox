//! Native command adapter for B07's single shortcut owner. No global shortcut is
//! registered here. Profile commands resolve through the existing reviewed main API.
use product_contract::ProjectContext;
use serde_json::Value;
use std::sync::Arc;
use tauri::Manager;
pub fn catalog(app: &tauri::AppHandle) -> Result<Value, String> {
    let host = crate::component::provider_host(app).map_err(str::to_owned)?;
    let terminals = app
        .try_state::<Arc<crate::terminal_host::Terminals>>()
        .ok_or("terminal_owner_unavailable")?;
    terminals.command_catalog(app, &host).map_err(str::to_owned)
}
/// The caller authenticates a command selection from catalog; expiry and native
/// window/context identity are checked again. This never creates or stops a PTY.
pub fn summon(
    app: &tauri::AppHandle,
    terminal_id: &str,
    context: Option<&ProjectContext>,
    operation_id: &str,
    deadline_ms: u64,
) -> Result<Value, String> {
    if let Some(context) = context {
        crate::component::provider_context(app, context).map_err(str::to_owned)?;
    }
    let terminals = app
        .try_state::<Arc<crate::terminal_host::Terminals>>()
        .ok_or("terminal_owner_unavailable")?;
    terminals
        .summon(app, terminal_id, context, operation_id, deadline_ms)
        .map_err(str::to_owned)
}
