//! Tauri wiring only. The shared core owns work, snapshots and verified files.
use crate::core::{
    channel::CHANNEL,
    update::{Service, Snapshot, UpdateError},
};
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter, Manager, State};
pub struct Updates(pub Arc<Service>);

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
    app.manage(Updates(service.clone()));
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
pub fn update_check(state: State<'_, Updates>) -> Result<Snapshot, UpdateError> {
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
