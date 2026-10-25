//! Capture the Notes-owned configuration before doctor/secret collection yields.
pub(super) async fn vault_snapshot(
    notes: &devbox_notes::NotesService,
) -> Result<devbox_tools::support::VaultSnapshot, devbox_protocol::Fail<devbox_tools::ToolsError>> {
    let configured = notes.vault().to_path_buf();
    tokio::task::spawn_blocking(move || devbox_tools::support::VaultSnapshot::capture(&configured))
        .await
        .map_err(|_| devbox_protocol::Fail::from(devbox_tools::ToolsError::SupportFailed))?
}
