//! Workspace companion factory and immutable per-window context admission.
use crate::{host::Host, private_metadata::MetadataRoot};
use product_contract::{Handshake, ProjectContext, RouteRequest, SessionGuard};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tauri::{Manager, WebviewWindow};
use terminal_engine::component::TerminalOwner;

type Result<T> = std::result::Result<T, &'static str>;
const RECORDS: &str = "terminal-sessions.json";
const MAX_SESSIONS: usize = 4096;
const MAX_WINDOWS: usize = 8;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
#[ts(rename = "TerminalRecord")]
pub(crate) struct Record {
    id: String,
    context: Option<ProjectContext>,
    state: String,
    #[serde(default)]
    initial_layout_revision: Option<String>,
    #[serde(default)]
    restore_generation: u64,
    #[serde(default)]
    last_restore_operation: Option<String>,
    #[serde(default)]
    restore_only: bool,
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
    controls: crate::wsl_controls::Controls,
    pending_logs: Mutex<VecDeque<PendingLog>>,
    summons: Mutex<crate::core::terminal_commands::Summons>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
#[ts(rename = "PendingTerminalLog")]
pub(crate) struct PendingLog {
    id: String,
    source: logs_engine::core::SourceSpec,
    context: Option<ProjectContext>,
    #[serde(skip)]
    created: std::time::Instant,
}

#[derive(Clone)]
pub(crate) struct TerminalLease {
    owner: Arc<Terminals>,
    peer: Arc<Peer>,
    id: String,
    keys: Vec<String>,
}
impl TerminalLease {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn stop(&self, app: &tauri::AppHandle) -> Result<()> {
        self.owner.stop_owned(app, &self.id, Some(&self.peer))
    }
    pub(crate) fn ready(&self) -> Result<bool> {
        self.peer
            .terminal
            .restored(&self.keys)
            .map_err(|_| "terminal_restore_failed")
    }
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

fn require_peer_session(peer: &Peer, header: &RouteRequest) -> Result<()> {
    let guard = peer.guard.lock().map_err(|_| "terminal_owner_busy")?;
    let current = guard.handshake();
    if current.session_id != header.session_id
        || current.installation_id != header.installation_id
        || peer.record.context != header.context
    {
        return Err("terminal_request_denied");
    }
    Ok(())
}

/// Caller holds the same mutex that publishes `stopping` before PTY retirement.
/// Closed events from the retiring renderer must not persist a shrinking layout.
fn peer_layout(
    inner: &Inner,
    peer: &Arc<Peer>,
    header: &RouteRequest,
    method: &str,
    args: Value,
) -> Result<Value> {
    require_peer_session(peer, header)?;
    if !inner
        .peers
        .get(peer.terminal.window())
        .is_some_and(|current| Arc::ptr_eq(current, peer))
    {
        return Err("terminal_owner_changed");
    }
    let record = inner
        .records
        .iter()
        .find(|record| record.id == peer.record.id)
        .ok_or("terminal_session_missing")?;
    // Preparing is a current new renderer initializing its first layout.
    if method == "save_terminal_layout" && !matches!(record.state.as_str(), "preparing" | "active")
    {
        return Err("terminal_session_stopping");
    }
    crate::terminal_profiles::layout(&inner.root, &peer.record.id, method, args)
}

pub(crate) fn wsl_management(method: &str) -> bool {
    matches!(
        method,
        "dashboard_snapshot"
            | "docker_action"
            | "wsl_control_status"
            | "open_distro_terminal"
            | "open_wsl_file_in_log_lens"
            | "open_wsl_journal_in_log_lens"
    )
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
                            || record.restore_generation > 9_007_199_254_740_991
                            || record
                                .last_restore_operation
                                .as_ref()
                                .is_some_and(|value| !id(value))
                            || record
                                .initial_layout_revision
                                .as_ref()
                                .is_some_and(|revision| {
                                    revision.len() != 64
                                        || !revision.bytes().all(|byte| {
                                            byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
                                        })
                                })
                            || !ids.insert(&record.id)
                            || record
                                .context
                                .as_ref()
                                .is_some_and(|context| context.validate().is_err())
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
        terminal_engine::component::initialize(app, inner.root.path())
            .map_err(|_| "terminal_owner_unavailable")?;
        *selected = Some(inner);
        Ok(())
    }

    fn queue_log(
        &self,
        window: &WebviewWindow,
        context: Option<ProjectContext>,
        method: &str,
        args: Value,
    ) -> Result<Value> {
        use tauri::Emitter;
        let source = if method == "open_wsl_file_in_log_lens" {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct File {
                distro: String,
                wsl_path: String,
            }
            let input: File = parse(args)?;
            logs_engine::core::SourceSpec::WslFile {
                distro: input.distro,
                path: input.wsl_path,
            }
        } else {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Journal {
                distro: String,
                unit: Option<String>,
            }
            let input: Journal = parse(args)?;
            logs_engine::core::SourceSpec::WslJournal {
                distro: input.distro,
                unit: input.unit,
            }
        };
        source
            .validate()
            .map_err(|_| "terminal_log_source_invalid")?;
        let main = window
            .app_handle()
            .get_webview_window("main")
            .ok_or("terminal_main_unavailable")?;
        if context.is_some() && product_shell_tauri::workspace_context(&main)? != context {
            return Err("terminal_log_context_changed");
        }
        let id = {
            let mut pending = self
                .pending_logs
                .lock()
                .map_err(|_| "terminal_owner_busy")?;
            pending
                .retain(|request| request.created.elapsed() < std::time::Duration::from_secs(120));
            if let Some(existing) = pending
                .iter()
                .find(|request| request.source == source && request.context == context)
            {
                existing.id.clone()
            } else {
                if pending.len() >= 8 {
                    return Err("terminal_log_pending");
                }
                let id = uuid::Uuid::new_v4().simple().to_string();
                pending.push_back(PendingLog {
                    id: id.clone(),
                    source,
                    context: context.clone(),
                    created: std::time::Instant::now(),
                });
                id
            }
        };
        // Only a wake-up ID crosses the event; paths stay in the native
        // one-time queue until the authenticated main consumer requests it.
        window
            .app_handle()
            .emit_to("main", "workspace://terminal-log-ready", json!({"id":id}))
            .map_err(|_| "terminal_log_delivery_pending")?;
        main.show().map_err(|_| "terminal_main_unavailable")?;
        main.set_focus().map_err(|_| "terminal_main_unavailable")?;
        Ok(Value::Null)
    }

    pub(crate) fn manage(
        &self,
        window: &WebviewWindow,
        host: &Host,
        header: &RouteRequest,
        method: &str,
        args: Value,
    ) -> Result<Value> {
        if matches!(method, "read_terminal_log" | "ack_terminal_log") {
            let mut pending = self
                .pending_logs
                .lock()
                .map_err(|_| "terminal_owner_busy")?;
            pending
                .retain(|request| request.created.elapsed() < std::time::Duration::from_secs(120));
            if method == "ack_terminal_log" {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Ack {
                    id: String,
                }
                let ack: Ack = parse(args)?;
                if ack.id.len() != 32
                    || !ack
                        .id
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
                {
                    return Err("terminal_args_invalid");
                }
                pending.retain(|request| request.id != ack.id);
                return Ok(Value::Null);
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Empty {}
            parse::<Empty>(args)?;
            return Ok(json!(pending
                .iter()
                .find(
                    |request| request.context.is_none() || request.context == header.context
                )));
        }
        self.initialize(window.app_handle(), host)?;
        if matches!(
            method,
            "open_wsl_file_in_log_lens" | "open_wsl_journal_in_log_lens"
        ) {
            return self.queue_log(window, header.context.clone(), method, args);
        }
        if matches!(method, "docker_action" | "wsl_control_status") {
            return tauri::async_runtime::block_on(self.wsl_control(
                window.app_handle(),
                host,
                method,
                args,
                header.deadline_ms,
            ));
        }
        if method == "dashboard_snapshot" {
            return tauri::async_runtime::block_on(terminal_engine::component::dispatch(
                window.app_handle(),
                method,
                args,
            ))
            .map_err(|_| "wsl_snapshot_unavailable");
        }
        if method == "open_distro_terminal" {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Open {
                operation_id: String,
                distro: String,
            }
            let input: Open = parse(args)?;
            if !id(&input.operation_id)
                || devbox_wsl::distro::validate_distro_name(&input.distro).is_err()
            {
                return Err("terminal_args_invalid");
            }
            // Explicit terminal creation may start the selected distro. The actual
            // PTY factory still checks the window's project target before launch.
            let profile = serde_json::from_value(json!({"id":input.operation_id,"name":input.distro,"tabs":[{"id":"main","title":input.distro,"layout":"grid","paneKeys":["main"],"sizing":{"columns":[1.0],"rows":[1.0]}}],"panes":[{"key":"main","distro":input.distro,"multiplexer":"native"}],"activeTabId":"main","activePaneKey":"main"})).map_err(|_| "terminal_args_invalid")?;
            return self.open_prepared(window, host, header, &input.operation_id, Some(profile));
        }
        match method {
            "terminal_commands" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Empty {}
                parse::<Empty>(args)?;
                self.command_catalog(window.app_handle(), host)
            }
            "summon_terminal" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    operation_id: String,
                    terminal_id: String,
                    deadline_ms: u64,
                }
                let input: Input = parse(args)?;
                self.summon(
                    window.app_handle(),
                    &input.terminal_id,
                    header.context.as_ref(),
                    &input.operation_id,
                    input.deadline_ms,
                )
            }
            "restore_terminal" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    id: String,
                    operation_id: String,
                    expected_generation: u64,
                }
                let input: Input = parse(args)?;
                let (record, layout) = {
                    let selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
                    let inner = selected.as_ref().ok_or("terminal_owner_unavailable")?;
                    let record = inner
                        .records
                        .iter()
                        .find(|record| record.id == input.id)
                        .ok_or("terminal_session_missing")?
                        .clone();
                    if crate::core::terminal_commands::restore_generation(
                        record.restore_generation,
                        record.last_restore_operation.as_deref(),
                        &record.state,
                        input.expected_generation,
                        &input.operation_id,
                    )?
                    .is_none()
                    {
                        return Ok(json!(record));
                    }
                    let (layout, _) =
                        crate::terminal_profiles::read_layout(&inner.root, &input.id)?;
                    (record, layout)
                };
                let mut header = header.clone();
                header.context = record.context;
                self.open_window(
                    window,
                    host,
                    &header,
                    &input.id,
                    layout,
                    Some((&input.operation_id, input.expected_generation)),
                )
            }
            "open_terminal_profile" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    operation_id: String,
                    profile_id: String,
                    revision: String,
                }
                let input: Input = parse(args)?;
                let (profile, revision) =
                    self.prepare_profile(window.app_handle(), host, &input.profile_id)?;
                if revision != input.revision {
                    return Err("terminal_profile_changed");
                }
                self.open_prepared(window, host, header, &input.operation_id, Some(profile))
            }
            "list_workspace_profiles" | "save_workspace_profile" | "delete_workspace_profile" => {
                self.profiles(method, args)
            }
            "terminal_sessions" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Empty {}
                parse::<Empty>(args)?;
                let inner = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
                Ok(json!(inner
                    .as_ref()
                    .ok_or("terminal_owner_unavailable")?
                    .records
                    .iter()
                    .filter(|record| matches!(
                        record.state.as_str(),
                        "preparing" | "active" | "stopping"
                    ))
                    .chain(
                        inner
                            .as_ref()
                            .ok_or("terminal_owner_unavailable")?
                            .records
                            .iter()
                            .rev()
                            .filter(|record| !matches!(
                                record.state.as_str(),
                                "preparing" | "active" | "stopping"
                            ))
                    )
                    .take(64)
                    .collect::<Vec<_>>()))
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
                self.open_prepared(window, host, header, &input.operation_id, None)
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
                // Main selection can change; the explicit target still names its own retained context.
                if method == "focus_terminal" {
                    let target = window
                        .app_handle()
                        .get_webview_window(&label)
                        .ok_or("terminal_window_unavailable")?;
                    target.show().map_err(|_| "terminal_window_unavailable")?;
                    target
                        .unminimize()
                        .map_err(|_| "terminal_window_unavailable")?;
                    target
                        .set_focus()
                        .map_err(|_| "terminal_window_unavailable")?;
                } else {
                    self.stop_owned(window.app_handle(), &input.id, None)?;
                }
                Ok(Value::Null)
            }
            _ => Err("terminal_method_invalid"),
        }
    }

    fn stop_owned(
        &self,
        app: &tauri::AppHandle,
        session_id: &str,
        expected: Option<&Arc<Peer>>,
    ) -> Result<()> {
        let label = format!("terminal-{session_id}");
        {
            let selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
            let inner = selected.as_ref().ok_or("terminal_owner_unavailable")?;
            if inner
                .records
                .iter()
                .any(|record| record.id == session_id && record.state == "stopped")
            {
                return Ok(());
            }
        }
        let peer = self.peer(&label)?;
        if expected.is_some_and(|expected| !Arc::ptr_eq(expected, &peer)) {
            return Err("terminal_owner_changed");
        }
        {
            let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
            let inner = selected.as_mut().ok_or("terminal_owner_unavailable")?;
            inner
                .records
                .iter_mut()
                .find(|record| record.id == session_id)
                .ok_or("terminal_session_missing")?
                .state = "stopping".into();
            save(inner)?;
        }
        peer.terminal
            .stop(app)
            .map_err(|_| "terminal_retirement_pending")?;
        let ui_app = app.clone();
        let ui_label = label.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        app.run_on_main_thread(move || {
            let result = ui_app
                .get_webview_window(&ui_label)
                .map(|target| target.destroy().map_err(|_| "terminal_window_unavailable"))
                .unwrap_or(Ok(()));
            let _ = sender.send(result);
        })
        .map_err(|_| "terminal_window_unavailable")?;
        receiver
            .recv()
            .map_err(|_| "terminal_window_unavailable")??;
        let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
        let inner = selected.as_mut().ok_or("terminal_owner_unavailable")?;
        inner
            .records
            .iter_mut()
            .find(|record| record.id == session_id)
            .ok_or("terminal_session_missing")?
            .state = "stopped".into();
        save(inner)?;
        inner.peers.remove(&label);
        Ok(())
    }
    pub(crate) fn command_catalog(&self, app: &tauri::AppHandle, host: &Host) -> Result<Value> {
        self.initialize(app, host)?;
        let selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
        let inner = selected.as_ref().ok_or("terminal_owner_unavailable")?;
        let profiles =
            crate::terminal_profiles::dispatch(&inner.root, "list_workspace_profiles", json!({}))?;
        let profiles: Vec<terminal_engine::component::WorkspaceProfile> =
            serde_json::from_value(profiles["profiles"].clone())
                .map_err(|_| "terminal_profiles_invalid")?;
        let profiles = profiles
            .iter()
            .map(|profile| {
                let revision = crate::definitions::digest(
                    &serde_json::to_vec(profile).map_err(|_| "terminal_profile_invalid")?,
                );
                Ok(json!({"id":profile.id,"name":profile.name,"revision":revision}))
            })
            .collect::<Result<Vec<_>>>()?;
        let windows = inner
            .records
            .iter()
            .filter(|record| {
                record.state == "active"
                    && inner.peers.contains_key(&format!("terminal-{}", record.id))
            })
            .map(|record| json!({"id":record.id,"context":record.context,"generation":record.restore_generation}))
            .collect::<Vec<_>>();
        Ok(json!({"profiles":profiles,"windows":windows,"defaultShortcut":null}))
    }
    pub(crate) fn summon(
        &self,
        app: &tauri::AppHandle,
        terminal_id: &str,
        context: Option<&ProjectContext>,
        operation: &str,
        deadline: u64,
    ) -> Result<Value> {
        let label = format!("terminal-{terminal_id}");
        let peer = self.peer(&label)?;
        if peer.record.context.as_ref() != context {
            return Err("terminal_context_changed");
        }
        let target = app
            .get_webview_window(&label)
            .ok_or("terminal_window_unavailable")?;
        let hide = target
            .is_visible()
            .map_err(|_| "terminal_window_unavailable")?
            && crate::platform::terminal_focus::focused(&target)?
            && !target
                .is_minimized()
                .map_err(|_| "terminal_window_unavailable")?;
        // Separate from PTY locks. Serialize receipt reservation and idempotent window effects.
        let mut summons = self.summons.lock().map_err(|_| "terminal_owner_busy")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "terminal_command_expired")?
            .as_millis() as u64;
        let receipt = summons.reserve(operation, terminal_id, context, deadline, now, hide)?;
        if receipt.hide {
            target.hide().map_err(|_| "terminal_window_unavailable")?;
        } else {
            target.show().map_err(|_| "terminal_window_unavailable")?;
            target
                .unminimize()
                .map_err(|_| "terminal_window_unavailable")?;
            target
                .set_focus()
                .map_err(|_| "terminal_window_unavailable")?;
        }
        Ok(json!({"id":terminal_id,"visible":!receipt.hide}))
    }
    pub(crate) fn profile_choices(&self, app: &tauri::AppHandle, host: &Host) -> Result<Value> {
        self.initialize(app, host)?;
        let profiles = self.profiles("list_workspace_profiles", json!({}))?;
        let profiles = profiles["profiles"]
            .as_array()
            .ok_or("terminal_profiles_invalid")?;
        Ok(json!(profiles
            .iter()
            .map(|profile| json!({"id":profile["id"],"name":profile["name"]}))
            .collect::<Vec<_>>()))
    }
    pub(crate) fn owned_lease(
        self: &Arc<Self>,
        session_id: &str,
        keys: Vec<String>,
    ) -> Result<TerminalLease> {
        let peer = self.peer(&format!("terminal-{session_id}"))?;
        Ok(TerminalLease {
            owner: self.clone(),
            peer,
            id: session_id.into(),
            keys,
        })
    }

    pub(crate) async fn wsl_control(
        &self,
        app: &tauri::AppHandle,
        host: &Host,
        method: &str,
        args: Value,
        deadline: u64,
    ) -> Result<Value> {
        self.initialize(app, host)?;
        let root = MetadataRoot::open(&host.component("terminal")?)?;
        if method == "wsl_control_status" {
            return self.controls.status(&root, args);
        }
        self.controls.execute(&root, host, args, deadline).await
    }

    pub(crate) fn prepare_profile(
        &self,
        app: &tauri::AppHandle,
        host: &Host,
        id: &str,
    ) -> Result<(terminal_engine::component::WorkspaceProfile, String)> {
        self.initialize(app, host)?;
        let inner = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
        crate::terminal_profiles::snapshot(
            &inner.as_ref().ok_or("terminal_owner_unavailable")?.root,
            id,
        )
    }

    pub(crate) fn open_prepared(
        &self,
        window: &WebviewWindow,
        host: &Host,
        header: &RouteRequest,
        operation_id: &str,
        layout: Option<terminal_engine::component::WorkspaceProfile>,
    ) -> Result<Value> {
        self.open_window(window, host, header, operation_id, layout, None)
    }

    fn open_window(
        &self,
        window: &WebviewWindow,
        host: &Host,
        header: &RouteRequest,
        operation_id: &str,
        mut layout: Option<terminal_engine::component::WorkspaceProfile>,
        restoration: Option<(&str, u64)>,
    ) -> Result<Value> {
        self.initialize(window.app_handle(), host)?;
        if !id(operation_id) {
            return Err("terminal_operation_invalid");
        }
        if let Some(layout) = &mut layout {
            layout.id = operation_id.into();
            layout.validate().map_err(|_| "terminal_profile_invalid")?;
        }
        let initial_revision = layout
            .as_ref()
            .map(|layout| {
                serde_json::to_vec(layout)
                    .map(|bytes| crate::definitions::digest(&bytes))
                    .map_err(|_| "terminal_profile_invalid")
            })
            .transpose()?;
        let context = header.context.clone();
        if let Some(context) = &context {
            // Window creation is metadata-only. The launch factory retains
            // and revalidates the full project/distro lease at actual PTY start,
            // including an explicitly selected stopped WSL target.
            host.projects()?.binding(context)?;
        }
        crate::files_host::current_deadline(header.deadline_ms)?;
        let label = format!("terminal-{}", operation_id);
        {
            let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
            let inner = selected.as_mut().ok_or("terminal_owner_unavailable")?;
            let previous = inner
                .records
                .iter()
                .position(|record| record.id == operation_id);
            let prior = previous.map(|index| inner.records[index].clone());
            let record = if let Some((restore_operation, expected)) = restoration {
                let prior = prior.as_ref().ok_or("terminal_session_missing")?;
                let Some(generation) = crate::core::terminal_commands::restore_generation(
                    prior.restore_generation,
                    prior.last_restore_operation.as_deref(),
                    &prior.state,
                    expected,
                    restore_operation,
                )?
                else {
                    return Ok(json!(prior));
                };
                if prior.context != context || inner.peers.contains_key(&label) {
                    return Err("terminal_restore_conflict");
                }
                let (current, _) =
                    crate::terminal_profiles::read_layout(&inner.root, operation_id)?;
                if serde_json::to_value(current).map_err(|_| "terminal_profile_invalid")?
                    != serde_json::to_value(&layout).map_err(|_| "terminal_profile_invalid")?
                {
                    return Err("terminal_layout_changed");
                }
                Record {
                    state: "preparing".into(),
                    restore_generation: generation,
                    last_restore_operation: Some(restore_operation.into()),
                    restore_only: true,
                    ..prior.clone()
                }
            } else {
                if let Some(prior) = &prior {
                    if prior.context != context || prior.initial_layout_revision != initial_revision
                    {
                        return Err("terminal_operation_conflict");
                    }
                    return Ok(json!(prior));
                }
                if inner.records.len() >= MAX_SESSIONS {
                    return Err("terminal_session_limit");
                }
                Record {
                    id: operation_id.to_owned(),
                    context: context.clone(),
                    state: "preparing".into(),
                    initial_layout_revision: initial_revision.clone(),
                    restore_generation: 0,
                    last_restore_operation: None,
                    restore_only: false,
                }
            };
            if inner.peers.len() >= MAX_WINDOWS {
                return Err("terminal_session_limit");
            }
            let mut guard = SessionGuard::for_window(
                Handshake {
                    protocol_version: 1,
                    product: "workspace".into(),
                    installation_id: header.installation_id.clone(),
                    session_id: uuid::Uuid::new_v4().to_string(),
                },
                &label,
            )?;
            guard.bind_context(context.clone())?;
            let peer = Arc::new(Peer {
                record: record.clone(),
                guard: Mutex::new(guard),
                terminal: TerminalOwner::with_restore_only(label.clone(), record.restore_only)
                    .map_err(|_| "terminal_window_invalid")?,
            });
            if let Some(layout) = layout.as_ref().filter(|_| restoration.is_none()) {
                let (current, revision) =
                    crate::terminal_profiles::read_layout(&inner.root, operation_id)?;
                if current.is_some() {
                    return Err("terminal_layout_changed");
                }
                crate::terminal_profiles::layout(
                    &inner.root,
                    operation_id,
                    "save_terminal_layout",
                    json!({"expectedRevision":revision,"layout":layout}),
                )?;
            }
            if let Some(index) = previous {
                inner.records[index] = record;
            } else {
                inner.records.push(record);
            }
            // Persist operation identity before creating any window or process owner.
            if let Err(error) = save(inner) {
                if let (Some(index), Some(prior)) = (previous, prior) {
                    inner.records[index] = prior;
                } else {
                    inner.records.pop();
                }
                return Err(error);
            }
            inner.peers.insert(label.clone(), peer);
        }
        let app = window.app_handle().clone();
        let ui_app = app.clone();
        let ui_label = label.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let queued = app.run_on_main_thread(move || {
            let result = tauri::WebviewWindowBuilder::new(
                &ui_app,
                &ui_label,
                tauri::WebviewUrl::App(
                    format!(
                        "index.html?surface=terminal&id={}",
                        ui_label.trim_start_matches("terminal-")
                    )
                    .into(),
                ),
            )
            .title("Devbox Workspace · 터미널")
            .inner_size(1100.0, 760.0)
            .min_inner_size(640.0, 420.0)
            .build()
            .map(|_| ())
            .map_err(|_| "terminal_window_unavailable");
            let _ = sender.send(result);
        });
        // This is a retained blocking worker. Never time out and abandon a queued UI
        // creation that could later produce a window without its native owner.
        let result = if queued.is_ok() {
            receiver
                .recv()
                .unwrap_or(Err("terminal_window_unavailable"))
        } else {
            Err("terminal_window_unavailable")
        };
        let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
        let inner = selected.as_mut().ok_or("terminal_owner_unavailable")?;
        let record = inner
            .records
            .iter_mut()
            .find(|record| record.id == operation_id)
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
            json!({"handshake": guard.handshake(), "context": peer.record.context, "id": peer.record.id, "restoreOnly": peer.record.restore_only}),
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
        // Admission can precede a worker wait. The same window label may now
        // belong to a restored incarnation; never transfer an old request.
        require_peer_session(&peer, header)?;
        crate::files_host::current_deadline(header.deadline_ms)?;
        if method != "close_session" {
            if let Some(context) = &peer.record.context {
                host.projects()?.binding(context)?;
            }
        }
        if method == "terminal_window_policy" {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Empty {}
            parse::<Empty>(args)?;
            return Ok(json!({"shortcutRegistered":false,"activeShortcut":null,
                "trayEnabled":true,"closeBehavior":"hideToTray","issues":[],
                "visible":window.is_visible().map_err(|_|"terminal_window_unavailable")?,
                "focused":crate::platform::terminal_focus::focused(window)?}));
        }
        if matches!(
            method,
            "open_wsl_file_in_log_lens" | "open_wsl_journal_in_log_lens"
        ) {
            return self.queue_log(window, peer.record.context.clone(), method, args);
        }
        if matches!(
            method,
            "inspect_shell_integration" | "update_shell_integration"
        ) {
            let distro = args
                .get("distro")
                .and_then(Value::as_str)
                .ok_or("terminal_args_invalid")?;
            let lease =
                crate::platform::terminal_launch::capture_running(host, distro, header.deadline_ms)
                    .map_err(|_| "wsl_target_unavailable")?;
            let result = terminal_engine::component::shell_integration_owned(
                window.app_handle(),
                method,
                args,
                lease.as_ref(),
            )
            .await
            .map_err(|_| "terminal_shell_integration_failed");
            let retired = lease.retire().map_err(|_| "wsl_target_retirement_pending");
            return result.and_then(|value| retired.map(|_| value));
        }
        if matches!(method, "docker_action" | "wsl_control_status") {
            return self
                .wsl_control(window.app_handle(), host, method, args, header.deadline_ms)
                .await;
        }
        if matches!(method, "terminal_preferences" | "set_terminal_preference") {
            let selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
            return crate::terminal_profiles::preferences(
                &selected.as_ref().ok_or("terminal_owner_unavailable")?.root,
                method,
                args,
            );
        }
        if matches!(method, "terminal_layout" | "save_terminal_layout") {
            let selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
            let inner = selected.as_ref().ok_or("terminal_owner_unavailable")?;
            return peer_layout(inner, &peer, header, method, args);
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
        ) {
            return terminal_engine::component::dispatch(window.app_handle(), method, args)
                .await
                .map_err(|_| "terminal_operation_failed");
        }
        let factory = crate::platform::terminal_launch::Factory {
            host,
            context: peer.record.context.as_ref(),
            deadline: header.deadline_ms,
        };
        let value = peer
            .terminal
            .dispatch(
                window.app_handle(),
                window.label(),
                method,
                args,
                Some(&factory),
            )
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
        let peers: Vec<_> = {
            let mut selected = self.inner.lock().map_err(|_| "terminal_owner_busy")?;
            if let Some(inner) = selected.as_mut() {
                for record in &mut inner.records {
                    if inner.peers.contains_key(&format!("terminal-{}", record.id)) {
                        record.state = "stopping".into();
                    }
                }
                save(inner)?;
                inner.peers.values().cloned().collect()
            } else {
                Vec::new()
            }
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_peer(record: Record) -> (Arc<Peer>, RouteRequest) {
        let label = format!("terminal-{}", record.id);
        let handshake = Handshake {
            protocol_version: 1,
            product: "workspace".into(),
            installation_id: "fixture-installation".into(),
            session_id: uuid::Uuid::new_v4().to_string(),
        };
        let header = RouteRequest {
            protocol_version: 1,
            installation_id: handshake.installation_id.clone(),
            session_id: handshake.session_id.clone(),
            request_id: uuid::Uuid::new_v4().to_string(),
            deadline_ms: 1000,
            route: "terminal".into(),
            context: record.context.clone(),
        };
        let mut guard = SessionGuard::for_window(handshake, &label).unwrap();
        guard.bind_context(record.context.clone()).unwrap();
        (
            Arc::new(Peer {
                record,
                guard: Mutex::new(guard),
                terminal: TerminalOwner::new(label).unwrap(),
            }),
            header,
        )
    }

    #[test]
    fn retirement_preserves_layout_against_late_autosave_and_replaced_window() {
        let directory = tempfile::tempdir().unwrap();
        let root = MetadataRoot::open(directory.path()).unwrap();
        let record = Record {
            id: uuid::Uuid::new_v4().to_string(),
            context: None,
            state: "active".into(),
            initial_layout_revision: None,
            restore_generation: 0,
            last_restore_operation: None,
            restore_only: false,
        };
        let (peer, header) = make_peer(record.clone());
        let mut inner = Inner {
            root,
            records: vec![record.clone()],
            peers: HashMap::from([(peer.terminal.window().to_owned(), peer.clone())]),
        };
        let complete = json!({
            "id":record.id,"name":"Owned fixture",
            "tabs":[{"id":"main","title":"Main","layout":"grid","paneKeys":["one","two"],"sizing":{"columns":[0.5,0.5],"rows":[1.0]}}],
            "panes":[{"key":"one","distro":"Fixture"},{"key":"two","distro":"Fixture"}],
            "activeTabId":"main","activePaneKey":"two"
        });
        let (_, revision) = crate::terminal_profiles::read_layout(&inner.root, &record.id).unwrap();
        peer_layout(
            &inner,
            &peer,
            &header,
            "save_terminal_layout",
            json!({"expectedRevision":revision,"layout":complete}),
        )
        .unwrap();
        let retained = crate::terminal_profiles::read_layout(&inner.root, &record.id).unwrap();
        let shrinking = json!({
            "id":record.id,"name":"Owned fixture",
            "tabs":[{"id":"main","title":"Main","layout":"grid","paneKeys":["two"],"sizing":{"columns":[1.0],"rows":[1.0]}}],
            "panes":[{"key":"two","distro":"Fixture"}],
            "activeTabId":"main","activePaneKey":"two"
        });
        let late_save = json!({"expectedRevision":retained.1,"layout":shrinking});
        for state in ["stopping", "stopped", "interrupted"] {
            // The peer retains its original active Record, like a queued IPC.
            inner.records[0].state = state.into();
            assert_eq!(
                peer_layout(
                    &inner,
                    &peer,
                    &header,
                    "save_terminal_layout",
                    late_save.clone()
                )
                .unwrap_err(),
                "terminal_session_stopping"
            );
            assert_eq!(
                crate::terminal_profiles::read_layout(&inner.root, &record.id).unwrap(),
                retained
            );
        }

        inner.records[0].state = "preparing".into();
        inner.records[0].restore_generation = 1;
        let (restored, current_header) = make_peer(inner.records[0].clone());
        inner
            .peers
            .insert(restored.terminal.window().to_owned(), restored.clone());
        assert_eq!(
            peer_layout(
                &inner,
                &peer,
                &header,
                "save_terminal_layout",
                late_save.clone()
            )
            .unwrap_err(),
            "terminal_owner_changed"
        );
        // An old request may have passed admission before the worker was queued.
        assert_eq!(
            peer_layout(
                &inner,
                &restored,
                &header,
                "save_terminal_layout",
                late_save
            )
            .unwrap_err(),
            "terminal_request_denied"
        );
        assert_eq!(
            crate::terminal_profiles::read_layout(&inner.root, &record.id).unwrap(),
            retained
        );
        peer_layout(
            &inner,
            &restored,
            &current_header,
            "save_terminal_layout",
            json!({"expectedRevision":retained.1,"layout":complete}),
        )
        .unwrap();
        assert_eq!(
            crate::terminal_profiles::read_layout(&inner.root, &record.id)
                .unwrap()
                .0
                .unwrap()
                .panes
                .len(),
            2
        );
    }
}
