//! On-demand authenticated Workspace metadata. No idle polling, product launch,
//! renderer path, or independent database reader is used by this bridge.
use product_contract::{
    project_provider::{Delivery, MAX_BYTES},
    transport::Call,
};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Listener, Manager};
#[derive(Default)]
struct Bridge {
    refresh: tokio::sync::Mutex<Option<String>>,
    invalidation: AtomicU64,
    commit: std::sync::Mutex<()>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
pub(crate) fn invalidate(app: &tauri::AppHandle) {
    if let Some(bridge) = app.try_state::<Bridge>() {
        let _commit = bridge
            .commit
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        bridge.invalidation.fetch_add(1, Ordering::AcqRel);
        crate::search::disconnect_project_provider(app);
    } else {
        crate::search::disconnect_project_provider(app);
    }
}
pub(crate) async fn refresh(app: &tauri::AppHandle, deadline: u64) {
    let Some(bridge) = app.try_state::<Bridge>() else {
        return;
    };
    let deadline = deadline.min(now().saturating_add(3000));
    let result = async {
        let mut epoch = tokio::time::timeout(
            Duration::from_millis(deadline.saturating_sub(now())),
            bridge.refresh.lock(),
        )
        .await
        .map_err(|_| "project_provider_busy")?;
        let before = bridge.invalidation.load(Ordering::Acquire);
        let value = crate::suite::remote(
            app,
            "workspace",
            Call::ProjectSnapshot {
                verify_current: true,
            },
            deadline,
        )
        .await?;
        let bytes = serde_json::to_vec(&value).map_err(|_| "project_provider_invalid")?;
        if bytes.len() > MAX_BYTES || now() >= deadline {
            return Err("project_provider_invalid");
        }
        let delivery: Delivery =
            serde_json::from_slice(&bytes).map_err(|_| "project_provider_invalid")?;
        let _commit = bridge.commit.lock().map_err(|_| "project_provider_busy")?;
        if uuid::Uuid::parse_str(&delivery.epoch).is_err()
            || !crate::suite::connection_ready(app)
            || before != bridge.invalidation.load(Ordering::Acquire)
        {
            return Err("project_provider_stale");
        }
        if epoch.as_ref() != Some(&delivery.epoch) {
            crate::search::disconnect_project_provider(app);
        }
        crate::search::install_project_snapshot(
            app,
            &serde_json::to_vec(&delivery.snapshot).map_err(|_| "project_provider_invalid")?,
        )
        .map_err(|_| "project_provider_invalid")?;
        *epoch = Some(delivery.epoch);
        Ok::<_, &'static str>(())
    }
    .await;
    if result.is_err() {
        invalidate(app);
    }
}
pub(crate) fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("knowledge-project-provider")
        .setup(|app, _| {
            app.manage(Bridge::default());
            let owner = app.clone();
            app.listen("suite-disconnected", move |_| invalidate(&owner));
            Ok(())
        })
        .build()
}
