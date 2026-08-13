//! Production bridge between the scheduler and the platform process guards.
//!
//! This module owns the short-lived plaintext environment snapshot, the
//! per-run log streams, and the one terminal result shared by wait/terminate
//! callers. Platform-specific modules only expose spawn/identity primitives;
//! no command, environment value, or absolute path is copied into an
//! [`AdapterError`].

use super::environment::EnvironmentProtectorState;
#[cfg(windows)]
use super::windows;
use super::wsl;
use crate::core::models::{RunExecutionMetadata, TargetKind};
use crate::logs::{LogStream, LogStreamHandle, LogStreams, LOG_RELATIVE_ROOT};
use crate::scheduler::{
    AdapterError, AdapterFuture, ExecutionAdapter, ExecutionExit, ExecutionHandle, ExecutionRequest,
};
use crate::storage::DatabaseState;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use zeroize::{Zeroize, Zeroizing};

const DEFAULT_TERMINATION_GRACE: Duration = Duration::from_secs(2);
const DRAIN_BUFFER_BYTES: usize = 16 * 1024;
const REDACTED_BYTES: &[u8] = b"<redacted>";
const WAIT_OWNER_ABORT_TIMEOUT: Duration = Duration::from_millis(250);

fn relative_log_dir(run_id: &str) -> String {
    // Persist a portable slash-separated app-relative value. `PathBuf` is
    // still used for filesystem access, but Windows display formatting may
    // contain backslashes that are not valid in the DB contract.
    format!("{LOG_RELATIVE_ROOT}/{run_id}")
}

#[derive(Debug, Clone, Copy)]
enum FailureCode {
    EnvironmentUnavailable,
    Storage,
    LogOpen,
    LogWrite,
    TargetUnavailable,
    Spawn,
    Handshake,
    MetadataCas,
    Wait,
    Termination,
}

impl FailureCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::EnvironmentUnavailable => "environment-unavailable",
            Self::Storage => "storage-failed",
            Self::LogOpen => "log-open-failed",
            Self::LogWrite => "log-write-failed",
            Self::TargetUnavailable => "target-unavailable",
            Self::Spawn => "spawn-failed",
            Self::Handshake => "handshake-failed",
            Self::MetadataCas => "metadata-cas-failed",
            Self::Wait => "wait-failed",
            Self::Termination => "termination-timeout",
        }
    }
}

fn failure(code: FailureCode) -> AdapterError {
    AdapterError::new(code.as_str())
}

/// Byte-oriented per-run secret redaction. At most `max_len - 1` bytes are
/// carried between reads, so a secret split across pipe chunks is still found.
struct SecretRedactor {
    secrets: Vec<Vec<u8>>,
    carry: Vec<u8>,
    max_len: usize,
}

impl SecretRedactor {
    fn from_environment(environment: &std::collections::BTreeMap<String, String>) -> Self {
        let mut secrets = environment
            .values()
            .filter(|value| !value.is_empty())
            .map(|value| value.as_bytes().to_vec())
            .collect::<Vec<_>>();
        secrets.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        secrets.dedup();
        let max_len = secrets.first().map_or(0, Vec::len);
        Self {
            secrets,
            carry: Vec::new(),
            max_len,
        }
    }

    fn redact(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut input = std::mem::take(&mut self.carry);
        input.extend_from_slice(bytes);
        if self.max_len == 0 {
            return input;
        }
        let safe_start_count = input.len().saturating_sub(self.max_len.saturating_sub(1));
        let (output, consumed) = self.redact_prefix(&input, safe_start_count);
        self.carry.extend_from_slice(&input[consumed..]);
        input.zeroize();
        output
    }

    fn finish(&mut self) -> Vec<u8> {
        let mut input = std::mem::take(&mut self.carry);
        if self.max_len == 0 {
            return input;
        }
        let (output, _) = self.redact_prefix(&input, input.len());
        input.zeroize();
        output
    }

    fn redact_prefix(&self, input: &[u8], scan_limit: usize) -> (Vec<u8>, usize) {
        let mut output = Vec::with_capacity(input.len());
        let mut index = 0;
        while index < scan_limit {
            if let Some(length) = self
                .secrets
                .iter()
                .find(|secret| input[index..].starts_with(secret))
                .map(Vec::len)
            {
                output.extend_from_slice(REDACTED_BYTES);
                index += length;
            } else {
                output.push(input[index]);
                index += 1;
            }
        }
        (output, index)
    }
}

impl Drop for SecretRedactor {
    fn drop(&mut self) {
        for secret in &mut self.secrets {
            secret.zeroize();
        }
        self.carry.zeroize();
    }
}

/// The scheduler-facing process adapter. Tauri setup installs one instance for
/// the process-wide coordinator so manual and scheduled runs share this exact
/// environment/log/process boundary.
#[derive(Clone)]
pub struct PlatformExecutionAdapter {
    database: Arc<DatabaseState>,
    protector: EnvironmentProtectorState,
    app_data_root: Arc<PathBuf>,
    termination_grace: Duration,
}

impl PlatformExecutionAdapter {
    pub fn new(
        database: Arc<DatabaseState>,
        protector: EnvironmentProtectorState,
        app_data_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            database,
            protector,
            app_data_root: Arc::new(app_data_root.into()),
            termination_grace: DEFAULT_TERMINATION_GRACE,
        }
    }

    pub fn with_termination_grace(mut self, grace: Duration) -> Self {
        self.termination_grace = grace.max(Duration::from_millis(1));
        self
    }

    async fn spawn_request(
        &self,
        request: ExecutionRequest,
    ) -> Result<Arc<dyn ExecutionHandle>, AdapterError> {
        let ciphertext = self
            .database
            .get_job_environment_ciphertext(&request.job.id)
            .map_err(|_| failure(FailureCode::Storage))?;
        let plaintext = self
            .protector
            .decrypt_optional_for_execution(ciphertext.as_deref())
            .map_err(|_| failure(FailureCode::EnvironmentUnavailable))?;

        let streams = LogStreams::open_default(self.app_data_root.as_path(), &request.run.id)
            .map_err(|_| failure(FailureCode::LogOpen))?;
        let log_dir = relative_log_dir(&request.run.id);
        if !self
            .database
            .mark_run_log_dir(
                &request.run.id,
                &request.owner_instance_id,
                &request.attempt_token,
                &log_dir,
            )
            .map_err(|_| failure(FailureCode::Storage))?
        {
            return Err(failure(FailureCode::MetadataCas));
        }

        let result = self
            .spawn_with_plaintext(&request, plaintext.as_map(), streams)
            .await;
        // This is deliberately immediately after the platform spawn/handshake
        // boundary. The returned handle owns no environment bytes.
        drop(plaintext);
        result
    }

    async fn spawn_with_plaintext(
        &self,
        request: &ExecutionRequest,
        environment: &std::collections::BTreeMap<String, String>,
        streams: LogStreams,
    ) -> Result<Arc<dyn ExecutionHandle>, AdapterError> {
        let stdout_redactor = SecretRedactor::from_environment(environment);
        let stderr_redactor = SecretRedactor::from_environment(environment);
        match request.job.target_kind {
            TargetKind::Windows => {
                self.spawn_windows(
                    request,
                    environment,
                    streams,
                    stdout_redactor,
                    stderr_redactor,
                )
                .await
            }
            TargetKind::Wsl => {
                self.spawn_wsl(
                    request,
                    environment,
                    streams,
                    stdout_redactor,
                    stderr_redactor,
                )
                .await
            }
        }
    }

    #[cfg(not(windows))]
    async fn spawn_windows(
        &self,
        _request: &ExecutionRequest,
        _environment: &std::collections::BTreeMap<String, String>,
        _streams: LogStreams,
        _stdout_redactor: SecretRedactor,
        _stderr_redactor: SecretRedactor,
    ) -> Result<Arc<dyn ExecutionHandle>, AdapterError> {
        Err(failure(FailureCode::TargetUnavailable))
    }

    #[cfg(windows)]
    async fn spawn_windows(
        &self,
        request: &ExecutionRequest,
        environment: &std::collections::BTreeMap<String, String>,
        streams: LogStreams,
        stdout_redactor: SecretRedactor,
        stderr_redactor: SecretRedactor,
    ) -> Result<Arc<dyn ExecutionHandle>, AdapterError> {
        let mut child = windows::spawn(
            &request.job.command,
            request.job.cwd.as_deref().map(Path::new),
            environment,
        )
        .map_err(|_| failure(FailureCode::Spawn))?;
        let metadata = RunExecutionMetadata {
            log_dir: Some(relative_log_dir(&request.run.id)),
            target_pid: Some(i64::from(child.identity().pid)),
            target_process_created_at: Some(child.identity().created_at),
            ..RunExecutionMetadata::default()
        };
        let stdout = child
            .take_stdout_file()
            .map_err(|_| failure(FailureCode::Spawn))?;
        let stderr = child
            .take_stderr_file()
            .map_err(|_| failure(FailureCode::Spawn))?;
        let handle = WindowsExecutionHandle::start_with_files(
            child,
            streams,
            stdout,
            stderr,
            stdout_redactor,
            stderr_redactor,
            metadata,
            self.termination_grace,
        );
        Ok(Arc::new(handle))
    }

    async fn spawn_wsl(
        &self,
        request: &ExecutionRequest,
        environment: &std::collections::BTreeMap<String, String>,
        streams: LogStreams,
        mut stdout_redactor: SecretRedactor,
        stderr_redactor: SecretRedactor,
    ) -> Result<Arc<dyn ExecutionHandle>, AdapterError> {
        let distro = request
            .job
            .target_distro
            .as_deref()
            .ok_or_else(|| failure(FailureCode::Spawn))?;
        let cwd = match request.job.cwd.as_deref() {
            None => None,
            Some(path) if path.starts_with('/') => Some(path.to_owned()),
            Some(path) => wsl::convert_windows_path(distro, path)
                .await
                .map(Some)
                .map_err(|_| failure(FailureCode::Spawn))?,
        };
        let mut child = wsl::spawn(
            distro,
            cwd.as_deref(),
            &request.job.command,
            &request.run.id,
            environment,
        )
        .map_err(|_| failure(FailureCode::Spawn))?;
        let handshake = match child.read_handshake(&request.run.id).await {
            Ok(value) => value,
            Err(_) => {
                child.abort_spawn().await;
                return Err(failure(FailureCode::Handshake));
            }
        };
        let mut handshake_stdout = handshake.consumed_stdout;
        let consumed_stdout = stdout_redactor.redact(&handshake_stdout);
        handshake_stdout.zeroize();
        if streams
            .append(LogStream::Stdout, &consumed_stdout)
            .await
            .is_err()
        {
            child.abort_spawn().await;
            return Err(failure(FailureCode::LogWrite));
        }
        let metadata = RunExecutionMetadata {
            log_dir: Some(relative_log_dir(&request.run.id)),
            target_pid: Some(i64::from(handshake.identity.pid)),
            target_pgid: Some(i64::from(handshake.identity.pgid)),
            target_sid: Some(i64::from(handshake.identity.sid)),
            process_marker: Some(handshake.identity.marker.clone()),
            ..RunExecutionMetadata::default()
        };
        let stdout = match child.take_stdout() {
            Some(stdout) => stdout,
            None => {
                child.abort_spawn().await;
                return Err(failure(FailureCode::Spawn));
            }
        };
        let stderr = match child.take_stderr() {
            Some(stderr) => stderr,
            None => {
                child.abort_spawn().await;
                return Err(failure(FailureCode::Spawn));
            }
        };
        let (distro, child, _, _) = child.into_parts();
        let handle = WslExecutionHandle::start(
            distro,
            child,
            handshake.identity,
            stdout,
            stderr,
            streams,
            stdout_redactor,
            stderr_redactor,
            metadata,
            self.termination_grace,
        );
        Ok(Arc::new(handle))
    }
}

impl ExecutionAdapter for PlatformExecutionAdapter {
    fn spawn(&self, request: ExecutionRequest) -> AdapterFuture<'_, Arc<dyn ExecutionHandle>> {
        let adapter = self.clone();
        Box::pin(async move { adapter.spawn_request(request).await })
    }

    fn recover_stale(&self, request: ExecutionRequest) -> AdapterFuture<'_, ()> {
        let adapter = self.clone();
        Box::pin(async move {
            match request.job.target_kind {
                TargetKind::Wsl => {
                    let pid = u32::try_from(
                        request
                            .run
                            .target_pid
                            .ok_or_else(|| failure(FailureCode::Storage))?,
                    )
                    .map_err(|_| failure(FailureCode::Storage))?;
                    let pgid = u32::try_from(
                        request
                            .run
                            .target_pgid
                            .ok_or_else(|| failure(FailureCode::Storage))?,
                    )
                    .map_err(|_| failure(FailureCode::Storage))?;
                    let sid = u32::try_from(
                        request
                            .run
                            .target_sid
                            .ok_or_else(|| failure(FailureCode::Storage))?,
                    )
                    .map_err(|_| failure(FailureCode::Storage))?;
                    let marker = request
                        .run
                        .process_marker
                        .clone()
                        .ok_or_else(|| failure(FailureCode::Storage))?;
                    let distro = request
                        .job
                        .target_distro
                        .as_deref()
                        .ok_or_else(|| failure(FailureCode::TargetUnavailable))?;
                    let identity = crate::core::shell::WslProcessIdentity {
                        pid,
                        pgid,
                        sid,
                        marker,
                    };
                    wsl::recover_stale_group(distro, &identity, adapter.termination_grace)
                        .await
                        .map_err(|_| failure(FailureCode::Termination))
                }
                TargetKind::Windows => {
                    #[cfg(windows)]
                    {
                        let pid = u32::try_from(
                            request
                                .run
                                .target_pid
                                .ok_or_else(|| failure(FailureCode::Storage))?,
                        )
                        .map_err(|_| failure(FailureCode::Storage))?;
                        let created_at = request
                            .run
                            .target_process_created_at
                            .ok_or_else(|| failure(FailureCode::Storage))?;
                        windows::recover_stale(pid, created_at, adapter.termination_grace)
                            .map_err(|_| failure(FailureCode::Termination))
                    }
                    #[cfg(not(windows))]
                    {
                        Err(failure(FailureCode::TargetUnavailable))
                    }
                }
            }
        })
    }
}

struct SharedTerminal {
    result: watch::Sender<Option<Result<ExecutionExit, AdapterError>>>,
    terminate_requests: mpsc::UnboundedSender<TerminateRequest>,
}

type TerminalWatchReceiver = watch::Receiver<Option<Result<ExecutionExit, AdapterError>>>;
type TerminateRequestReceiver = mpsc::UnboundedReceiver<TerminateRequest>;

struct TerminateRequest {
    response: oneshot::Sender<Result<ExecutionExit, AdapterError>>,
}

impl SharedTerminal {
    fn new() -> (Self, TerminalWatchReceiver, TerminateRequestReceiver) {
        let (result, receiver) = watch::channel(None);
        let (terminate_requests, terminate_rx) = mpsc::unbounded_channel();
        (
            Self {
                result,
                terminate_requests,
            },
            receiver,
            terminate_rx,
        )
    }

    fn request_terminate(&self) -> AdapterFuture<'static, ExecutionExit> {
        let terminal = self.result.subscribe();
        if let Some(result) = terminal.borrow().clone() {
            return Box::pin(async move { result });
        }
        let (response, receiver) = oneshot::channel();
        if self
            .terminate_requests
            .send(TerminateRequest { response })
            .is_err()
        {
            return Box::pin(async move {
                SharedTerminal::wait(terminal)
                    .await
                    .map_err(|_| failure(FailureCode::Termination))
            });
        }
        let terminal_for_response = terminal.clone();
        Box::pin(async move {
            tokio::select! {
                response = receiver => match response {
                    Ok(result) => result,
                    Err(_) => SharedTerminal::wait(terminal_for_response).await,
                },
                terminal = SharedTerminal::wait(terminal) => terminal,
            }
        })
    }

    async fn wait(
        mut receiver: watch::Receiver<Option<Result<ExecutionExit, AdapterError>>>,
    ) -> Result<ExecutionExit, AdapterError> {
        loop {
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            receiver
                .changed()
                .await
                .map_err(|_| failure(FailureCode::Wait))?;
        }
    }
}

type ChildWaitResult = Result<ExecutionExit, AdapterError>;

/// Own the WSL wrapper in one task so a monitor can request direct wrapper
/// reaping without borrowing the child while the normal wait is pending.
/// Process-group termination remains a separate, identity-checked operation.
struct ReapRequest {
    response: oneshot::Sender<ChildWaitResult>,
}

fn spawn_wsl_wait_owner(
    mut child: Child,
    mut reap_rx: mpsc::UnboundedReceiver<ReapRequest>,
    wait_tx: oneshot::Sender<ChildWaitResult>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                result = child.wait() => {
                    let result = result
                        .map(|status| ExecutionExit { exit_code: status.code() })
                        .map_err(|_| failure(FailureCode::Wait));
                    let _ = wait_tx.send(result);
                    return;
                }
                Some(request) = reap_rx.recv() => request,
                else => {
                    let result = child
                        .wait()
                        .await
                        .map(|status| ExecutionExit { exit_code: status.code() })
                        .map_err(|_| failure(FailureCode::Wait));
                    let _ = wait_tx.send(result);
                    return;
                }
            };
            let result = match child.start_kill() {
                Ok(()) => {
                    match tokio::time::timeout(WAIT_OWNER_ABORT_TIMEOUT, child.wait()).await {
                        Ok(Ok(status)) => Ok(ExecutionExit {
                            exit_code: status.code(),
                        }),
                        Ok(Err(_)) => Err(failure(FailureCode::Wait)),
                        Err(_) => Err(failure(FailureCode::Termination)),
                    }
                }
                Err(_) => Err(failure(FailureCode::Termination)),
            };
            let retryable = result.is_err();
            let _ = event.response.send(result);
            if !retryable {
                return;
            }
        }
    })
}

async fn finish_wsl_wait_owner(wait_task: &mut JoinHandle<()>) {
    if tokio::time::timeout(WAIT_OWNER_ABORT_TIMEOUT, &mut *wait_task)
        .await
        .is_err()
    {
        wait_task.abort();
    }
}

/// Ask the wait owner to kill/reap the wrapper and bound the acknowledgement.
/// Aborting the owner is a final fallback; the WSL spawn path sets
/// `kill_on_drop`, so dropping its child cannot leave a wrapper orphaned.
async fn reap_wsl_wrapper(
    reap_tx: &mpsc::UnboundedSender<ReapRequest>,
    wait_task: &mut JoinHandle<()>,
    timeout: Duration,
) -> Result<ExecutionExit, AdapterError> {
    let (response, wait_rx) = oneshot::channel();
    if reap_tx.send(ReapRequest { response }).is_err() {
        return Err(failure(FailureCode::Wait));
    }
    match tokio::time::timeout(timeout, wait_rx).await {
        Ok(Ok(result)) => {
            finish_wsl_wait_owner(wait_task).await;
            result
        }
        Ok(Err(_)) => Err(failure(FailureCode::Wait)),
        Err(_) => Err(failure(FailureCode::Termination)),
    }
}

fn spawn_drain_error_channel() -> (mpsc::UnboundedSender<()>, mpsc::UnboundedReceiver<()>) {
    mpsc::unbounded_channel()
}

fn abort_drain_tasks(tasks: &mut [JoinHandle<Result<(), AdapterError>>]) {
    for task in tasks {
        task.abort();
    }
}

enum DrainControl {
    Complete(Result<(), AdapterError>),
    Terminate(TerminateRequest),
    Failure,
}

async fn join_drains_with_control(
    tasks: &mut [JoinHandle<Result<(), AdapterError>>],
    terminate_rx: &mut mpsc::UnboundedReceiver<TerminateRequest>,
    failure_rx: &mut mpsc::UnboundedReceiver<()>,
) -> DrainControl {
    tokio::select! {
        result = join_drain_tasks(tasks) => DrainControl::Complete(result),
        Some(request) = terminate_rx.recv() => DrainControl::Terminate(request),
        Some(_) = failure_rx.recv() => DrainControl::Failure,
    }
}

async fn join_drain_tasks(
    tasks: &mut [JoinHandle<Result<(), AdapterError>>],
) -> Result<(), AdapterError> {
    let mut first_error = None;
    for task in tasks {
        match task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) if first_error.is_none() => first_error = Some(error),
            Ok(Err(_)) => {}
            Err(_) if first_error.is_none() => first_error = Some(failure(FailureCode::LogWrite)),
            Err(_) => {}
        }
    }
    first_error.map_or(Ok(()), Err)
}

async fn join_drain_tasks_bounded(
    tasks: &mut [JoinHandle<Result<(), AdapterError>>],
    timeout: Duration,
) -> Result<(), AdapterError> {
    match tokio::time::timeout(timeout, join_drain_tasks(tasks)).await {
        Ok(result) => result,
        Err(_) => {
            abort_drain_tasks(tasks);
            Err(failure(FailureCode::LogWrite))
        }
    }
}

async fn drain_async_reader<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    mut reader: R,
    stream: LogStreamHandle,
    failure_tx: mpsc::UnboundedSender<()>,
    mut redactor: SecretRedactor,
) -> Result<(), AdapterError> {
    use tokio::io::AsyncReadExt;
    let mut buffer = Zeroizing::new(vec![0_u8; DRAIN_BUFFER_BYTES]);
    loop {
        let read = match reader.read(&mut buffer).await {
            Ok(read) => read,
            Err(_) => {
                let _ = failure_tx.send(());
                return Err(failure(FailureCode::LogWrite));
            }
        };
        if read == 0 {
            buffer.zeroize();
            let tail = redactor.finish();
            if tail.is_empty() {
                return Ok(());
            }
            if stream.append(&tail).await.is_err() {
                let _ = failure_tx.send(());
                return Err(failure(FailureCode::LogWrite));
            }
            return Ok(());
        }
        let redacted = redactor.redact(&buffer[..read]);
        buffer[..read].zeroize();
        if redacted.is_empty() {
            continue;
        }
        if stream.append(&redacted).await.is_err() {
            let _ = failure_tx.send(());
            return Err(failure(FailureCode::LogWrite));
        }
    }
}

#[cfg(windows)]
fn spawn_windows_drain(
    file: std::fs::File,
    stream: LogStreamHandle,
    failure_tx: mpsc::UnboundedSender<()>,
    mut redactor: SecretRedactor,
) -> JoinHandle<Result<(), AdapterError>> {
    let (tx, mut rx) = mpsc::channel::<Zeroizing<Vec<u8>>>(8);
    let reader = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Read;
        let mut file = file;
        let mut buffer = Zeroizing::new(vec![0_u8; DRAIN_BUFFER_BYTES]);
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                return Ok(());
            }
            let chunk = Zeroizing::new(buffer[..read].to_vec());
            buffer[..read].zeroize();
            if tx.blocking_send(chunk).is_err() {
                return Ok(());
            }
        }
    });
    tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let mut chunk = chunk;
            let redacted = redactor.redact(&chunk);
            chunk.zeroize();
            if stream.append(&redacted).await.is_err() {
                let _ = failure_tx.send(());
                return Err(failure(FailureCode::LogWrite));
            }
        }
        let tail = redactor.finish();
        if !tail.is_empty() && stream.append(&tail).await.is_err() {
            let _ = failure_tx.send(());
            return Err(failure(FailureCode::LogWrite));
        }
        match reader.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) | Err(_) => {
                let _ = failure_tx.send(());
                Err(failure(FailureCode::LogWrite))
            }
        }
    })
}

#[cfg(windows)]
struct WindowsExecutionHandle {
    shared: Arc<SharedTerminal>,
    metadata: RunExecutionMetadata,
}

#[cfg(windows)]
impl WindowsExecutionHandle {
    #[allow(clippy::too_many_arguments)]
    fn start_with_files(
        child: windows::WindowsChild,
        streams: LogStreams,
        stdout: std::fs::File,
        stderr: std::fs::File,
        stdout_redactor: SecretRedactor,
        stderr_redactor: SecretRedactor,
        metadata: RunExecutionMetadata,
        grace: Duration,
    ) -> Self {
        let child = Arc::new(child);
        let (shared, _receiver, mut terminate_rx) = SharedTerminal::new();
        let (failure_tx, mut failure_rx) = spawn_drain_error_channel();
        let mut drains = vec![
            spawn_windows_drain(
                stdout,
                streams.handle(LogStream::Stdout),
                failure_tx.clone(),
                stdout_redactor,
            ),
            spawn_windows_drain(
                stderr,
                streams.handle(LogStream::Stderr),
                failure_tx,
                stderr_redactor,
            ),
        ];
        let result_tx = shared.result.clone();
        let child_for_wait = Arc::clone(&child);
        tokio::spawn(async move {
            let (wait_tx, mut wait_rx) = tokio::sync::oneshot::channel();
            tokio::task::spawn_blocking(move || {
                let result = child_for_wait
                    .wait(None)
                    .map(|code| ExecutionExit {
                        exit_code: code.map(|value| value as i32),
                    })
                    .map_err(|_| failure(FailureCode::Wait));
                let _ = wait_tx.send(result);
            });
            let mut termination_response = None;
            let mut log_failure = false;
            let mut wait_complete = false;
            let mut natural_result = None;
            let mut retry_tick = tokio::time::interval(Duration::from_millis(100));
            retry_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let (mut outcome, process_waited) = loop {
                enum MonitorEvent {
                    Wait(ChildWaitResult),
                    Terminate(TerminateRequest),
                    LogFailure,
                    RetryTree,
                }
                let event = tokio::select! {
                    result = &mut wait_rx, if !wait_complete => MonitorEvent::Wait(
                        result.unwrap_or_else(|_| Err(failure(FailureCode::Wait)))
                    ),
                    Some(request) = terminate_rx.recv() => MonitorEvent::Terminate(request),
                    Some(_) = failure_rx.recv() => MonitorEvent::LogFailure,
                    _ = retry_tick.tick(), if wait_complete => MonitorEvent::RetryTree,
                };
                match event {
                    MonitorEvent::Wait(mut result) => {
                        wait_complete = true;
                        natural_result = Some(result.clone());
                        let child_for_tree = Arc::clone(&child);
                        let tree_gone = matches!(
                            tokio::task::spawn_blocking(move || {
                                child_for_tree.ensure_tree_gone(grace)
                            })
                            .await,
                            Ok(Ok(()))
                        );
                        if !tree_gone {
                            // The root has exited (or wait itself failed), but
                            // the Job Object still has members or could not be
                            // queried. Keep the actor alive for an explicit
                            // retry; publishing a terminal result here would
                            // lose the only safe tree owner.
                            continue;
                        }
                        if let Err(error) = result {
                            result = Err(error.with_cleanup_confirmed());
                        }
                        if log_failure && result.is_ok() {
                            result = Err(AdapterError::confirmed(FailureCode::LogWrite.as_str()));
                        }
                        break (result, true);
                    }
                    MonitorEvent::RetryTree => {
                        let Some(mut result) = natural_result.clone() else {
                            continue;
                        };
                        let child_for_tree = Arc::clone(&child);
                        let tree_gone = matches!(
                            tokio::task::spawn_blocking(move || {
                                child_for_tree.ensure_tree_gone(grace)
                            })
                            .await,
                            Ok(Ok(()))
                        );
                        if !tree_gone {
                            continue;
                        }
                        if let Err(error) = result {
                            result = Err(error.with_cleanup_confirmed());
                        }
                        if log_failure && result.is_ok() {
                            result = Err(AdapterError::confirmed(FailureCode::LogWrite.as_str()));
                        }
                        break (result, true);
                    }
                    MonitorEvent::Terminate(request) => {
                        let killed = if wait_complete {
                            let child_for_tree = Arc::clone(&child);
                            tokio::task::spawn_blocking(move || {
                                child_for_tree.ensure_tree_gone(grace)
                            })
                            .await
                            .map(|result| result.map(|()| 0))
                        } else {
                            let child_for_term = Arc::clone(&child);
                            tokio::task::spawn_blocking(move || {
                                child_for_term.terminate_and_wait(grace)
                            })
                            .await
                        };
                        match killed {
                            Ok(Ok(code)) => {
                                let result = if log_failure {
                                    Err(AdapterError::confirmed(FailureCode::LogWrite.as_str()))
                                } else {
                                    Ok(ExecutionExit {
                                        exit_code: Some(code as i32),
                                    })
                                };
                                termination_response = Some(request.response);
                                break (result, false);
                            }
                            _ => {
                                let _ = request
                                    .response
                                    .send(Err(failure(FailureCode::Termination)));
                                // Keep the process/job and reader ownership in
                                // this actor so a later stop/shutdown request
                                // performs a real second attempt.
                            }
                        }
                    }
                    MonitorEvent::LogFailure => {
                        log_failure = true;
                        let child_for_term = Arc::clone(&child);
                        let killed = tokio::task::spawn_blocking(move || {
                            child_for_term.terminate_and_wait(grace)
                        })
                        .await;
                        if matches!(&killed, Ok(Ok(_))) {
                            break (
                                Err(AdapterError::confirmed(FailureCode::LogWrite.as_str())),
                                false,
                            );
                        }
                        // Do not abort the readers or publish a terminal
                        // error while the process tree is unverified.  The
                        // next terminate request, or the natural wait path,
                        // retries with the same Job Object.
                    }
                }
            };
            let drains_result = if process_waited {
                match join_drains_with_control(&mut drains, &mut terminate_rx, &mut failure_rx)
                    .await
                {
                    DrainControl::Complete(result) => result,
                    DrainControl::Terminate(request) => {
                        let child_for_term = Arc::clone(&child);
                        let killed = tokio::task::spawn_blocking(move || {
                            child_for_term.terminate_and_wait(grace)
                        })
                        .await;
                        if matches!(&killed, Ok(Ok(_))) {
                            outcome = Ok(ExecutionExit { exit_code: None });
                            termination_response = Some(request.response);
                        } else {
                            let _ = request
                                .response
                                .send(Err(failure(FailureCode::Termination)));
                            outcome = Err(failure(FailureCode::Termination));
                            abort_drain_tasks(&mut drains);
                        }
                        join_drain_tasks_bounded(&mut drains, grace).await
                    }
                    DrainControl::Failure => {
                        let child_for_term = Arc::clone(&child);
                        let killed = tokio::task::spawn_blocking(move || {
                            child_for_term.terminate_and_wait(grace)
                        })
                        .await;
                        if !matches!(&killed, Ok(Ok(_))) {
                            abort_drain_tasks(&mut drains);
                        }
                        outcome = Err(AdapterError::confirmed(FailureCode::LogWrite.as_str()));
                        join_drain_tasks_bounded(&mut drains, grace).await
                    }
                }
            } else {
                join_drain_tasks_bounded(&mut drains, grace).await
            };
            let final_result = match (outcome, drains_result) {
                (Err(error), _) => Err(error),
                (Ok(exit), Ok(())) => Ok(exit),
                // Once the process/tree wait path has completed, a log
                // drain failure is still a cleanup witness. Preserve that
                // fact explicitly instead of making the scheduler guess
                // from the error category.
                (Ok(_), Err(error)) => Err(error.with_cleanup_confirmed()),
            };
            if let Some(response) = termination_response {
                let _ = response.send(final_result.clone());
            }
            let _ = result_tx.send(Some(final_result));
        });
        Self {
            shared: Arc::new(shared),
            metadata,
        }
    }
}

#[cfg(windows)]
impl ExecutionHandle for WindowsExecutionHandle {
    fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
        self.shared.request_terminate()
    }

    fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
        let receiver = self.shared.result.subscribe();
        Box::pin(async move { SharedTerminal::wait(receiver).await })
    }

    fn metadata(&self) -> RunExecutionMetadata {
        self.metadata.clone()
    }
}

struct WslExecutionHandle {
    shared: Arc<SharedTerminal>,
    metadata: RunExecutionMetadata,
}

impl WslExecutionHandle {
    #[allow(clippy::too_many_arguments)]
    fn start(
        distro: String,
        child: tokio::process::Child,
        identity: crate::core::shell::WslProcessIdentity,
        stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
        stderr: tokio::process::ChildStderr,
        streams: LogStreams,
        stdout_redactor: SecretRedactor,
        stderr_redactor: SecretRedactor,
        metadata: RunExecutionMetadata,
        grace: Duration,
    ) -> Self {
        let (shared, _receiver, mut terminate_rx) = SharedTerminal::new();
        let (failure_tx, mut failure_rx) = spawn_drain_error_channel();
        let mut drains = vec![
            tokio::spawn(drain_async_reader(
                stdout,
                streams.handle(LogStream::Stdout),
                failure_tx.clone(),
                stdout_redactor,
            )),
            tokio::spawn(drain_async_reader(
                stderr,
                streams.handle(LogStream::Stderr),
                failure_tx,
                stderr_redactor,
            )),
        ];
        let result_tx = shared.result.clone();
        tokio::spawn(async move {
            let (wait_tx, mut wait_rx) = oneshot::channel();
            let (reap_tx, reap_rx) = mpsc::unbounded_channel();
            let mut wait_task = spawn_wsl_wait_owner(child, reap_rx, wait_tx);
            let mut log_failure = false;
            let mut wait_complete = false;
            let mut termination_response = None;
            let mut natural_result = None;
            let mut retry_tick = tokio::time::interval(Duration::from_millis(100));
            retry_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let (mut outcome, process_waited) = loop {
                enum MonitorEvent {
                    Wait(ChildWaitResult),
                    Terminate(TerminateRequest),
                    LogFailure,
                    RetryTree,
                }
                let event = tokio::select! {
                    result = &mut wait_rx, if !wait_complete => MonitorEvent::Wait(
                        result.unwrap_or_else(|_| Err(failure(FailureCode::Wait)))
                    ),
                    Some(request) = terminate_rx.recv() => MonitorEvent::Terminate(request),
                    Some(_) = failure_rx.recv() => MonitorEvent::LogFailure,
                    _ = retry_tick.tick(), if wait_complete => MonitorEvent::RetryTree,
                };
                match event {
                    MonitorEvent::Wait(mut result) => {
                        wait_complete = true;
                        natural_result = Some(result.clone());
                        if wsl::confirm_group_gone(&distro, &identity, grace)
                            .await
                            .is_err()
                        {
                            // The wrapper exited (or its wait failed) before
                            // its supervisor proved that the exact group was
                            // empty. Keep the actor alive for a bounded probe
                            // retry instead of terminalizing an unverified
                            // descendant tree.
                            continue;
                        }
                        if let Err(error) = result {
                            result = Err(error.with_cleanup_confirmed());
                        }
                        if log_failure && result.is_ok() {
                            result = Err(AdapterError::confirmed(FailureCode::LogWrite.as_str()));
                        }
                        finish_wsl_wait_owner(&mut wait_task).await;
                        break (result, true);
                    }
                    MonitorEvent::RetryTree => {
                        let Some(mut result) = natural_result.clone() else {
                            continue;
                        };
                        if wsl::confirm_group_gone(&distro, &identity, grace)
                            .await
                            .is_err()
                        {
                            continue;
                        }
                        if let Err(error) = result {
                            result = Err(error.with_cleanup_confirmed());
                        }
                        if log_failure && result.is_ok() {
                            result = Err(AdapterError::confirmed(FailureCode::LogWrite.as_str()));
                        }
                        finish_wsl_wait_owner(&mut wait_task).await;
                        break (result, true);
                    }
                    MonitorEvent::Terminate(request) => {
                        let cleaned = if wait_complete {
                            wsl::confirm_group_gone(&distro, &identity, grace)
                                .await
                                .map(|()| ExecutionExit { exit_code: None })
                                .map_err(|_| failure(FailureCode::Termination))
                        } else if wsl::terminate_group(&distro, &identity, grace)
                            .await
                            .is_ok()
                        {
                            reap_wsl_wrapper(&reap_tx, &mut wait_task, grace).await
                        } else {
                            Err(failure(FailureCode::Termination))
                        };
                        let result = match cleaned {
                            Ok(_) if log_failure => {
                                Err(AdapterError::confirmed(FailureCode::LogWrite.as_str()))
                            }
                            Ok(exit) => Ok(exit),
                            Err(error) => {
                                let _ = request.response.send(Err(error));
                                continue;
                            }
                        };
                        termination_response = Some(request.response);
                        break (result, false);
                    }
                    MonitorEvent::LogFailure => {
                        log_failure = true;
                        let cleaned = if wait_complete {
                            wsl::confirm_group_gone(&distro, &identity, grace)
                                .await
                                .is_ok()
                        } else if wsl::terminate_group(&distro, &identity, grace)
                            .await
                            .is_ok()
                        {
                            reap_wsl_wrapper(&reap_tx, &mut wait_task, grace)
                                .await
                                .is_ok()
                        } else {
                            false
                        };
                        if cleaned {
                            break (
                                Err(AdapterError::confirmed(FailureCode::LogWrite.as_str())),
                                false,
                            );
                        }
                        // Preserve the wait owner and readers while identity
                        // or group cleanup is unverified.  A later stop or
                        // natural wrapper event can retry the same boundary.
                    }
                }
            };
            let drains_result = if process_waited {
                match join_drains_with_control(&mut drains, &mut terminate_rx, &mut failure_rx)
                    .await
                {
                    DrainControl::Complete(result) => result,
                    DrainControl::Terminate(request) => {
                        let killed = wsl::terminate_group(&distro, &identity, grace).await;
                        if killed.is_ok() {
                            outcome = Ok(ExecutionExit { exit_code: None });
                            termination_response = Some(request.response);
                        } else {
                            let _ = request
                                .response
                                .send(Err(failure(FailureCode::Termination)));
                            outcome = Err(failure(FailureCode::Termination));
                            abort_drain_tasks(&mut drains);
                        }
                        join_drain_tasks_bounded(&mut drains, grace).await
                    }
                    DrainControl::Failure => {
                        let killed = wsl::terminate_group(&distro, &identity, grace).await;
                        if killed.is_err() {
                            abort_drain_tasks(&mut drains);
                        }
                        outcome = Err(AdapterError::confirmed(FailureCode::LogWrite.as_str()));
                        join_drain_tasks_bounded(&mut drains, grace).await
                    }
                }
            } else {
                join_drain_tasks_bounded(&mut drains, grace).await
            };
            let final_result = match (outcome, drains_result) {
                (Err(error), _) => Err(error),
                (Ok(exit), Ok(())) => Ok(exit),
                (Ok(_), Err(error)) => Err(error.with_cleanup_confirmed()),
            };
            if let Some(response) = termination_response {
                let _ = response.send(final_result.clone());
            }
            let _ = result_tx.send(Some(final_result));
        });
        Self {
            shared: Arc::new(shared),
            metadata,
        }
    }
}

impl ExecutionHandle for WslExecutionHandle {
    fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
        self.shared.request_terminate()
    }

    fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
        let receiver = self.shared.result.subscribe();
        Box::pin(async move { SharedTerminal::wait(receiver).await })
    }

    fn metadata(&self) -> RunExecutionMetadata {
        self.metadata.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::process::Stdio;
    use tempfile::tempdir;
    use tokio::io::{duplex, AsyncWriteExt};

    #[tokio::test]
    async fn async_reader_drains_every_byte_until_eof() {
        let root = tempdir().unwrap();
        let streams = LogStreams::open(
            root.path(),
            "fixture",
            crate::logs::LogLimits {
                segment_bytes: 4,
                max_segments: 8,
            },
        )
        .unwrap();
        let (mut writer, reader) = duplex(8);
        let writer_task = tokio::spawn(async move {
            writer.write_all(b"prefix\0binary\noutput").await.unwrap();
        });
        let (failure_tx, mut failure_rx) = spawn_drain_error_channel();
        drain_async_reader(
            reader,
            streams.handle(LogStream::Stdout),
            failure_tx,
            SecretRedactor::from_environment(&std::collections::BTreeMap::new()),
        )
        .await
        .unwrap();
        writer_task.await.unwrap();
        assert!(failure_rx.try_recv().is_err());
        let response = streams
            .tail_log(LogStream::Stdout, Some("0"), usize::MAX)
            .await
            .unwrap();
        assert_eq!(response.data, b"prefix\0binary\noutput");
        assert_eq!(response.next_cursor, "20");
    }

    #[test]
    fn redacts_secrets_across_pipe_chunks_and_prefers_longest_value() {
        let environment = std::collections::BTreeMap::from([
            ("SHORT".to_owned(), "abc".to_owned()),
            ("LONG".to_owned(), "abcdef".to_owned()),
        ]);
        let mut redactor = SecretRedactor::from_environment(&environment);
        let mut output = redactor.redact(b"prefix-ab");
        output.extend(redactor.redact(b"c-suffix-abcdef-tail"));
        output.extend(redactor.finish());
        assert_eq!(output, b"prefix-<redacted>-suffix-<redacted>-tail");
        assert!(!output.windows(b"abc".len()).any(|window| window == b"abc"));
        assert!(!output
            .windows(b"abcdef".len())
            .any(|window| window == b"abcdef"));
    }

    #[tokio::test]
    async fn concurrent_wait_and_terminate_share_one_terminal_result() {
        let (shared, _receiver, mut terminate_rx) = SharedTerminal::new();
        let monitor = Arc::new(shared);
        let monitor_for_task = Arc::clone(&monitor);
        tokio::spawn(async move {
            if let Some(request) = terminate_rx.recv().await {
                let result = Ok(ExecutionExit { exit_code: Some(0) });
                let _ = request.response.send(result.clone());
                let _ = monitor_for_task.result.send(Some(result));
            }
        });
        let handle = Arc::new(TestSharedHandle { shared: monitor });
        let wait = Arc::clone(&handle);
        let terminate = Arc::clone(&handle);
        let (wait, terminated) = tokio::join!(wait.wait(), terminate.terminate());
        assert_eq!(wait.unwrap().exit_code, Some(0));
        assert_eq!(terminated.unwrap().exit_code, Some(0));
    }

    #[tokio::test]
    async fn terminate_failure_does_not_cache_and_second_request_retries_actor() {
        let (shared, _receiver, mut terminate_rx) = SharedTerminal::new();
        let monitor = Arc::new(shared);
        let monitor_for_task = Arc::clone(&monitor);
        tokio::spawn(async move {
            let first = terminate_rx.recv().await.expect("first terminate request");
            let _ = first.response.send(Err(failure(FailureCode::Termination)));
            let second = terminate_rx.recv().await.expect("second terminate request");
            let result = Ok(ExecutionExit { exit_code: Some(7) });
            let _ = second.response.send(result.clone());
            let _ = monitor_for_task.result.send(Some(result));
        });
        let handle = TestSharedHandle { shared: monitor };
        assert!(handle.terminate().await.is_err());
        let retry = handle.terminate().await.unwrap();
        assert_eq!(retry.exit_code, Some(7));
        assert_eq!(handle.wait().await.unwrap().exit_code, Some(7));
    }

    #[tokio::test]
    async fn terminate_after_terminal_returns_the_published_result() {
        let (shared, _receiver, mut terminate_rx) = SharedTerminal::new();
        let monitor = Arc::new(shared);
        let monitor_for_task = Arc::clone(&monitor);
        tokio::spawn(async move {
            let request = terminate_rx.recv().await.expect("terminate request");
            let result = Ok(ExecutionExit { exit_code: Some(3) });
            let _ = request.response.send(result.clone());
            let _ = monitor_for_task.result.send(Some(result));
        });
        let handle = TestSharedHandle {
            shared: Arc::clone(&monitor),
        };
        assert_eq!(handle.terminate().await.unwrap().exit_code, Some(3));
        assert_eq!(handle.terminate().await.unwrap().exit_code, Some(3));
    }

    #[tokio::test]
    async fn concurrent_terminate_requests_share_terminal_result() {
        let (shared, _receiver, mut terminate_rx) = SharedTerminal::new();
        let monitor = Arc::new(shared);
        let monitor_for_task = Arc::clone(&monitor);
        tokio::spawn(async move {
            let request = terminate_rx.recv().await.expect("terminate request");
            let result = Ok(ExecutionExit { exit_code: Some(4) });
            let _ = request.response.send(result.clone());
            let _ = monitor_for_task.result.send(Some(result));
        });
        let first = TestSharedHandle {
            shared: Arc::clone(&monitor),
        };
        let second = TestSharedHandle { shared: monitor };
        let (first, second) = tokio::join!(first.terminate(), second.terminate());
        assert_eq!(first.unwrap().exit_code, Some(4));
        assert_eq!(second.unwrap().exit_code, Some(4));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wsl_wrapper_hang_is_reaped_after_group_termination() {
        let mut command = tokio::process::Command::new("sleep");
        command
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = command.spawn().unwrap();
        let (wait_tx, _wait_rx) = oneshot::channel();
        let (reap_tx, reap_rx) = mpsc::unbounded_channel();
        let mut wait_task = spawn_wsl_wait_owner(child, reap_rx, wait_tx);

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            reap_wsl_wrapper(&reap_tx, &mut wait_task, Duration::from_millis(250)),
        )
        .await
        .expect("wrapper reap must be bounded")
        .expect("wrapper reap must succeed");
        assert_eq!(result.exit_code, None);
        assert!(wait_task.is_finished());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropped_reap_sender_does_not_kill_the_wrapper() {
        let mut command = tokio::process::Command::new("sh");
        command
            .args(["-c", "sleep 0.05; exit 7"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = command.spawn().unwrap();
        let (wait_tx, mut wait_rx) = oneshot::channel();
        let (reap_tx, reap_rx) = mpsc::unbounded_channel();
        drop(reap_tx);
        let mut wait_task = spawn_wsl_wait_owner(child, reap_rx, wait_tx);
        let result = tokio::time::timeout(Duration::from_secs(1), &mut wait_rx)
            .await
            .expect("natural wait must be bounded")
            .unwrap()
            .unwrap();
        let _ = (&mut wait_task).await;
        assert_eq!(result.exit_code, Some(7));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn wsl_wrapper_wait_owner_reports_natural_exit() {
        let mut command = tokio::process::Command::new("sh");
        command
            .args(["-c", "exit 7"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = command.spawn().unwrap();
        let (wait_tx, mut wait_rx) = oneshot::channel();
        let (_reap_tx, reap_rx) = mpsc::unbounded_channel();
        let mut wait_task = spawn_wsl_wait_owner(child, reap_rx, wait_tx);
        let result = tokio::time::timeout(Duration::from_secs(1), &mut wait_rx)
            .await
            .expect("natural wait must be bounded")
            .unwrap()
            .unwrap();
        let _ = (&mut wait_task).await;
        assert_eq!(result.exit_code, Some(7));
    }

    struct TestSharedHandle {
        shared: Arc<SharedTerminal>,
    }

    impl ExecutionHandle for TestSharedHandle {
        fn terminate(&self) -> AdapterFuture<'_, ExecutionExit> {
            self.shared.request_terminate()
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            let receiver = self.shared.result.subscribe();
            Box::pin(async move { SharedTerminal::wait(receiver).await })
        }
    }
}
