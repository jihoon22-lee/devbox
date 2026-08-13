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
use tokio::sync::{mpsc, oneshot, watch, Notify};
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
    #[cfg(not(windows))]
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
            #[cfg(not(windows))]
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

/// The scheduler-facing process adapter. It is intentionally not registered
/// with lifecycle's idle scheduler in this phase; setup/wiring can inject the
/// same value when the lifecycle replacement lands in its own feature.
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
}

struct SharedTerminal {
    result: watch::Sender<Option<Result<ExecutionExit, AdapterError>>>,
    terminate: Arc<Notify>,
}

impl SharedTerminal {
    fn new() -> (
        Self,
        watch::Receiver<Option<Result<ExecutionExit, AdapterError>>>,
    ) {
        let (result, receiver) = watch::channel(None);
        (
            Self {
                result,
                terminate: Arc::new(Notify::new()),
            },
            receiver,
        )
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
fn spawn_wsl_wait_owner(
    mut child: Child,
    mut reap_rx: oneshot::Receiver<()>,
    wait_tx: oneshot::Sender<ChildWaitResult>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = tokio::select! {
            result = child.wait() => result
                .map(|status| ExecutionExit { exit_code: status.code() })
                .map_err(|_| failure(FailureCode::Wait)),
            reap_request = &mut reap_rx => match reap_request {
                Ok(()) => {
                    // Group termination has already validated the WSL
                    // identity. The direct wrapper is ours to reap only for
                    // this explicit request after that validation.
                    let kill_result = child.start_kill();
                    let wait_result = child.wait().await;
                    match kill_result {
                        Ok(()) => wait_result
                            .map(|status| ExecutionExit { exit_code: status.code() })
                            .map_err(|_| failure(FailureCode::Wait)),
                        Err(_) => Err(failure(FailureCode::Termination)),
                    }
                }
                // A dropped sender means group validation failed. Do not
                // signal the wrapper in that fail-closed path; let it exit
                // naturally while the detached owner preserves ownership.
                Err(_) => child
                    .wait()
                    .await
                    .map(|status| ExecutionExit { exit_code: status.code() })
                    .map_err(|_| failure(FailureCode::Wait)),
            },
        };
        let _ = wait_tx.send(result);
    })
}

/// Cancel a wait owner without making a failed identity/termination path wait
/// forever. The WSL child was spawned with `kill_on_drop`, so a task that does
/// not acknowledge cancellation still cannot retain the wrapper indefinitely.
async fn abort_wsl_wait_owner(wait_task: &mut JoinHandle<()>) {
    wait_task.abort();
    let _ = tokio::time::timeout(WAIT_OWNER_ABORT_TIMEOUT, &mut *wait_task).await;
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
    reap_tx: &mut Option<oneshot::Sender<()>>,
    wait_rx: &mut oneshot::Receiver<ChildWaitResult>,
    wait_task: &mut JoinHandle<()>,
    timeout: Duration,
) -> Result<ExecutionExit, AdapterError> {
    if let Some(sender) = reap_tx.take() {
        let _ = sender.send(());
    }
    match tokio::time::timeout(timeout, &mut *wait_rx).await {
        Ok(Ok(result)) => {
            finish_wsl_wait_owner(wait_task).await;
            result
        }
        Ok(Err(_)) => {
            finish_wsl_wait_owner(wait_task).await;
            Err(failure(FailureCode::Wait))
        }
        Err(_) => {
            abort_wsl_wait_owner(wait_task).await;
            Err(failure(FailureCode::Termination))
        }
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
    Terminate,
    Failure,
}

async fn join_drains_with_control(
    tasks: &mut [JoinHandle<Result<(), AdapterError>>],
    terminate: &Notify,
    failure_rx: &mut mpsc::UnboundedReceiver<()>,
) -> DrainControl {
    tokio::select! {
        result = join_drain_tasks(tasks) => DrainControl::Complete(result),
        _ = terminate.notified() => DrainControl::Terminate,
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
        let (shared, _receiver) = SharedTerminal::new();
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
        let term = Arc::clone(&shared.terminate);
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
            let (mut outcome, process_waited) = tokio::select! {
                result = &mut wait_rx => (
                    result.unwrap_or_else(|_| Err(failure(FailureCode::Wait))),
                    true,
                ),
                _ = term.notified() => {
                    let child_for_term = Arc::clone(&child);
                    let killed = tokio::task::spawn_blocking(move || child_for_term.terminate_and_wait(grace)).await;
                    let outcome = match killed {
                        Ok(Ok(code)) => {
                            let _ = wait_rx.await;
                            Ok(ExecutionExit { exit_code: Some(code as i32) })
                        }
                        _ => {
                            abort_drain_tasks(&mut drains);
                            Err(failure(FailureCode::Termination))
                        }
                    };
                    (outcome, false)
                }
                Some(_) = failure_rx.recv() => {
                    let child_for_term = Arc::clone(&child);
                    let killed = tokio::task::spawn_blocking(move || child_for_term.terminate_and_wait(grace)).await;
                    let killed_ok = matches!(&killed, Ok(Ok(_)));
                    if !killed_ok {
                        abort_drain_tasks(&mut drains);
                    }
                    if killed_ok {
                        let _ = wait_rx.await;
                    }
                    (Err(failure(FailureCode::LogWrite)), false)
                }
            };
            let drains_result = if process_waited {
                match join_drains_with_control(&mut drains, &term, &mut failure_rx).await {
                    DrainControl::Complete(result) => result,
                    DrainControl::Terminate => {
                        let child_for_term = Arc::clone(&child);
                        let killed = tokio::task::spawn_blocking(move || {
                            child_for_term.terminate_and_wait(grace)
                        })
                        .await;
                        if matches!(&killed, Ok(Ok(_))) {
                            outcome = Ok(ExecutionExit { exit_code: None });
                        } else {
                            outcome = Err(failure(FailureCode::Termination));
                            abort_drain_tasks(&mut drains);
                        }
                        join_drain_tasks(&mut drains).await
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
                        outcome = Err(failure(FailureCode::LogWrite));
                        join_drain_tasks(&mut drains).await
                    }
                }
            } else {
                join_drain_tasks(&mut drains).await
            };
            let final_result = match (outcome, drains_result) {
                (Err(error), _) => Err(error),
                (Ok(exit), Ok(())) => Ok(exit),
                (Ok(_), Err(error)) => Err(error),
            };
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
        // `notify_one` retains a permit if terminate races monitor startup;
        // `notify_waiters` would lose that request before the monitor polls.
        self.shared.terminate.notify_one();
        let receiver = self.shared.result.subscribe();
        Box::pin(async move { SharedTerminal::wait(receiver).await })
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
        let (shared, _receiver) = SharedTerminal::new();
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
        let term = Arc::clone(&shared.terminate);
        tokio::spawn(async move {
            let (wait_tx, mut wait_rx) = oneshot::channel();
            let (reap_tx, reap_rx) = oneshot::channel();
            let mut reap_tx = Some(reap_tx);
            let mut wait_task = spawn_wsl_wait_owner(child, reap_rx, wait_tx);
            enum MonitorEvent {
                Wait(ChildWaitResult),
                Terminate,
                LogFailure,
            }
            let event = tokio::select! {
                result = &mut wait_rx => MonitorEvent::Wait(
                    result.unwrap_or_else(|_| Err(failure(FailureCode::Wait)))
                ),
                _ = term.notified() => MonitorEvent::Terminate,
                Some(_) = failure_rx.recv() => MonitorEvent::LogFailure,
            };
            let mut detached_cleanup = false;
            let (mut outcome, process_waited) = match event {
                MonitorEvent::Wait(result) => {
                    finish_wsl_wait_owner(&mut wait_task).await;
                    (result, true)
                }
                MonitorEvent::Terminate => {
                    let group_terminated = wsl::terminate_group(&distro, &identity, grace)
                        .await
                        .is_ok();
                    if !group_terminated {
                        reap_tx.take();
                        detached_cleanup = true;
                        (Err(failure(FailureCode::Termination)), false)
                    } else {
                        let wrapper_reaped =
                            reap_wsl_wrapper(&mut reap_tx, &mut wait_rx, &mut wait_task, grace)
                                .await
                                .is_ok();
                        if !wrapper_reaped {
                            abort_drain_tasks(&mut drains);
                            (Err(failure(FailureCode::Termination)), false)
                        } else {
                            (Ok(ExecutionExit { exit_code: None }), false)
                        }
                    }
                }
                MonitorEvent::LogFailure => {
                    let group_terminated = wsl::terminate_group(&distro, &identity, grace)
                        .await
                        .is_ok();
                    if !group_terminated {
                        reap_tx.take();
                        detached_cleanup = true;
                    } else {
                        let wrapper_reaped =
                            reap_wsl_wrapper(&mut reap_tx, &mut wait_rx, &mut wait_task, grace)
                                .await
                                .is_ok();
                        if !wrapper_reaped {
                            abort_drain_tasks(&mut drains);
                        }
                    }
                    (Err(failure(FailureCode::LogWrite)), false)
                }
            };
            let drains_result = if detached_cleanup {
                // The identity/group check failed, so closing pipes or
                // force-aborting readers could perturb an unverified process.
                // Drop JoinHandles to detach both drains and let their owned
                // streams finish naturally.
                drop(drains);
                Ok(())
            } else if process_waited {
                match join_drains_with_control(&mut drains, &term, &mut failure_rx).await {
                    DrainControl::Complete(result) => result,
                    DrainControl::Terminate => {
                        let killed = wsl::terminate_group(&distro, &identity, grace).await;
                        if killed.is_ok() {
                            outcome = Ok(ExecutionExit { exit_code: None });
                        } else {
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
                        outcome = Err(failure(FailureCode::LogWrite));
                        join_drain_tasks_bounded(&mut drains, grace).await
                    }
                }
            } else {
                join_drain_tasks_bounded(&mut drains, grace).await
            };
            let final_result = match (outcome, drains_result) {
                (Err(error), _) => Err(error),
                (Ok(exit), Ok(())) => Ok(exit),
                (Ok(_), Err(error)) => Err(error),
            };
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
        self.shared.terminate.notify_one();
        let receiver = self.shared.result.subscribe();
        Box::pin(async move { SharedTerminal::wait(receiver).await })
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
        let (shared, _receiver) = SharedTerminal::new();
        let monitor = Arc::new(shared);
        let monitor_for_task = Arc::clone(&monitor);
        tokio::spawn(async move {
            monitor_for_task.terminate.notified().await;
            let _ = monitor_for_task
                .result
                .send(Some(Ok(ExecutionExit { exit_code: Some(0) })));
        });
        let handle = Arc::new(TestSharedHandle { shared: monitor });
        let wait = Arc::clone(&handle);
        let terminate = Arc::clone(&handle);
        let (wait, terminated) = tokio::join!(wait.wait(), terminate.terminate());
        assert_eq!(wait.unwrap().exit_code, Some(0));
        assert_eq!(terminated.unwrap().exit_code, Some(0));
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
        let (wait_tx, mut wait_rx) = oneshot::channel();
        let (reap_tx, reap_rx) = oneshot::channel();
        let mut reap_tx = Some(reap_tx);
        let mut wait_task = spawn_wsl_wait_owner(child, reap_rx, wait_tx);

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            reap_wsl_wrapper(
                &mut reap_tx,
                &mut wait_rx,
                &mut wait_task,
                Duration::from_millis(250),
            ),
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
        let (reap_tx, reap_rx) = oneshot::channel();
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
        let (_reap_tx, reap_rx) = oneshot::channel();
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
            self.shared.terminate.notify_one();
            let receiver = self.shared.result.subscribe();
            Box::pin(async move { SharedTerminal::wait(receiver).await })
        }

        fn wait(&self) -> AdapterFuture<'_, ExecutionExit> {
            let receiver = self.shared.result.subscribe();
            Box::pin(async move { SharedTerminal::wait(receiver).await })
        }
    }
}
