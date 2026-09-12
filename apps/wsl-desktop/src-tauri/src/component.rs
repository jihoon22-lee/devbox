//! Native component boundary. Workspace must admit the actual caller window,
//! context and owned session before routing an operation to these adapters.
//! Initialization starts no terminal, global shortcut, tray or legacy writer.
pub use crate::commands::dashboard::docker_action_owned;
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

pub const COMMANDS: &[&str] = &[
    "list_distros",
    "dashboard_snapshot",
    "docker_ps",
    "docker_action",
    "detect_multiplexers",
    "inspect_shell_integration",
    "update_shell_integration",
    "list_workspace_profiles",
    "save_workspace_profile",
    "delete_workspace_profile",
    "windows_build_number",
    "start_session",
    "attach_session",
    "write_session",
    "broadcast",
    "resize_session",
    "close_session",
    "list_sessions",
];
#[cfg(feature = "desktop")]
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    data_root(app)?;
    if is_product(app)
        && matches!(
            method,
            "start_session"
                | "attach_session"
                | "write_session"
                | "broadcast"
                | "resize_session"
                | "close_session"
                | "list_sessions"
        )
    {
        return Err("terminal_owner_required".into());
    }
    let result = match method {
        "list_distros" => crate::commands::dashboard::__component_list_distros(app, args).await,
        "dashboard_snapshot" => {
            crate::commands::dashboard::__component_dashboard_snapshot(app, args).await
        }
        "docker_ps" => crate::commands::dashboard::__component_docker_ps(app, args).await,
        "docker_action" => crate::commands::dashboard::__component_docker_action(app, args).await,
        "detect_multiplexers" => {
            crate::commands::multiplexer::__component_detect_multiplexers(app, args).await
        }
        "inspect_shell_integration" => {
            crate::commands::shell_integration::__component_inspect_shell_integration(app, args)
                .await
        }
        "update_shell_integration" => {
            crate::commands::shell_integration::__component_update_shell_integration(app, args)
                .await
        }
        "list_workspace_profiles" => {
            crate::commands::workspace::__component_list_workspace_profiles(app, args).await
        }
        "save_workspace_profile" => {
            crate::commands::workspace::__component_save_workspace_profile(app, args).await
        }
        "delete_workspace_profile" => {
            crate::commands::workspace::__component_delete_workspace_profile(app, args).await
        }
        "windows_build_number" => {
            crate::commands::terminal::__component_windows_build_number(app, args).await
        }
        "start_session" => crate::commands::terminal::__component_start_session(app, args).await,
        "attach_session" => crate::commands::terminal::__component_attach_session(app, args).await,
        "write_session" => crate::commands::terminal::__component_write_session(app, args).await,
        "broadcast" => crate::commands::terminal::__component_broadcast(app, args).await,
        "resize_session" => crate::commands::terminal::__component_resize_session(app, args).await,
        "close_session" => crate::commands::terminal::__component_close_session(app, args).await,
        "list_sessions" => crate::commands::terminal::__component_list_sessions(app, args).await,
        _ => Err("terminal_method_invalid".into()),
    };
    data_root(app)?;
    result
}
