//! Workspace companion factory and immutable per-window context admission.
use crate::{host::Host, private_metadata::MetadataRoot};
use product_contract::{Handshake, ProjectContext, RouteRequest, SessionGuard};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tauri::{Manager, WebviewWindow};
use wsl_desktop_lib::component::TerminalOwner;

type Result<T> = std::result::Result<T, &'static str>;
const RECORDS: &str = "terminal-sessions.json";
const MAX_SESSIONS: usize = 64;
const MAX_WINDOWS: usize = 8;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    id: String,
    context: ProjectContext,
    state: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Store {
    schema_version: u32,
    records: Vec<Record>,
}
struct Inner {
    root: MetadataRoot,
    records: Vec<Record>,
    peers: HashMap<String, Arc<Peer>>,
}
struct Peer {
    record: Record,
    guard: Mutex<SessionGuard>,
    terminal: TerminalOwner,
}
#[derive(Default)]
pub(crate) struct Terminals {
    inner: Mutex<Option<Inner>>,
}

fn id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.to_string() == value)
}
fn local(window: &WebviewWindow) -> bool {
    window.url().is_ok_and(|url| {
        (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
            || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
            || (cfg!(debug_assertions)
                && url.scheme() == "http"
                && url.host_str() == Some("localhost")
                && url.port() == Some(1430))
    })
}
fn save(inner: &Inner) -> Result<()> {
    let bytes = serde_json::to_vec(&Store {
        schema_version: 1,
        records: inner.records.clone(),
    })
    .map_err(|_| "terminal_store_invalid")?;
    inner.root.write(RECORDS, &bytes)
}
fn parse<T: serde::de::DeserializeOwned>(args: Value) -> Result<T> {
    if !args.is_object() {
        return Err("terminal_args_invalid");
    }
    serde_json::from_value(args).map_err(|_| "terminal_args_invalid")
}

impl Terminals {
    fn initialize(&self, app: &tauri::AppHandle, host: &Host) -> Result<()> {
        let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
        if let Some(inner) = selected.as_ref() {
            if inner.root.path() != host.component("terminal")? {
                return Err("terminal_store_changed");
            }
            return inner.root.revalidate();
        }
        let root = MetadataRoot::open(&host.component("terminal")?)?;
        let mut records = match root.read(RECORDS)? {
            Some(bytes) => {
                let store: Store =
                    serde_json::from_slice(&bytes).map_err(|_| "terminal_store_invalid")?;
                let mut ids = std::collections::HashSet::new();
                if store.schema_version != 1
                    || store.records.len() > MAX_SESSIONS
                    || store.records.iter().any(|record| {
                        !id(&record.id)
                            || !ids.insert(&record.id)
                            || record.context.validate().is_err()
                            || !matches!(
                                record.state.as_str(),
                                "preparing" | "active" | "stopping" | "stopped" | "interrupted"
                            )
                    })
                {
                    return Err("terminal_store_invalid");
                }
                store.records
            }
            None => Vec::new(),
        };
        // Process restart never adopts stale PTY IDs or sends old start commands.
        for record in &mut records {
            if record.state != "stopped" {
                record.state = "interrupted".into();
            }
        }
        let inner = Inner {
            root,
            records,
            peers: HashMap::new(),
        };
        save(&inner)?;
        wsl_desktop_lib::component::initialize(app, inner.root.path())
            .map_err(|_| "terminal_owner_unavailable")?;
        *selected = Some(inner);
        Ok(())
    }

    pub(crate) fn manage(
        &self,
        window: &WebviewWindow,
        host: &Host,
        header: &RouteRequest,
        method: &str,
        args: Value,
    ) -> Result<Value> {
        self.initialize(window.app_handle(), host)?;
        match method {
            "list_workspace_profiles" | "save_workspace_profile" | "delete_workspace_profile" => {
                self.profiles(method, args)
            }
            "terminal_sessions" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Empty {}
                parse::<Empty>(args)?;
                let inner = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
                Ok(json!(
                    inner.as_ref().ok_or("terminal_owner_unavailable")?.records
                ))
            }
            "open_terminal" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Open {
                    operation_id: String,
                }
                let input: Open = parse(args)?;
                if !id(&input.operation_id) {
                    return Err("terminal_operation_invalid");
                }
                let context = header.context.as_ref().ok_or("project_required")?;
                host.projects()?
                    .admit_selection(host.helper_directory()?, context)?;
                crate::files_host::current_deadline(header.deadline_ms)?;
                let label = format!("terminal-{}", input.operation_id);
                {
                    let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
                    let inner = selected.as_mut().ok_or("terminal_owner_unavailable")?;
                    if let Some(record) = inner
                        .records
                        .iter()
                        .find(|record| record.id == input.operation_id)
                    {
                        if record.context != *context {
                            return Err("terminal_operation_conflict");
                        }
                        return Ok(json!(record));
                    }
                    if inner.records.len() >= MAX_SESSIONS || inner.peers.len() >= MAX_WINDOWS {
                        return Err("terminal_session_limit");
                    }
                    let record = Record {
                        id: input.operation_id.clone(),
                        context: context.clone(),
                        state: "preparing".into(),
                    };
                    let mut guard = SessionGuard::for_window(
                        Handshake {
                            protocol_version: 1,
                            product: "workspace".into(),
                            installation_id: header.installation_id.clone(),
                            session_id: uuid::Uuid::new_v4().to_string(),
                        },
                        &label,
                    )?;
                    guard.bind_context(Some(context.clone()))?;
                    let peer = Arc::new(Peer {
                        record: record.clone(),
                        guard: Mutex::new(guard),
                        terminal: TerminalOwner::new(label.clone())
                            .map_err(|_| "terminal_window_invalid")?,
                    });
                    inner.records.push(record);
                    // Persist operation identity before creating any window or process owner.
                    if let Err(error) = save(inner) {
                        inner.records.pop();
                        return Err(error);
                    }
                    inner.peers.insert(label.clone(), peer);
                }
                let app = window.app_handle().clone();
                let ui_app = app.clone();
                let ui_label = label.clone();
                let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                app.run_on_main_thread(move || {
                    let result = tauri::WebviewWindowBuilder::new(
                        &ui_app,
                        &ui_label,
                        tauri::WebviewUrl::App("index.html?surface=terminal".into()),
                    )
                    .title("Devbox Workspace · 터미널")
                    .inner_size(1100.0, 760.0)
                    .min_inner_size(640.0, 420.0)
                    .build()
                    .map(|_| ())
                    .map_err(|_| "terminal_window_unavailable");
                    let _ = sender.send(result);
                })
                .map_err(|_| "terminal_window_unavailable")?;
                // This is a retained blocking worker. Never time out and abandon a queued UI
                // creation that could later produce a window without its native owner.
                let result = receiver
                    .recv()
                    .unwrap_or(Err("terminal_window_unavailable"));
                let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
                let inner = selected.as_mut().ok_or("terminal_owner_unavailable")?;
                let record = inner
                    .records
                    .iter_mut()
                    .find(|record| record.id == input.operation_id)
                    .ok_or("terminal_session_missing")?;
                record.state = if result.is_ok() {
                    "active"
                } else {
                    "interrupted"
                }
                .into();
                let response = json!(record);
                if result.is_err() {
                    inner.peers.remove(&label);
                }
                save(inner)?;
                result?;
                Ok(response)
            }
            "focus_terminal" | "stop_terminal" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Selected {
                    id: String,
                }
                let input: Selected = parse(args)?;
                if !id(&input.id) {
                    return Err("terminal_session_missing");
                }
                let label = format!("terminal-{}", input.id);
                let peer = self.peer(&label)?;
                // Main selection can change; the explicit target still names its own retained context.
                if method == "focus_terminal" {
                    let target = window
                        .app_handle()
                        .get_webview_window(&label)
                        .ok_or("terminal_window_unavailable")?;
                    target.show().map_err(|_| "terminal_window_unavailable")?;
                    target
                        .set_focus()
                        .map_err(|_| "terminal_window_unavailable")?;
                } else {
                    {
                        let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
                        let inner = selected.as_mut().ok_or("terminal_owner_unavailable")?;
                        inner
                            .records
                            .iter_mut()
                            .find(|record| record.id == input.id)
                            .ok_or("terminal_session_missing")?
                            .state = "stopping".into();
                        save(inner)?;
                    }
                    peer.terminal
                        .stop(window.app_handle())
                        .map_err(|_| "terminal_retirement_pending")?;
                    let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
                    let inner = selected.as_mut().ok_or("terminal_owner_unavailable")?;
                    inner
                        .records
                        .iter_mut()
                        .find(|record| record.id == input.id)
                        .ok_or("terminal_session_missing")?
                        .state = "stopped".into();
                    save(inner)?;
                    inner.peers.remove(&label);
                    drop(selected);
                    if let Some(target) = window.app_handle().get_webview_window(&label) {
                        let _ = target.destroy();
                    }
                }
                Ok(Value::Null)
            }
            _ => Err("terminal_method_invalid"),
        }
    }

    fn profiles(&self, method: &str, args: Value) -> Result<Value> {
        let selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
        let inner = selected.as_ref().ok_or("terminal_owner_unavailable")?;
        crate::terminal_profiles::dispatch(&inner.root, method, args)
    }

    fn peer(&self, label: &str) -> Result<Arc<Peer>> {
        self.inner
            .lock()
            .map_err(|_| "terminal_owner_busy")?
            .as_ref()
            .and_then(|inner| inner.peers.get(label))
            .cloned()
            .ok_or("terminal_peer_denied")
    }

    pub(crate) fn describe(&self, window: &WebviewWindow) -> Result<Value> {
        if !local(window) {
            return Err("terminal_peer_denied");
        }
        let peer = self.peer(window.label())?;
        let guard = peer.guard.lock().map_err(|_| "terminal_owner_busy")?;
        Ok(
            json!({"handshake": guard.handshake(), "context": peer.record.context, "id": peer.record.id}),
        )
    }

    /// Authenticate on arrival, before waiting for a bounded execution worker.
    pub(crate) fn authorize(&self, window: &WebviewWindow, header: &RouteRequest) -> Result<()> {
        if !local(window) {
            return Err("terminal_peer_denied");
        }
        let peer = self.peer(window.label())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis() as u64;
        let result = peer
            .guard
            .lock()
            .map_err(|_| "terminal_owner_busy")?
            .authorize(window.label(), true, header, now, &["terminal"])
            .map_err(|_| "terminal_request_denied");
        result
    }

    pub(crate) async fn execute(
        &self,
        window: &WebviewWindow,
        host: &Host,
        header: &RouteRequest,
        method: &str,
        args: Value,
    ) -> Result<Value> {
        let peer = self.peer(window.label())?;
        crate::files_host::current_deadline(header.deadline_ms)?;
        if method != "close_session" {
            host.projects()?.binding(&peer.record.context)?;
        }
        if matches!(method, "terminal_layout" | "save_terminal_layout") {
            let selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
            let inner = selected.as_ref().ok_or("terminal_owner_unavailable")?;
            return crate::terminal_profiles::layout(&inner.root, &peer.record.id, method, args);
        }
        if method == "start_session" {
            {
                let selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
                let inner = selected.as_ref().ok_or("terminal_owner_unavailable")?;
                let (layout, _) =
                    crate::terminal_profiles::read_layout(&inner.root, &peer.record.id)?;
                let layout = layout.ok_or("terminal_layout_required")?;
                if !layout.panes.iter().any(|pane| {
                    args.get("paneKey").and_then(Value::as_str) == Some(&pane.key)
                        && args.get("distro").and_then(Value::as_str) == Some(&pane.distro)
                        && args.get("cwd").and_then(Value::as_str) == pane.cwd.as_deref()
                }) {
                    return Err("terminal_layout_changed");
                }
            }
            host.projects()?
                .admit_selection(host.helper_directory()?, &peer.record.context)?;
            crate::files_host::current_deadline(header.deadline_ms)?;
        }
        if matches!(
            method,
            "list_workspace_profiles" | "save_workspace_profile" | "delete_workspace_profile"
        ) {
            return self.profiles(method, args);
        }
        if matches!(
            method,
            "list_distros"
                | "dashboard_snapshot"
                | "docker_ps"
                | "detect_multiplexers"
                | "windows_build_number"
                | "inspect_shell_integration"
        ) {
            return wsl_desktop_lib::component::dispatch(window.app_handle(), method, args)
                .await
                .map_err(|_| "terminal_operation_failed");
        }
        let value = peer
            .terminal
            .dispatch(window.app_handle(), window.label(), method, args)
            .await
            .map_err(|error| match error.as_str() {
                "terminal_start_pending" => "terminal_start_pending",
                "terminal_start_interrupted" => "terminal_start_interrupted",
                "terminal_pane_conflict" => "terminal_pane_conflict",
                "terminal_retirement_pending" => "terminal_retirement_pending",
                _ => "terminal_operation_failed",
            })?;
        Ok(value)
    }

    pub(crate) fn owns(&self, label: &str) -> bool {
        self.peer(label).is_ok()
    }
    pub(crate) fn shutdown(&self, app: &tauri::AppHandle) -> Result<()> {
        let peers: Vec<_> = self
            .inner
            .lock()
            .map_err(|_| "terminal_owner_busy")?
            .as_ref()
            .map(|inner| inner.peers.values().cloned().collect())
            .unwrap_or_default();
        let mut pending = false;
        for peer in peers {
            pending |= peer.terminal.stop(app).is_err();
        }
        if pending {
            return Err("terminal_retirement_pending");
        }
        if let Some(inner) = self
            .inner
            .lock()
            .map_err(|_| "terminal_owner_busy")?
            .as_mut()
        {
            for record in &mut inner.records {
                if inner.peers.contains_key(&format!("terminal-{}", record.id)) {
                    record.state = "stopped".into();
                }
            }
            save(inner)?;
            inner.peers.clear();
        }
        Ok(())
    }
}
