//! Native-only, no-shell MCP stdio transport.
//!
//! Executable and cwd paths are selected by native dialogs and retained behind
//! opaque, expiring IDs. Resolved environment values and process output remain
//! backend-owned. Every cancellation, timeout, framing failure, or protocol
//! failure invalidates the connection and terminates its complete owned tree.

use super::mcp::{
    filter_reflected_list_definitions, interpret_exchange, project_result_for_ipc,
    sanitize_server_projection, McpConnectResult, McpInvokeResult, McpTimelineEntry,
};
use super::process_tree::ProcessTree;
use crate::commands::request::{
    is_sensitive_name, unseal_environment_value, EnvironmentVariable, Redactor,
};
use crate::core::mcp::{self, Era, EraPreference, RpcMessage, ServerProjection};
use crate::platform::platform_sealer;
use devbox_filesystem::{filesystem_identity, FilesystemIdentity};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri_plugin_dialog::DialogExt;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{watch, Mutex as AsyncMutex};
use zeroize::Zeroizing;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

const MAX_CONNECTIONS: usize = 8;
const MAX_SELECTIONS: usize = 32;
const SELECTION_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_ARGS: usize = 64;
const MAX_ARG_BYTES: usize = 8 * 1024;
const MAX_ARGS_BYTES: usize = 64 * 1024;
const MAX_ENV_BINDINGS: usize = 64;
const MAX_ENV_NAME_BYTES: usize = 256;
const MAX_ENV_BYTES: usize = 256 * 1024;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 120_000;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const MAX_STDERR_LINES: usize = 256;
const MAX_LINE_BYTES: usize = mcp::MAX_RESPONSE_BYTES;
const MAX_EXCHANGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_EXCHANGE_MESSAGES: usize = 1_000;
const MAX_LIST_PAGES: usize = 100;
const MAX_RETAINED_LIST_BYTES: usize = 16 * 1024 * 1024;
const GRACEFUL_SHUTDOWN: Duration = Duration::from_millis(500);
const STDERR_JOIN_TIMEOUT: Duration = Duration::from_millis(250);

const SELECTION_INVALID: &str = "mcp_stdio_selection_invalid";
const PROFILE_INVALID: &str = "mcp_stdio_profile_invalid";
const ENVIRONMENT_INVALID: &str = "mcp_stdio_environment_invalid";
const SPAWN_FAILED: &str = "mcp_stdio_spawn_failed";
const TRANSPORT_FAILED: &str = "mcp_stdio_transport_failed";
const PROTOCOL_INVALID: &str = "mcp_stdio_protocol_invalid";
const MESSAGE_TOO_LARGE: &str = "mcp_stdio_message_too_large";
const REQUEST_TIMEOUT: &str = "mcp_stdio_request_timeout";
const REQUEST_CANCELLED: &str = "mcp_stdio_request_cancelled";
const CONNECTION_STALE: &str = "mcp_stdio_connection_stale";
const CLEANUP_FAILED: &str = "mcp_stdio_cleanup_failed";
const CONNECTION_LIMIT: &str = "mcp_stdio_connection_limit";
const REQUEST_LIMIT: &str = "mcp_stdio_request_limit";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionKind {
    Executable,
    Directory,
}

impl SelectionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::Directory => "directory",
        }
    }
}

#[derive(Debug, Clone)]
struct StoredSelection {
    kind: SelectionKind,
    canonical: PathBuf,
    identity: FilesystemIdentity,
    expires_at: Instant,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpNativeSelection {
    selection_id: String,
    kind: &'static str,
    label: String,
    expires_at_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpStdioEnvironmentBinding {
    child_name: String,
    source_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpStdioProfile {
    executable_selection_id: String,
    cwd_selection_id: Option<String>,
    era: EraPreference,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    environment: Vec<McpStdioEnvironmentBinding>,
    timeout_ms: u64,
}

#[derive(Clone)]
struct ReviewedPath {
    canonical: PathBuf,
    identity: FilesystemIdentity,
    directory: bool,
}

struct PreparedStdioProfile {
    executable: ReviewedPath,
    cwd: Option<ReviewedPath>,
    args: Vec<String>,
    environment: Vec<(String, Zeroizing<String>)>,
    timeout: Duration,
    redactor: Arc<Redactor>,
}

struct ExchangeRequest<'a> {
    value: &'a Value,
    request_id: &'a str,
    era: Era,
    method: &'a str,
    params: &'a Value,
    timeout: Duration,
    redactor: &'a Redactor,
}

struct ResolvedEnvironment {
    values: Vec<(String, Zeroizing<String>)>,
    secrets: Vec<Zeroizing<String>>,
}

#[derive(Default)]
struct ExplorerTracker {
    seen: HashMap<String, BTreeSet<Vec<u8>>>,
    list_pages: HashMap<String, usize>,
    list_bytes: HashMap<String, usize>,
    list_cursors: HashMap<String, Option<String>>,
    used_cursors: HashMap<String, BTreeSet<Vec<u8>>>,
    tool_schemas: BTreeMap<String, Value>,
    prompt_schemas: BTreeMap<String, BTreeMap<String, bool>>,
}

struct StoredConnection {
    process: StdioProcess,
    server: ServerProjection,
    timeout: Duration,
    redactor: Arc<Redactor>,
    explorer: ExplorerTracker,
}

#[derive(Default)]
struct McpStdioInner {
    selections: HashMap<String, StoredSelection>,
    connections: HashMap<String, Arc<AsyncMutex<StoredConnection>>>,
    active: HashMap<(String, String), watch::Sender<bool>>,
    pending_connections: usize,
}

#[derive(Default)]
pub struct McpStdioState {
    inner: Mutex<McpStdioInner>,
}

struct ConnectionAttemptGuard<'a> {
    state: &'a McpStdioState,
}

impl Drop for ConnectionAttemptGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.state.inner.lock() {
            inner.pending_connections = inner.pending_connections.saturating_sub(1);
        }
    }
}

struct ActiveRequestGuard<'a> {
    state: &'a McpStdioState,
    connection_id: String,
    request_id: String,
}

impl Drop for ActiveRequestGuard<'_> {
    fn drop(&mut self) {
        self.state
            .finish_request(&self.connection_id, &self.request_id);
    }
}

impl McpStdioState {
    fn begin_connection_attempt(&self) -> Result<ConnectionAttemptGuard<'_>, &'static str> {
        let mut inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        if inner
            .connections
            .len()
            .saturating_add(inner.pending_connections)
            >= MAX_CONNECTIONS
        {
            return Err(CONNECTION_LIMIT);
        }
        inner.pending_connections = inner
            .pending_connections
            .checked_add(1)
            .ok_or(CONNECTION_LIMIT)?;
        Ok(ConnectionAttemptGuard { state: self })
    }

    fn store_selection(
        &self,
        selection: StoredSelection,
        label: String,
    ) -> Result<McpNativeSelection, &'static str> {
        let now = Instant::now();
        let mut inner = self.inner.lock().map_err(|_| SELECTION_INVALID)?;
        inner.selections.retain(|_, stored| stored.expires_at > now);
        if inner.selections.len() >= MAX_SELECTIONS {
            return Err(SELECTION_INVALID);
        }
        let kind = selection.kind;
        let remaining = selection.expires_at.saturating_duration_since(now);
        let remaining_ms = u64::try_from(remaining.as_millis()).map_err(|_| SELECTION_INVALID)?;
        let expires_at_ms = now_unix_ms()
            .map_err(|_| SELECTION_INVALID)?
            .checked_add(remaining_ms)
            .ok_or(SELECTION_INVALID)?;
        for _ in 0..4 {
            let id = random_hex_128().map_err(|_| SELECTION_INVALID)?;
            if !inner.selections.contains_key(&id) {
                inner.selections.insert(id.clone(), selection);
                return Ok(McpNativeSelection {
                    selection_id: id,
                    kind: kind.as_str(),
                    label,
                    expires_at_ms,
                });
            }
        }
        Err(SELECTION_INVALID)
    }

    fn reviewed_selection(
        &self,
        selection_id: &str,
        expected: SelectionKind,
    ) -> Result<ReviewedPath, &'static str> {
        validate_opaque_id(selection_id).map_err(|_| SELECTION_INVALID)?;
        let now = Instant::now();
        let mut inner = self.inner.lock().map_err(|_| SELECTION_INVALID)?;
        inner.selections.retain(|_, stored| stored.expires_at > now);
        let stored = inner
            .selections
            .get(selection_id)
            .filter(|stored| stored.kind == expected)
            .ok_or(SELECTION_INVALID)?;
        revalidate_reviewed_path(&stored.canonical, stored.identity, expected)
            .map_err(|_| SELECTION_INVALID)
    }

    fn insert_connection(
        &self,
        connection: StoredConnection,
    ) -> Result<String, Box<(&'static str, StoredConnection)>> {
        let mut inner = match self.inner.lock() {
            Ok(inner) => inner,
            Err(_) => return Err(Box::new((CONNECTION_STALE, connection))),
        };
        if inner.connections.len() >= MAX_CONNECTIONS {
            return Err(Box::new((CONNECTION_LIMIT, connection)));
        }
        let Some(id) = (0..4).find_map(|_| {
            random_hex_128()
                .ok()
                .filter(|candidate| !inner.connections.contains_key(candidate))
        }) else {
            return Err(Box::new((CONNECTION_LIMIT, connection)));
        };
        inner
            .connections
            .insert(id.clone(), Arc::new(AsyncMutex::new(connection)));
        Ok(id)
    }

    fn begin_request(
        &self,
        connection_id: &str,
        request_id: &str,
    ) -> Result<(Arc<AsyncMutex<StoredConnection>>, watch::Receiver<bool>), &'static str> {
        validate_opaque_id(connection_id).map_err(|_| CONNECTION_STALE)?;
        mcp::validate_request_id(request_id).map_err(|_| REQUEST_LIMIT)?;
        let mut inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        let connection = inner
            .connections
            .get(connection_id)
            .cloned()
            .ok_or(CONNECTION_STALE)?;
        let key = (connection_id.to_string(), request_id.to_string());
        if inner.active.keys().any(|(id, _)| id == connection_id) || inner.active.contains_key(&key)
        {
            return Err(REQUEST_LIMIT);
        }
        let (sender, receiver) = watch::channel(false);
        inner.active.insert(key, sender);
        Ok((connection, receiver))
    }

    fn finish_request(&self, connection_id: &str, request_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner
                .active
                .remove(&(connection_id.to_string(), request_id.to_string()));
        }
    }

    fn cancel_request(&self, connection_id: &str, request_id: &str) -> Result<bool, &'static str> {
        validate_opaque_id(connection_id).map_err(|_| CONNECTION_STALE)?;
        mcp::validate_request_id(request_id).map_err(|_| REQUEST_LIMIT)?;
        let inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        let Some(sender) = inner
            .active
            .get(&(connection_id.to_string(), request_id.to_string()))
        else {
            return Ok(false);
        };
        sender.send(true).map_err(|_| CONNECTION_STALE)?;
        Ok(true)
    }

    fn remove_connection(
        &self,
        connection_id: &str,
    ) -> Result<Arc<AsyncMutex<StoredConnection>>, &'static str> {
        validate_opaque_id(connection_id).map_err(|_| CONNECTION_STALE)?;
        let mut inner = self.inner.lock().map_err(|_| CONNECTION_STALE)?;
        for ((id, _), sender) in &inner.active {
            if id == connection_id {
                let _ = sender.send(true);
            }
        }
        inner.active.retain(|(id, _), _| id != connection_id);
        inner
            .connections
            .remove(connection_id)
            .ok_or(CONNECTION_STALE)
    }

    fn invalidate_connection(&self, connection_id: &str) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.connections.remove(connection_id);
            inner.active.retain(|(id, _), _| id != connection_id);
        }
    }
}

struct StdioProcess {
    child: Child,
    tree: ProcessTree,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr_ring: Arc<AsyncMutex<Zeroizing<Vec<u8>>>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
    terminated: bool,
}

impl StdioProcess {
    async fn spawn(profile: &PreparedStdioProfile) -> Result<Self, String> {
        revalidate_path(&profile.executable).map_err(|_| SELECTION_INVALID.to_string())?;
        if let Some(cwd) = &profile.cwd {
            revalidate_path(cwd).map_err(|_| SELECTION_INVALID.to_string())?;
        }
        let mut command = Command::new(&profile.executable.canonical);
        command
            .args(&profile.args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &profile.cwd {
            command.current_dir(&cwd.canonical);
        }
        for name in runtime_environment_allowlist() {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for (name, value) in &profile.environment {
            command.env(name, value.as_str());
        }
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(target_os = "windows")]
        command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);

        let mut child = command.spawn().map_err(|_| SPAWN_FAILED.to_string())?;
        let tree = match ProcessTree::assign(&child) {
            Ok(tree) => tree,
            Err(()) => {
                ProcessTree::terminate_unassigned(&mut child).await;
                return Err(SPAWN_FAILED.into());
            }
        };
        let Some(stdin) = child.stdin.take() else {
            return terminate_failed_spawn(child, tree).await;
        };
        let Some(stdout) = child.stdout.take() else {
            return terminate_failed_spawn(child, tree).await;
        };
        let Some(mut stderr) = child.stderr.take() else {
            return terminate_failed_spawn(child, tree).await;
        };
        let stderr_ring = Arc::new(AsyncMutex::new(Zeroizing::new(Vec::new())));
        let task_ring = Arc::clone(&stderr_ring);
        let stderr_redactor = Arc::clone(&profile.redactor);
        let stderr_task = tokio::spawn(async move {
            let mut buffer = Zeroizing::new([0_u8; 4096]);
            loop {
                let count = match stderr.read(&mut buffer[..]).await {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                let sanitized = sanitize_stderr_chunk(stderr_redactor.as_ref(), &buffer[..count]);
                let mut ring = task_ring.lock().await;
                append_stderr_ring(&mut ring, &sanitized);
            }
        });
        Ok(Self {
            child,
            tree,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
            stderr_ring,
            stderr_task: Some(stderr_task),
            terminated: false,
        })
    }

    async fn send(&mut self, value: &Value) -> Result<(), String> {
        let line = encode_json_line(value)?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| CONNECTION_STALE.to_string())?;
        stdin
            .write_all(&line)
            .await
            .map_err(|_| TRANSPORT_FAILED.to_string())?;
        stdin
            .flush()
            .await
            .map_err(|_| TRANSPORT_FAILED.to_string())
    }

    async fn exchange(
        &mut self,
        request: ExchangeRequest<'_>,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<super::mcp::InterpretedExchange, String> {
        if *cancellation.borrow() {
            return Err(REQUEST_CANCELLED.into());
        }
        let started = Instant::now();
        let exchange = self.exchange_unbounded(request.value, request.request_id, request.era);
        tokio::pin!(exchange);
        let messages = tokio::select! {
            biased;
            changed = cancellation.changed() => {
                let _ = changed;
                return Err(REQUEST_CANCELLED.into());
            }
            result = tokio::time::timeout(request.timeout, &mut exchange) => {
                result.map_err(|_| REQUEST_TIMEOUT.to_string())??
            }
        };
        interpret_exchange(
            messages,
            request.request_id,
            request.era,
            request.redactor,
            started,
            request.method,
            request.params,
        )
        .map_err(|_| PROTOCOL_INVALID.to_string())
    }

    async fn exchange_unbounded(
        &mut self,
        value: &Value,
        request_id: &str,
        era: Era,
    ) -> Result<Vec<Value>, String> {
        self.send(value).await?;
        let mut messages = Vec::new();
        let mut total_bytes = 0usize;
        loop {
            if messages.len() >= MAX_EXCHANGE_MESSAGES {
                return Err(MESSAGE_TOO_LARGE.into());
            }
            let (message, bytes) = read_json_line(&mut self.stdout).await?;
            total_bytes = total_bytes
                .checked_add(bytes)
                .ok_or_else(|| MESSAGE_TOO_LARGE.to_string())?;
            if total_bytes > MAX_EXCHANGE_BYTES {
                return Err(MESSAGE_TOO_LARGE.into());
            }
            let parsed = mcp::parse_rpc_message(message.clone(), request_id, era)
                .map_err(|_| PROTOCOL_INVALID.to_string())?;
            let complete = matches!(parsed, RpcMessage::Response { .. });
            messages.push(message);
            if complete {
                return Ok(messages);
            }
        }
    }

    async fn send_notification(&mut self, value: &Value, timeout: Duration) -> Result<(), String> {
        tokio::time::timeout(timeout, self.send(value))
            .await
            .map_err(|_| REQUEST_TIMEOUT.to_string())?
    }

    async fn cancel_and_terminate(&mut self, request_id: &str) -> bool {
        if let Ok(notification) = mcp::build_legacy_cancelled(request_id) {
            let _ =
                tokio::time::timeout(Duration::from_millis(200), self.send(&notification)).await;
        }
        self.terminate(false).await
    }

    async fn terminate(&mut self, graceful: bool) -> bool {
        if self.terminated {
            return true;
        }
        self.stdin.take();
        let gone = if graceful {
            match tokio::time::timeout(GRACEFUL_SHUTDOWN, self.child.wait()).await {
                Ok(Ok(_)) => self.tree.terminate_descendants(),
                Ok(Err(_)) | Err(_) => self.tree.terminate(&mut self.child).await,
            }
        } else {
            self.tree.terminate(&mut self.child).await
        };
        self.terminated = gone;
        self.finish_stderr().await;
        gone
    }

    async fn finish_stderr(&mut self) {
        let Some(mut task) = self.stderr_task.take() else {
            return;
        };
        if tokio::time::timeout(STDERR_JOIN_TIMEOUT, &mut task)
            .await
            .is_err()
        {
            task.abort();
            let _ = task.await;
        }
        let mut ring = self.stderr_ring.lock().await;
        ring.clear();
    }
}

async fn terminate_failed_spawn(
    mut child: Child,
    mut tree: ProcessTree,
) -> Result<StdioProcess, String> {
    let _ = tree.terminate(&mut child).await;
    Err(SPAWN_FAILED.into())
}

enum ModernConnectFailure {
    Fallback,
    Error(String),
}

struct NegotiatedConnection {
    process: StdioProcess,
    server: ServerProjection,
    timeline: Vec<McpTimelineEntry>,
}

#[tauri::command]
pub async fn pick_mcp_stdio_executable(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<McpStdioState>>,
) -> Result<Option<McpNativeSelection>, String> {
    pick_native_selection(app, state.inner().as_ref(), SelectionKind::Executable).await
}

#[tauri::command]
pub async fn pick_mcp_stdio_cwd(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<McpStdioState>>,
) -> Result<Option<McpNativeSelection>, String> {
    pick_native_selection(app, state.inner().as_ref(), SelectionKind::Directory).await
}

async fn pick_native_selection(
    app: tauri::AppHandle,
    state: &McpStdioState,
    kind: SelectionKind,
) -> Result<Option<McpNativeSelection>, String> {
    let selected = tauri::async_runtime::spawn_blocking(move || match kind {
        SelectionKind::Executable => app.dialog().file().blocking_pick_file(),
        SelectionKind::Directory => app.dialog().file().blocking_pick_folder(),
    })
    .await
    .map_err(|_| SELECTION_INVALID.to_string())?;
    let Some(selected) = selected else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| SELECTION_INVALID.to_string())?;
    let (stored, label) =
        tauri::async_runtime::spawn_blocking(move || build_stored_selection(&path, kind))
            .await
            .map_err(|_| SELECTION_INVALID.to_string())??;
    state
        .store_selection(stored, label)
        .map(Some)
        .map_err(ToOwned::to_owned)
}

#[tauri::command]
pub async fn connect_mcp_stdio(
    state: tauri::State<'_, Arc<McpStdioState>>,
    profile: McpStdioProfile,
    environment: Vec<EnvironmentVariable>,
) -> Result<McpConnectResult, String> {
    let state = state.inner().as_ref();
    let _attempt = state
        .begin_connection_attempt()
        .map_err(ToOwned::to_owned)?;
    let preference = profile.era;
    let prepared = prepare_profile(state, profile, environment)?;
    let negotiated = match preference {
        EraPreference::Modern => connect_modern(&prepared, false)
            .await
            .map_err(modern_failure_code)?,
        EraPreference::Legacy => connect_legacy(&prepared).await?,
        EraPreference::Auto => match connect_modern(&prepared, true).await {
            Ok(connected) => connected,
            Err(ModernConnectFailure::Fallback) => connect_legacy(&prepared).await?,
            Err(ModernConnectFailure::Error(code)) => return Err(code),
        },
    };
    let server = negotiated.server.clone();
    let timeline = negotiated.timeline.clone();
    let connection = StoredConnection {
        process: negotiated.process,
        server: negotiated.server,
        timeout: prepared.timeout,
        redactor: Arc::clone(&prepared.redactor),
        explorer: ExplorerTracker::default(),
    };
    let connection_id = match state.insert_connection(connection) {
        Ok(id) => id,
        Err(failure) => {
            let (code, mut connection) = *failure;
            return if connection.process.terminate(false).await {
                Err(code.to_string())
            } else {
                Err(CLEANUP_FAILED.into())
            };
        }
    };
    Ok(McpConnectResult {
        connection_id,
        server,
        session_managed: false,
        timeline,
    })
}

#[tauri::command]
pub async fn invoke_mcp_stdio(
    state: tauri::State<'_, Arc<McpStdioState>>,
    connection_id: String,
    request_id: String,
    method: String,
    params: Value,
) -> Result<McpInvokeResult, String> {
    mcp::validate_operation(&method, &params).map_err(ToOwned::to_owned)?;
    let requested_cursor = params
        .get("cursor")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let state = state.inner().as_ref();
    let (connection, mut cancellation) = state
        .begin_request(&connection_id, &request_id)
        .map_err(ToOwned::to_owned)?;
    let _request = ActiveRequestGuard {
        state,
        connection_id: connection_id.clone(),
        request_id: request_id.clone(),
    };

    let mut connection = connection.lock().await;
    connection
        .explorer
        .validate_list_request(&method, &params)?;
    if !mcp::has_capability(&connection.server.capabilities, &method) {
        return Err(mcp::CAPABILITY_UNAVAILABLE.into());
    }
    connection.explorer.validate_arguments(&method, &params)?;
    let era = connection.server.era;
    let request = match era {
        Era::Modern => mcp::build_modern_request(&request_id, &method, params.clone()),
        Era::Legacy => mcp::build_legacy_request(&request_id, &method, params.clone()),
    }
    .map_err(ToOwned::to_owned)?;
    let timeout = connection.timeout;
    let redactor = Arc::clone(&connection.redactor);
    let exchange = connection
        .process
        .exchange(
            ExchangeRequest {
                value: &request,
                request_id: &request_id,
                era,
                method: &method,
                params: &params,
                timeout,
                redactor: redactor.as_ref(),
            },
            &mut cancellation,
        )
        .await;
    let interpreted = match exchange {
        Ok(interpreted) => interpreted,
        Err(code) => {
            let cleanup = if code == REQUEST_CANCELLED || code == REQUEST_TIMEOUT {
                connection.process.cancel_and_terminate(&request_id).await
            } else {
                connection.process.terminate(false).await
            };
            drop(connection);
            state.invalidate_connection(&connection_id);
            return if cleanup {
                Err(code)
            } else {
                Err(CLEANUP_FAILED.into())
            };
        }
    };
    match finalize_invoke(
        &mut connection,
        &method,
        requested_cursor.as_deref(),
        era,
        interpreted,
    ) {
        Ok(invoke) => Ok(invoke),
        Err(code) => {
            let cleanup = connection.process.terminate(false).await;
            drop(connection);
            state.invalidate_connection(&connection_id);
            Err(if cleanup { code } else { CLEANUP_FAILED.into() })
        }
    }
}

fn finalize_invoke(
    connection: &mut StoredConnection,
    method: &str,
    requested_cursor: Option<&str>,
    era: Era,
    interpreted: super::mcp::InterpretedExchange,
) -> Result<McpInvokeResult, String> {
    let mut invoke = match interpreted.final_result {
        Ok(result) => {
            mcp::validate_operation_result(method, &result, era)
                .map_err(|_| PROTOCOL_INVALID.to_string())?;
            let (result, rejected) =
                filter_reflected_list_definitions(&result, method, connection.redactor.as_ref())
                    .map_err(|code| map_stdio_result_error(&code))?;
            if rejected > 0 {
                eprintln!("mcp stdio: excluded {rejected} reflected definitions");
            }
            connection
                .explorer
                .update_list_result(method, &result, requested_cursor)
                .map_err(|code| map_stdio_result_error(&code))?;
            let next_cursor = list_method(method)
                .then(|| result.get("nextCursor").and_then(Value::as_str))
                .flatten()
                .map(ToOwned::to_owned);
            let projected = project_result_for_ipc(connection.redactor.as_ref(), &result, method)
                .map_err(|code| map_stdio_result_error(&code))?;
            mcp::validate_json(&projected, mcp::MAX_RESPONSE_BYTES)
                .map_err(map_stdio_result_error)?;
            McpInvokeResult {
                result: Some(projected),
                error_code: None,
                rpc_error_code: None,
                next_cursor,
                timeline: interpreted.timeline,
            }
        }
        Err(error) => McpInvokeResult {
            result: None,
            error_code: Some(map_rpc_error(error.code).into()),
            rpc_error_code: Some(error.code),
            next_cursor: None,
            timeline: interpreted.timeline,
        },
    };
    let timeline = std::mem::take(&mut invoke.timeline);
    let timeline_value =
        serde_json::to_value(&timeline).map_err(|_| PROTOCOL_INVALID.to_string())?;
    mcp::validate_json(&timeline_value, MAX_EXCHANGE_BYTES).map_err(map_stdio_result_error)?;
    invoke.timeline = timeline;
    Ok(invoke)
}

fn map_stdio_result_error(code: &str) -> String {
    if matches!(code, mcp::RESPONSE_TOO_LARGE | mcp::REQUEST_TOO_LARGE) {
        MESSAGE_TOO_LARGE.into()
    } else {
        PROTOCOL_INVALID.into()
    }
}

#[tauri::command]
pub fn cancel_mcp_stdio(
    state: tauri::State<'_, Arc<McpStdioState>>,
    connection_id: String,
    request_id: String,
) -> Result<bool, String> {
    state
        .cancel_request(&connection_id, &request_id)
        .map_err(ToOwned::to_owned)
}

#[tauri::command]
pub async fn disconnect_mcp_stdio(
    state: tauri::State<'_, Arc<McpStdioState>>,
    connection_id: String,
) -> Result<(), String> {
    let connection = state
        .remove_connection(&connection_id)
        .map_err(ToOwned::to_owned)?;
    let mut connection = connection.lock().await;
    if connection.process.terminate(true).await {
        Ok(())
    } else {
        Err(CLEANUP_FAILED.into())
    }
}

async fn connect_modern(
    profile: &PreparedStdioProfile,
    allow_fallback: bool,
) -> Result<NegotiatedConnection, ModernConnectFailure> {
    let request_id = "discover-1";
    let params = json!({});
    let request = mcp::build_modern_request(request_id, "server/discover", params.clone())
        .map_err(|code| ModernConnectFailure::Error(code.into()))?;
    let mut process = StdioProcess::spawn(profile)
        .await
        .map_err(ModernConnectFailure::Error)?;
    let (_sender, mut cancellation) = watch::channel(false);
    let interpreted = process
        .exchange(
            ExchangeRequest {
                value: &request,
                request_id,
                era: Era::Modern,
                method: "server/discover",
                params: &params,
                timeout: profile.timeout,
                redactor: profile.redactor.as_ref(),
            },
            &mut cancellation,
        )
        .await;
    let interpreted = match interpreted {
        Ok(value) => value,
        Err(code) => {
            let cleanup = process.terminate(false).await;
            if !cleanup {
                return Err(ModernConnectFailure::Error(CLEANUP_FAILED.into()));
            }
            if allow_fallback && code == REQUEST_TIMEOUT {
                return Err(ModernConnectFailure::Fallback);
            }
            return Err(ModernConnectFailure::Error(code));
        }
    };
    match interpreted.final_result {
        Ok(result) => {
            let server = mcp::project_discover(&result)
                .map_err(ToOwned::to_owned)
                .and_then(|server| sanitize_server_projection(profile.redactor.as_ref(), server));
            let server = match server {
                Ok(server) => server,
                Err(code) => {
                    return if process.terminate(false).await {
                        Err(ModernConnectFailure::Error(code))
                    } else {
                        Err(ModernConnectFailure::Error(CLEANUP_FAILED.into()))
                    };
                }
            };
            Ok(NegotiatedConnection {
                process,
                server,
                timeline: interpreted.timeline,
            })
        }
        Err(error) => {
            let fallback = allow_fallback
                && (mcp::is_modern_method_not_found(&error)
                    || (error.code == -32022 && {
                        let versions = mcp::supported_versions_from_error(&error);
                        !versions.iter().any(|value| value == mcp::MODERN_VERSION)
                            && versions.iter().any(|value| value == mcp::LEGACY_VERSION)
                    }));
            let cleanup = process.terminate(false).await;
            if !cleanup {
                Err(ModernConnectFailure::Error(CLEANUP_FAILED.into()))
            } else if fallback {
                Err(ModernConnectFailure::Fallback)
            } else {
                Err(ModernConnectFailure::Error(
                    map_rpc_error(error.code).into(),
                ))
            }
        }
    }
}

async fn connect_legacy(profile: &PreparedStdioProfile) -> Result<NegotiatedConnection, String> {
    let request_id = "initialize-1";
    let params = json!({});
    let request = mcp::build_legacy_initialize(request_id).map_err(ToOwned::to_owned)?;
    let mut process = StdioProcess::spawn(profile).await?;
    let (_sender, mut cancellation) = watch::channel(false);
    let interpreted = process
        .exchange(
            ExchangeRequest {
                value: &request,
                request_id,
                era: Era::Legacy,
                method: "initialize",
                params: &params,
                timeout: profile.timeout,
                redactor: profile.redactor.as_ref(),
            },
            &mut cancellation,
        )
        .await;
    let interpreted = match interpreted {
        Ok(value) => value,
        Err(code) => {
            return terminate_connect_error(process, code).await;
        }
    };
    let result = match interpreted.final_result {
        Ok(value) => value,
        Err(_) => return terminate_connect_error(process, mcp::VERSION_UNSUPPORTED.into()).await,
    };
    let server = match mcp::project_legacy_initialize(&result)
        .map_err(ToOwned::to_owned)
        .and_then(|server| sanitize_server_projection(profile.redactor.as_ref(), server))
    {
        Ok(server) => server,
        Err(code) => return terminate_connect_error(process, code).await,
    };
    let initialized = mcp::build_legacy_initialized();
    if let Err(code) = process
        .send_notification(&initialized, profile.timeout.min(Duration::from_secs(2)))
        .await
    {
        return terminate_connect_error(process, code).await;
    }
    let mut timeline = interpreted.timeline;
    let sequence = match u32::try_from(timeline.len() + 1) {
        Ok(sequence) => sequence,
        Err(_) => {
            return terminate_connect_error(process, mcp::RESPONSE_TOO_LARGE.into()).await;
        }
    };
    timeline.push(McpTimelineEntry {
        sequence,
        offset_ms: 0,
        direction: "outgoing".into(),
        kind: "notification".into(),
        method: Some("notifications/initialized".into()),
        request_id: None,
        payload: None,
    });
    Ok(NegotiatedConnection {
        process,
        server,
        timeline,
    })
}

async fn terminate_connect_error(
    mut process: StdioProcess,
    code: String,
) -> Result<NegotiatedConnection, String> {
    if process.terminate(false).await {
        Err(code)
    } else {
        Err(CLEANUP_FAILED.into())
    }
}

fn modern_failure_code(failure: ModernConnectFailure) -> String {
    match failure {
        ModernConnectFailure::Fallback => mcp::VERSION_UNSUPPORTED.into(),
        ModernConnectFailure::Error(code) => code,
    }
}

fn prepare_profile(
    state: &McpStdioState,
    profile: McpStdioProfile,
    environment: Vec<EnvironmentVariable>,
) -> Result<PreparedStdioProfile, String> {
    validate_profile_shape(&profile)?;
    let executable = state
        .reviewed_selection(&profile.executable_selection_id, SelectionKind::Executable)
        .map_err(ToOwned::to_owned)?;
    let cwd = profile
        .cwd_selection_id
        .as_deref()
        .map(|id| state.reviewed_selection(id, SelectionKind::Directory))
        .transpose()
        .map_err(ToOwned::to_owned)?;
    let sealer = platform_sealer();
    let resolved = resolve_environment(&profile.environment, &environment, sealer.as_ref())?;
    Ok(PreparedStdioProfile {
        executable,
        cwd,
        args: profile.args,
        environment: resolved.values,
        timeout: Duration::from_millis(profile.timeout_ms),
        redactor: Arc::new(Redactor::from_secrets(resolved.secrets)),
    })
}

fn validate_profile_shape(profile: &McpStdioProfile) -> Result<(), String> {
    validate_opaque_id(&profile.executable_selection_id)
        .map_err(|_| PROFILE_INVALID.to_string())?;
    if let Some(cwd) = &profile.cwd_selection_id {
        validate_opaque_id(cwd).map_err(|_| PROFILE_INVALID.to_string())?;
    }
    if profile.timeout_ms < MIN_TIMEOUT_MS
        || profile.timeout_ms > MAX_TIMEOUT_MS
        || profile.args.len() > MAX_ARGS
        || profile.environment.len() > MAX_ENV_BINDINGS
    {
        return Err(PROFILE_INVALID.into());
    }
    let mut total = 0usize;
    for arg in &profile.args {
        if arg.len() > MAX_ARG_BYTES || arg.contains('\0') || arg.chars().any(char::is_control) {
            return Err(PROFILE_INVALID.into());
        }
        total = total
            .checked_add(arg.len())
            .ok_or_else(|| PROFILE_INVALID.to_string())?;
        if total > MAX_ARGS_BYTES {
            return Err(PROFILE_INVALID.into());
        }
    }
    Ok(())
}

fn resolve_environment(
    bindings: &[McpStdioEnvironmentBinding],
    environment: &[EnvironmentVariable],
    sealer: &dyn devbox_secrets::Sealer,
) -> Result<ResolvedEnvironment, String> {
    if bindings.len() > MAX_ENV_BINDINGS || environment.len() > 1_024 {
        return Err(ENVIRONMENT_INVALID.into());
    }
    let mut sources = HashMap::new();
    for variable in environment {
        if variable.key.is_empty()
            || variable.key.len() > MAX_ENV_NAME_BYTES
            || variable.key.chars().any(char::is_control)
            || variable.value.len() > mcp::MAX_JSON_STRING_BYTES
            || sources
                .insert(variable.key.to_ascii_uppercase(), variable)
                .is_some()
        {
            return Err(ENVIRONMENT_INVALID.into());
        }
    }
    let protected = runtime_environment_allowlist()
        .iter()
        .map(|name| name.to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    let mut child_names = BTreeSet::new();
    let mut source_names = BTreeSet::new();
    let mut resolved = Vec::with_capacity(bindings.len());
    let mut secrets = Vec::new();
    let mut total = 0usize;
    for binding in bindings {
        if !valid_environment_name(&binding.child_name)
            || binding.source_name.is_empty()
            || binding.source_name.len() > MAX_ENV_NAME_BYTES
            || binding.source_name.chars().any(char::is_control)
            || protected.contains(&binding.child_name.to_ascii_uppercase())
            || !child_names.insert(binding.child_name.to_ascii_uppercase())
            || !source_names.insert(binding.source_name.to_ascii_uppercase())
        {
            return Err(ENVIRONMENT_INVALID.into());
        }
        let variable = sources
            .get(&binding.source_name.to_ascii_uppercase())
            .ok_or_else(|| ENVIRONMENT_INVALID.to_string())?;
        let value = if variable.secret {
            unseal_environment_value(variable, sealer)
                .map_err(|_| ENVIRONMENT_INVALID.to_string())?
        } else {
            Zeroizing::new(variable.value.clone())
        };
        if value.chars().any(char::is_control) || (variable.secret && value.is_empty()) {
            return Err(ENVIRONMENT_INVALID.into());
        }
        total = total
            .checked_add(binding.child_name.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .ok_or_else(|| ENVIRONMENT_INVALID.to_string())?;
        if total > MAX_ENV_BYTES {
            return Err(ENVIRONMENT_INVALID.into());
        }
        if variable.secret
            || is_sensitive_name(&binding.child_name)
            || is_sensitive_name(&binding.source_name)
        {
            secrets.push(Zeroizing::new(value.to_string()));
        }
        resolved.push((binding.child_name.clone(), value));
    }
    Ok(ResolvedEnvironment {
        values: resolved,
        secrets,
    })
}

fn valid_environment_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ENV_NAME_BYTES
        && value.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_alphabetic() || (index > 0 && byte.is_ascii_digit())
        })
}

fn runtime_environment_allowlist() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &[
            "PATH",
            "PATHEXT",
            "SYSTEMROOT",
            "WINDIR",
            "COMSPEC",
            "TEMP",
            "TMP",
        ]
    }
    #[cfg(not(target_os = "windows"))]
    {
        &["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE"]
    }
}

fn build_stored_selection(
    path: &Path,
    kind: SelectionKind,
) -> Result<(StoredSelection, String), String> {
    let directory = kind == SelectionKind::Directory;
    let identity =
        filesystem_identity(path, directory).map_err(|_| SELECTION_INVALID.to_string())?;
    let canonical = path
        .canonicalize()
        .map_err(|_| SELECTION_INVALID.to_string())?;
    if filesystem_identity(&canonical, directory).map_err(|_| SELECTION_INVALID.to_string())?
        != identity
    {
        return Err(SELECTION_INVALID.into());
    }
    #[cfg(unix)]
    if kind == SelectionKind::Executable {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&canonical).map_err(|_| SELECTION_INVALID.to_string())?;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(SELECTION_INVALID.into());
        }
    }
    let label = safe_selection_label(&canonical, kind);
    let expires_at = Instant::now()
        .checked_add(SELECTION_TTL)
        .ok_or_else(|| SELECTION_INVALID.to_string())?;
    Ok((
        StoredSelection {
            kind,
            canonical,
            identity,
            expires_at,
        },
        label,
    ))
}

fn revalidate_reviewed_path(
    canonical: &Path,
    identity: FilesystemIdentity,
    kind: SelectionKind,
) -> Result<ReviewedPath, ()> {
    let directory = kind == SelectionKind::Directory;
    if filesystem_identity(canonical, directory).map_err(|_| ())? != identity
        || canonical.canonicalize().map_err(|_| ())? != canonical
    {
        return Err(());
    }
    Ok(ReviewedPath {
        canonical: canonical.to_path_buf(),
        identity,
        directory,
    })
}

fn revalidate_path(path: &ReviewedPath) -> Result<(), ()> {
    if filesystem_identity(&path.canonical, path.directory).map_err(|_| ())? != path.identity
        || path.canonical.canonicalize().map_err(|_| ())? != path.canonical
    {
        Err(())
    } else {
        Ok(())
    }
}

fn safe_selection_label(path: &Path, kind: SelectionKind) -> String {
    let fallback = match kind {
        SelectionKind::Executable => "Selected executable",
        SelectionKind::Directory => "Selected directory",
    };
    let label = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback);
    if label.is_empty() || label.len() > 256 || label.chars().any(char::is_control) {
        fallback.into()
    } else {
        label.into()
    }
}

fn encode_json_line(value: &Value) -> Result<Vec<u8>, String> {
    mcp::validate_json(value, mcp::MAX_REQUEST_BYTES).map_err(ToOwned::to_owned)?;
    let mut bytes = serde_json::to_vec(value).map_err(|_| PROTOCOL_INVALID.to_string())?;
    if bytes.len() > mcp::MAX_REQUEST_BYTES || bytes.contains(&b'\n') || bytes.contains(&b'\r') {
        return Err(PROTOCOL_INVALID.into());
    }
    bytes.push(b'\n');
    Ok(bytes)
}

fn sanitize_stderr_chunk(redactor: &Redactor, bytes: &[u8]) -> Zeroizing<Vec<u8>> {
    let text = String::from_utf8_lossy(bytes);
    let redacted = Zeroizing::new(redactor.redact_text(&text));
    Zeroizing::new(
        redacted
            .chars()
            .filter(|character| *character == '\n' || !character.is_control())
            .collect::<String>()
            .into_bytes(),
    )
}

fn append_stderr_ring(ring: &mut Vec<u8>, chunk: &[u8]) {
    ring.extend_from_slice(chunk);
    let overflow = ring.len().saturating_sub(MAX_STDERR_BYTES);
    if overflow > 0 {
        ring.drain(..overflow);
    }
    while stderr_line_count(ring) > MAX_STDERR_LINES {
        match ring.iter().position(|byte| *byte == b'\n') {
            Some(end) => {
                ring.drain(..=end);
            }
            None => break,
        }
    }
}

fn stderr_line_count(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    bytes.iter().filter(|byte| **byte == b'\n').count() + usize::from(bytes.last() != Some(&b'\n'))
}

async fn read_json_line<R>(reader: &mut R) -> Result<(Value, usize), String>
where
    R: AsyncBufRead + Unpin,
{
    let mut bytes = Vec::new();
    let count = reader
        .take((MAX_LINE_BYTES + 2) as u64)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|_| TRANSPORT_FAILED.to_string())?;
    if count == 0 {
        return Err(TRANSPORT_FAILED.into());
    }
    if bytes.last() != Some(&b'\n') {
        return Err(if bytes.len() > MAX_LINE_BYTES {
            MESSAGE_TOO_LARGE.into()
        } else {
            PROTOCOL_INVALID.into()
        });
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    if bytes.is_empty() || bytes.len() > MAX_LINE_BYTES {
        return Err(if bytes.len() > MAX_LINE_BYTES {
            MESSAGE_TOO_LARGE.into()
        } else {
            PROTOCOL_INVALID.into()
        });
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| PROTOCOL_INVALID.to_string())?;
    let value = serde_json::from_str::<Value>(text).map_err(|_| PROTOCOL_INVALID.to_string())?;
    mcp::validate_json(&value, MAX_LINE_BYTES).map_err(|code| match code {
        mcp::RESPONSE_TOO_LARGE => MESSAGE_TOO_LARGE.to_string(),
        _ => PROTOCOL_INVALID.to_string(),
    })?;
    Ok((value, bytes.len()))
}

impl ExplorerTracker {
    fn validate_list_request(&self, method: &str, params: &Value) -> Result<(), String> {
        if !list_method(method) {
            return Ok(());
        }
        let requested = params.get("cursor").and_then(Value::as_str);
        let pages = self.list_pages.get(method).copied().unwrap_or_default();
        if pages == 0 {
            if requested.is_none() {
                Ok(())
            } else {
                Err(mcp::CURSOR_INVALID.into())
            }
        } else {
            match self.list_cursors.get(method) {
                Some(Some(expected)) if requested == Some(expected.as_str()) => Ok(()),
                _ => Err(mcp::CURSOR_INVALID.into()),
            }
        }
    }

    fn validate_arguments(&self, method: &str, params: &Value) -> Result<(), String> {
        if method == "tools/call" {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
            let schema = self
                .tool_schemas
                .get(name)
                .ok_or_else(|| mcp::SCHEMA_UNSUPPORTED.to_string())?;
            mcp::validate_tool_arguments(
                schema,
                params
                    .get("arguments")
                    .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?,
            )
            .map_err(ToOwned::to_owned)?;
        }
        if method == "prompts/get" {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
            let schema = self
                .prompt_schemas
                .get(name)
                .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
            mcp::validate_prompt_values(schema, params.get("arguments"))
                .map_err(ToOwned::to_owned)?;
        }
        Ok(())
    }

    fn update_list_result(
        &mut self,
        method: &str,
        result: &Value,
        requested_cursor: Option<&str>,
    ) -> Result<(), String> {
        let (list_key, identity_key) = match method {
            "tools/list" => ("tools", "name"),
            "resources/list" => ("resources", "uri"),
            "resources/templates/list" => ("resourceTemplates", "uriTemplate"),
            "prompts/list" => ("prompts", "name"),
            _ => return Ok(()),
        };
        let items = result
            .get(list_key)
            .and_then(Value::as_array)
            .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
        let next_cursor = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let current_pages = self.list_pages.get(method).copied().unwrap_or_default();
        if (current_pages == 0 && requested_cursor.is_some())
            || (current_pages > 0
                && !matches!(
                    self.list_cursors.get(method),
                    Some(Some(expected)) if requested_cursor == Some(expected.as_str())
                ))
        {
            return Err(mcp::CURSOR_INVALID.into());
        }
        let next_pages = current_pages
            .checked_add(1)
            .ok_or_else(|| mcp::RESPONSE_TOO_LARGE.to_string())?;
        if next_pages > MAX_LIST_PAGES {
            return Err(mcp::RESPONSE_TOO_LARGE.into());
        }
        let page_bytes = serde_json::to_vec(items)
            .map_err(|_| mcp::MESSAGE_INVALID.to_string())?
            .len();
        let next_bytes = self
            .list_bytes
            .get(method)
            .copied()
            .unwrap_or_default()
            .checked_add(page_bytes)
            .ok_or_else(|| mcp::RESPONSE_TOO_LARGE.to_string())?;
        if next_bytes > MAX_RETAINED_LIST_BYTES {
            return Err(mcp::RESPONSE_TOO_LARGE.into());
        }
        let mut seen = self.seen.get(method).cloned().unwrap_or_default();
        let mut used = self.used_cursors.get(method).cloned().unwrap_or_default();
        if let Some(cursor) = requested_cursor {
            if !used.insert(cursor.as_bytes().to_vec()) {
                return Err(mcp::CURSOR_INVALID.into());
            }
        }
        if next_cursor.as_ref().is_some_and(|cursor| {
            requested_cursor == Some(cursor.as_str()) || used.contains(cursor.as_bytes())
        }) {
            return Err(mcp::CURSOR_INVALID.into());
        }
        for item in items {
            let identity = item
                .get(identity_key)
                .and_then(Value::as_str)
                .ok_or_else(|| mcp::MESSAGE_INVALID.to_string())?;
            if !seen.insert(identity.as_bytes().to_vec()) || seen.len() > mcp::MAX_LIST_ITEMS {
                return Err(mcp::MESSAGE_INVALID.into());
            }
        }
        let mut tool_schemas = self.tool_schemas.clone();
        let mut prompt_schemas = self.prompt_schemas.clone();
        if method == "tools/list" {
            for (name, schema) in mcp::tool_schemas(result).map_err(ToOwned::to_owned)? {
                if tool_schemas
                    .insert(name, schema.clone())
                    .is_some_and(|existing| existing != schema)
                {
                    return Err(mcp::MESSAGE_INVALID.into());
                }
            }
        } else if method == "prompts/list" {
            for (name, schema) in mcp::prompt_schemas(result).map_err(ToOwned::to_owned)? {
                if prompt_schemas
                    .insert(name, schema.clone())
                    .is_some_and(|existing| existing != schema)
                {
                    return Err(mcp::MESSAGE_INVALID.into());
                }
            }
        }
        self.seen.insert(method.into(), seen);
        self.list_pages.insert(method.into(), next_pages);
        self.list_bytes.insert(method.into(), next_bytes);
        self.list_cursors.insert(method.into(), next_cursor);
        self.used_cursors.insert(method.into(), used);
        self.tool_schemas = tool_schemas;
        self.prompt_schemas = prompt_schemas;
        Ok(())
    }
}

fn list_method(method: &str) -> bool {
    matches!(
        method,
        "tools/list" | "resources/list" | "resources/templates/list" | "prompts/list"
    )
}

fn map_rpc_error(code: i64) -> &'static str {
    match code {
        -32022 => mcp::VERSION_UNSUPPORTED,
        -32021 | -32601 => mcp::CAPABILITY_UNAVAILABLE,
        -32020 => mcp::MESSAGE_INVALID,
        _ => mcp::SERVER_ERROR,
    }
}

fn validate_opaque_id(value: &str) -> Result<(), ()> {
    if value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(())
    }
}

fn random_hex_128() -> Result<String, ()> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| ())?;
    let mut output = String::with_capacity(32);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").map_err(|_| ())?;
    }
    Ok(output)
}

fn now_unix_ms() -> Result<u64, ()> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ())?
        .as_millis();
    u64::try_from(millis).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use devbox_secrets::{SealError, Sealer};

    struct TestSealer;

    impl Sealer for TestSealer {
        fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
            Ok(plaintext.as_bytes().iter().rev().copied().collect())
        }

        fn unseal(&self, ciphertext: &[u8]) -> Result<Zeroizing<String>, SealError> {
            String::from_utf8(ciphertext.iter().rev().copied().collect())
                .map(Zeroizing::new)
                .map_err(|_| SealError::InvalidInput)
        }
    }

    fn opaque() -> String {
        "a".repeat(32)
    }

    #[test]
    fn profile_shape_is_structured_and_bounded() {
        let mut profile = McpStdioProfile {
            executable_selection_id: opaque(),
            cwd_selection_id: None,
            era: EraPreference::Auto,
            args: vec!["--safe".into(), "literal value".into()],
            environment: Vec::new(),
            timeout_ms: 5_000,
        };
        assert!(validate_profile_shape(&profile).is_ok());
        profile.args[0] = "bad\narg".into();
        assert_eq!(
            validate_profile_shape(&profile),
            Err(PROFILE_INVALID.into())
        );
        profile.args = vec!["x".repeat(MAX_ARG_BYTES + 1)];
        assert_eq!(
            validate_profile_shape(&profile),
            Err(PROFILE_INVALID.into())
        );
    }

    #[test]
    fn environment_binding_resolves_only_explicit_sources_and_redacts_sensitive_values() {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

        let sealed = devbox_secrets::seal_v1(&TestSealer, "secret-token").unwrap();
        let environment = vec![
            EnvironmentVariable {
                key: "SOURCE_TOKEN".into(),
                value: B64.encode(sealed),
                secret: true,
            },
            EnvironmentVariable {
                key: "PLAIN".into(),
                value: "visible".into(),
                secret: false,
            },
        ];
        let bindings = vec![McpStdioEnvironmentBinding {
            child_name: "API_TOKEN".into(),
            source_name: "SOURCE_TOKEN".into(),
        }];
        let resolved = resolve_environment(&bindings, &environment, &TestSealer).unwrap();
        assert_eq!(resolved.values[0].0, "API_TOKEN");
        assert_eq!(resolved.values[0].1.as_str(), "secret-token");
        let redactor = Redactor::from_secrets(resolved.secrets);
        assert_eq!(
            redactor.redact_text("prefix secret-token suffix"),
            "prefix [REDACTED] suffix"
        );

        let missing = vec![McpStdioEnvironmentBinding {
            child_name: "OTHER".into(),
            source_name: "MISSING".into(),
        }];
        assert_eq!(
            resolve_environment(&missing, &environment, &TestSealer).map(|_| ()),
            Err(ENVIRONMENT_INVALID.into())
        );
    }

    #[test]
    fn runtime_environment_names_cannot_be_overwritten() {
        let source = EnvironmentVariable {
            key: "PLAIN".into(),
            value: "value".into(),
            secret: false,
        };
        let protected = runtime_environment_allowlist()[0].to_string();
        let binding = McpStdioEnvironmentBinding {
            child_name: protected,
            source_name: "PLAIN".into(),
        };
        assert_eq!(
            resolve_environment(&[binding], &[source], &TestSealer).map(|_| ()),
            Err(ENVIRONMENT_INVALID.into())
        );
    }

    #[test]
    fn environment_values_and_names_are_portable_across_windows() {
        use base64::{engine::general_purpose::STANDARD as B64, Engine as _};

        let duplicate_sources = vec![
            EnvironmentVariable {
                key: "TOKEN".into(),
                value: "one".into(),
                secret: false,
            },
            EnvironmentVariable {
                key: "token".into(),
                value: "two".into(),
                secret: false,
            },
        ];
        assert_eq!(
            resolve_environment(&[], &duplicate_sources, &TestSealer).map(|_| ()),
            Err(ENVIRONMENT_INVALID.into())
        );

        let control_value = EnvironmentVariable {
            key: "SOURCE".into(),
            value: "line\nbreak".into(),
            secret: false,
        };
        let binding = McpStdioEnvironmentBinding {
            child_name: "CHILD".into(),
            source_name: "SOURCE".into(),
        };
        assert_eq!(
            resolve_environment(&[binding], &[control_value], &TestSealer).map(|_| ()),
            Err(ENVIRONMENT_INVALID.into())
        );

        let sealed_empty = devbox_secrets::seal_v1(&TestSealer, "").unwrap();
        let empty_secret = EnvironmentVariable {
            key: "EMPTY_SECRET".into(),
            value: B64.encode(sealed_empty),
            secret: true,
        };
        let binding = McpStdioEnvironmentBinding {
            child_name: "CHILD".into(),
            source_name: "EMPTY_SECRET".into(),
        };
        assert_eq!(
            resolve_environment(&[binding], &[empty_secret], &TestSealer).map(|_| ()),
            Err(ENVIRONMENT_INVALID.into())
        );
    }

    #[test]
    fn json_line_encoder_never_emits_embedded_newlines() {
        let line = encode_json_line(&json!({"value": "one\ntwo"})).unwrap();
        assert_eq!(line.last(), Some(&b'\n'));
        assert_eq!(
            line[..line.len() - 1]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count(),
            0
        );
    }

    #[test]
    fn stderr_ring_redacts_controls_and_keeps_only_bounded_newest_lines() {
        let redactor = Redactor::from_secrets(vec![Zeroizing::new("secret-token".into())]);
        let sanitized = sanitize_stderr_chunk(
            &redactor,
            b"prefix secret-token\x1b[31m suffix\nsecond\tline\n",
        );
        let text = String::from_utf8(sanitized.to_vec()).unwrap();
        assert!(!text.contains("secret-token"));
        assert!(!text.contains('\x1b'));
        assert!(!text.contains('\t'));

        let mut ring = Vec::new();
        for index in 0..(MAX_STDERR_LINES + 8) {
            append_stderr_ring(&mut ring, format!("line-{index}\n").as_bytes());
        }
        assert!(stderr_line_count(&ring) <= MAX_STDERR_LINES);
        assert!(!String::from_utf8_lossy(&ring).contains("line-0\n"));

        append_stderr_ring(&mut ring, &vec![b'x'; MAX_STDERR_BYTES + 100]);
        assert!(ring.len() <= MAX_STDERR_BYTES);
    }

    #[tokio::test]
    async fn json_line_reader_accepts_lf_and_crlf_and_rejects_incomplete_data() {
        let mut lf = BufReader::new(&b"{\"jsonrpc\":\"2.0\"}\n"[..]);
        assert_eq!(read_json_line(&mut lf).await.unwrap().0["jsonrpc"], "2.0");
        let mut crlf = BufReader::new(&b"{\"jsonrpc\":\"2.0\"}\r\n"[..]);
        assert_eq!(read_json_line(&mut crlf).await.unwrap().0["jsonrpc"], "2.0");
        let mut incomplete = BufReader::new(&b"{}"[..]);
        assert_eq!(
            read_json_line(&mut incomplete).await,
            Err(PROTOCOL_INVALID.into())
        );
    }

    #[test]
    fn explorer_rejects_cursor_skips_and_duplicate_identities() {
        let mut explorer = ExplorerTracker::default();
        assert_eq!(
            explorer.validate_list_request("tools/list", &json!({"cursor": "skip"})),
            Err(mcp::CURSOR_INVALID.into())
        );
        let first = json!({
            "tools": [{"name": "one", "inputSchema": {"type": "object"}}],
            "nextCursor": "next"
        });
        explorer
            .update_list_result("tools/list", &first, None)
            .unwrap();
        assert!(explorer
            .validate_list_request("tools/list", &json!({"cursor": "next"}))
            .is_ok());
        let duplicate = json!({
            "tools": [{"name": "one", "inputSchema": {"type": "object"}}]
        });
        assert_eq!(
            explorer.update_list_result("tools/list", &duplicate, Some("next")),
            Err(mcp::MESSAGE_INVALID.into())
        );
    }

    #[test]
    fn opaque_ids_are_exact_lower_hex() {
        assert!(validate_opaque_id(&opaque()).is_ok());
        assert!(validate_opaque_id(&"A".repeat(32)).is_err());
        assert!(validate_opaque_id(&"a".repeat(31)).is_err());
    }
}
