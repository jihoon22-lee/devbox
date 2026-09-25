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

pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "component_storage_unavailable")?
        .join("webhooks");
    if !app.manage(ComponentRoot(root)) {
        return Err("component_state_conflict".into());
    }
    if !app.manage(crate::commands::server_state()) {
        return Err("component_state_conflict".into());
    }
    Ok(())
}

/// Called only by the product's authorized source-owner route.
pub fn prepare_api_handoff(
    app: &tauri::AppHandle,
    selection: HandoffSelection,
) -> Result<serde_json::Value, String> {
    crate::commands::prepare_api_handoff(app, selection)
}

/// Native product lifecycle owns only this process's temporary listener.
pub fn listener_running(app: &tauri::AppHandle) -> bool {
    app.try_state::<std::sync::Arc<crate::commands::ServerState>>()
        .is_some_and(|state| crate::commands::server_status(state).running)
}
pub fn stop_owned_listener(app: &tauri::AppHandle) -> Result<(), String> {
    crate::commands::stop_server(app.state()).map(|_| ())
}
/// Native profile runner with no Tauri application, event loop or renderer.
/// The owning process/Job controls its lifetime, independent of an interactive app.
pub struct OwnedProfile {
    state: std::sync::Arc<crate::commands::ServerState>,
}
impl OwnedProfile {
    pub fn start(root: &std::path::Path, id: &str) -> Result<Self, String> {
        let profile = crate::core::service_profile::load_profile(root, id)?;
        let state = crate::commands::server_state();
        *state
            .rules
            .lock()
            .map_err(|_| "component_state_unavailable")? =
            crate::core::service_profile::rules_map(&profile);
        crate::commands::start_server_inner(&state, Some(profile.bind), profile.port, Some(false))?;
        Ok(Self { state })
    }
    pub fn wait(self) -> Result<(), String> {
        crate::commands::wait_server(&self.state)
    }
}
impl Drop for OwnedProfile {
    fn drop(&mut self) {
        let _ = crate::commands::stop_server_inner(&self.state);
    }
}
/// WP07's Logs provider may request only this bounded header-name/body-preview
/// projection from the already-authorized Webhook owner. No raw vault is read.
pub fn prepare_log_handoff(
    app: &tauri::AppHandle,
    selection: HandoffSelection,
) -> Result<devbox_applink::WebhookLogPayload, String> {
    crate::commands::prepare_log_handoff(app, selection)
}

pub enum HandoffSelection {
    History { history_id: u64 },
    Fixture { id: String },
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    #[test]
    fn headless_profile_owns_its_listener_and_drop_releases_the_port() {
        use std::net::{TcpListener, TcpStream};
        let root = tempfile::tempdir().unwrap();
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let executable = root.path().join("fixture.exe");
        std::fs::write(&executable, []).unwrap();
        let definition = crate::core::service_profile::export_run_definition_in(
            root.path(),
            &executable,
            "127.0.0.1",
            port,
            vec![],
            1000,
        )
        .unwrap();
        let id = &definition.services[0].id;
        let worker = OwnedProfile::start(root.path(), id).unwrap();
        assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
        assert!(OwnedProfile::start(root.path(), id).is_err());
        drop(worker);
        let rebound = TcpListener::bind(("127.0.0.1", port)).unwrap();
        drop(rebound);
    }
}
