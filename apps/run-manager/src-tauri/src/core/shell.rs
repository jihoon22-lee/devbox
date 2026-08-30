//! Pure command-boundary and process-identity helpers for the execution
//! adapters.
//!
//! This module intentionally does not spawn a process or read the host.  The
//! platform adapters consume these values at the OS boundary, which keeps the
//! command syntax and the safety checks testable on every target.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use uuid::Uuid;

pub const DEVBOX_RUN_MARKER: &str = "DEVBOX_RUN_MARKER";
const DEVBOX_WRAPPER_ARG0: &str = "devbox-run-supervisor";
pub const HANDSHAKE_START: &str = "__DEVBOX_RUN_HANDSHAKE_V1__";
pub const HANDSHAKE_END: &str = "__DEVBOX_RUN_HANDSHAKE_END__";

/// An error raised before an untrusted value crosses a process boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellError {
    EmptyField(&'static str),
    NulByte(&'static str),
    InvalidEnvironmentKey(String),
    DuplicateEnvironmentKey(String),
    ReservedEnvironmentKey(String),
    InvalidUuid(&'static str),
    InvalidNumericField(&'static str),
    InvalidDistro,
    HandshakeNotFound,
    HandshakeMalformed,
    HandshakeRunIdMismatch,
    MarkerMismatch,
    IdentityMismatch(&'static str),
}

impl fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::NulByte(field) => write!(formatter, "{field} contains a NUL byte"),
            Self::InvalidEnvironmentKey(key) => {
                write!(formatter, "invalid environment key `{key}`")
            }
            Self::DuplicateEnvironmentKey(key) => {
                write!(formatter, "duplicate environment key `{key}`")
            }
            Self::ReservedEnvironmentKey(key) => {
                write!(formatter, "reserved environment key `{key}`")
            }
            Self::InvalidUuid(field) => write!(formatter, "invalid UUID in {field}"),
            Self::InvalidNumericField(field) => {
                write!(formatter, "invalid numeric value in {field}")
            }
            Self::InvalidDistro => formatter.write_str("WSL distribution is invalid"),
            Self::HandshakeNotFound => formatter.write_str("WSL handshake frame was not found"),
            Self::HandshakeMalformed => formatter.write_str("WSL handshake frame is malformed"),
            Self::HandshakeRunIdMismatch => {
                formatter.write_str("WSL handshake run ID does not match the attempt")
            }
            Self::MarkerMismatch => formatter.write_str("WSL process marker does not match"),
            Self::IdentityMismatch(field) => {
                write!(formatter, "WSL process identity mismatch in {field}")
            }
        }
    }
}

impl std::error::Error for ShellError {}

impl From<devbox_wsl::WslError> for ShellError {
    fn from(err: devbox_wsl::WslError) -> Self {
        match err {
            devbox_wsl::WslError::InvalidDistro(_) => ShellError::InvalidDistro,
            devbox_wsl::WslError::InvalidPath(_) => ShellError::EmptyField("WSL path"),
        }
    }
}

fn validate_nul_free(field: &'static str, value: &str) -> Result<(), ShellError> {
    if value.contains('\0') {
        Err(ShellError::NulByte(field))
    } else {
        Ok(())
    }
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), ShellError> {
    validate_nul_free(field, value)?;
    if value.is_empty() {
        Err(ShellError::EmptyField(field))
    } else {
        Ok(())
    }
}

/// Validate a run/attempt UUID without accepting alternate UUID syntaxes such
/// as braces or a `urn:` prefix.
pub fn validate_uuid(field: &'static str, value: &str) -> Result<Uuid, ShellError> {
    validate_nonempty(field, value)?;
    let uuid = Uuid::parse_str(value).map_err(|_| ShellError::InvalidUuid(field))?;
    if uuid.to_string().eq_ignore_ascii_case(value) {
        Ok(uuid)
    } else {
        Err(ShellError::InvalidUuid(field))
    }
}

/// Environment keys are deliberately narrower than the host API accepts so
/// they remain valid in both Win32 and Linux environments.
pub fn validate_environment_key(key: &str) -> Result<(), ShellError> {
    validate_nonempty("environment key", key)?;
    let mut characters = key.bytes();
    let Some(first) = characters.next() else {
        return Err(ShellError::InvalidEnvironmentKey(key.to_owned()));
    };
    if !(first == b'_' || first.is_ascii_alphabetic())
        || !characters.all(|character| character == b'_' || character.is_ascii_alphanumeric())
    {
        return Err(ShellError::InvalidEnvironmentKey(key.to_owned()));
    }
    if key.eq_ignore_ascii_case("WSLENV") || key.eq_ignore_ascii_case(DEVBOX_RUN_MARKER) {
        return Err(ShellError::ReservedEnvironmentKey(key.to_owned()));
    }
    Ok(())
}

/// Validate an environment map before it is passed to either adapter.
pub fn validate_environment(environment: &BTreeMap<String, String>) -> Result<(), ShellError> {
    let mut folded_keys = BTreeSet::new();
    for (key, value) in environment {
        validate_environment_key(key)?;
        if !folded_keys.insert(key.to_ascii_uppercase()) {
            return Err(ShellError::DuplicateEnvironmentKey(key.clone()));
        }
        validate_nul_free("environment value", value)?;
    }
    Ok(())
}

/// The command line and application name are kept as separate values for
/// `CreateProcessW`.  The `/C` suffix intentionally remains shell syntax: it
/// must not be quoted as an ordinary argv element, or `cmd.exe` would execute
/// a literal command instead of the user's requested pipeline/redirection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsShellCommand {
    pub application_name: String,
    pub command_line: String,
}

/// A direct Win32 process launch. `application_name` is intentionally `None`
/// so CreateProcessW applies its executable search rules to the first quoted
/// argv element. Unlike [`WindowsShellCommand`], no `cmd.exe` syntax is
/// involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsProcessCommand {
    pub application_name: Option<String>,
    pub command_line: String,
}

pub fn build_windows_shell_command(command: &str) -> Result<WindowsShellCommand, ShellError> {
    build_windows_shell_command_with_application("cmd.exe", command)
}

pub fn build_windows_shell_command_with_application(
    application_name: &str,
    command: &str,
) -> Result<WindowsShellCommand, ShellError> {
    validate_nonempty("application name", application_name)?;
    validate_nul_free("command", command)?;
    if command.is_empty() {
        return Err(ShellError::EmptyField("command"));
    }

    Ok(WindowsShellCommand {
        application_name: application_name.to_owned(),
        command_line: format!("cmd.exe /D /S /C {command}"),
    })
}

/// Quote an argv vector using the Win32 `CommandLineToArgvW`/CRT-compatible
/// backslash rule.  The WSL adapter uses `Command::arg` directly, but keeping
/// this encoder here makes the boundary explicit for callers that must hand a
/// complete command line to `CreateProcessW`.
pub fn quote_windows_argv(arguments: &[&str]) -> Result<String, ShellError> {
    let mut encoded = String::new();
    for (index, argument) in arguments.iter().enumerate() {
        validate_nul_free("argv", argument)?;
        if index != 0 {
            encoded.push(' ');
        }
        encoded.push_str(&quote_windows_argv_argument(argument));
    }
    Ok(encoded)
}

pub fn build_windows_process_command(
    program: &str,
    arguments: &[String],
) -> Result<WindowsProcessCommand, ShellError> {
    validate_nonempty("process program", program)?;
    let mut argv = Vec::with_capacity(arguments.len().saturating_add(1));
    argv.push(program);
    for argument in arguments {
        validate_nul_free("process argv", argument)?;
        argv.push(argument);
    }
    let command_line = quote_windows_argv(&argv)?;
    if command_line.encode_utf16().count().saturating_add(1) > 32_767 {
        return Err(ShellError::InvalidNumericField("process argv size"));
    }
    Ok(WindowsProcessCommand {
        application_name: None,
        command_line,
    })
}

fn quote_windows_argv_argument(argument: &str) -> String {
    if !argument.is_empty()
        && !argument
            .bytes()
            .any(|character| character == b' ' || character == b'\t' || character == b'"')
    {
        return argument.to_owned();
    }

    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        if character == '"' {
            quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
            quoted.push('"');
        } else {
            quoted.push_str(&"\\".repeat(backslashes));
            quoted.push(character);
        }
        backslashes = 0;
    }
    // Backslashes before the closing quote must be doubled.
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// The WSL command is passed as a separate `bash -c` positional argument.
/// Secrets never appear in this fixed supervisor script; they are supplied
/// through the child environment by [`prepare_wsl_environment`].
pub fn build_wsl_wrapper(run_id: &str, command: &str) -> Result<String, ShellError> {
    validate_uuid("run ID", run_id)?;
    validate_nul_free("command", command)?;
    if command.is_empty() {
        return Err(ShellError::EmptyField("command"));
    }

    // Keep a fixed supervisor shell as the process-group leader.  The user
    // command is consumed from `$1` by a nested bash, never interpolated into
    // this script.  Thus trailing `#`, unmatched `)`, heredocs, and other
    // shell syntax cannot comment out or rebalance the fixed cleanup code.
    // TERM is sent to every observed member except the supervisor; stubborn
    // descendants are escalated to KILL before the supervisor exits with the
    // user's original status.
    let wrapper = build_wsl_supervisor(run_id, r#"bash --noprofile --norc -lc "$1";"#)?;
    Ok(wrapper)
}

/// A process-mode supervisor invokes the supplied argv with `"$@"`. The
/// executable and arguments are separate `wsl.exe` argv values and are never
/// parsed as shell source.
pub fn build_wsl_process_wrapper(run_id: &str) -> Result<String, ShellError> {
    build_wsl_supervisor(run_id, r#""$@";"#)
}

fn build_wsl_supervisor(
    run_id: &str,
    fixed_invocation: &'static str,
) -> Result<String, ShellError> {
    validate_uuid("run ID", run_id)?;
    const SUPERVISOR: &str = r#"
printf '%s\n' '__DEVBOX_RUN_HANDSHAKE_V1__' "$DEVBOX_RUN_MARKER" "$$" "$(ps -o pgid= -p \"$$\")" "$(ps -o sid= -p \"$$\")" '__DEVBOX_RUN_HANDSHAKE_END__';
set +e;
__DEVBOX_RUN_INVOCATION__
__devbox_status=$?;
trap '' TERM;
__devbox_group_pids() {
  ps -eo pid=,pgid= | awk -v group="$$" '$2 == group && $1 != group { print $1 }';
};
for __devbox_pid in $(__devbox_group_pids); do
  kill -TERM "$__devbox_pid" 2>/dev/null || true;
done;
for __devbox_attempt in 1 2 3 4 5 6 7 8 9 10; do
  __devbox_pids=$(__devbox_group_pids);
  [ -z "$__devbox_pids" ] && break;
  sleep 0.05;
done;
for __devbox_pid in $(__devbox_group_pids); do
  kill -KILL "$__devbox_pid" 2>/dev/null || true;
done;
wait 2>/dev/null || true;
exit "$__devbox_status"
"#;
    // Keep these protocol literals in sync with the parser while avoiding any
    // user-controlled interpolation into the shell source.
    let wrapper = SUPERVISOR
        .replace("__DEVBOX_RUN_HANDSHAKE_V1__", HANDSHAKE_START)
        .replace("__DEVBOX_RUN_HANDSHAKE_END__", HANDSHAKE_END)
        .replace("__DEVBOX_RUN_INVOCATION__", fixed_invocation);
    Ok(wrapper)
}

/// The complete WSL argv and child environment.  `environment` contains the
/// values that the platform adapter must pass using `Command::env`; callers
/// must not serialize it into [`wrapper`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslCommandSpec {
    pub argv: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub wrapper: String,
}

pub fn build_wsl_command(
    distro: &str,
    cwd: Option<&str>,
    command: &str,
    run_id: &str,
    environment: &BTreeMap<String, String>,
    existing_wslenv: Option<&str>,
) -> Result<WslCommandSpec, ShellError> {
    if let Some(cwd) = cwd {
        validate_nonempty("WSL cwd", cwd)?;
    }
    validate_environment(environment)?;
    let wrapper = build_wsl_wrapper(run_id, command)?;
    let child_environment = prepare_wsl_environment(environment, existing_wslenv, run_id)?;

    // 순수 argv 조립(wsl.exe -d <distro> [--cd <cwd>] --)은 devbox-wsl 프리미티브를 쓴다.
    // 빈 command 베이스에 실행 정책(setsid·bash 플래그·wrapper)을 덧붙인다.
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, cwd, "")
        .map_err(|_| ShellError::InvalidDistro)?;
    argv.extend([
        "setsid".to_owned(),
        "bash".to_owned(),
        "--noprofile".to_owned(),
        "--norc".to_owned(),
        "-lc".to_owned(),
        wrapper.clone(),
        DEVBOX_WRAPPER_ARG0.to_owned(),
        command.to_owned(),
    ]);

    Ok(WslCommandSpec {
        argv,
        environment: child_environment,
        wrapper,
    })
}

pub fn build_wsl_process_command(
    distro: &str,
    cwd: Option<&str>,
    program: &str,
    arguments: &[String],
    run_id: &str,
    environment: &BTreeMap<String, String>,
    existing_wslenv: Option<&str>,
) -> Result<WslCommandSpec, ShellError> {
    if let Some(cwd) = cwd {
        validate_nonempty("WSL cwd", cwd)?;
    }
    validate_nonempty("process program", program)?;
    for argument in arguments {
        validate_nul_free("process argv", argument)?;
    }
    validate_environment(environment)?;
    let wrapper = build_wsl_process_wrapper(run_id)?;
    let child_environment = prepare_wsl_environment(environment, existing_wslenv, run_id)?;
    let mut argv = devbox_wsl::argv::build_exec_argv(distro, cwd, "")
        .map_err(|_| ShellError::InvalidDistro)?;
    argv.extend([
        "setsid".to_owned(),
        "bash".to_owned(),
        "--noprofile".to_owned(),
        "--norc".to_owned(),
        "-c".to_owned(),
        wrapper.clone(),
        DEVBOX_WRAPPER_ARG0.to_owned(),
        program.to_owned(),
    ]);
    argv.extend(arguments.iter().cloned());
    Ok(WslCommandSpec {
        argv,
        environment: child_environment,
        wrapper,
    })
}

/// Merge WSLENV without allowing a caller to retain a conflicting direction
/// flag or duplicate entry for a managed variable.
pub fn merge_wslenv(
    existing: Option<&str>,
    managed_keys: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<String, ShellError> {
    let mut managed = BTreeSet::new();
    let mut managed_folded = BTreeSet::new();
    for key in managed_keys {
        let key = key.as_ref();
        validate_environment_key(key)?;
        managed.insert(key.to_owned());
        managed_folded.insert(key.to_ascii_uppercase());
    }

    let mut entries = Vec::new();
    if let Some(existing) = existing {
        validate_nul_free("WSLENV", existing)?;
        for entry in existing.split(':').filter(|entry| !entry.is_empty()) {
            let name = entry.split('/').next().unwrap_or_default();
            if name.eq_ignore_ascii_case(DEVBOX_RUN_MARKER)
                || managed_folded.contains(&name.to_ascii_uppercase())
            {
                continue;
            }
            entries.push(entry.to_owned());
        }
    }
    entries.extend(managed.into_iter().map(|key| format!("{key}/w")));
    entries.push(format!("{DEVBOX_RUN_MARKER}/w"));
    Ok(entries.join(":"))
}

pub fn prepare_wsl_environment(
    environment: &BTreeMap<String, String>,
    existing_wslenv: Option<&str>,
    run_id: &str,
) -> Result<BTreeMap<String, String>, ShellError> {
    validate_environment(environment)?;
    let run_id = validate_uuid("run ID", run_id)?.to_string();
    let mut child_environment = environment.clone();
    let managed_keys = child_environment.keys().cloned().collect::<Vec<_>>();
    child_environment.insert(DEVBOX_RUN_MARKER.to_owned(), run_id.to_owned());
    child_environment.insert(
        "WSLENV".to_owned(),
        merge_wslenv(existing_wslenv, managed_keys.iter().map(String::as_str))?,
    );
    Ok(child_environment)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WslHandshake {
    pub run_id: Uuid,
    pub pid: u32,
    pub pgid: u32,
    pub sid: u32,
}

impl WslHandshake {
    pub fn run_id_string(self) -> String {
        self.run_id.to_string()
    }

    pub fn identity(self) -> WslProcessIdentity {
        WslProcessIdentity {
            pid: self.pid,
            pgid: self.pgid,
            sid: self.sid,
            marker: self.run_id_string(),
        }
    }
}

/// Parse a complete frame from any position in stdout.  Lines before the
/// frame may contain distro startup noise, and lines after it belong to the
/// user's command.
pub fn parse_wsl_handshake(
    stdout: &[u8],
    expected_run_id: &str,
) -> Result<WslHandshake, ShellError> {
    let expected = validate_uuid("run ID", expected_run_id)?;
    let lines = stdout
        .split(|byte| *byte == b'\n')
        .map(|line| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            std::str::from_utf8(line)
                .ok()
                .map(|line| line.trim_matches(|character: char| character.is_ascii_whitespace()))
        })
        .collect::<Vec<_>>();

    let mut saw_start = false;
    let mut saw_run_id_mismatch = false;
    for index in 0..lines.len() {
        if lines[index] != Some(HANDSHAKE_START) {
            continue;
        }
        saw_start = true;
        let Some(frame) = lines.get(index..index + 6) else {
            continue;
        };
        if frame.iter().any(Option::is_none) || frame[5] != Some(HANDSHAKE_END) {
            continue;
        }
        let run_id = validate_uuid("handshake run ID", frame[1].unwrap())?;
        if run_id != expected {
            saw_run_id_mismatch = true;
            continue;
        }
        let pid = parse_positive_number("handshake PID", frame[2].unwrap())?;
        let pgid = parse_positive_number("handshake PGID", frame[3].unwrap())?;
        let sid = parse_positive_number("handshake SID", frame[4].unwrap())?;
        return Ok(WslHandshake {
            run_id,
            pid,
            pgid,
            sid,
        });
    }

    if saw_run_id_mismatch {
        Err(ShellError::HandshakeRunIdMismatch)
    } else if saw_start {
        Err(ShellError::HandshakeMalformed)
    } else {
        Err(ShellError::HandshakeNotFound)
    }
}

fn parse_positive_number(field: &'static str, value: &str) -> Result<u32, ShellError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ShellError::InvalidNumericField(field));
    }
    let number = value
        .parse::<u32>()
        .map_err(|_| ShellError::InvalidNumericField(field))?;
    if number == 0 {
        return Err(ShellError::InvalidNumericField(field));
    }
    Ok(number)
}

/// `/proc/<pid>/environ` is NUL-separated.  Checking complete entries avoids
/// accepting a marker that merely appears as a prefix of another variable.
pub fn environ_contains_exact_marker(environ: &[u8], run_id: &str) -> Result<bool, ShellError> {
    let run_id = validate_uuid("run ID", run_id)?.to_string();
    let expected = format!("{DEVBOX_RUN_MARKER}={run_id}");
    Ok(environ
        .split(|byte| *byte == 0)
        .any(|entry| entry == expected.as_bytes()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslProcessIdentity {
    pub pid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub marker: String,
}

impl WslProcessIdentity {
    pub fn validate(&self) -> Result<(), ShellError> {
        if self.pid == 0 {
            return Err(ShellError::InvalidNumericField("PID"));
        }
        if self.pgid == 0 {
            return Err(ShellError::InvalidNumericField("PGID"));
        }
        if self.sid == 0 {
            return Err(ShellError::InvalidNumericField("SID"));
        }
        validate_uuid("process marker", &self.marker)?;
        Ok(())
    }
}

pub fn validate_wsl_handshake_identity(
    handshake: WslHandshake,
    expected_run_id: &str,
    environ: &[u8],
) -> Result<WslProcessIdentity, ShellError> {
    let expected = validate_uuid("run ID", expected_run_id)?;
    if handshake.run_id != expected {
        return Err(ShellError::HandshakeRunIdMismatch);
    }
    if !environ_contains_exact_marker(environ, expected_run_id)? {
        return Err(ShellError::MarkerMismatch);
    }
    Ok(handshake.identity())
}

/// Identity validation is a mandatory precondition for every WSL group kill.
/// Callers must obtain `observed` from fresh `/proc` data immediately before
/// building the signal command; a stale DB row alone is never sufficient.
pub fn validate_wsl_identity(
    expected: &WslProcessIdentity,
    observed: &WslProcessIdentity,
) -> Result<(), ShellError> {
    expected.validate()?;
    observed.validate()?;
    if expected.marker != observed.marker {
        return Err(ShellError::IdentityMismatch("marker"));
    }
    if expected.pid != observed.pid {
        return Err(ShellError::IdentityMismatch("PID"));
    }
    if expected.pgid != observed.pgid {
        return Err(ShellError::IdentityMismatch("PGID"));
    }
    if expected.sid != observed.sid {
        return Err(ShellError::IdentityMismatch("SID"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WslSignal {
    Term,
    Kill,
}

impl WslSignal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Term => "TERM",
            Self::Kill => "KILL",
        }
    }
}

pub fn build_wsl_group_signal_argv(
    distro: &str,
    signal: WslSignal,
    pgid: u32,
) -> Result<Vec<String>, ShellError> {
    devbox_wsl::distro::validate_distro_name(distro).map_err(|_| ShellError::InvalidDistro)?;
    if pgid == 0 {
        return Err(ShellError::InvalidNumericField("PGID"));
    }
    Ok(vec![
        "wsl.exe".to_owned(),
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "kill".to_owned(),
        format!("-{}", signal.as_str()),
        "--".to_owned(),
        format!("-{pgid}"),
    ])
}

pub fn build_wsl_group_probe_argv(distro: &str, pgid: u32) -> Result<Vec<String>, ShellError> {
    devbox_wsl::distro::validate_distro_name(distro).map_err(|_| ShellError::InvalidDistro)?;
    if pgid == 0 {
        return Err(ShellError::InvalidNumericField("PGID"));
    }
    Ok(vec![
        "wsl.exe".to_owned(),
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "kill".to_owned(),
        "-0".to_owned(),
        "--".to_owned(),
        format!("-{pgid}"),
    ])
}

/// A numeric-only `/proc` path used by the platform adapter for a fresh marker
/// and process-group identity check.  It is returned as argv rather than a
/// shell fragment so a PID can never become command syntax.
pub fn build_wsl_proc_environ_argv(distro: &str, pid: u32) -> Result<Vec<String>, ShellError> {
    devbox_wsl::distro::validate_distro_name(distro).map_err(|_| ShellError::InvalidDistro)?;
    if pid == 0 {
        return Err(ShellError::InvalidNumericField("PID"));
    }
    Ok(vec![
        "wsl.exe".to_owned(),
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "cat".to_owned(),
        "--".to_owned(),
        format!("/proc/{pid}/environ"),
    ])
}

pub fn build_wsl_proc_stat_argv(distro: &str, pid: u32) -> Result<Vec<String>, ShellError> {
    devbox_wsl::distro::validate_distro_name(distro).map_err(|_| ShellError::InvalidDistro)?;
    if pid == 0 {
        return Err(ShellError::InvalidNumericField("PID"));
    }
    Ok(vec![
        "wsl.exe".to_owned(),
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "cat".to_owned(),
        "--".to_owned(),
        format!("/proc/{pid}/stat"),
    ])
}

pub fn build_wsl_proc_dir_probe_argv(distro: &str, pid: u32) -> Result<Vec<String>, ShellError> {
    devbox_wsl::distro::validate_distro_name(distro).map_err(|_| ShellError::InvalidDistro)?;
    if pid == 0 {
        return Err(ShellError::InvalidNumericField("PID"));
    }
    Ok(vec![
        "wsl.exe".to_owned(),
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "test".to_owned(),
        "-d".to_owned(),
        format!("/proc/{pid}"),
    ])
}

/// Parse Linux `/proc/<pid>/stat`'s process-group and session columns.  The
/// command name is enclosed in parentheses and may itself contain spaces or
/// `)`; parsing starts after the final `)` rather than splitting the prefix.
pub fn parse_proc_stat_identity(
    pid: u32,
    stat: &[u8],
    marker: &str,
) -> Result<WslProcessIdentity, ShellError> {
    if pid == 0 {
        return Err(ShellError::InvalidNumericField("PID"));
    }
    validate_uuid("process marker", marker)?;
    let stat = std::str::from_utf8(stat)
        .map_err(|_| ShellError::InvalidNumericField("/proc stat"))?
        .trim();
    let after_comm = stat
        .rfind(')')
        .and_then(|index| stat.get(index + 1..))
        .ok_or(ShellError::InvalidNumericField("/proc stat"))?;
    let fields = after_comm.split_whitespace().collect::<Vec<_>>();
    // After `comm`, field 3 (state) is index 0; pgrp is field 5 (index 2),
    // and session is field 6 (index 3).
    let pgid = parse_positive_number("/proc PGID", fields.get(2).copied().unwrap_or_default())?;
    let sid = parse_positive_number("/proc SID", fields.get(3).copied().unwrap_or_default())?;
    Ok(WslProcessIdentity {
        pid,
        pgid,
        sid,
        marker: marker.to_owned(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslTerminationPlan {
    pub term: Vec<String>,
    pub probe: Vec<String>,
    pub kill: Vec<String>,
    pub marker: String,
    pub pid: u32,
    pub pgid: u32,
    pub sid: u32,
}

pub fn build_wsl_termination_plan(
    distro: &str,
    identity: &WslProcessIdentity,
) -> Result<WslTerminationPlan, ShellError> {
    devbox_wsl::distro::validate_distro_name(distro).map_err(|_| ShellError::InvalidDistro)?;
    identity.validate()?;
    Ok(WslTerminationPlan {
        term: build_wsl_group_signal_argv(distro, WslSignal::Term, identity.pgid)?,
        probe: build_wsl_group_probe_argv(distro, identity.pgid)?,
        kill: build_wsl_group_signal_argv(distro, WslSignal::Kill, identity.pgid)?,
        marker: identity.marker.clone(),
        pid: identity.pid,
        pgid: identity.pgid,
        sid: identity.sid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_id() -> &'static str {
        "123e4567-e89b-12d3-a456-426614174000"
    }

    #[test]
    fn rejects_nul_and_reserved_environment_keys() {
        let mut environment = BTreeMap::from([(String::from("TOKEN"), String::from("a\0b"))]);
        assert_eq!(
            validate_environment(&environment),
            Err(ShellError::NulByte("environment value"))
        );

        environment.clear();
        environment.insert(String::from("WSLENV"), String::from("TOKEN/w"));
        assert!(matches!(
            validate_environment(&environment),
            Err(ShellError::ReservedEnvironmentKey(_))
        ));
        environment.clear();
        environment.insert(String::from(DEVBOX_RUN_MARKER), String::from("spoof"));
        assert!(matches!(
            validate_environment(&environment),
            Err(ShellError::ReservedEnvironmentKey(_))
        ));
        environment.clear();
        environment.insert(String::from("wslenv"), String::from("spoof"));
        assert!(matches!(
            validate_environment(&environment),
            Err(ShellError::ReservedEnvironmentKey(_))
        ));

        environment.clear();
        environment.insert(String::from("TOKEN"), String::from("first"));
        environment.insert(String::from("token"), String::from("second"));
        assert!(matches!(
            validate_environment(&environment),
            Err(ShellError::DuplicateEnvironmentKey(_))
        ));
    }

    #[test]
    fn preserves_cmd_shell_syntax_after_c_switch() {
        let command = build_windows_shell_command(r#"echo "hello world" & echo done"#).unwrap();
        assert_eq!(command.application_name, "cmd.exe");
        assert_eq!(
            command.command_line,
            r#"cmd.exe /D /S /C echo "hello world" & echo done"#
        );
    }

    #[test]
    fn windows_argv_quote_keeps_single_argument_boundaries() {
        let encoded = quote_windows_argv(&["wsl.exe", "--", "hello world", r#"a\"b"#]).unwrap();
        assert_eq!(encoded, r#"wsl.exe -- "hello world" "a\\\"b""#);
    }

    #[test]
    fn windows_process_command_never_adds_cmd_shell() {
        let command = build_windows_process_command(
            "tool.exe",
            &["hello world".into(), "a&b".into(), "$(literal)".into()],
        )
        .unwrap();
        assert_eq!(command.application_name, None);
        assert_eq!(
            command.command_line,
            r#"tool.exe "hello world" a&b $(literal)"#
        );
        assert!(!command.command_line.contains("cmd.exe"));
    }

    #[test]
    fn wsl_process_command_keeps_metacharacters_in_separate_argv() {
        let spec = build_wsl_process_command(
            "Ubuntu",
            Some("/home/user/project"),
            "printf",
            &["%s".into(), "$(not-executed); rm -rf nope".into()],
            run_id(),
            &BTreeMap::new(),
            None,
        )
        .unwrap();
        assert!(spec.wrapper.contains(r#""$@";"#));
        assert!(!spec.wrapper.contains("not-executed"));
        assert_eq!(
            &spec.argv[spec.argv.len() - 3..],
            ["printf", "%s", "$(not-executed); rm -rf nope"]
        );
        assert!(!spec.argv.iter().any(|value| value == "-lc"));
    }

    #[test]
    fn wslenv_replaces_managed_conflicts_and_keeps_unrelated_flags() {
        let merged = merge_wslenv(
            Some("PATH/l:secret/u:SECRET/w:OTHER:devbox_run_marker/u"),
            ["SECRET", "TOKEN"],
        )
        .unwrap();
        assert_eq!(merged, "PATH/l:OTHER:SECRET/w:TOKEN/w:DEVBOX_RUN_MARKER/w");
    }

    #[test]
    fn wsl_command_keeps_secrets_out_of_wrapper_and_uses_fixed_argv() {
        let environment =
            BTreeMap::from([(String::from("SECRET"), String::from("value with spaces"))]);
        let spec = build_wsl_command(
            "Ubuntu 24.04",
            Some("/tmp/work tree"),
            "printf '%s' \"$SECRET\" | cat",
            run_id(),
            &environment,
            Some("PATH/l"),
        )
        .unwrap();
        assert_eq!(
            &spec.argv[..11],
            [
                "wsl.exe",
                "-d",
                "Ubuntu 24.04",
                "--cd",
                "/tmp/work tree",
                "--",
                "setsid",
                "bash",
                "--noprofile",
                "--norc",
                "-lc",
            ]
        );
        assert!(spec.wrapper.contains("bash --noprofile --norc -lc \"$1\""));
        assert!(spec.wrapper.contains("__devbox_group_pids"));
        assert!(!spec.wrapper.contains("printf '%s' \"$SECRET\" | cat"));
        assert_eq!(
            spec.argv.last().map(String::as_str),
            Some("printf '%s' \"$SECRET\" | cat")
        );
        assert!(!spec.wrapper.contains("value with spaces"));
        assert_eq!(
            spec.environment.get("SECRET"),
            Some(&String::from("value with spaces"))
        );
        assert_eq!(
            spec.environment.get(DEVBOX_RUN_MARKER),
            Some(&String::from(run_id()))
        );
        assert_eq!(
            spec.environment.get("WSLENV"),
            Some(&String::from("PATH/l:SECRET/w:DEVBOX_RUN_MARKER/w"))
        );
        assert!(!spec.wrapper.contains("value with spaces"));
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_keeps_shell_syntax_out_of_fixed_cleanup_and_reaps_descendants() {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("background.pid");
        let command = format!("sleep 30 & echo $! > {}", pid_file.to_string_lossy());
        let run = |command: &str| {
            let wrapper = build_wsl_wrapper(run_id(), command).unwrap();
            assert!(!wrapper.contains(command));
            std::process::Command::new("setsid")
                .args([
                    "bash",
                    "--noprofile",
                    "--norc",
                    "-lc",
                    &wrapper,
                    DEVBOX_WRAPPER_ARG0,
                    command,
                ])
                .env(DEVBOX_RUN_MARKER, run_id())
                .output()
                .expect("setsid/bash fixture must be available")
        };
        let syntax = run("printf '%s' 'literal # and )'");
        assert!(syntax.status.success(), "syntax output: {syntax:?}");
        let heredoc = run("cat <<'EOF'\nliteral # and )\nEOF");
        assert!(heredoc.status.success(), "heredoc output: {heredoc:?}");
        assert!(String::from_utf8_lossy(&heredoc.stdout).contains("literal # and )"));
        let output = run(&command);
        assert!(output.status.success(), "supervisor output: {:?}", output);
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            let alive = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
            if !alive {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("background group member {pid} survived supervisor exit");
    }

    #[test]
    fn parses_handshake_after_noise_and_validates_exact_marker() {
        let stdout = format!(
            "wsl.exe: startup noise\n{HANDSHAKE_START}\n{}\n42\n41\n40\n{HANDSHAKE_END}\nuser output\n",
            run_id()
        );
        let handshake = parse_wsl_handshake(stdout.as_bytes(), run_id()).unwrap();
        let environ = format!("PATH=/bin\0{DEVBOX_RUN_MARKER}={}\0", run_id());
        let identity =
            validate_wsl_handshake_identity(handshake, run_id(), environ.as_bytes()).unwrap();
        assert_eq!(identity.pid, 42);
        assert!(!environ_contains_exact_marker(
            b"DEVBOX_RUN_MARKER=123e4567-e89b-12d3-a456-42661417400\0",
            run_id()
        )
        .unwrap());
    }

    #[test]
    fn rejects_a_complete_handshake_for_another_run() {
        let other = "123e4567-e89b-12d3-a456-426614174001";
        let stdout = format!("{HANDSHAKE_START}\n{other}\n42\n41\n40\n{HANDSHAKE_END}\n");
        assert_eq!(
            parse_wsl_handshake(stdout.as_bytes(), run_id()),
            Err(ShellError::HandshakeRunIdMismatch)
        );
    }

    #[test]
    fn termination_plan_is_group_only_and_identity_must_match() {
        let identity = WslProcessIdentity {
            pid: 42,
            pgid: 41,
            sid: 40,
            marker: run_id().to_owned(),
        };
        let plan = build_wsl_termination_plan("Ubuntu", &identity).unwrap();
        assert_eq!(
            plan.term,
            vec!["wsl.exe", "-d", "Ubuntu", "--", "kill", "-TERM", "--", "-41"]
        );
        assert_eq!(
            plan.probe,
            vec!["wsl.exe", "-d", "Ubuntu", "--", "kill", "-0", "--", "-41"]
        );
        assert_eq!(
            plan.kill,
            vec!["wsl.exe", "-d", "Ubuntu", "--", "kill", "-KILL", "--", "-41"]
        );

        let mut other = identity.clone();
        other.pid = 43;
        assert_eq!(
            validate_wsl_identity(&identity, &other),
            Err(ShellError::IdentityMismatch("PID"))
        );
    }

    #[test]
    fn parses_proc_stat_group_and_session_after_command_name() {
        let stat = b"42 (worker with ) name) S 1 41 40 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let identity = parse_proc_stat_identity(42, stat, run_id()).unwrap();
        assert_eq!(identity.pgid, 41);
        assert_eq!(identity.sid, 40);
        assert_eq!(
            build_wsl_proc_stat_argv("Ubuntu", 42).unwrap(),
            vec![
                "wsl.exe",
                "-d",
                "Ubuntu",
                "--",
                "cat",
                "--",
                "/proc/42/stat"
            ]
        );
        assert_eq!(
            build_wsl_proc_dir_probe_argv("Ubuntu", 42).unwrap(),
            vec!["wsl.exe", "-d", "Ubuntu", "--", "test", "-d", "/proc/42"]
        );
    }
}
