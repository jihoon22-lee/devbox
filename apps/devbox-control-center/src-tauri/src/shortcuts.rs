//! Control Center owns the native shortcut registration. No startup entry is
//! added, and other installation namespaces cannot become simultaneous owners.
#[path = "../../../../devbox-launcher/src-tauri/src/hotkey.rs"]
#[allow(dead_code)] // The product uses the common multi-binding worker, not legacy window toggling.
mod native;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, Manager};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Config {
    pub accelerator: String,
    pub enabled: bool,
    #[serde(default)]
    pub terminal: bool,
    #[serde(default)]
    pub capture: bool,
    #[serde(default)]
    pub project: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            accelerator: "Ctrl+Alt+Space".into(),
            enabled: false,
            terminal: false,
            capture: false,
            project: false,
        }
    }
}
impl Config {
    fn validate(&self) -> Result<(), String> {
        if self.terminal || self.capture || self.project {
            return Err("shortcut_command_unavailable".into());
        }
        native::validate(&native::ShortcutConfig {
            accelerator: self.accelerator.clone(),
            enabled: self.enabled,
        })
    }
    fn bindings(&self) -> Vec<(String, String)> {
        if !self.enabled {
            return Vec::new();
        }
        let mut values = vec![("control-center.launcher".into(), self.accelerator.clone())];
        for (enabled, command, key) in [
            (self.terminal, "workspace.summon-terminal", "Ctrl+Alt+T"),
            (self.capture, "knowledge.quick-capture", "Ctrl+Alt+N"),
            (self.project, "workspace.open-current-project", "Ctrl+Alt+P"),
        ] {
            if enabled {
                values.push((command.into(), key.into()));
            }
        }
        values
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    #[serde(flatten)]
    pub config: Config,
    pub registration: native::RegistrationState,
    pub alternatives: Vec<String>,
    pub issue: Option<String>,
}
#[derive(Default)]
pub(crate) struct Owner {
    state: Mutex<State>,
}
struct State {
    config: Config,
    runtime: native::RuntimeState,
    issue: Option<String>,
    #[cfg(windows)]
    lease: Option<Lease>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            config: Config::default(),
            runtime: native::RuntimeState::default(),
            issue: None,
            #[cfg(windows)]
            lease: None,
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.runtime.stop_global_listener();
    }
}
#[cfg(windows)]
struct Lease(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for Lease {}
#[cfg(windows)]
impl Drop for Lease {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
impl Lease {
    fn acquire() -> Result<Self, String> {
        use windows::{
            core::w,
            Win32::{
                Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS},
                System::Threading::CreateMutexW,
            },
        };
        // An existence lease is handle-owned, not thread-owned. No wait/release
        // operation is transferred between Tokio threads.
        let handle = unsafe { CreateMutexW(None, false, w!("Local\\Devbox.v08.ShortcutOwner")) }
            .map_err(|_| "shortcut_owner_unavailable")?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err("shortcut_other_installation".into());
        }
        Ok(Self(handle))
    }
}
fn callback() -> native::ShortcutCallback {
    std::sync::Arc::new(|app, command| {
        if let Some(window) = app.get_webview_window("main") {
            let was_focused = window.is_focused().unwrap_or(false);
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            let _ = window.emit(
                "suite-shortcut",
                serde_json::json!({"command":command,"wasFocused":was_focused}),
            );
        }
    })
}
fn path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "shortcut_unavailable")?;
    let parent = root.parent().ok_or("shortcut_unavailable")?;
    devbox_filesystem::ensure_no_links(parent).map_err(|_| "shortcut_unavailable")?;
    match std::fs::create_dir(&root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("shortcut_unavailable".into()),
    }
    devbox_filesystem::ensure_no_links(&root).map_err(|_| "shortcut_unavailable")?;
    Ok(root.join("suite-shortcuts.json"))
}
fn view(state: &State) -> View {
    View {
        config: state.config.clone(),
        registration: if !state.config.enabled {
            native::RegistrationState::Disabled
        } else {
            state.runtime.registration()
        },
        alternatives: native::SHORTCUTS
            .iter()
            .filter(|key| **key != state.config.accelerator)
            .map(|key| (*key).into())
            .collect(),
        issue: state.issue.clone(),
    }
}
impl Owner {
    pub(crate) fn load(&self, app: &tauri::AppHandle) -> Result<View, String> {
        let mut state = self.state.lock().map_err(|_| "shortcut_busy")?;
        let path = path(app)?;
        if state.runtime.registration() == native::RegistrationState::Pending {
            state.runtime.stop_global_listener();
        }
        if path.exists() {
            let (mut file, identity) = devbox_filesystem::open_filesystem_object(&path, false)
                .map_err(|_| "shortcut_unavailable")?;
            use std::io::Read;
            let mut bytes = Vec::new();
            (&mut file)
                .take(4097)
                .read_to_end(&mut bytes)
                .map_err(|_| "shortcut_unavailable")?;
            if bytes.len() > 4096
                || devbox_filesystem::filesystem_identity(&path, false)
                    .map_err(|_| "shortcut_unavailable")?
                    != identity
            {
                return Err("shortcut_invalid".into());
            }
            let config: Config = serde_json::from_slice(&bytes).map_err(|_| "shortcut_invalid")?;
            config.validate()?;
            // Loading a preference never registers global input before the
            // installation connection has been explicitly approved.
            state.config = config;
        }
        Ok(view(&state))
    }
    pub(crate) fn configure(&self, app: &tauri::AppHandle, config: Config) -> Result<View, String> {
        config.validate()?;
        self.load(app)?; // Corrupt/future settings cannot be silently replaced.
        let mut state = self.state.lock().map_err(|_| "shortcut_busy")?;
        #[cfg(windows)]
        if config.enabled && state.lease.is_none() {
            match Lease::acquire() {
                Ok(lease) => state.lease = Some(lease),
                Err(error) => {
                    state.issue = Some(error);
                    return Ok(view(&state));
                }
            }
        }
        let previous = state.config.clone();
        state.runtime.stop_global_listener();
        native::start_bindings(app, &config.bindings(), state.runtime.clone(), callback());
        let registered = !config.enabled
            || state.runtime.registration() == native::RegistrationState::Registered;
        if !registered {
            state.runtime.stop_global_listener();
            native::start_bindings(app, &previous.bindings(), state.runtime.clone(), callback());
            let restored = previous.enabled
                && state.runtime.registration() == native::RegistrationState::Registered;
            state.issue = Some(
                if restored {
                    "shortcut_registration_failed_previous_restored"
                } else if previous.enabled {
                    "shortcut_previous_restore_failed"
                } else {
                    "shortcut_registration_failed"
                }
                .into(),
            );
            #[cfg(windows)]
            if !restored {
                state.lease.take();
            }
            return Ok(view(&state));
        }
        let saved = path(app).and_then(|path| {
            let bytes = serde_json::to_vec(&config).map_err(|_| "shortcut_invalid")?;
            devbox_filesystem::atomic_write(&path, &bytes)
                .map_err(|_| "shortcut_save_failed".into())
        });
        if let Err(error) = saved {
            state.runtime.stop_global_listener();
            native::start_bindings(app, &previous.bindings(), state.runtime.clone(), callback());
            state.issue = Some(error);
            return Ok(view(&state));
        }
        state.config = config;
        state.issue = None;
        #[cfg(windows)]
        if !state.config.enabled {
            state.lease.take();
        }
        Ok(view(&state))
    }
    pub(crate) fn stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.runtime.stop_global_listener();
            #[cfg(windows)]
            {
                state.lease.take();
            }
        }
    }
}
