//! Knowledge's selected generation remains authoritative for background owners.
use std::path::{Path, PathBuf};
#[derive(Debug, PartialEq, Eq)]
pub enum CollectorPlan {
    WaitForStore,
    IndexOnly { search: PathBuf },
    Full { activity: PathBuf, search: PathBuf },
}
pub fn plan(root: &Path) -> Result<CollectorPlan, String> {
    let Some(manifest) = knowledge_stores::read(root)? else {
        return Ok(CollectorPlan::WaitForStore);
    };
    let search = knowledge_stores::directory(root, &manifest, "search")?;
    if knowledge_stores::collection_consent(root)? {
        Ok(CollectorPlan::Full {
            activity: knowledge_stores::directory(root, &manifest, "activity")?,
            search,
        })
    } else {
        Ok(CollectorPlan::IndexOnly { search })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collectors_wait_for_a_selected_store_and_explicit_consent() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(plan(root.path()).unwrap(), CollectorPlan::WaitForStore);
        let manifest = knowledge_stores::create_empty(root.path()).unwrap();
        let search = knowledge_stores::directory(root.path(), &manifest, "search").unwrap();
        assert_eq!(
            plan(root.path()).unwrap(),
            CollectorPlan::IndexOnly {
                search: search.clone()
            }
        );
        let activity = knowledge_stores::directory(root.path(), &manifest, "activity").unwrap();
        let connection = rusqlite::Connection::open(activity.join("data.db")).unwrap();
        connection.execute("INSERT INTO settings(key,value) VALUES('product_activity_collection_v1','enabled')", []).unwrap();
        assert_eq!(
            plan(root.path()).unwrap(),
            CollectorPlan::Full { activity, search }
        );
    }
}

use crate::remote::RemoteSession;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use workspace_core::lanes::{Lane, Lanes};
#[derive(Default)]
struct Initialized {
    manifest: Option<knowledge_stores::Manifest>,
    search: bool,
    activity: bool,
    failed: bool,
}
pub struct Collectors {
    app: tauri::AppHandle,
    root: PathBuf,
    initialized: Mutex<Initialized>,
    lanes: Lanes,
    stopping: AtomicBool,
}
enum Call {
    Activity(activity_engine::api::ActivityCall),
    Search(content_index_engine::api::SearchCall),
    Settings(content_index_engine::api::SearchSettingsCall),
    Draft(activity_engine::component::DraftDelivery),
    RootHealth,
}
impl Collectors {
    pub fn new(app: tauri::AppHandle, root: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            app,
            root,
            initialized: Mutex::default(),
            lanes: Lanes::default(),
            stopping: AtomicBool::new(false),
        })
    }
    pub fn initialize(&self, activity_request: bool) -> Result<(), &'static str> {
        let mut state = self
            .initialized
            .lock()
            .map_err(|_| "knowledge_agent_unavailable")?;
        if self.stopping.load(Ordering::Acquire) {
            return Err("knowledge_agent_unavailable");
        }
        if state.failed {
            return Err("knowledge_agent_unavailable");
        }
        let manifest = knowledge_stores::read(&self.root)
            .map_err(|_| "knowledge_store_unavailable")?
            .ok_or("knowledge_store_missing")?;
        if state.manifest.as_ref().is_some_and(|old| old != &manifest) {
            return Err("knowledge_store_changed");
        }
        let plan = plan(&self.root).map_err(|_| "knowledge_store_unavailable")?;
        let (search, collect) = match plan {
            CollectorPlan::WaitForStore => return Err("knowledge_store_missing"),
            CollectorPlan::IndexOnly { search } => (search, false),
            CollectorPlan::Full { search, .. } => (search, true),
        };
        // A partially initialized engine cannot be safely initialized again.
        state.failed = true;
        state.manifest = Some(manifest.clone());
        if !state.search {
            content_index_engine::component::initialize(
                &self.app,
                &search,
                Some(self.root.join("integration")),
            )
            .map_err(|_| "knowledge_agent_unavailable")?;
            state.search = true;
        }
        if !state.activity && (collect || activity_request) {
            let activity = knowledge_stores::directory(&self.root, &manifest, "activity")
                .map_err(|_| "knowledge_store_unavailable")?;
            // Queries/settings need managed state even without consent. The
            // existing engine stays paused and never samples before StartTracking.
            activity_engine::component::initialize(
                &self.app,
                &activity,
                self.root.join("integration"),
            )
            .map_err(|_| "knowledge_agent_unavailable")?;
            state.activity = true;
        }
        state.failed = false;
        Ok(())
    }
    pub async fn dispatch(
        self: &Arc<Self>,
        session: Arc<RemoteSession>,
        component: &str,
        request: Value,
    ) -> Result<Value, &'static str> {
        let incoming: product_ipc::IncomingRequest =
            serde_json::from_value(request).map_err(|_| "component_args_invalid")?;
        let raw = json!({"method":incoming.method,"args":incoming.args});
        let (call, route) = match component {
            "knowledge.activity" if incoming.method == "native_draft_delivery" => (
                Call::Draft(
                    serde_json::from_value(incoming.args).map_err(|_| "component_args_invalid")?,
                ),
                "activity",
            ),
            "knowledge.activity" => {
                let call: activity_engine::api::ActivityCall =
                    serde_json::from_value(raw).map_err(|_| "component_args_invalid")?;
                if matches!(
                    call,
                    activity_engine::api::ActivityCall::SendDigestToKnowledge { .. }
                        | activity_engine::api::ActivityCall::AutostartStatus {}
                        | activity_engine::api::ActivityCall::SetAutostart { .. }
                ) {
                    return Err("component_args_invalid");
                }
                (Call::Activity(call), "activity")
            }
            "knowledge.search"
                if incoming.method == "native_root_health" && incoming.args == json!({}) =>
            {
                (Call::RootHealth, "search")
            }
            "knowledge.search" => {
                let call = serde_json::from_value(raw).map_err(|_| "component_args_invalid")?;
                if matches!(
                    call,
                    content_index_engine::api::SearchCall::SearchFiles { .. }
                        | content_index_engine::api::SearchCall::SearchContent { .. }
                ) {
                    return Err("component_args_invalid");
                }
                (Call::Search(call), "search")
            }
            "knowledge.search-settings" => (
                Call::Settings(serde_json::from_value(raw).map_err(|_| "component_args_invalid")?),
                "search",
            ),
            _ => return Err("component_args_invalid"),
        };
        let activity = matches!(&call, Call::Activity(_) | Call::Draft(_));
        let control = match &call {
            Call::Activity(call) => call.class() == product_ipc::ExecutionClass::Control,
            Call::Settings(call) => call.class() == product_ipc::ExecutionClass::Control,
            Call::Draft(activity_engine::component::DraftDelivery::Finish { .. }) => true,
            _ => false,
        };
        let lane = if control {
            Lane::EngineStop
        } else {
            Lane::Engine
        };
        let permit = self.lanes.try_enter(lane)?;
        let owner = self.clone();
        tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "request_expired")?
                .as_millis() as u64;
            session.authorize(&incoming.header, &[route], now, |_| false)?;
            let worker = tokio::time::timeout(
                std::time::Duration::from_millis(incoming.header.deadline_ms.saturating_sub(now)),
                owner.lanes.workers(lane).acquire_owned(),
            )
            .await
            .map_err(|_| "request_expired")?
            .map_err(|_| "knowledge_agent_unavailable")?;
            tauri::async_runtime::spawn_blocking(move || {
                let _worker = worker;
                workspace_core::current_deadline(incoming.header.deadline_ms)?;
                owner.initialize(activity)?;
                tauri::async_runtime::block_on(async {
                    match call {
                        Call::Activity(call) => {
                            let result = activity_engine::api::dispatch(&owner.app, call).await;
                            crate::tray::refresh(&owner.app);
                            result
                        }
                        Call::Search(call) => {
                            content_index_engine::api::dispatch_search(&owner.app, call).await
                        }
                        Call::Settings(call) => {
                            content_index_engine::api::dispatch_settings(&owner.app, call).await
                        }
                        Call::Draft(call) => {
                            activity_engine::component::draft_delivery(&owner.app, call).await
                        }
                        Call::RootHealth => Ok(json!(
                            content_index_engine::component::product_root_health(&owner.app)
                        )),
                    }
                })
                .map_err(|error| {
                    if activity {
                        activity_engine::api::classify(&error)
                    } else {
                        content_index_engine::api::classify(&error)
                    }
                })
            })
            .await
            .map_err(|_| "knowledge_agent_unavailable")?
        })
        .await
        .map_err(|_| "knowledge_agent_unavailable")?
    }
    pub fn set_tracking(&self, enabled: bool) -> Result<(), &'static str> {
        let _permit = self.lanes.try_enter(Lane::EngineStop)?;
        self.initialize(true)?;
        tauri::async_runtime::block_on(activity_engine::api::dispatch(
            &self.app,
            if enabled {
                activity_engine::api::ActivityCall::StartTracking {}
            } else {
                activity_engine::api::ActivityCall::StopTracking {}
            },
        ))
        .map(|_| ())
        .map_err(|_| "knowledge_agent_unavailable")
    }
    pub async fn shutdown(self: &Arc<Self>) {
        self.stopping.store(true, Ordering::Release);
        while self.lanes.active(Lane::Engine) != 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let owner = self.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            drop(owner.initialized.lock());
        })
        .await;
        let _ = activity_engine::component::shutdown_agent(&self.app).await;
        let _ = content_index_engine::component::shutdown_agent(&self.app).await;
    }
}
