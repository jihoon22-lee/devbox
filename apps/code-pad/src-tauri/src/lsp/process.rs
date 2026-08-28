//! No-shell child process management for a local stdio language server.
//!
//! The process owns three independent streams: stdout is parsed exclusively as
//! JSON-RPC, stdin is written exclusively by the protocol writer, and stderr is
//! copied into a bounded diagnostic ring.  Keeping these paths separate makes
//! accidental protocol/log mixing impossible at the API boundary.

use super::transport::{
    FrameLimits, JsonRpcMessage, JsonRpcReader, JsonRpcWriter, PendingError, PendingRequests,
    PendingResultError, RequestCancellation, RequestId, RpcError, RpcId, TransportError,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{broadcast, mpsc, Mutex, Notify};
use tokio::time::sleep;

#[cfg(windows)]
#[path = "windows_job.rs"]
mod windows_job;
#[cfg(windows)]
use windows_job::WindowsJobObject;

pub const DEFAULT_STDERR_CAPACITY: usize = 64 * 1024;
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
/// A cancellation is best effort, but it must not inherit an unbounded wait
/// for the protocol writer. If the writer is wedged, the stream may contain a
/// partial frame; terminating the child is safer than continuing to reuse a
/// corrupt JSON-RPC channel.
const ABORT_NOTIFICATION_TIMEOUT: Duration = Duration::from_millis(100);

/// An argv-only child command.  No shell string is ever accepted or expanded.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    /// Only explicitly listed variables are passed to the child.  The parent
    /// environment is otherwise cleared, preventing unrelated secrets from
    /// reaching a language server.
    pub env: BTreeMap<OsString, OsString>,
}

impl ProcessSpec {
    pub fn new(executable: impl Into<PathBuf>, current_dir: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            args: Vec::new(),
            current_dir: current_dir.into(),
            env: BTreeMap::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    fn validate(&self) -> Result<(), ProcessError> {
        if self.executable.as_os_str().is_empty() {
            return Err(ProcessError::InvalidSpec(
                "executable cannot be empty".into(),
            ));
        }
        if self.current_dir.as_os_str().is_empty() {
            return Err(ProcessError::InvalidSpec(
                "current directory cannot be empty".into(),
            ));
        }
        for (label, value) in [
            ("executable", self.executable.as_os_str()),
            ("current directory", self.current_dir.as_os_str()),
        ] {
            reject_nul(label, value)?;
        }
        for (index, arg) in self.args.iter().enumerate() {
            reject_nul(&format!("argument {index}"), arg.as_os_str())?;
        }
        for (key, value) in &self.env {
            reject_nul("environment key", key.as_os_str())?;
            reject_nul("environment value", value.as_os_str())?;
        }
        Ok(())
    }
}

fn reject_nul(label: &str, value: &OsStr) -> Result<(), ProcessError> {
    if value.to_string_lossy().contains('\0') {
        return Err(ProcessError::InvalidSpec(format!(
            "{label} contains an embedded NUL"
        )));
    }
    Ok(())
}

#[derive(Debug)]
pub enum ProcessError {
    InvalidSpec(String),
    ExecutableNotFound(PathBuf),
    CurrentDirNotFound(PathBuf),
    Spawn(io::Error),
    MissingPipe(&'static str),
    Transport(TransportError),
    Request(RequestError),
    #[cfg(windows)]
    JobObject(String),
    ShutdownTimeout,
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpec(message) => {
                write!(f, "invalid language-server process spec: {message}")
            }
            Self::ExecutableNotFound(path) => write!(
                f,
                "language-server executable is not a file: {}",
                path.display()
            ),
            Self::CurrentDirNotFound(path) => write!(
                f,
                "language-server current directory is not a directory: {}",
                path.display()
            ),
            Self::Spawn(error) => write!(f, "failed to spawn language server: {error}"),
            Self::MissingPipe(name) => write!(f, "language-server {name} pipe was not created"),
            Self::Transport(error) => error.fmt(f),
            Self::Request(error) => error.fmt(f),
            #[cfg(windows)]
            Self::JobObject(error) => {
                write!(f, "language-server process-tree job failed: {error}")
            }
            Self::ShutdownTimeout => {
                f.write_str("language-server did not exit before the shutdown timeout")
            }
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Transport(error) => Some(error),
            Self::Request(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TransportError> for ProcessError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

#[derive(Debug)]
pub enum RequestError {
    Timeout,
    Cancelled,
    Disconnected,
    Transport(TransportError),
    Remote(RpcError),
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => f.write_str("language-server request timed out"),
            Self::Cancelled => f.write_str("language-server request was cancelled"),
            Self::Disconnected => f.write_str("language server disconnected"),
            Self::Transport(error) => error.fmt(f),
            Self::Remote(error) => write!(
                f,
                "language server returned {}: {}",
                error.code, error.message
            ),
        }
    }
}

impl std::error::Error for RequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}

/// A byte ring that retains only the newest diagnostics.
#[derive(Debug, Clone)]
pub struct BoundedStderr {
    capacity: usize,
    bytes: VecDeque<u8>,
    dropped_bytes: u64,
}

impl BoundedStderr {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: VecDeque::with_capacity(capacity.min(4096)),
            dropped_bytes: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn push(&mut self, chunk: &[u8]) {
        if self.capacity == 0 {
            self.dropped_bytes = self
                .dropped_bytes
                .saturating_add(chunk.len().try_into().unwrap_or(u64::MAX));
            return;
        }
        if chunk.len() >= self.capacity {
            let keep_from = chunk.len() - self.capacity;
            let replaced = self.bytes.len().saturating_add(keep_from);
            self.dropped_bytes = self
                .dropped_bytes
                .saturating_add(replaced.try_into().unwrap_or(u64::MAX));
            self.bytes.clear();
            self.bytes.extend(&chunk[keep_from..]);
            return;
        }
        let overflow = self
            .bytes
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(self.capacity);
        for _ in 0..overflow {
            self.bytes.pop_front();
        }
        self.dropped_bytes = self
            .dropped_bytes
            .saturating_add(overflow.try_into().unwrap_or(u64::MAX));
        self.bytes.extend(chunk);
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.iter().copied().collect()
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes()).into_owned()
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn truncated(&self) -> bool {
        self.dropped_bytes > 0
    }

    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }
}

#[derive(Debug, Clone)]
pub struct StderrEvent {
    pub bytes: Vec<u8>,
    pub dropped_bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub enum IncomingMessage {
    Message(JsonRpcMessage),
    UnknownResponse(JsonRpcMessage),
    ProtocolError(String),
    Eof,
    Exited { code: Option<i32> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Stopping,
    Exited { code: Option<i32> },
    Failed { reason: String },
}

enum ChildCommand {
    Kill,
}

#[cfg(unix)]
fn kill_process_group(process_group: libc::pid_t) {
    // A negative pid targets the process group created for this LSP child. Do
    // not ever send a group signal to the caller's own process group.
    if process_group > 1 {
        unsafe {
            let _ = libc::kill(-process_group, libc::SIGKILL);
        }
    }
}

#[derive(Clone)]
struct ProcessTreeCleanup {
    #[cfg(windows)]
    job: Arc<WindowsJobObject>,
    #[cfg(unix)]
    process_group: libc::pid_t,
    #[cfg(unix)]
    process_group_alive: Arc<AtomicBool>,
}

impl ProcessTreeCleanup {
    fn terminate(&self) {
        #[cfg(windows)]
        let _ = self.job.terminate();
        #[cfg(unix)]
        if self.process_group_alive.swap(false, Ordering::AcqRel) {
            kill_process_group(self.process_group);
        }
    }
}

struct ProcessInner {
    writer: Mutex<JsonRpcWriter<ChildStdin>>,
    pending: PendingRequests,
    child_commands: mpsc::Sender<ChildCommand>,
    state: Arc<Mutex<ProcessState>>,
    exit_notify: Arc<Notify>,
    messages: broadcast::Sender<IncomingMessage>,
    stderr: Arc<Mutex<BoundedStderr>>,
    stderr_events: broadcast::Sender<StderrEvent>,
    cleanup: ProcessTreeCleanup,
}

impl Drop for ProcessInner {
    fn drop(&mut self) {
        self.cleanup.terminate();
        let _ = self.child_commands.try_send(ChildCommand::Kill);
    }
}

/// A running local JSON-RPC child and its independent protocol/diagnostic
/// channels.
#[derive(Clone)]
pub struct LspProcess {
    inner: Arc<ProcessInner>,
}

impl LspProcess {
    pub async fn spawn(spec: ProcessSpec) -> Result<Self, ProcessError> {
        Self::spawn_with_options(spec, FrameLimits::default(), DEFAULT_STDERR_CAPACITY).await
    }

    pub async fn spawn_with_options(
        spec: ProcessSpec,
        frame_limits: FrameLimits,
        stderr_capacity: usize,
    ) -> Result<Self, ProcessError> {
        spec.validate()?;
        let executable = fs::canonicalize(&spec.executable)
            .map_err(|_| ProcessError::ExecutableNotFound(spec.executable.clone()))?;
        if !executable.is_file() {
            return Err(ProcessError::ExecutableNotFound(executable));
        }
        let current_dir = fs::canonicalize(&spec.current_dir)
            .map_err(|_| ProcessError::CurrentDirNotFound(spec.current_dir.clone()))?;
        if !current_dir.is_dir() {
            return Err(ProcessError::CurrentDirNotFound(current_dir));
        }

        let mut command = Command::new(&executable);
        command
            .args(&spec.args)
            .current_dir(current_dir)
            .env_clear()
            .envs(&spec.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(ProcessError::Spawn)?;
        #[cfg(unix)]
        let process_group = match child.id().and_then(|id| i32::try_from(id).ok()) {
            Some(id) if id > 1 => id,
            _ => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(ProcessError::Spawn(io::Error::other(
                    "child process group unavailable",
                )));
            }
        };
        #[cfg(windows)]
        let job = match WindowsJobObject::assign_to(&child) {
            Ok(job) => Arc::new(job),
            Err(error) => {
                // Do not continue with a process-only fallback.  The child
                // must be reaped before this spawn failure is returned.
                let _ = child.start_kill();
                let _ = child.wait().await;
                return Err(ProcessError::JobObject(error));
            }
        };
        let stdin = child
            .stdin
            .take()
            .ok_or(ProcessError::MissingPipe("stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(ProcessError::MissingPipe("stdout"))?;
        let stderr_input = child
            .stderr
            .take()
            .ok_or(ProcessError::MissingPipe("stderr"))?;
        let (child_commands, child_command_rx) = mpsc::channel(2);
        let (messages, _) = broadcast::channel(64);
        let (stderr_events, _) = broadcast::channel(64);
        let stderr = Arc::new(Mutex::new(BoundedStderr::new(stderr_capacity)));
        #[cfg(unix)]
        let process_group_alive = Arc::new(AtomicBool::new(true));
        let cleanup = ProcessTreeCleanup {
            #[cfg(windows)]
            job: Arc::clone(&job),
            #[cfg(unix)]
            process_group,
            #[cfg(unix)]
            process_group_alive: Arc::clone(&process_group_alive),
        };
        let inner = Arc::new(ProcessInner {
            writer: Mutex::new(JsonRpcWriter::with_limits(stdin, frame_limits)),
            pending: PendingRequests::new(),
            child_commands,
            state: Arc::new(Mutex::new(ProcessState::Running)),
            exit_notify: Arc::new(Notify::new()),
            messages,
            stderr: Arc::clone(&stderr),
            stderr_events,
            cleanup: cleanup.clone(),
        });

        spawn_stdout_task(
            stdout,
            frame_limits,
            inner.pending.clone(),
            inner.messages.clone(),
            inner.state.clone(),
            inner.child_commands.clone(),
        );
        spawn_stderr_task(
            stderr_input,
            inner.stderr.clone(),
            inner.stderr_events.clone(),
        );
        spawn_wait_task(
            child,
            child_command_rx,
            inner.pending.clone(),
            inner.state.clone(),
            inner.messages.clone(),
            inner.exit_notify.clone(),
            cleanup,
        );
        Ok(Self { inner })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<IncomingMessage> {
        self.inner.messages.subscribe()
    }

    pub fn subscribe_stderr(&self) -> broadcast::Receiver<StderrEvent> {
        self.inner.stderr_events.subscribe()
    }

    pub async fn stderr(&self) -> BoundedStderr {
        self.inner.stderr.lock().await.clone()
    }

    pub async fn state(&self) -> ProcessState {
        self.inner.state.lock().await.clone()
    }

    pub async fn pending_len(&self) -> usize {
        self.inner.pending.len().await
    }

    pub async fn notify(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
    ) -> Result<(), ProcessError> {
        self.write_message(JsonRpcMessage::notification(method, params))
            .await
    }

    /// Reply to a server-initiated JSON-RPC request. The client layer keeps
    /// the allowlist of supported methods; the process layer only serializes
    /// the already-decided response.
    pub async fn respond(&self, id: RpcId, result: Value) -> Result<(), ProcessError> {
        self.write_message(JsonRpcMessage::response(id, result))
            .await
    }

    pub async fn respond_error(&self, id: RpcId, error: RpcError) -> Result<(), ProcessError> {
        self.write_message(JsonRpcMessage::error(id, error)).await
    }

    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, RequestError> {
        self.request_inner(method.into(), params, timeout, None)
            .await
    }

    pub async fn request_with_cancel(
        &self,
        method: impl Into<String>,
        params: Option<Value>,
        timeout: Duration,
        cancellation: RequestCancellation,
    ) -> Result<Value, RequestError> {
        self.request_inner(method.into(), params, timeout, Some(cancellation))
            .await
    }

    async fn request_inner(
        &self,
        method: String,
        params: Option<Value>,
        timeout: Duration,
        cancellation: Option<RequestCancellation>,
    ) -> Result<Value, RequestError> {
        let (id, mut receiver) = self.inner.pending.register().await;
        if cancellation
            .as_ref()
            .is_some_and(RequestCancellation::is_cancelled)
        {
            let _ = self.inner.pending.cancel(id).await;
            return Err(RequestError::Cancelled);
        }
        if timeout.is_zero() {
            let _ = self.inner.pending.cancel(id).await;
            return Err(RequestError::Timeout);
        }

        // The request deadline covers serialization, writer-lock contention,
        // pipe backpressure, and the response wait. Previously the timeout
        // started only after write_message completed, so a child that stopped
        // reading stdin could pin the caller indefinitely.
        let deadline = Instant::now() + timeout;
        let write_future = self.write_message(JsonRpcMessage::request(id, method, params));
        tokio::pin!(write_future);
        let write_result: Result<Result<(), ProcessError>, RequestError> =
            if let Some(cancellation) = cancellation.as_ref() {
                tokio::select! {
                    result = &mut write_future => Ok(result),
                    _ = sleep(timeout) => Err(RequestError::Timeout),
                    _ = cancellation.cancelled() => Err(RequestError::Cancelled),
                }
            } else {
                match tokio::time::timeout(timeout, &mut write_future).await {
                    Ok(result) => Ok(result),
                    Err(_) => Err(RequestError::Timeout),
                }
            };
        match write_result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                let _ = self.inner.pending.cancel(id).await;
                return Err(RequestError::Transport(process_error_transport(error)));
            }
            Err(error) => {
                // A timed-out/cancelled write may have emitted only part of a
                // frame. Do not leave a potentially corrupted process alive
                // for later requests.
                let _ = self.inner.pending.cancel(id).await;
                self.force_kill();
                return Err(error);
            }
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(self.abort_request(id, false).await);
        }

        let result = if let Some(cancellation) = cancellation {
            tokio::select! {
                result = &mut receiver => map_pending_result(result),
                _ = sleep(remaining) => Err(self.abort_request(id, false).await),
                _ = cancellation.cancelled() => Err(self.abort_request(id, true).await),
            }
        } else {
            match tokio::time::timeout(remaining, &mut receiver).await {
                Ok(result) => map_pending_result(result),
                Err(_) => Err(self.abort_request(id, false).await),
            }
        };
        result
    }

    async fn abort_request(&self, id: RequestId, cancelled: bool) -> RequestError {
        if self.inner.pending.cancel(id).await {
            let notification = self.notify("$/cancelRequest", Some(json!({ "id": id.value() })));
            match tokio::time::timeout(ABORT_NOTIFICATION_TIMEOUT, notification).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) | Err(_) => {
                    // A partial/blocked cancellation notification makes the
                    // stdio stream unsafe to reuse. The child command is
                    // bounded and the wait task owns process-tree cleanup.
                    self.force_kill();
                }
            }
        }
        if cancelled {
            RequestError::Cancelled
        } else {
            RequestError::Timeout
        }
    }

    async fn write_message(&self, message: JsonRpcMessage) -> Result<(), ProcessError> {
        let mut writer = self.inner.writer.lock().await;
        writer
            .write_message(&message)
            .await
            .map_err(ProcessError::Transport)
    }

    pub async fn shutdown(&self) -> Result<(), ProcessError> {
        self.shutdown_with_timeout(DEFAULT_SHUTDOWN_TIMEOUT).await
    }

    pub async fn shutdown_with_timeout(&self, timeout: Duration) -> Result<(), ProcessError> {
        {
            let mut state = self.inner.state.lock().await;
            if matches!(*state, ProcessState::Exited { .. }) {
                return Ok(());
            }
            *state = ProcessState::Stopping;
        }
        let deadline = Instant::now() + timeout;
        let shutdown_error = if timeout.is_zero() {
            Some(RequestError::Timeout)
        } else {
            let response_timeout = deadline.saturating_duration_since(Instant::now());
            self.request("shutdown", None, response_timeout).await.err()
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero()
            && tokio::time::timeout(remaining, self.notify("exit", None))
                .await
                .is_err()
        {
            self.force_kill();
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            let writer_shutdown = async {
                let mut writer = self.inner.writer.lock().await;
                let _ = writer.shutdown().await;
            };
            if tokio::time::timeout(remaining, writer_shutdown)
                .await
                .is_err()
            {
                self.force_kill();
            }
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if !self.wait_for_exit(remaining).await {
            self.force_kill();
            let _ = self.wait_for_exit(Duration::from_millis(500)).await;
            return Err(ProcessError::ShutdownTimeout);
        }
        if let Some(error) = shutdown_error {
            if !matches!(error, RequestError::Disconnected | RequestError::Timeout) {
                return Err(ProcessError::Request(error));
            }
        }
        Ok(())
    }

    pub async fn wait_for_exit(&self, timeout: Duration) -> bool {
        if is_finished(&self.state().await) {
            return true;
        }
        let notified = self.inner.exit_notify.notified();
        if timeout.is_zero() {
            return is_finished(&self.state().await);
        }
        let _ = tokio::time::timeout(timeout, notified).await;
        is_finished(&self.state().await)
    }

    fn force_kill(&self) {
        let _ = self.inner.child_commands.try_send(ChildCommand::Kill);
    }
}

fn process_error_transport(error: ProcessError) -> TransportError {
    match error {
        ProcessError::Transport(error) => error,
        other => TransportError::InvalidMessage(other.to_string()),
    }
}

fn map_pending_result(
    result: Result<super::transport::PendingResult, tokio::sync::oneshot::error::RecvError>,
) -> Result<Value, RequestError> {
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(PendingResultError::Remote(error))) => Err(RequestError::Remote(error)),
        Ok(Err(PendingResultError::Failure(error))) => match error {
            PendingError::Disconnected => Err(RequestError::Disconnected),
            PendingError::Cancelled => Err(RequestError::Cancelled),
            PendingError::Transport(message) => Err(RequestError::Transport(
                TransportError::InvalidMessage(message),
            )),
        },
        Err(_) => Err(RequestError::Disconnected),
    }
}

fn is_finished(state: &ProcessState) -> bool {
    matches!(
        state,
        ProcessState::Exited { .. } | ProcessState::Failed { .. }
    )
}

fn spawn_stdout_task<R>(
    stdout: R,
    limits: FrameLimits,
    pending: PendingRequests,
    messages: broadcast::Sender<IncomingMessage>,
    state: Arc<Mutex<ProcessState>>,
    child_commands: mpsc::Sender<ChildCommand>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = JsonRpcReader::with_limits(stdout, limits);
        loop {
            match reader.read_message().await {
                Ok(Some(message)) => {
                    if message.is_response() {
                        if !pending.resolve(&message).await {
                            let _ = messages.send(IncomingMessage::UnknownResponse(message));
                        }
                    } else {
                        let _ = messages.send(IncomingMessage::Message(message));
                    }
                }
                Ok(None) => {
                    pending.fail_all(PendingError::Disconnected).await;
                    let _ = messages.send(IncomingMessage::Eof);
                    break;
                }
                Err(error) => {
                    let message = error.to_string();
                    pending
                        .fail_all(PendingError::Transport(message.clone()))
                        .await;
                    *state.lock().await = ProcessState::Failed {
                        reason: message.clone(),
                    };
                    let _ = messages.send(IncomingMessage::ProtocolError(message));
                    let _ = child_commands.send(ChildCommand::Kill).await;
                    break;
                }
            }
        }
    });
}

fn spawn_stderr_task<R>(
    mut input: R,
    ring: Arc<Mutex<BoundedStderr>>,
    events: broadcast::Sender<StderrEvent>,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 8192];
        loop {
            let read = match input.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => read,
                Err(_) => break,
            };
            let mut ring = ring.lock().await;
            ring.push(&buffer[..read]);
            let _ = events.send(StderrEvent {
                bytes: buffer[..read].to_vec(),
                dropped_bytes: ring.dropped_bytes(),
                truncated: ring.truncated(),
            });
        }
    });
}

fn spawn_wait_task(
    mut child: Child,
    mut commands: mpsc::Receiver<ChildCommand>,
    pending: PendingRequests,
    state: Arc<Mutex<ProcessState>>,
    messages: broadcast::Sender<IncomingMessage>,
    exit_notify: Arc<Notify>,
    cleanup: ProcessTreeCleanup,
) {
    tokio::spawn(async move {
        let (status, kill_requested) = tokio::select! {
            result = child.wait() => (result, false),
            command = commands.recv() => {
                let kill_requested = matches!(command, Some(ChildCommand::Kill));
                if kill_requested {
                    cleanup.terminate();
                    let _ = child.start_kill();
                }
                // A closed command channel is not an explicit kill request.
                // The child still gets reaped, but on Unix the post-exit
                // process-group cleanup must remain enabled so descendants
                // cannot survive after an unexpected owner/task shutdown.
                (child.wait().await, kill_requested)
            }
        };
        #[cfg(windows)]
        let _ = kill_requested;
        #[cfg(windows)]
        cleanup.terminate();
        #[cfg(unix)]
        if !kill_requested {
            // The leader can exit while descendants keep the group alive;
            // clean that group once, but never again from Drop after the
            // process group has been marked inactive.
            cleanup.terminate();
        }
        match status {
            Ok(status) => {
                let code = status.code();
                let mut state = state.lock().await;
                if !matches!(*state, ProcessState::Failed { .. }) {
                    *state = ProcessState::Exited { code };
                }
                drop(state);
                pending.fail_all(PendingError::Disconnected).await;
                let _ = messages.send(IncomingMessage::Exited { code });
            }
            Err(error) => {
                let reason = error.to_string();
                *state.lock().await = ProcessState::Failed {
                    reason: reason.clone(),
                };
                pending
                    .fail_all(PendingError::Transport(reason.clone()))
                    .await;
                let _ = messages.send(IncomingMessage::ProtocolError(reason));
            }
        }
        exit_notify.notify_one();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stderr_ring_keeps_newest_bytes_and_reports_truncation() {
        let mut ring = BoundedStderr::new(5);
        ring.push(b"1234");
        ring.push(b"56789");
        assert_eq!(ring.bytes(), b"56789");
        assert!(ring.truncated());
        assert_eq!(ring.dropped_bytes(), 4);
        ring.push(b"ab");
        assert_eq!(ring.text(), "789ab");
        assert_eq!(ring.dropped_bytes(), 6);
    }

    #[test]
    fn oversized_stderr_chunk_drops_oldest_chunk_bytes() {
        let mut ring = BoundedStderr::new(4);
        ring.push(b"123456");
        assert_eq!(ring.bytes(), b"3456");
        assert_eq!(ring.dropped_bytes(), 2);
    }

    #[test]
    fn process_spec_preserves_argv_and_rejects_nul() {
        let safe = ProcessSpec::new("server", ".").arg("--flag; & | $HOME");
        assert_eq!(safe.args[0], OsString::from("--flag; & | $HOME"));
        let invalid = ProcessSpec::new("server\0bad", ".");
        assert!(matches!(
            invalid.validate(),
            Err(ProcessError::InvalidSpec(_))
        ));
    }
}
