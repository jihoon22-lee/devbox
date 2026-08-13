//! WSL process execution boundary.
//!
//! The command syntax and identity protocol live in `core::shell`; this
//! module is responsible only for handing argv/environment values to
//! `wsl.exe` and for issuing the already-validated process-group commands.

use std::collections::BTreeMap;
use std::fmt;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::time::sleep;

use crate::core::shell::{
    build_wsl_command, build_wsl_proc_dir_probe_argv, build_wsl_proc_environ_argv,
    build_wsl_proc_stat_argv, build_wsl_termination_plan, build_wslpath_conversion_argv,
    parse_proc_stat_identity, parse_wsl_handshake, validate_wsl_handshake_identity,
    validate_wsl_identity, ShellError, WslCommandSpec, WslProcessIdentity, WslTerminationPlan,
};

const HANDSHAKE_BUFFER_LIMIT: usize = 64 * 1024;
const TERMINATION_POLL_INTERVAL: Duration = Duration::from_millis(25);

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
    distro: String,
    child: Child,
    stdout: Option<BufReader<ChildStdout>>,
    stderr: Option<ChildStderr>,
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
    ) -> Result<WslProcessIdentity, WslExecutionError> {
        let mut stdout = self.stdout.take().ok_or(WslExecutionError::HandshakeEof)?;
        let mut bytes = Vec::with_capacity(1024);
        let mut line = Vec::with_capacity(128);
        loop {
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
                    let environ = read_process_environ(&self.distro, handshake.pid).await?;
                    let identity =
                        validate_wsl_handshake_identity(handshake, expected_run_id, &environ)?;
                    let observed = read_process_identity(&self.distro, &identity).await?;
                    validate_wsl_identity(&identity, &observed)?;
                    // Keep the buffered reader, not only its underlying pipe.
                    // `read_until` may have prefetched command output after
                    // the frame; returning this BufReader preserves every
                    // such byte for the log adapter.
                    self.stdout = Some(stdout);
                    return Ok(identity);
                }
                Err(ShellError::HandshakeNotFound | ShellError::HandshakeMalformed) => continue,
                Err(error) => return Err(error.into()),
            }
        }
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
        let observed = validate_identity(&self.distro, identity).await?;
        validate_wsl_identity(identity, &observed)?;
        let plan = build_wsl_termination_plan(&self.distro, identity)?;
        run_helper_status(&plan.term).await?;

        if !wait_for_group_gone(&self.distro, &plan, grace).await? {
            // TERM may leave a process alive long enough for its session or
            // group identity to change.  Re-read all four identity fields
            // before escalating; a marker-only check is not sufficient for a
            // destructive group signal.
            validate_identity(&self.distro, identity).await?;
            run_helper_status(&plan.kill).await?;
            if !wait_for_group_gone(&self.distro, &plan, grace).await? {
                return Err(WslExecutionError::ProcessGroupStillAlive);
            }
        }
        self.child.wait().await.map_err(Into::into)
    }
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
    let inherited_wslenv = std::env::var("WSLENV").ok();
    let spec = build_wsl_command(
        distro,
        cwd,
        command,
        run_id,
        environment,
        inherited_wslenv.as_deref(),
    )?;
    spawn_spec_for_distro(distro, spec)
}

pub fn spawn_spec(spec: WslCommandSpec) -> Result<WslChild, WslExecutionError> {
    let distro = spec.argv.get(2).cloned().ok_or(ShellError::InvalidDistro)?;
    let mut argv = spec.argv.into_iter();
    let program = argv.next().ok_or(ShellError::EmptyField("WSL program"))?;
    let mut command = Command::new(program);
    command
        .args(argv)
        .envs(spec.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().map(BufReader::new);
    let stderr = child.stderr.take();
    Ok(WslChild {
        distro,
        child,
        stdout,
        stderr,
    })
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
    child.distro = distro.to_owned();
    Ok(child)
}

/// Convert a Windows path with `wslpath -u` using argv boundaries.  The
/// resulting path is validated before it is suitable for `--cd`.
pub async fn convert_windows_path(
    distro: &str,
    windows_path: &str,
) -> Result<String, WslExecutionError> {
    let argv = build_wslpath_conversion_argv(distro, windows_path)?;
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

/// Verify a persisted WSL identity immediately before cleanup.  The caller
/// supplies fresh `/proc` outputs from the same distro; no signal command is
/// built unless marker, PID, PGID and SID all match.
pub async fn validate_identity(
    distro: &str,
    expected: &WslProcessIdentity,
) -> Result<WslProcessIdentity, WslExecutionError> {
    let environ = read_process_environ(distro, expected.pid).await?;
    if !crate::core::shell::environ_contains_exact_marker(&environ, &expected.marker)? {
        return Err(WslExecutionError::Shell(ShellError::MarkerMismatch));
    }
    let observed = read_process_identity(distro, expected).await?;
    validate_wsl_identity(expected, &observed)?;
    Ok(observed)
}

async fn read_process_environ(distro: &str, pid: u32) -> Result<Vec<u8>, WslExecutionError> {
    let argv = build_wsl_proc_environ_argv(distro, pid)?;
    let output = run_helper_output(&argv).await?;
    if !output.status.success() {
        return Err(WslExecutionError::CommandFailed {
            argv,
            code: output.status.code(),
        });
    }
    Ok(output.stdout)
}

async fn read_process_identity(
    distro: &str,
    expected: &WslProcessIdentity,
) -> Result<WslProcessIdentity, WslExecutionError> {
    let argv = build_wsl_proc_stat_argv(distro, expected.pid)?;
    let output = run_helper_output(&argv).await?;
    if !output.status.success() {
        return Err(WslExecutionError::CommandFailed {
            argv,
            code: output.status.code(),
        });
    }
    parse_proc_stat_identity(expected.pid, &output.stdout, &expected.marker).map_err(Into::into)
}

async fn run_helper_status(argv: &[String]) -> Result<(), WslExecutionError> {
    let output = run_helper_output(argv).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(WslExecutionError::CommandFailed {
            argv: argv.to_owned(),
            code: output.status.code(),
        })
    }
}

async fn run_helper_output(argv: &[String]) -> Result<std::process::Output, WslExecutionError> {
    let Some(program) = argv.first() else {
        return Err(WslExecutionError::Shell(ShellError::EmptyField(
            "helper program",
        )));
    };
    let mut command = Command::new(program);
    command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command.output().await.map_err(Into::into)
}

async fn wait_for_group_gone(
    distro: &str,
    plan: &WslTerminationPlan,
    timeout: Duration,
) -> Result<bool, WslExecutionError> {
    let deadline = Instant::now() + timeout;
    loop {
        let probe = run_helper_output(&plan.probe).await?;
        if !probe.status.success() {
            // A failed kill -0 is followed by a numeric /proc check.  If the
            // marker-bearing PID still exists, it must not be treated as a
            // vanished group (it may have escaped the group).
            let proc_probe = build_wsl_proc_dir_probe_argv(distro, plan.pid)?;
            let proc = run_helper_output(&proc_probe).await?;
            if !proc.status.success() {
                return Ok(true);
            }
            let environ = read_process_environ(distro, plan.pid).await?;
            if !crate::core::shell::environ_contains_exact_marker(&environ, &plan.marker)? {
                return Err(WslExecutionError::Shell(ShellError::MarkerMismatch));
            }
            return Ok(false);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        sleep(TERMINATION_POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(spec.argv[5], "--");
        assert_eq!(spec.argv[6], "setsid");
        assert_eq!(spec.argv[7], "bash");
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
}
