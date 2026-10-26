//! Tauri wiring only. The shared core owns work, snapshots and verified files.
use crate::core::{
    channel::CHANNEL,
    update::{Service, Snapshot, UpdateError},
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};
pub struct Updates(pub Arc<Service>, Mutex<bool>);

pub fn initialize(app: &AppHandle) {
    let root = app
        .path()
        .app_local_data_dir()
        .ok()
        .map(|path| path.join("updates"));
    let publish = {
        let app = app.clone();
        move |snapshot| {
            let _ = app.emit("devbox://update", snapshot);
        }
    };
    let service = Service::new(CHANNEL, env!("CARGO_PKG_VERSION"), root.clone(), publish);
    #[cfg(devbox_update_fixture)]
    let service = {
        if let Some(path) = std::env::var_os("DEVBOX_UPDATE_FIXTURE") {
            let fixture = crate::core::update::fixture::Fixture::open(
                std::path::Path::new(&path),
                env!("DEVBOX_UPDATE_FIXTURE_SOURCE"),
            );
            match fixture.and_then(|fixture| {
                let app = app.clone();
                Service::fixture(CHANNEL, root, fixture, move |snapshot| {
                    let _ = app.emit("devbox://update", snapshot);
                })
            }) {
                Ok(fixture) => fixture,
                Err(_) => {
                    eprintln!("Invalid accept update fixture");
                    service
                }
            }
        } else {
            service
        }
    };
    // Inspect only owned runner stages before automatic discovery can obscure recovery.
    use super::update_runner::RecoveryOutcome;
    let mut pending_recovery = false;
    match super::update_runner::inspect(app) {
        Ok(recovery) => {
            let error = match recovery.outcome {
                RecoveryOutcome::InstallFailed => Some(UpdateError::InstallFailed),
                RecoveryOutcome::RelaunchFailed => Some(UpdateError::RelaunchFailed),
                RecoveryOutcome::UnknownBusy => Some(UpdateError::RecoveryRequired),
                RecoveryOutcome::None | RecoveryOutcome::Success => None,
            };
            if let Some(error) = error {
                service.recovered_install(
                    error,
                    matches!(recovery.outcome, RecoveryOutcome::UnknownBusy),
                );
            }
            if matches!(
                recovery.outcome,
                RecoveryOutcome::InstallFailed | RecoveryOutcome::RelaunchFailed
            ) {
                pending_recovery = true;
            } else if matches!(recovery.outcome, RecoveryOutcome::Success)
                && let Some(operation) = recovery.operation
                && super::update_runner::acknowledge(app, operation).is_err()
            {
                service.recovered_install(UpdateError::RecoveryRequired, true);
            }
        }
        Err(_) => service.recovered_install(UpdateError::RecoveryRequired, true),
    }
    app.manage(Updates(service.clone(), Mutex::new(pending_recovery)));
    tauri::async_runtime::spawn(service.clone().run());
    // This lifetime belongs to Rust setup, independent of any renderer or daemon connection.
    tauri::async_runtime::spawn(async move {
        loop {
            let _ = service.check(true);
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}
#[tauri::command]
pub fn update_status(state: State<'_, Updates>) -> Snapshot {
    state.inner().0.snapshot()
}
#[tauri::command]
pub fn update_check(app: AppHandle, state: State<'_, Updates>) -> Result<Snapshot, UpdateError> {
    let mut pending = state.inner().1.lock().expect("update recovery");
    if *pending {
        let recovery = match super::update_runner::inspect(&app) {
            Ok(recovery)
                if recovery.outcome != super::update_runner::RecoveryOutcome::UnknownBusy =>
            {
                recovery
            }
            _ => {
                state
                    .inner()
                    .0
                    .recovered_install(UpdateError::RecoveryRequired, true);
                return Ok(state.inner().0.snapshot());
            }
        };
        if let Some(operation) = recovery.operation {
            if super::update_runner::acknowledge(&app, operation).is_err() {
                state
                    .inner()
                    .0
                    .recovered_install(UpdateError::RecoveryRequired, true);
                return Ok(state.inner().0.snapshot());
            }
        } else if recovery.outcome != super::update_runner::RecoveryOutcome::None {
            // A terminal runner is still alive: retain the notice and retry only after exit proof.
            return Ok(state.inner().0.snapshot());
        }
        *pending = false;
    }
    drop(pending);
    state.inner().0.check(false)
}
#[tauri::command]
pub fn update_download(
    state: State<'_, Updates>,
    generation: u64,
) -> Result<Snapshot, UpdateError> {
    state.inner().0.download(generation)
}
#[tauri::command]
pub fn update_cancel(state: State<'_, Updates>, generation: u64) -> Result<Snapshot, UpdateError> {
    state.inner().0.cancel(generation)
}
