//! WSL process execution boundary.
//!
//! The command syntax and identity protocol live in `core::shell`; this
//! module is responsible only for handing argv/environment values to
//! `wsl.exe` and for issuing the already-validated process-group commands.

use std::collections::BTreeMap;
use std::fmt;
use std::process::{Output, Stdio};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::time::sleep;
use zeroize::{Zeroize, Zeroizing};

use crate::core::shell::{
    build_wsl_command, build_wsl_completion_probe_argv, build_wsl_guarded_signal_argv,
    build_wsl_proc_environ_argv, build_wsl_proc_stat_argv, build_wsl_process_command,
    parse_proc_stat_identity, parse_wsl_handshake, validate_wsl_handshake_identity,
    validate_wsl_identity, ShellError, WslCommandSpec, WslProcessIdentity, WslSignal,
};

const HANDSHAKE_BUFFER_LIMIT: usize = 64 * 1024;
const TERMINATION_POLL_INTERVAL: Duration = Duration::from_millis(200);
const HELPER_COMMAND_TIMEOUT: Duration = Duration::from_secs(2);
const HELPER_REAP_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub enum WslExecutionError {
    Shell(ShellError),
    Io(std::io::Error),
    CommandFailed {
        argv: Vec<String>,
        code: Option<i32>,
    },
    HandshakeOutputTooLarge,
    HandshakeEof,
    ProcessGroupStillAlive,
    HelperTimeout,
}

impl fmt::Display for WslExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shell(error) => error.fmt(formatter),
            Self::Io(error) => write!(formatter, "WSL process I/O failed: {error}"),
            Self::CommandFailed { code, .. } => {
                write!(
                    formatter,
                    "WSL helper command failed with exit code {code:?}"
                )
            }
            Self::HandshakeOutputTooLarge => {
                formatter.write_str("WSL handshake output exceeded its safety limit")
            }
            Self::HandshakeEof => formatter.write_str("WSL process ended before handshake"),
            Self::ProcessGroupStillAlive => {
                formatter.write_str("WSL process group did not terminate before the deadline")
            }
            Self::HelperTimeout => formatter.write_str("WSL helper command timed out"),
        }
    }
}

impl std::error::Error for WslExecutionError {}

impl From<ShellError> for WslExecutionError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl From<std::io::Error> for WslExecutionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// The child handle is retained until the Linux process group has been
/// validated and terminated.  stdout/stderr are exposed for the future log
/// adapter, but no log policy is implemented here.
pub struct WslChild {
    distro: Target,
    child: Child,
    stdout: Option<BufReader<ChildStdout>>,
    stderr: Option<ChildStderr>,
}

/// Bytes consumed while locating the handshake are returned to the adapter
/// so startup noise and the frame itself are written to stdout losslessly.
pub struct WslHandshakeRead {
    pub identity: WslProcessIdentity,
    pub consumed_stdout: Vec<u8>,
}

impl WslChild {
    pub fn take_stdout(&mut self) -> Option<BufReader<ChildStdout>> {
        self.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.stderr.take()
    }

    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Read stdout until a complete framed handshake is available.  WSL
    /// startup noise is allowed before the frame; the reader never assumes
    /// the handshake is the first line.
    pub async fn read_handshake(
        &mut self,
        expected_run_id: &str,
    ) -> Result<WslHandshakeRead, WslExecutionError> {
        let mut stdout = self.stdout.take().ok_or(WslExecutionError::HandshakeEof)?;
        let mut bytes = Zeroizing::new(Vec::with_capacity(1024));
        let mut line = Zeroizing::new(Vec::with_capacity(128));
        loop {
            line.zeroize();
            line.clear();
            let read = stdout.read_until(b'\n', &mut line).await?;
            if read == 0 {
                return Err(WslExecutionError::HandshakeEof);
            }
            bytes.extend_from_slice(&line);
            if bytes.len() > HANDSHAKE_BUFFER_LIMIT {
                return Err(WslExecutionError::HandshakeOutputTooLarge);
            }
            match parse_wsl_handshake(&bytes, expected_run_id) {
                Ok(handshake) => {
                    let environ = self.distro.read_process_environ(handshake.pid).await?;
                    let identity =
                        validate_wsl_handshake_identity(handshake, expected_run_id, &environ)?;
                    let observed = self.distro.read_process_identity(&identity).await?;
                    validate_wsl_identity(&identity, &observed)?;
                    // The supervisor cannot execute user code until its exact
                    // marker/PID/group/session has been validated. This removes
                    // the short-command race without a timing-dependent sleep.
                    let mut stdin = self
                        .child
                        .stdin
                        .take()
                        .ok_or(WslExecutionError::HandshakeEof)?;
                    stdin.write_all(expected_run_id.as_bytes()).await?;
                    stdin.write_all(b"\n").await?;
                    stdin.shutdown().await?;
                    // Keep the buffered reader, not only its underlying pipe.
                    // `read_until` may have prefetched command output after
                    // the frame; returning this BufReader preserves every
                    // such byte for the log adapter.
                    self.stdout = Some(stdout);
                    let consumed_stdout = bytes.to_vec();
                    bytes.zeroize();
                    return Ok(WslHandshakeRead {
                        identity,
                        consumed_stdout,
                    });
                }
                Err(ShellError::HandshakeNotFound | ShellError::HandshakeMalformed) => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Move the child and remaining readers to the production monitor. The
    /// monitor becomes the sole owner after handshake validation.
    pub fn into_parts(
        self,
    ) -> (
        String,
        Child,
        Option<BufReader<ChildStdout>>,
        Option<ChildStderr>,
    ) {
        (self.distro.distro, self.child, self.stdout, self.stderr)
    }

    pub fn into_bound_parts(
        self,
    ) -> (
        Target,
        Child,
        Option<BufReader<ChildStdout>>,
        Option<ChildStderr>,
    ) {
        (self.distro, self.child, self.stdout, self.stderr)
    }

    /// Best-effort cleanup for a spawn attempt that never obtained a trusted
    /// PID/PGID/SID/marker tuple. Without an exact identity, no destructive
    /// group signal is constructed; the Windows-side wrapper is still reaped.
    pub async fn abort_spawn(mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }

    pub async fn wait(&mut self) -> Result<std::process::ExitStatus, WslExecutionError> {
        self.child.wait().await.map_err(Into::into)
    }

    /// Validate the fresh process identity, then TERM the process group and
    /// escalate to KILL only after the grace deadline.  A single-PID kill is
    /// intentionally impossible through this API.
    pub async fn terminate_group(
        &mut self,
        identity: &WslProcessIdentity,
        grace: Duration,
    ) -> Result<std::process::ExitStatus, WslExecutionError> {
        self.distro.terminate_group(identity, grace).await?;
        self.child.wait().await.map_err(Into::into)
    }
}

/// Validate and terminate a WSL process group without borrowing the child.
/// The production monitor can therefore wait on the child concurrently while
/// this path performs TERM, identity re-check, timeout, and KILL escalation.
pub async fn terminate_group(
    distro: &str,
    identity: &WslProcessIdentity,
    grace: Duration,
) -> Result<(), WslExecutionError> {
    Target::direct(distro)
        .terminate_group(identity, grace)
        .await
}

/// Startup recovery boundary. A missing leader is safe only when its process
/// group probe is also gone; otherwise the persisted identity is retained and
/// the caller must keep the run blocked rather than guessing at a recycled
/// PGID.
pub async fn recover_stale_group(
    distro: &str,
    identity: &WslProcessIdentity,
    grace: Duration,
) -> Result<(), WslExecutionError> {
    Target::direct(distro)
        .recover_stale_group(identity, grace)
        .await
}

/// Confirm the post-exit supervisor invariant without issuing a signal.  A
/// natural wrapper exit is publishable only after the exact persisted process
/// group is absent; an alive group is reported as a cleanup failure rather
/// than guessed-away after the leader has disappeared.
pub async fn confirm_group_gone(
    distro: &str,
    identity: &WslProcessIdentity,
    timeout: Duration,
) -> Result<(), WslExecutionError> {
    Target::direct(distro)
        .confirm_group_gone(identity, timeout)
        .await
}

/// Construct and spawn the WSL command.  `Command::arg` is used for every
/// argv boundary; environment values are never interpolated into the `-lc`
/// script.
pub fn spawn(
    distro: &str,
    cwd: Option<&str>,
    command: &str,
    run_id: &str,
    environment: &BTreeMap<String, String>,
) -> Result<WslChild, WslExecutionError> {
    Target::direct(distro).spawn(cwd, command, run_id, environment)
}

pub fn spawn_process(
    distro: &str,
    cwd: Option<&str>,
    program: &str,
    arguments: &[String],
    run_id: &str,
    environment: &BTreeMap<String, String>,
) -> Result<WslChild, WslExecutionError> {
    Target::direct(distro).spawn_process(cwd, program, arguments, run_id, environment)
}

pub fn spawn_spec(spec: WslCommandSpec) -> Result<WslChild, WslExecutionError> {
    let distro = spec.argv.get(2).cloned().ok_or(ShellError::InvalidDistro)?;
    spawn_spec_bound(spec, Target::direct(&distro))
}
pub fn spawn_spec_bound(
    spec: WslCommandSpec,
    target: Target,
) -> Result<WslChild, WslExecutionError> {
    let mut environment = spec.environment;
    let bound = match target.bind(spec.argv, true) {
        Ok(argv) => argv,
        Err(error) => {
            zeroize_environment(&mut environment);
            return Err(error);
        }
    };
    let mut argv = bound.into_iter();
    let Some(program) = argv.next() else {
        zeroize_environment(&mut environment);
        return Err(ShellError::EmptyField("WSL program").into());
    };
    let mut command = Command::new(program);
    command
        .args(argv)
        .envs(environment.iter())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A monitor timeout must not leave the Windows-side wrapper alive
        // after its wait owner is aborted. Descendants are still controlled
        // only through the validated process-group path below.
        .kill_on_drop(true);
    let spawned = command.spawn();
    zeroize_environment(&mut environment);
    let mut child = spawned?;
    let stdout = child.stdout.take().map(BufReader::new);
    let stderr = child.stderr.take();
    Ok(WslChild {
        distro: target,
        child,
        stdout,
        stderr,
    })
}

fn zeroize_environment(environment: &mut BTreeMap<String, String>) {
    let environment = std::mem::take(environment);
    for (mut key, mut value) in environment {
        key.zeroize();
        value.zeroize();
    }
}

/// Spawn a spec while retaining the distribution needed for termination.
pub fn spawn_spec_for_distro(
    distro: &str,
    spec: WslCommandSpec,
) -> Result<WslChild, WslExecutionError> {
    if spec.argv.get(2).map(String::as_str) != Some(distro) {
        return Err(WslExecutionError::Shell(ShellError::InvalidDistro));
    }
    let mut child = spawn_spec(spec)?;
    child.distro = Target::direct(distro);
    Ok(child)
}

/// Convert a Windows path with `wslpath -u` using argv boundaries.  The
/// resulting path is validated before it is suitable for `--cd`.
pub async fn convert_windows_path(
    distro: &str,
    windows_path: &str,
) -> Result<String, WslExecutionError> {
    Target::direct(distro)
        .convert_windows_path(windows_path)
        .await
}

/// Verify a persisted WSL identity immediately before cleanup.  The caller
/// supplies fresh `/proc` outputs from the same distro; no signal command is
/// built unless marker, PID, PGID and SID all match.
pub async fn validate_identity(
    distro: &str,
    expected: &WslProcessIdentity,
) -> Result<WslProcessIdentity, WslExecutionError> {
    Target::direct(distro).validate_identity(expected).await
}

/// Read-only membership check for a broker-selected process. The current
/// leader marker and the selected PID/start tick must both remain exact.
pub async fn contains_process(
    distro: &str,
    owner: &WslProcessIdentity,
    pid: u32,
    start_tick: u64,
) -> Result<bool, WslExecutionError> {
    Target::direct(distro)
        .contains_process(owner, pid, start_tick)
        .await
}

async fn run_helper_output(argv: &[String]) -> Result<Output, WslExecutionError> {
    run_helper_output_with_timeout(argv, HELPER_COMMAND_TIMEOUT).await
}

async fn collect_helper_output(
    child: &mut Child,
    stdout: ChildStdout,
) -> Result<Output, std::io::Error> {
    let mut stdout_bytes = Vec::new();
    let mut stdout = stdout.take((HANDSHAKE_BUFFER_LIMIT + 1) as u64);
    let (read_result, status_result) =
        tokio::join!(stdout.read_to_end(&mut stdout_bytes), child.wait(),);
    read_result?;
    if stdout_bytes.len() > HANDSHAKE_BUFFER_LIMIT {
        return Err(std::io::Error::other("helper output limit"));
    }
    let status = status_result?;
    Ok(Output {
        status,
        stdout: stdout_bytes,
        stderr: Vec::new(),
    })
}

async fn kill_and_reap_helper(child: &mut Child) {
    let _ = child.start_kill();
    let _ = tokio::time::timeout(HELPER_REAP_TIMEOUT, child.wait()).await;
}

async fn run_helper_output_with_timeout(
    argv: &[String],
    timeout: Duration,
) -> Result<Output, WslExecutionError> {
    run_helper_with_input(argv, timeout, None).await
}

async fn run_helper_with_input(
    argv: &[String],
    timeout: Duration,
    input: Option<&str>,
) -> Result<Output, WslExecutionError> {
    let Some(program) = argv.first() else {
        return Err(WslExecutionError::Shell(ShellError::EmptyField(
            "helper program",
        )));
    };
    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let error = std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "helper stdout was not piped",
            );
            kill_and_reap_helper(&mut child).await;
            return Err(error.into());
        }
    };
    let exchange = async {
        if let Some(input) = input {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| std::io::Error::other("helper stdin unavailable"))?;
            let bytes = Zeroizing::new(format!("{input}\n"));
            stdin.write_all(bytes.as_bytes()).await?;
            stdin.shutdown().await?;
        }
        collect_helper_output(&mut child, stdout).await
    };
    match tokio::time::timeout(timeout, exchange).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => {
            kill_and_reap_helper(&mut child).await;
            Err(error.into())
        }
        Err(_) => {
            // The collection future is dropped before this branch, so the
            // child is available for an explicit kill and reap. `kill_on_drop`
            // remains a final fallback if the runtime cannot reap promptly.
            kill_and_reap_helper(&mut child).await;
            Err(WslExecutionError::HelperTimeout)
        }
    }
}

fn group_witness(output: &Output) -> Result<&str, WslExecutionError> {
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| std::io::Error::other("invalid group witness"))?;
    let mut frames = text
        .lines()
        .filter_map(|line| line.strip_prefix("__DEVBOX_GROUP_V1__:"));
    let frame = frames
        .next()
        .ok_or_else(|| std::io::Error::other("missing group witness"))?;
    if frames.next().is_some()
        || text
            .lines()
            .last()
            .and_then(|line| line.strip_prefix("__DEVBOX_GROUP_V1__:"))
            != Some(frame)
    {
        return Err(std::io::Error::other("ambiguous group witness").into());
    }
    if frame == "mismatch" && output.status.code() == Some(80) {
        return Err(ShellError::MarkerMismatch.into());
    }
    Ok(frame)
}

/// Native product binding is retained for launch, observation and cleanup.
pub trait CommandBinding: Send + Sync {
    fn bind(&self, argv: Vec<String>) -> Result<Vec<String>, WslExecutionError>;
    fn bind_launch(&self, argv: Vec<String>) -> Result<Vec<String>, WslExecutionError> {
        self.bind(argv)
    }
}
#[derive(Clone)]
pub struct Target {
    distro: String,
    binding: Option<std::sync::Arc<dyn CommandBinding>>,
}
impl Target {
    pub async fn convert_windows_path(
        &self,
        windows_path: &str,
    ) -> Result<String, WslExecutionError> {
        let argv = devbox_wsl::argv::build_wslpath_argv(self.name(), windows_path)
            .map_err(|e| WslExecutionError::Shell(e.into()))?;
        let argv = self.bind(argv, true)?;
        let output = run_helper_output(&argv).await?;
        if !output.status.success() {
            return Err(WslExecutionError::CommandFailed {
                argv,
                code: output.status.code(),
            });
        }
        let path = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if path.is_empty() || path.contains('\0') {
            return Err(WslExecutionError::Shell(ShellError::EmptyField(
                "converted WSL path",
            )));
        }
        Ok(path)
    }
    pub fn spawn(
        &self,
        cwd: Option<&str>,
        command: &str,
        run_id: &str,
        environment: &BTreeMap<String, String>,
    ) -> Result<WslChild, WslExecutionError> {
        let inherited_wslenv = std::env::var("WSLENV").ok();
        let spec = build_wsl_command(
            self.distro.as_str(),
            cwd,
            command,
            run_id,
            environment,
            inherited_wslenv.as_deref(),
        )?;
        spawn_spec_bound(spec, self.clone())
    }
    pub fn spawn_process(
        &self,
        cwd: Option<&str>,
        program: &str,
        arguments: &[String],
        run_id: &str,
        environment: &BTreeMap<String, String>,
    ) -> Result<WslChild, WslExecutionError> {
        let inherited_wslenv = std::env::var("WSLENV").ok();
        let spec = build_wsl_process_command(
            self.distro.as_str(),
            cwd,
            program,
            arguments,
            run_id,
            environment,
            inherited_wslenv.as_deref(),
        )?;
        spawn_spec_bound(spec, self.clone())
    }
    pub fn direct(distro: &str) -> Self {
        Self {
            distro: distro.into(),
            binding: None,
        }
    }
    pub fn bound(distro: &str, binding: std::sync::Arc<dyn CommandBinding>) -> Self {
        Self {
            distro: distro.into(),
            binding: Some(binding),
        }
    }
    pub fn name(&self) -> &str {
        &self.distro
    }
    fn bind(&self, argv: Vec<String>, launch: bool) -> Result<Vec<String>, WslExecutionError> {
        let Some(binding) = &self.binding else {
            return Ok(argv);
        };
        if argv.len() < 3 || argv[0] != "wsl.exe" || argv[1] != "-d" || argv[2] != self.distro {
            return Err(WslExecutionError::Shell(ShellError::InvalidDistro));
        }
        if launch {
            binding.bind_launch(argv)
        } else {
            binding.bind(argv)
        }
    }
    async fn bind_query(&self, argv: &[String]) -> Result<Vec<String>, WslExecutionError> {
        let target = self.clone();
        let argv = argv.to_vec();
        tokio::task::spawn_blocking(move || target.bind(argv, false))
            .await
            .map_err(|_| std::io::Error::other("runtime-target-unavailable"))?
    }
    async fn output(&self, argv: &[String]) -> Result<Output, WslExecutionError> {
        run_helper_output(&self.bind_query(argv).await?).await
    }
    async fn control_until(
        &self,
        argv: &[String],
        identity: &WslProcessIdentity,
        deadline: Instant,
    ) -> Result<Output, WslExecutionError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let bound = tokio::time::timeout(remaining, self.bind_query(argv))
            .await
            .map_err(|_| WslExecutionError::HelperTimeout)??;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(WslExecutionError::HelperTimeout);
        }
        run_helper_with_input(&bound, remaining, Some(&identity.marker)).await
    }
    async fn signal_owned_group(
        &self,
        identity: &WslProcessIdentity,
        signal: WslSignal,
        deadline: Instant,
    ) -> Result<(), WslExecutionError> {
        let argv = build_wsl_guarded_signal_argv(self.distro.as_str(), identity, signal)?;
        let output = self.control_until(&argv, identity, deadline).await?;
        match group_witness(&output)? {
            "signalled" | "gone" if output.status.success() => Ok(()),
            _ => Err(WslExecutionError::CommandFailed {
                argv,
                code: output.status.code(),
            }),
        }
    }
    pub async fn terminate_group(
        &self,
        identity: &WslProcessIdentity,
        grace: Duration,
    ) -> Result<(), WslExecutionError> {
        // Delivery, observations and escalation share one total budget. A
        // failed/ambiguous observation may trigger a fresh guarded escalation,
        // but can never establish successful cleanup or grant signal authority.
        let phase_budget = grace.saturating_add(HELPER_COMMAND_TIMEOUT / 2);
        let mut last_error = None;
        for signal in [WslSignal::Term, WslSignal::Kill] {
            let deadline = Instant::now() + phase_budget;
            // Reserve completion observation time in both phases. A cold TERM
            // helper cannot consume KILL's budget or its final witness.
            let observation_budget = grace.min(HELPER_COMMAND_TIMEOUT / 2);
            let delivery_deadline = (deadline - observation_budget)
                .min(Instant::now() + grace.max(HELPER_COMMAND_TIMEOUT));
            let delivered = self
                .signal_owned_group(identity, signal, delivery_deadline)
                .await;
            if matches!(
                &delivered,
                Err(WslExecutionError::Shell(ShellError::MarkerMismatch))
            ) {
                // A raced natural exit is accepted only with a new explicit
                // absence witness; a live replacement retains MarkerMismatch.
                if self
                    .confirm_group_gone_until(identity, deadline)
                    .await
                    .is_ok()
                {
                    return Ok(());
                }
                return delivered;
            }
            last_error = delivered.err();
            if self.wait_for_group_gone(identity, deadline).await? {
                return Ok(());
            }
        }
        Err(last_error.unwrap_or(WslExecutionError::ProcessGroupStillAlive))
    }
    pub async fn recover_stale_group(
        &self,
        identity: &WslProcessIdentity,
        grace: Duration,
    ) -> Result<(), WslExecutionError> {
        self.terminate_group(identity, grace).await
    }
    pub async fn confirm_group_gone(
        &self,
        identity: &WslProcessIdentity,
        timeout: Duration,
    ) -> Result<(), WslExecutionError> {
        self.confirm_group_gone_until(identity, Instant::now() + timeout)
            .await
    }
    async fn confirm_group_gone_until(
        &self,
        identity: &WslProcessIdentity,
        deadline: Instant,
    ) -> Result<(), WslExecutionError> {
        let argv = build_wsl_completion_probe_argv(self.distro.as_str(), identity)?;
        let output = self.control_until(&argv, identity, deadline).await?;
        match group_witness(&output)? {
            "gone" if output.status.success() => Ok(()),
            "present" if output.status.success() => Err(WslExecutionError::ProcessGroupStillAlive),
            _ => Err(WslExecutionError::CommandFailed {
                argv,
                code: output.status.code(),
            }),
        }
    }

    pub async fn validate_identity(
        &self,
        expected: &WslProcessIdentity,
    ) -> Result<WslProcessIdentity, WslExecutionError> {
        let environ = self.read_process_environ(expected.pid).await?;
        if !crate::core::shell::environ_contains_exact_marker(&environ, &expected.marker)? {
            return Err(WslExecutionError::Shell(ShellError::MarkerMismatch));
        }
        let observed = self.read_process_identity(expected).await?;
        validate_wsl_identity(expected, &observed)?;
        Ok(observed)
    }
    pub async fn contains_process(
        &self,
        owner: &WslProcessIdentity,
        pid: u32,
        start_tick: u64,
    ) -> Result<bool, WslExecutionError> {
        self.validate_identity(owner).await?;
        let argv = build_wsl_proc_stat_argv(self.distro.as_str(), pid)?;
        let output = self.output(&argv).await?;
        if !output.status.success() {
            return Err(WslExecutionError::CommandFailed {
                argv,
                code: output.status.code(),
            });
        }
        let observed = parse_proc_stat_identity(pid, &output.stdout, &owner.marker)?;
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| WslExecutionError::Shell(ShellError::InvalidNumericField("start tick")))?;
        let tick = text
            .rsplit_once(") ")
            .and_then(|(_, fields)| fields.split_whitespace().nth(19))
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|tick| *tick > 0);
        if tick != Some(start_tick) {
            return Err(WslExecutionError::Shell(ShellError::InvalidNumericField(
                "start tick",
            )));
        }
        self.validate_identity(owner).await?;
        Ok(observed.pgid == owner.pgid && observed.sid == owner.sid)
    }
    async fn read_process_environ(&self, pid: u32) -> Result<Vec<u8>, WslExecutionError> {
        let argv = build_wsl_proc_environ_argv(self.distro.as_str(), pid)?;
        let output = self.output(&argv).await?;
        if !output.status.success() {
            return Err(WslExecutionError::CommandFailed {
                argv,
                code: output.status.code(),
            });
        }
        Ok(output.stdout)
    }
    async fn read_process_identity(
        &self,
        expected: &WslProcessIdentity,
    ) -> Result<WslProcessIdentity, WslExecutionError> {
        let argv = build_wsl_proc_stat_argv(self.distro.as_str(), expected.pid)?;
        let output = self.output(&argv).await?;
        if !output.status.success() {
            return Err(WslExecutionError::CommandFailed {
                argv,
                code: output.status.code(),
            });
        }
        parse_proc_stat_identity(expected.pid, &output.stdout, &expected.marker).map_err(Into::into)
    }
    async fn wait_for_group_gone(
        &self,
        identity: &WslProcessIdentity,
        deadline: Instant,
    ) -> Result<bool, WslExecutionError> {
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(false);
            }
            match self.confirm_group_gone(identity, remaining).await {
                Ok(()) => return Ok(true),
                Err(WslExecutionError::ProcessGroupStillAlive) => {}
                Err(WslExecutionError::HelperTimeout) if Instant::now() >= deadline => {
                    return Ok(false)
                }
                Err(error @ WslExecutionError::Shell(ShellError::MarkerMismatch)) => {
                    return Err(error)
                }
                // Unknown completion is not absence. Escalation revalidates
                // current membership, so probe transport failures cannot block it.
                Err(_) => return Ok(false),
            }
            sleep(
                TERMINATION_POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn every_native_cleanup_and_observation_rechecks_the_retained_target() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        struct Closed(Arc<AtomicUsize>);
        impl CommandBinding for Closed {
            fn bind(&self, argv: Vec<String>) -> Result<Vec<String>, WslExecutionError> {
                assert_eq!(argv[2], "Fixture");
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(std::io::Error::other("synthetic-retired-binding").into())
            }
        }
        let attempts = Arc::new(AtomicUsize::new(0));
        let target = Target::bound("Fixture", Arc::new(Closed(attempts.clone())));
        let identity = WslProcessIdentity {
            pid: 100,
            pgid: 100,
            sid: 100,
            marker: "10000000-0000-4000-8000-000000000001".into(),
        };
        assert!(target
            .confirm_group_gone(&identity, Duration::from_secs(1))
            .await
            .is_err());
        assert!(target
            .recover_stale_group(&identity, Duration::from_secs(1))
            .await
            .is_err());
        assert!(target
            .terminate_group(&identity, Duration::from_secs(1))
            .await
            .is_err());
        assert!(target.contains_process(&identity, 101, 123).await.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 10);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn completion_uses_one_retained_binding_and_requires_an_explicit_witness() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        struct Probe {
            calls: Arc<AtomicUsize>,
            script: &'static str,
        }
        impl CommandBinding for Probe {
            fn bind(&self, argv: Vec<String>) -> Result<Vec<String>, WslExecutionError> {
                assert_eq!(&argv[..4], &["wsl.exe", "-d", "Fixture", "--exec"]);
                assert_eq!(&argv[10..], &["100", "100", "100", "probe"]);
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(vec![
                    "sh".into(),
                    "-c".into(),
                    format!("read marker; {}", self.script),
                ])
            }
        }
        let identity = WslProcessIdentity {
            pid: 100,
            pgid: 100,
            sid: 100,
            marker: "10000000-0000-4000-8000-000000000001".into(),
        };
        for (script, success) in [
            ("printf '__DEVBOX_GROUP_V1__:gone\\n'", true),
            ("printf '__DEVBOX_GROUP_V1__:present\\n'", false),
            ("printf '__DEVBOX_GROUP_V1__:gone\\n'; exit 1", false),
            ("printf 'noise\\n__DEVBOX_GROUP_V1__:gone\\n'", true),
            ("true", false),
        ] {
            let calls = Arc::new(AtomicUsize::new(0));
            let target = Target::bound(
                "Fixture",
                Arc::new(Probe {
                    calls: calls.clone(),
                    script,
                }),
            );
            assert_eq!(
                target
                    .confirm_group_gone(&identity, Duration::from_secs(1))
                    .await
                    .is_ok(),
                success
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn owned_stop_checks_marker_group_and_session_before_term_and_kill() {
        use std::io::BufRead;
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        struct Owned(std::process::Child);
        impl Drop for Owned {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        struct Native(Arc<AtomicUsize>);
        impl CommandBinding for Native {
            fn bind(&self, argv: Vec<String>) -> Result<Vec<String>, WslExecutionError> {
                assert_eq!(&argv[..4], &["wsl.exe", "-d", "Fixture", "--exec"]);
                if argv.last().is_some_and(|action| action != "probe") {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
                Ok(argv[4..].to_vec())
            }
        }
        let marker = "10000000-0000-4000-8000-000000000001";
        let forged = "20000000-0000-4000-8000-000000000002";
        let mut child=Owned(std::process::Command::new("setsid")
            .args(["python3","-c","import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);print('ready',flush=True);time.sleep(30)"])
            .env("DEVBOX_RUN_MARKER",marker)
            .env("UNRELATED",format!("prefix\nDEVBOX_RUN_MARKER={forged}"))
            .stdout(Stdio::piped()).spawn().unwrap());
        let mut ready = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let identity = WslProcessIdentity {
            pid: child.0.id(),
            pgid: child.0.id(),
            sid: child.0.id(),
            marker: marker.into(),
        };
        let signals = Arc::new(AtomicUsize::new(0));
        let target = Target::bound("Fixture", Arc::new(Native(signals.clone())));
        for wrong in [
            WslProcessIdentity {
                marker: forged.into(),
                ..identity.clone()
            },
            WslProcessIdentity {
                pgid: identity.pgid + 1,
                ..identity.clone()
            },
            WslProcessIdentity {
                sid: identity.sid + 1,
                ..identity.clone()
            },
        ] {
            assert!(matches!(
                target
                    .terminate_group(&wrong, Duration::from_millis(200))
                    .await,
                Err(WslExecutionError::Shell(ShellError::MarkerMismatch))
            ));
            assert!(child.0.try_wait().unwrap().is_none());
        }
        assert_eq!(signals.swap(0, Ordering::SeqCst), 3);
        // Deliberately do not reap until the production termination API
        // returns. A zombie is not a live process retaining user resources.
        target
            .terminate_group(&identity, Duration::from_millis(500))
            .await
            .unwrap();
        assert!(!child.0.wait().unwrap().success());
        assert_eq!(signals.load(Ordering::SeqCst), 2);
        // A repeated stop of the already absent group is safe, with no signal
        // admitted against a missing or replacement process.
        target
            .terminate_group(&identity, Duration::from_millis(200))
            .await
            .unwrap();
    }

    #[test]
    fn spec_uses_wsl_exe_and_piped_streams_without_script_environment_prefix() {
        let environment = BTreeMap::from([(String::from("TOKEN"), String::from("secret"))]);
        let spec = build_wsl_command(
            "Ubuntu",
            Some("/work tree"),
            "printf '%s' \"$TOKEN\"",
            "123e4567-e89b-12d3-a456-426614174000",
            &environment,
            None,
        )
        .unwrap();
        assert_eq!(spec.argv[0], "wsl.exe");
        assert_eq!(spec.argv[3], "--cd");
        assert_eq!(spec.argv[5], "--exec");
        assert_eq!(spec.argv[6], "setsid");
        assert_eq!(spec.argv[7], "--wait");
        assert_eq!(spec.argv[8], "bash");
        assert!(!spec.wrapper.contains("secret"));
        assert_eq!(spec.environment["TOKEN"], "secret");
    }

    #[test]
    fn distro_override_cannot_desynchronize_spawn_and_cleanup_identity() {
        let spec = build_wsl_command(
            "Ubuntu",
            None,
            "true",
            "123e4567-e89b-12d3-a456-426614174000",
            &BTreeMap::new(),
            None,
        )
        .unwrap();
        assert!(matches!(
            spawn_spec_for_distro("Debian", spec),
            Err(WslExecutionError::Shell(ShellError::InvalidDistro))
        ));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn real_supervisor_keeps_identity_until_stubborn_child_cleanup_without_a_test_reaper() {
        use std::io::{BufRead, Write};
        struct Native;
        impl CommandBinding for Native {
            fn bind(&self, argv: Vec<String>) -> Result<Vec<String>, WslExecutionError> {
                assert!(!argv
                    .iter()
                    .any(|value| value.contains("10000000-0000-4000-8000-000000000001")));
                Ok(argv[4..].to_vec())
            }
        }
        struct Owned(std::process::Child, bool);
        impl Drop for Owned {
            fn drop(&mut self) {
                // Only this retained child's newly created group is ours.
                if !self.1 {
                    let _ = std::process::Command::new("kill")
                        .args(["-KILL", "--", &format!("-{}", self.0.id())])
                        .status();
                }
                let _ = self.0.wait();
            }
        }
        let marker = "10000000-0000-4000-8000-000000000001";
        let wrapper = crate::core::shell::build_wsl_process_wrapper(marker).unwrap();
        let mut child = Owned(std::process::Command::new("setsid").args([
            "bash", "--noprofile", "--norc", "-c", &wrapper, "devbox-run-supervisor",
            "python3", "-c", "import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);print('ready',flush=True);time.sleep(30)",
        ]).env("DEVBOX_RUN_MARKER",marker).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap(), false);
        let mut output = std::io::BufReader::new(child.0.stdout.take().unwrap());
        let mut handshake = String::new();
        for _ in 0..6 {
            output.read_line(&mut handshake).unwrap();
        }
        let handshake = parse_wsl_handshake(handshake.as_bytes(), marker).unwrap();
        let identity = validate_wsl_handshake_identity(
            handshake,
            marker,
            format!("DEVBOX_RUN_MARKER={marker}\0").as_bytes(),
        )
        .unwrap();
        writeln!(child.0.stdin.take().unwrap(), "{marker}").unwrap();
        let mut ready = String::new();
        output.read_line(&mut ready).unwrap();
        assert_eq!(ready, "ready\n");
        let target = Target::bound("Fixture", std::sync::Arc::new(Native));
        target
            .terminate_group(&identity, Duration::from_millis(300))
            .await
            .unwrap();
        child.1 = true;
        assert!(!child.0.wait().unwrap().success());
        target
            .confirm_group_gone(&identity, Duration::from_secs(1))
            .await
            .unwrap();
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn retired_unreaped_leader_does_not_prevent_killing_owned_descendants() {
        use std::io::BufRead;
        struct Native;
        impl CommandBinding for Native {
            fn bind(&self, argv: Vec<String>) -> Result<Vec<String>, WslExecutionError> {
                Ok(argv[4..].to_vec())
            }
        }
        struct Owned(std::process::Child, bool);
        impl Drop for Owned {
            fn drop(&mut self) {
                if !self.1 {
                    let _ = std::process::Command::new("kill")
                        .args(["-KILL", "--", &format!("-{}", self.0.id())])
                        .status();
                }
                let _ = self.0.wait();
            }
        }
        let marker = "10000000-0000-4000-8000-000000000001";
        let script = "import os,signal,time\nif os.fork()==0:\n signal.signal(signal.SIGTERM,signal.SIG_IGN)\n print('ready',flush=True)\nwhile True: time.sleep(1)";
        let mut child = Owned(
            std::process::Command::new("setsid")
                .args(["python3", "-c", script])
                .env("DEVBOX_RUN_MARKER", marker)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
            false,
        );
        let mut ready = String::new();
        std::io::BufReader::new(child.0.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let identity = WslProcessIdentity {
            pid: child.0.id(),
            pgid: child.0.id(),
            sid: child.0.id(),
            marker: marker.into(),
        };
        let target = Target::bound("Fixture", std::sync::Arc::new(Native));
        target
            .terminate_group(&identity, Duration::from_millis(100))
            .await
            .unwrap();
        child.1 = true;
        child.0.wait().unwrap();
        target
            .confirm_group_gone(&identity, Duration::from_secs(1))
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn noisy_or_failed_observation_cannot_skip_fresh_kill_admission() {
        use std::sync::{Arc, Mutex};
        struct Fixture {
            actions: Mutex<Vec<String>>,
            probe_fails: bool,
            term_times_out: bool,
        }
        impl CommandBinding for Fixture {
            fn bind(&self, argv: Vec<String>) -> Result<Vec<String>, WslExecutionError> {
                let action = argv.last().unwrap().clone();
                let mut actions = self.actions.lock().unwrap();
                let killed = actions.iter().any(|action| action == "KILL");
                actions.push(action.clone());
                let script = match action.as_str() {
                    "TERM" if self.term_times_out => "read marker; exec sleep 10",
                    "TERM" => "read marker; printf 'ambiguous acknowledgement\\n'",
                    "KILL" => {
                        "read marker; printf 'startup noise\\n__DEVBOX_GROUP_V1__:signalled\\n'"
                    }
                    _ if killed => "read marker; printf '__DEVBOX_GROUP_V1__:gone\\n'",
                    _ if self.probe_fails => "read marker; exit 1",
                    _ => "read marker; printf '__DEVBOX_GROUP_V1__:present\\n'",
                };
                Ok(vec!["sh".into(), "-c".into(), script.into()])
            }
        }
        for (probe_fails, term_times_out) in [(true, false), (false, false), (true, true)] {
            let binding = Arc::new(Fixture {
                actions: Mutex::new(Vec::new()),
                probe_fails,
                term_times_out,
            });
            let target = Target::bound("Fixture", binding.clone());
            let identity = WslProcessIdentity {
                pid: 100,
                pgid: 100,
                sid: 100,
                marker: "10000000-0000-4000-8000-000000000001".into(),
            };
            target
                .terminate_group(&identity, Duration::from_millis(50))
                .await
                .unwrap();
            let actions = binding.actions.lock().unwrap();
            assert!(actions.iter().any(|action| action == "KILL"));
            assert_eq!(actions.last().unwrap(), "probe");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn helper_timeout_kills_and_reaps_the_helper() {
        let argv = vec!["sleep".to_owned(), "60".to_owned()];
        let error = tokio::time::timeout(
            Duration::from_secs(1),
            run_helper_output_with_timeout(&argv, Duration::from_millis(10)),
        )
        .await
        .expect("helper cleanup must be bounded")
        .unwrap_err();
        assert!(matches!(error, WslExecutionError::HelperTimeout));
        assert_eq!(error.to_string(), "WSL helper command timed out");
    }
}
