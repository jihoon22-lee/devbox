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

pub const COMMANDS: &[&str] = &[
    "server_status",
    "export_run_service_definition",
    "start_server",
    "stop_server",
    "list_history",
    "clear_history",
    "copy_masked_history",
    "copy_raw_history",
    "copy_history_headers",
    "delete_history",
    "replay_history",
    "list_fixtures",
    "save_fixture",
    "delete_fixture",
    "clear_fixtures",
    "fixture_to_rule",
    "replay_fixture",
    "list_rules",
    "preview_rule_conflicts",
    "set_rule",
    "delete_rule",
    "reset_rule_sequence",
];

/// No dynamic command loading or legacy application startup occurs here.
pub async fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method {
        "export_run_service_definition" => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Input {}
            let _: Input = serde_json::from_value(args).map_err(|_| "component_args_invalid")?;
            let mut definition =
                crate::commands::export_run_service_definition(app.clone(), app.state())?;
            for service in &mut definition.services {
                service.name = service.name.replacen("Webhook Lab", "API Studio", 1);
            }
            serde_json::to_value(definition).map_err(|_| "component_response_invalid".into())
        }
        "server_status" => crate::commands::__component_server_status(app, args).await,
        "start_server" => crate::commands::__component_start_server(app, args).await,
        "stop_server" => crate::commands::__component_stop_server(app, args).await,
        "list_history" => crate::commands::__component_list_history(app, args).await,
        "clear_history" => crate::commands::__component_clear_history(app, args).await,
        "copy_masked_history" => crate::commands::__component_copy_masked_history(app, args).await,
        "copy_raw_history" => crate::commands::__component_copy_raw_history(app, args).await,
        "copy_history_headers" => {
            crate::commands::__component_copy_history_headers(app, args).await
        }
        "delete_history" => crate::commands::__component_delete_history(app, args).await,
        "replay_history" => crate::commands::__component_replay_history(app, args).await,
        "list_fixtures" => crate::commands::__component_list_fixtures(app, args).await,
        "save_fixture" => crate::commands::__component_save_fixture(app, args).await,
        "delete_fixture" => crate::commands::__component_delete_fixture(app, args).await,
        "clear_fixtures" => crate::commands::__component_clear_fixtures(app, args).await,
        "fixture_to_rule" => crate::commands::__component_fixture_to_rule(app, args).await,
        "replay_fixture" => crate::commands::__component_replay_fixture(app, args).await,
        "list_rules" => crate::commands::__component_list_rules(app, args).await,
        "preview_rule_conflicts" => {
            crate::commands::__component_preview_rule_conflicts(app, args).await
        }
        "set_rule" => crate::commands::__component_set_rule(app, args).await,
        "delete_rule" => crate::commands::__component_delete_rule(app, args).await,
        "reset_rule_sequence" => crate::commands::__component_reset_rule_sequence(app, args).await,
        _ => Err("component_method_unavailable".into()),
    }
}

/// Called only by the product's authorized source-owner route.
pub fn prepare_api_handoff(
    app: &tauri::AppHandle,
    args: serde_json::Value,
    saved: bool,
) -> Result<serde_json::Value, String> {
    crate::commands::prepare_api_handoff(app, args, saved)
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
    args: serde_json::Value,
    saved: bool,
) -> Result<devbox_applink::WebhookLogPayload, String> {
    crate::commands::prepare_log_handoff(app, args, saved)
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
