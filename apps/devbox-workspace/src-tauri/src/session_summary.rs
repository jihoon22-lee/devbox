//! Native Session summary delivery surface for authenticated product transport.
use product_contract::session_summary::Metadata;
use std::sync::Arc;
use tauri::Manager;
/// Native-only entrypoint for the authenticated B07 transport. Reuses the prepared
/// operation's immutable metadata and rejects changed/archived Session bindings.
/// This call does not publish, launch another product or assert peer authentication.
pub fn delivery(
    app: &tauri::AppHandle,
    operation_id: &str,
) -> std::result::Result<Metadata, String> {
    if !uuid::Uuid::parse_str(operation_id).is_ok_and(|id| id.to_string() == operation_id) {
        return Err("session_summary_invalid".into());
    }
    let sessions = app
        .try_state::<Arc<crate::development_host::Sessions>>()
        .ok_or("session_summary_unavailable")?;
    let receipt = sessions
        .summary_delivery(operation_id)
        .map_err(str::to_owned)?;
    crate::component::provider_context(app, &receipt.metadata.binding.context)
        .map_err(str::to_owned)?;
    Ok(receipt.metadata)
}
