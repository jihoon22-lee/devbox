//! Embedded Manager tools only. The suite's installer does not use legacy batch
//! install, cleanup_partials, startup migration or a legacy executable fallback.
use crate::commands::{dev_setup, diagnostics};
use tauri::Manager;
pub(crate) struct EmbeddedTools;

pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    if !app.manage(EmbeddedTools)
        || !app.manage(diagnostics::DiagnosticsState::default())
        || !app.manage(dev_setup::DevSetupConfigurationState::default())
    {
        return Err("manager_state_conflict".into());
    }
    Ok(())
}
