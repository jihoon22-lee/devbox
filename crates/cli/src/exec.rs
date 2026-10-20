//! The systemd main process: capture output, persist an exit, and preserve the child's status.
mod secrets;
use devbox_runtime::{
    exit_record::{ExitRecord, ExitSource, ExitStore, Exited, now_ms, validate_identity},
    logfile::{MAX_LINE_BYTES, Stream, Writer},
};
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use secrets::{Redactor, Secrets};
use std::{
    ffi::OsString,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt, process::ExitStatusExt},
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    sync::mpsc,
};

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("invalid executable or command arguments")]
    Command,
    #[error("executable was not found or is not executable")]
    Executable,
    #[error(transparent)]
    Secret(#[from] secrets::SecretError),
    #[error(transparent)]
    Exit(#[from] devbox_runtime::exit_record::ExitError),
    #[error("execution I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("output reader failed")]
    Reader,
}
pub async fn run(
    run: String,
    attempt: Option<String>,
    secrets_stdin: bool,
    command: Vec<OsString>,
) -> anyhow::Result<ExitStatus> {
    validate_identity(&run)?;
    let attempt = attempt_id(attempt)?.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let executable = validate_command(&command)?;
    let secrets = if secrets_stdin {
        Secrets::read().await?
    } else {
        Secrets::default()
    };
    let stdout_mask = secrets.redactor()?;
    let stderr_mask = secrets.redactor()?;
    let paths = crate::paths()?;
    let store = ExitStore::new(&paths.data_dir.join("runs"), &run)?;
    // Install before spawn so even an immediate TERM is forwarded to the child group.
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let _execution = store.execution_guard()?;
    store.begin(&attempt)?;
    let mut writer = Writer::open(&store.log_path())?;
    writer.begin_attempt(&attempt)?;
    let mut child = Command::new(executable)
        .args(&command[1..])
        .envs(secrets.envs())
        .env("DEVBOX_RUN_ATTEMPT", &attempt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()?;
    let pid = Pid::from_raw(child.id().expect("spawned child has pid") as i32);
    let mut group = GroupGuard(Some(pid));
    let (send, mut receive) = mpsc::channel(4);
    let out = tokio::spawn(copy_stream(
        child.stdout.take().expect("stdout pipe"),
        Stream::Out,
        stdout_mask,
        send.clone(),
    ));
    let err = tokio::spawn(copy_stream(
        child.stderr.take().expect("stderr pipe"),
        Stream::Err,
        stderr_mask,
        send,
    ));
    let mut status = None;
    let mut drained = false;
    let mut cleanup_deadline = None;
    let mut drain_deadline = None;
    let mut stopped_readers = false;
    let result = async {
        while status.is_none() || !drained || cleanup_deadline.is_some() {
            tokio::select! {
                _ = term.recv() => { let _ = killpg(pid, Signal::SIGTERM); }
                result = child.wait(), if status.is_none() => {
                    // The direct child's status defines the run. Descendants must
                    // not keep MainPID alive through inherited output pipes.
                    status = Some(result?);
                    if killpg(pid, Signal::SIGTERM).is_ok() {
                        cleanup_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(200));
                    }
                    drain_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(1));
                }
                _ = deadline(cleanup_deadline), if cleanup_deadline.is_some() => {
                    let _ = killpg(pid, Signal::SIGKILL);
                    cleanup_deadline = None;
                }
                _ = deadline(drain_deadline), if status.is_some() && !drained => {
                    // A process outside our group can retain a pipe. systemd
                    // owns cgroup cleanup; do not wait forever or chase its PID.
                    out.abort();
                    err.abort();
                    stopped_readers = true;
                    receive.close();
                    while let Some((stream, bytes)) = receive.recv().await {
                        writer.write_line(stream, &bytes)?;
                    }
                    writer.write_line(Stream::System, "자식 종료 뒤 남은 출력 대기를 중단했습니다".as_bytes())?;
                    drained = true;
                }
                line = receive.recv(), if !drained => match line {
                    Some((stream, bytes)) => writer.write_line(stream, &bytes)?,
                    None => drained = true,
                }
            }
        }
        reader_result(out.await, stopped_readers)?;
        reader_result(err.await, stopped_readers)?;
        Ok::<_, ExecError>(())
    }
    .await;
    if let Err(error) = result {
        let _ = killpg(pid, Signal::SIGKILL);
        let _ = child.wait().await;
        return Err(error.into());
    }
    group.0 = None;
    let status = status.expect("wait completed");
    let record = ExitRecord {
        attempt_id: attempt.clone(),
        code: store.windows_exit(&attempt)?.or_else(|| status.code()),
        signal: status.signal().map(signal_name),
        ended_ms: now_ms(),
        by: ExitSource::Wrapper,
        service_result: None,
    };
    if store.finish(&record)? {
        notify(
            &paths,
            Exited {
                run_id: run,
                attempt_id: attempt,
            },
        )
        .await;
    }
    Ok(status)
}
async fn deadline(at: Option<tokio::time::Instant>) {
    if let Some(at) = at {
        tokio::time::sleep_until(at).await;
    } else {
        std::future::pending::<()>().await;
    }
}
fn reader_result(
    result: Result<Result<(), ExecError>, tokio::task::JoinError>,
    stopped: bool,
) -> Result<(), ExecError> {
    match result {
        Ok(result) => result,
        Err(error) if stopped && error.is_cancelled() => Ok(()),
        Err(_) => Err(ExecError::Reader),
    }
}
pub(crate) fn attempt_id(explicit: Option<String>) -> Result<Option<String>, ExecError> {
    let id = explicit.or_else(|| std::env::var("INVOCATION_ID").ok());
    if let Some(id) = &id {
        validate_identity(id)?;
    }
    Ok(id)
}
fn validate_command(command: &[OsString]) -> Result<PathBuf, ExecError> {
    let Some(first) = command.first().filter(|s| !s.is_empty()) else {
        return Err(ExecError::Command);
    };
    devbox_core::input::validate_command_argv(command.iter().map(|arg| arg.as_bytes()))
        .map_err(|_| ExecError::Command)?;
    let executable = |p: &std::path::Path| {
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let path = PathBuf::from(first);
    if first.as_bytes().contains(&b'/') {
        return if executable(&path) {
            Ok(path)
        } else {
            Err(ExecError::Executable)
        };
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(first))
        .find(|p| executable(p))
        .ok_or(ExecError::Executable)
}
struct GroupGuard(Option<Pid>);
impl Drop for GroupGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            let _ = killpg(pid, Signal::SIGKILL);
        }
    }
}
async fn copy_stream(
    mut input: impl AsyncRead + Unpin,
    stream: Stream,
    mut redactor: Redactor,
    output: mpsc::Sender<(Stream, Vec<u8>)>,
) -> Result<(), ExecError> {
    let mut chunk = [0u8; 8192];
    let mut line = Vec::new();
    loop {
        let count = input.read(&mut chunk).await?;
        let masked = redactor.push(&chunk[..count], count == 0);
        for byte in masked {
            if byte == b'\n' {
                output
                    .send((stream, std::mem::take(&mut line)))
                    .await
                    .map_err(|_| ExecError::Reader)?;
            } else if line.len() <= MAX_LINE_BYTES {
                line.push(byte);
            }
        }
        if count == 0 {
            if !line.is_empty() {
                output
                    .send((stream, line))
                    .await
                    .map_err(|_| ExecError::Reader)?;
            }
            return Ok(());
        }
    }
}
pub(crate) fn signal_name(number: i32) -> String {
    Signal::try_from(number)
        .map(|s| s.as_str().trim_start_matches("SIG").to_owned())
        .unwrap_or_else(|_| number.to_string())
}
pub(crate) async fn notify(paths: &devbox_core::Paths, event: Exited) {
    // Durable completion already exists. An absent/stalled daemon never changes the child result.
    let _ = tokio::time::timeout(Duration::from_millis(500), async {
        let client = crate::client::Client::connect(
            &paths.socket,
            devbox_protocol::message::ClientKind::Cli,
            paths.instance.as_str(),
        )
        .await?;
        client.raw_call("runtime.exited", &event, None).await
    })
    .await;
}
/// Reset Tokio's handler before reproducing signal termination for systemd Restart=/ExecStopPost.
#[allow(unsafe_code)]
pub fn exit_like_child(status: ExitStatus) -> ! {
    if let Some(number) = status.signal() {
        if let Ok(signal) = Signal::try_from(number) {
            // SAFETY: no Rust callback is installed; SIG_DFL restores the OS disposition at process exit.
            unsafe {
                let _ = nix::sys::signal::signal(signal, nix::sys::signal::SigHandler::SigDfl);
            }
            let mut set = nix::sys::signal::SigSet::empty();
            set.add(signal);
            let _ = nix::sys::signal::pthread_sigmask(
                nix::sys::signal::SigmaskHow::SIG_UNBLOCK,
                Some(&set),
                None,
            );
            let _ = nix::sys::signal::raise(signal);
        }
        std::process::exit(128 + number);
    }
    std::process::exit(status.code().unwrap_or(1));
}
