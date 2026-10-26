use super::native;
pub use crate::core::update::runner::recovery::{Recovery, RecoveryOutcome, startup_blocked};
use crate::core::{
    channel::CHANNEL,
    update::runner::{Result, recovery},
};
use std::path::Path;
use tauri::{AppHandle, Manager};
pub fn inspect(app: &AppHandle) -> Result<Recovery> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| super::RunnerError::Storage)?
        .join("updates");
    recovery::inspect(&root, CHANNEL, native::process_live)
}
pub fn acknowledge(app: &AppHandle, operation: u64) -> Result<()> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| super::RunnerError::Storage)?
        .join("updates");
    recovery::acknowledge(&root, CHANNEL, operation, native::process_live)
}
pub fn early_busy(root: &Path) -> Result<bool> {
    recovery::early_busy(root, CHANNEL, native::process_live)
}
