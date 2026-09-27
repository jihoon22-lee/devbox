//! A single installed owner of Workspace runtime stores. Metadata stays read-only.
use crate::remote::RemoteSession;
use product_contract::RouteRequest;
use product_ipc::{ComponentCall, IncomingRequest};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use workspace_core::{
    lanes::{Lane, Lanes},
    Host,
};

#[derive(Default)]
struct Shutdown(tokio::sync::OnceCell<()>);
impl Shutdown {
    async fn run<F, Fut>(&self, cleanup: F)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        self.0.get_or_init(cleanup).await;
    }
}

type Result<T> = std::result::Result<T, &'static str>;
macro_rules! call {
    ($name:ident, $engine:ty, $component:literal, $routes:ident) => {
        #[derive(Deserialize)]
        #[serde(transparent)]
        struct $name($engine);
        impl ComponentCall for $name {
            const COMPONENT: &'static str = $component;
            const MAX_ARGUMENT_BYTES: usize = agent_protocol::MAX_FRAME_BYTES;
            fn method(&self) -> &'static str {
                self.0.method()
            }
            fn routes(&self) -> &'static [&'static str] {
                workspace_core::runtime_policy::$routes(self.method())
            }
        }
    };
}
call!(
    RuntimeCall,
    runtime_engine::api::RuntimeCall,
    "workspace.runtime",
    runtime_routes
);
call!(
    ProcessesCall,
    ports_engine::api::PortsCall,
    "workspace.processes",
    processes_routes
);
call!(
    ProcessActionsCall,
    ports_engine::api::PortsCall,
    "workspace.process-actions",
    process_actions_routes
);
call!(
    LogsCall,
    logs_engine::api::LogsCall,
    "workspace.logs",
    logs_routes
);

enum Parsed {
    Query(workspace_core::runtime_queries::Call),
    Session(workspace_core::session_rpc::Call),
    Runtime(runtime_engine::api::RuntimeCall),
    Ports(ports_engine::api::PortsCall),
    Logs(logs_engine::api::LogsCall),
}
impl Parsed {
    fn lane(&self) -> Lane {
        match self {
            Self::Query(_) => Lane::Engine,
            Self::Session(call) => call.lane(),
            Self::Runtime(call) => call.lane(),
            Self::Ports(call) => call.lane(),
            Self::Logs(call) => call.lane(),
        }
    }
    async fn execute(self, app: &tauri::AppHandle) -> std::result::Result<Value, String> {
        match self {
            Self::Query(_) | Self::Session(_) => Err("session_runtime_invalid".into()),
            Self::Runtime(call) => runtime_engine::api::dispatch(app, call).await,
            Self::Ports(call) => ports_engine::api::dispatch(app, call).await,
            Self::Logs(call) => logs_engine::api::dispatch(app, call).await,
        }
    }
}
fn decode(
    component: &str,
    request: Value,
) -> Result<(RouteRequest, Parsed, &'static [&'static str])> {
    if component == "workspace.runtime"
        && request
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(|method| {
                matches!(
                    method,
                    "show_main_window"
                        | "hide_main_window"
                        | "quit_app"
                        | "startup_shortcut_status"
                        | "set_startup_shortcut_enabled"
                )
            })
    {
        return Err("method_unknown");
    }
    let incoming: IncomingRequest =
        serde_json::from_value(request).map_err(|_| "invalid_request")?;
    if component == "workspace.runtime" && incoming.method == "native_query" {
        let call: workspace_core::runtime_queries::Call =
            serde_json::from_value(incoming.args).map_err(|_| "invalid_request")?;
        call.validate()?;
        if call.context() != incoming.header.context.as_ref() {
            return Err("stale_context");
        }
        return Ok((incoming.header, Parsed::Query(call), &["sessions"]));
    }
    if component == "workspace.runtime" && incoming.method == "session_owner" {
        let call: workspace_core::session_rpc::Call =
            serde_json::from_value(incoming.args).map_err(|_| "invalid_request")?;
        call.validate()?;
        if call
            .live_context()
            .is_some_and(|context| incoming.header.context.as_ref() != Some(context))
        {
            return Err("stale_context");
        }
        return Ok((incoming.header, Parsed::Session(call), &["sessions"]));
    }
    macro_rules! decode {
        ($kind:ty, $variant:ident) => {{
            let request = incoming.decode::<$kind>().map_err(|_| "invalid_request")?;
            let routes = request.call.routes();
            Ok((request.header, Parsed::$variant(request.call.0), routes))
        }};
    }
    match component {
        "workspace.runtime" => decode!(RuntimeCall, Runtime),
        "workspace.processes" => decode!(ProcessesCall, Ports),
        "workspace.process-actions" => decode!(ProcessActionsCall, Ports),
        "workspace.logs" => decode!(LogsCall, Logs),
        _ => Err("method_unknown"),
    }
}
enum State {
    Cold,
    Ready(Arc<Host>),
    Failed,
}
pub struct Runtime {
    app: tauri::AppHandle,
    data: PathBuf,
    resources: PathBuf,
    state: Mutex<State>,
    lanes: Lanes,
    sessions: crate::session_runtime::Sessions,
    definitions: Mutex<workspace_core::definitions::Definitions>,
    stopping: std::sync::atomic::AtomicBool,
    active: std::sync::atomic::AtomicUsize,
    drained: tokio::sync::Notify,
    shutdown: Shutdown,
}
impl Runtime {
    pub fn new(app: tauri::AppHandle, data: PathBuf, resources: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            app,
            data,
            resources,
            state: Mutex::new(State::Cold),
            lanes: Lanes::default(),
            sessions: Default::default(),
            definitions: Default::default(),
            stopping: std::sync::atomic::AtomicBool::new(false),
            active: std::sync::atomic::AtomicUsize::new(0),
            drained: tokio::sync::Notify::new(),
            shutdown: Shutdown::default(),
        })
    }
    pub fn initialize(&self) -> Result<Arc<Host>> {
        let mut state = self.state.lock().map_err(|_| "runtime_owner_unavailable")?;
        if self.stopping.load(std::sync::atomic::Ordering::Acquire) {
            return Err("request_cancelled");
        }
        match &*state {
            State::Ready(host) => {
                host.component("runtime")?;
                host.component("processes")?;
                host.component("logs")?;
                return Ok(host.clone());
            }
            State::Failed => return Err("runtime_owner_unavailable"),
            State::Cold => {}
        }
        let executable = std::env::current_exe()
            .map_err(|_| "runtime_owner_unavailable")?
            .canonicalize()
            .map_err(|_| "runtime_owner_unavailable")?;
        if !product_shell_tauri::component_ready(
            &executable,
            "control-center",
            env!("CARGO_PKG_VERSION"),
        )? {
            return Err("suite_activation_pending");
        }
        let host = Arc::new(Host::open_read_only_with_resources(
            &self.data,
            self.resources.clone(),
        )?);
        let runtime = host.component("runtime")?;
        let common = host.component("common")?;
        let processes = host.component("processes")?;
        let logs = host.component("logs")?;
        // Do not retry partially initialized engine state. Missing/unselected
        // metadata above remains retryable after Workspace prepares its stores.
        *state = State::Failed;
        runtime_engine::component::initialize_agent_with_sources(
            &self.app,
            &runtime,
            &common,
            Arc::new(workspace_core::task_sources::Sources { host: host.clone() }),
        )
        .map_err(|_| "runtime_owner_unavailable")?;
        ports_engine::component::initialize(&self.app, &processes)
            .map_err(|_| "runtime_owner_unavailable")?;
        logs_engine::component::initialize(
            &self.app,
            &logs,
            workspace_core::runtime_logs::provider(&self.app, host.clone())?,
        )
        .map_err(|_| "runtime_owner_unavailable")?;
        *state = State::Ready(host.clone());
        Ok(host)
    }
    pub async fn dispatch(
        self: &Arc<Self>,
        session: Arc<RemoteSession>,
        component: &str,
        request: Value,
    ) -> Result<Value> {
        let (header, call, routes) = decode(component, request)?;
        let owner = self.clone();
        // The owned task retains its request/worker permits if a UI connection
        // disappears; dropping an RPC future cannot create extra native workers.
        tauri::async_runtime::spawn(async move {
            struct Active(Arc<Runtime>);
            impl Drop for Active {
                fn drop(&mut self) {
                    self.0
                        .active
                        .fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
                    self.0.drained.notify_one();
                }
            }
            owner
                .active
                .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            let _active = Active(owner.clone());
            if owner.stopping.load(std::sync::atomic::Ordering::Acquire) {
                return Err("request_cancelled");
            }
            let lane = call.lane();
            let _request = owner.lanes.try_enter(lane)?;
            {
                let owner = owner.clone();
                let header = header.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let host = owner.initialize()?;
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| "request_expired")?
                        .as_millis() as u64;
                    session.authorize(&header, routes, now, |context| {
                        host.projects()
                            .and_then(|projects| projects.binding(context))
                            .is_ok()
                    })
                })
                .await
                .map_err(|_| "worker_unavailable")??;
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "request_expired")?
                .as_millis() as u64;
            let workers = owner.lanes.workers(lane);
            let _worker = tokio::time::timeout(
                std::time::Duration::from_millis(header.deadline_ms.saturating_sub(now)),
                workers.acquire_owned(),
            )
            .await
            .map_err(|_| "request_expired")?
            .map_err(|_| "request_cancelled")?;
            workspace_core::current_deadline(header.deadline_ms)?;
            match call {
                Parsed::Query(call) => {
                    let host = owner.initialize()?;
                    workspace_core::runtime_queries::execute(
                        &owner.app,
                        &host,
                        &owner.definitions,
                        call,
                        header.deadline_ms,
                    )
                    .await
                }
                Parsed::Session(call) => {
                    let host = owner.initialize()?;
                    owner.sessions.dispatch(&owner.app, &host, call).await
                }
                other => other
                    .execute(&owner.app)
                    .await
                    .map_err(workspace_core::runtime_policy::issue),
            }
        })
        .await
        .map_err(|_| "worker_unavailable")?
    }
    pub async fn shutdown(self: &Arc<Self>) {
        self.shutdown.run(|| self.drain()).await;
    }
    async fn drain(self: &Arc<Self>) {
        self.stopping
            .store(true, std::sync::atomic::Ordering::Release);
        runtime_engine::component::request_shutdown(&self.app);
        let owner = self.clone();
        // Join any cold initialization before draining the initialized owners.
        let _ = tauri::async_runtime::spawn_blocking(move || {
            drop(owner.state.lock());
        })
        .await;
        runtime_engine::component::request_shutdown(&self.app);
        while self.active.load(std::sync::atomic::Ordering::Acquire) != 0 {
            self.drained.notified().await;
        }
        let _ = runtime_engine::component::shutdown(&self.app).await;
        let _ = logs_engine::component::shutdown(&self.app).await;
    }
}
pub fn response(result: Result<Value>) -> Value {
    match result {
        Ok(value) => json!({"operation":{"outcome":{"state":"succeeded"}},"value":value}),
        Err(issue) => crate::routes::failure(issue),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(method: &str) -> Value {
        json!({"header":{"protocolVersion":1,"installationId":"native","sessionId":"session","requestId":"one","deadlineMs":2000,"route":"tasks"},"method":method,"args":{}})
    }
    #[tokio::test]
    async fn concurrent_shutdowns_join_one_owner_cleanup_before_returning() {
        let shutdown = Arc::new(Shutdown::default());
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let first = {
            let (shutdown, calls, entered, release) = (
                shutdown.clone(),
                calls.clone(),
                entered.clone(),
                release.clone(),
            );
            tokio::spawn(async move {
                shutdown
                    .run(|| async {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        entered.notify_one();
                        release.notified().await;
                    })
                    .await;
            })
        };
        entered.notified().await;
        let mut second = {
            let (shutdown, calls) = (shutdown.clone(), calls.clone());
            tokio::spawn(async move {
                shutdown
                    .run(|| async {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    })
                    .await;
            })
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut second)
                .await
                .is_err()
        );
        release.notify_one();
        first.await.unwrap();
        second.await.unwrap();
        shutdown
            .run(|| async { panic!("completed cleanup must not run again") })
            .await;
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
    #[test]
    fn session_start_requires_the_exact_live_context_and_cleanup_uses_retained_scope() {
        let scope = json!({"session":"development-session","context":{
            "projectId":"project","worktreeId":"tree","target":{"kind":"windows"},"revision":1
        }});
        let mut start = request("session_owner");
        start["header"]["route"] = json!("sessions");
        start["args"] = json!({"kind":"start","args":{
            "scope":scope,"reference":"11111111-1111-4111-8111-111111111111","operation":"operation","service":false
        }});
        assert!(matches!(
            decode("workspace.runtime", start.clone()),
            Err("stale_context")
        ));
        start["header"]["context"] = scope["context"].clone();
        assert!(decode("workspace.runtime", start.clone()).is_ok());
        let mut cleanup = request("session_owner");
        cleanup["args"] = json!({"kind":"cancel","args":{"scope":scope,"operation":"operation"}});
        assert!(decode("workspace.runtime", cleanup).is_ok());
        start["args"]["args"]["scope"]["session"] = json!("x".repeat(129));
        assert!(matches!(
            decode("workspace.runtime", start),
            Err("session_runtime_invalid")
        ));
    }
    #[test]
    fn runtime_wire_uses_typed_methods_and_keeps_ui_lifecycle_out_of_agent() {
        assert!(decode("workspace.runtime", request("list_jobs")).is_ok());
        for method in [
            "quit_app",
            "show_main_window",
            "hide_main_window",
            "set_startup_shortcut_enabled",
            "unknown",
        ] {
            assert!(decode("workspace.runtime", request(method)).is_err());
        }
        let mut bad = request("list_jobs");
        bad["args"] = json!({"path":"private"});
        assert!(decode("workspace.runtime", bad).is_err());
        assert!(decode("workspace.files", request("list_jobs")).is_err());
    }
}
