//! Capture the Notes-owned configuration before doctor/secret collection yields.
use devbox_protocol::Fail;
use devbox_tools::{ToolsError, support::VaultSnapshot};

/// `None` while notes are off: with no vault to mask, the bundle leaves out logs and
/// crashes rather than assume the default vault.
pub(super) async fn vault_snapshot(
    s: &super::state::Services,
) -> Result<Option<VaultSnapshot>, Fail<ToolsError>> {
    let Some(configured) = s.notes_vault().map(std::path::Path::to_path_buf) else {
        return Ok(None);
    };
    tokio::task::spawn_blocking(move || VaultSnapshot::capture(&configured))
        .await
        .map_err(|_| Fail::from(ToolsError::SupportFailed))?
        .map(Some)
}
