//! Native component boundary. Workspace must admit the actual caller window,
//! context and owned session before routing an operation to these adapters.
//! Initialization starts no terminal, global shortcut, tray or legacy writer.
pub use crate::commands::dashboard::docker_action_owned;
#[cfg(feature = "desktop")]
pub use crate::commands::shell_integration::dispatch_owned as shell_integration_owned;
pub use crate::commands::terminal::{SessionInfo, StartedSession};
pub use crate::core::{
    runtime_snapshot::DashboardSnapshot,
    workspace::{ProfileStore, WorkspaceProfile},
};
#[cfg(feature = "desktop")]
pub use crate::terminal_owner::TerminalOwner;
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::Manager;

/// Native Workspace supplies an identity-pinned project/distro lease. Every
/// multiplexer probe and actual PTY launch uses its same exact WSL target.
pub trait TerminalLaunchLease: Send + Sync {
    fn revalidate(&self) -> Result<(), String>;
    fn bind_argv(&self, argv: Vec<String>) -> Result<Vec<String>, String>;
    /// Called after PTY child/reader retirement; failure retains this owner.
    fn retire(&self) -> Result<(), String>;
}
pub trait TerminalLaunchFactory: Send + Sync {
    fn capture(&self, distro: &str) -> Result<Arc<dyn TerminalLaunchLease>, String>;
}
struct ProductData {
    path: PathBuf,
    identity: devbox_filesystem::FilesystemIdentity,
    _handle: File,
}
impl ProductData {
    fn open(path: &Path) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("terminal_storage_unavailable".into());
        }
        devbox_filesystem::ensure_no_links(path).map_err(|_| "terminal_storage_unavailable")?;
        let (handle, identity) = devbox_filesystem::open_filesystem_object(path, true)
            .map_err(|_| "terminal_storage_unavailable")?;
        Ok(Self {
            path: path.into(),
            identity,
            _handle: handle,
        })
    }
    fn checked(&self) -> Result<PathBuf, String> {
        devbox_filesystem::ensure_no_links(&self.path).map_err(|_| "terminal_storage_changed")?;
        if devbox_filesystem::filesystem_identity(&self.path, true)
            .map_err(|_| "terminal_storage_changed")?
            != self.identity
        {
            return Err("terminal_storage_changed".into());
        }
        Ok(self.path.clone())
    }
}
pub fn is_product(app: &tauri::AppHandle) -> bool {
    app.try_state::<ProductData>().is_some()
}
pub(crate) fn data_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Some(data) = app.try_state::<ProductData>() {
        return data.checked();
    }
    if app.config().identifier != "com.devbox.wsldesktop" {
        return Err("terminal_state_unavailable".into());
    }
    app.path()
        .app_local_data_dir()
        .map_err(|_| "terminal_storage_unavailable".into())
}
pub fn initialize(app: &tauri::AppHandle, data: &Path) -> Result<(), String> {
    use crate::commands::{shell_integration::ShellIntegrationState, terminal::SessionState};
    if is_product(app)
        || app.try_state::<Arc<SessionState>>().is_some()
        || app.try_state::<ShellIntegrationState>().is_some()
    {
        return Err("terminal_state_conflict".into());
    }
    let data = ProductData::open(data)?;
    app.manage(Arc::new(SessionState::new_product()));
    app.manage(ShellIntegrationState::default());
    app.manage(data);
    Ok(())
}
