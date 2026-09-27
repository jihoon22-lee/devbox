//! Installed API Studio listener owner. Native tasks retain admission on disconnect.
use crate::remote::RemoteSession;
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use workspace_core::lanes::{Lane, Lanes};
pub struct Webhooks {
    app: tauri::AppHandle,
    root: PathBuf,
    executable: PathBuf,
    initialized: Mutex<Option<bool>>,
    lanes: Lanes,
    stopping: AtomicBool,
}
impl Webhooks {
    pub fn new(app: tauri::AppHandle, root: PathBuf, executable: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            app,
            root,
            executable,
            initialized: Mutex::new(None),
            lanes: Lanes::default(),
            stopping: AtomicBool::new(false),
        })
    }
    pub fn initialize(&self) -> Result<(), &'static str> {
        let mut state = self
            .initialized
            .lock()
            .map_err(|_| "component_unavailable")?;
        if self.stopping.load(Ordering::Acquire) {
            return Err("component_closing");
        }
        if let Some(ready) = *state {
            return if ready {
                Ok(())
            } else {
                Err("component_unavailable")
            };
        }
        *state = Some(false);
        webhook_host::component::initialize_at(&self.app, self.root.clone())
            .map_err(|_| "component_unavailable")?;
        webhook_host::component::profile_executable(&self.app, self.executable.clone())
            .map_err(|_| "component_unavailable")?;
        // Failed resume is queryable; don't discard intent or make stop/settings inaccessible.
        let _ = webhook_host::component::resume_listener(&self.app);
        *state = Some(true);
        Ok(())
    }
    pub async fn dispatch(
        self: &Arc<Self>,
        session: Arc<RemoteSession>,
        request: Value,
    ) -> Result<Value, &'static str> {
        let incoming: product_ipc::IncomingRequest =
            serde_json::from_value(request).map_err(|_| "component_args_invalid")?;
        enum Call {
            Engine(webhook_host::api::WebhookCall),
            Projection(webhook_host::component::Projection),
        }
        let call = if incoming.method == "native_projection" {
            Call::Projection(
                serde_json::from_value(incoming.args).map_err(|_| "component_args_invalid")?,
            )
        } else {
            Call::Engine(
                serde_json::from_value(
                    serde_json::json!({"method":incoming.method,"args":incoming.args}),
                )
                .map_err(|_| "component_args_invalid")?,
            )
        };
        let lane = match &call {
            Call::Engine(call) if call.class() == product_ipc::ExecutionClass::Control => {
                Lane::EngineStop
            }
            _ => Lane::Engine,
        };
        let permit = self.lanes.try_enter(lane)?;
        let owner = self.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "request_expired")?
                .as_millis() as u64;
            session.authorize(&incoming.header, &["webhooks"], now, |_| false)?;
            let worker = tokio::time::timeout(
                std::time::Duration::from_millis(incoming.header.deadline_ms.saturating_sub(now)),
                owner.lanes.workers(lane).acquire_owned(),
            )
            .await
            .map_err(|_| "request_expired")?
            .map_err(|_| "component_closing")?;
            tauri::async_runtime::spawn_blocking(move || {
                let _worker = worker;
                workspace_core::current_deadline(incoming.header.deadline_ms)?;
                owner.initialize()?;
                match call {
                    Call::Engine(call) => tauri::async_runtime::block_on(
                        webhook_host::api::dispatch(&owner.app, call),
                    ),
                    Call::Projection(call) => webhook_host::component::project(&owner.app, call),
                }
                .map_err(|error| webhook_host::api::classify(&error))
            })
            .await
            .map_err(|_| "component_unavailable")?
        })
        .await
        .map_err(|_| "component_unavailable")?
    }
    pub async fn shutdown(self: &Arc<Self>) {
        self.stopping.store(true, Ordering::Release);
        // Drain admitted work before the final socket stop. A request that
        // passed initialization just before stopping may still start a listener.
        while self.lanes.active(Lane::Engine) != 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let owner = self.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let _state = owner.initialized.lock();
            let _ = webhook_host::component::stop_owned_listener(&owner.app);
        })
        .await;
    }
}
