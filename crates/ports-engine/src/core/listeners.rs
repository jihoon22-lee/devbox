#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

//! Port Manager listener data model and identity checks.
//!
//! This module deliberately has no process execution or platform API calls. It
//! owns the data boundary shared by the Windows and WSL command adapters so a
//! frontend cannot turn a displayed PID into an arbitrary process-kill
//! request. Platform adapters must re-query an endpoint and its process
//! identity immediately before executing a signal/terminate operation, then
//! call validate_kill_target.

use devbox_process::{extract_port, parse_netstat_output, PortInfo};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_SOURCE_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_LISTENER_ROWS: usize = 4_096;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const MAX_WSL_DISTROS: usize = 16;
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const MAX_DETAIL_LOOKUPS: usize = 256;
pub const MAX_COMMAND_LINE_BYTES: usize = 8 * 1024;
pub const MAX_EXECUTABLE_PATH_BYTES: usize = 4 * 1024;
pub const MAX_NAME_BYTES: usize = 512;
pub const MAX_DISTRO_BYTES: usize = 128;
pub const MAX_CONTAINER_FIELD_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListenerSource {
    Windows,
    Wsl,
    Container,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListenerEndpoint {
    pub proto: String,
    pub local_addr: String,
    pub port: u16,
    pub state: String,
}

impl ListenerEndpoint {
    pub fn from_port(port: &PortInfo) -> Self {
        Self {
            proto: port.proto.clone(),
            local_addr: port.local_addr.clone(),
            port: port.port,
            state: port.state.clone(),
        }
    }

    pub fn validate_listener(&self) -> Result<(), ListenerError> {
        if self.port == 0
            || self.local_addr.is_empty()
            || self.local_addr.len() > MAX_NAME_BYTES
            || self.local_addr.chars().any(char::is_control)
            || self.proto.len() > 16
            || self.proto.chars().any(char::is_control)
            || self.state.len() > 32
            || self.state.chars().any(char::is_control)
        {
            return Err(ListenerError::InvalidRequest);
        }

        let proto = self.proto.to_ascii_uppercase();
        let state = self.state.to_ascii_uppercase();
        let valid_proto = matches!(
            proto.as_str(),
            "TCP" | "TCP4" | "TCP6" | "UDP" | "UDP4" | "UDP6"
        );
        let valid_state =
            state.is_empty() || matches!(state.as_str(), "LISTENING" | "UNCONN" | "BOUND");
        if !valid_proto || !valid_state {
            return Err(ListenerError::InvalidRequest);
        }
        Ok(())
    }

    pub fn is_listener(&self) -> bool {
        let state = self.state.to_ascii_uppercase();
        state.is_empty() || matches!(state.as_str(), "LISTENING" | "UNCONN" | "BOUND")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ListenerIdentity {
    /// Windows process creation FILETIME ticks (100 ns units since 1601 UTC).
    /// It is a decimal string on the wire so JavaScript cannot round it.
    Windows { pid: u32, start_time: String },
    /// Linux proc pid stat field 22, scoped by distro and PID.
    Wsl {
        distro: String,
        pid: u32,
        start_tick: u64,
    },
    /// Container identities never become process-kill requests.
    Container {
        engine: String,
        container_id: String,
        distro: String,
    },
}

impl ListenerIdentity {
    pub fn validate(&self) -> Result<(), ListenerError> {
        match self {
            Self::Windows { pid, start_time } => {
                if *pid == 0
                    || start_time.is_empty()
                    || start_time.len() > 20
                    || !start_time.bytes().all(|byte| byte.is_ascii_digit())
                    || start_time
                        .parse::<u64>()
                        .ok()
                        .is_none_or(|value| value == 0)
                {
                    return Err(ListenerError::InvalidRequest);
                }
            }
            Self::Wsl {
                distro,
                pid,
                start_tick,
            } => {
                validate_distro(distro)?;
                if *pid == 0 || *start_tick == 0 {
                    return Err(ListenerError::InvalidRequest);
                }
            }
            Self::Container {
                engine,
                container_id,
                distro,
            } => {
                validate_bounded_field(engine, MAX_CONTAINER_FIELD_BYTES)?;
                validate_bounded_field(container_id, MAX_CONTAINER_FIELD_BYTES)?;
                validate_distro(distro)?;
                if !matches!(engine.to_ascii_lowercase().as_str(), "docker" | "podman")
                    || container_id.is_empty()
                    || !container_id.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(ListenerError::InvalidRequest);
                }
            }
        }
        Ok(())
    }
}

/// A frontend request contains only endpoint and identity preconditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KillListenerRequest {
    pub endpoint: ListenerEndpoint,
    pub identity: ListenerIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenerSnapshot {
    pub endpoint: ListenerEndpoint,
    pub identity: ListenerIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillAction {
    WindowsProcess,
    WslProcess,
    ContainerHandoff,
}

/// Validated intent for the WSL Desktop-owned container stop action. The
/// one-time applink store is intentionally owned by the applink issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerStopHandoff {
    pub target_app: String,
    pub action: String,
    pub engine: String,
    pub container_id: String,
    pub distro: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenerError {
    InvalidRequest,
    StaleTarget,
    UnsupportedSource,
    SourceUnavailable,
    ProcessUnavailable,
    ProcessAccessDenied,
    CommandOutputTooLarge,
    CommandTimedOut,
}

impl ListenerError {
    /// Fixed user-facing text. No PID, path, distro, command line, or stderr
    /// is interpolated so failures cannot echo sensitive input.
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "요청을 확인할 수 없습니다.",
            Self::StaleTarget => "대상이 변경되어 종료하지 않았습니다. 목록을 새로고침하세요.",
            Self::UnsupportedSource => "이 대상은 Port Manager에서 직접 종료할 수 없습니다.",
            Self::SourceUnavailable => "listener 정보를 가져오지 못했습니다.",
            Self::ProcessUnavailable => "프로세스를 확인할 수 없습니다.",
            Self::ProcessAccessDenied => "프로세스를 종료할 권한이 없습니다.",
            Self::CommandOutputTooLarge => "listener 출력이 허용된 크기를 초과했습니다.",
            Self::CommandTimedOut => "listener 조회 시간이 허용 범위를 초과했습니다.",
        }
    }
}

impl fmt::Display for ListenerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for ListenerError {}

/// Compare both endpoint and identity. A reused PID with a different creation
/// time/start tick is rejected even when the port number is unchanged.
pub fn validate_kill_target(
    request: &KillListenerRequest,
    observed: &ListenerSnapshot,
) -> Result<KillAction, ListenerError> {
    request.endpoint.validate_listener()?;
    request.identity.validate()?;
    observed.endpoint.validate_listener()?;
    observed.identity.validate()?;

    if !request.endpoint.is_listener()
        || request.endpoint != observed.endpoint
        || request.identity != observed.identity
    {
        return Err(ListenerError::StaleTarget);
    }

    Ok(match request.identity {
        ListenerIdentity::Windows { .. } => KillAction::WindowsProcess,
        ListenerIdentity::Wsl { .. } => KillAction::WslProcess,
        ListenerIdentity::Container { .. } => KillAction::ContainerHandoff,
    })
}

pub fn container_stop_handoff(
    identity: &ListenerIdentity,
) -> Result<ContainerStopHandoff, ListenerError> {
    let ListenerIdentity::Container {
        engine,
        container_id,
        distro,
    } = identity
    else {
        return Err(ListenerError::UnsupportedSource);
    };
    identity.validate()?;
    Ok(ContainerStopHandoff {
        target_app: "wsl-desktop".to_owned(),
        action: "stop-container".to_owned(),
        engine: engine.clone(),
        container_id: container_id.clone(),
        distro: distro.clone(),
    })
}

pub fn parse_windows_ports(input: &str) -> Result<Vec<PortInfo>, ListenerError> {
    ensure_output_bound(input.as_bytes())?;
    Ok(parse_netstat_output(input)
        .into_iter()
        .filter(|port| port.port > 0)
        .take(MAX_LISTENER_ROWS)
        .collect())
}

/// Minimal ss -H -lntup parser. ss is invoked with fixed argv by the command
/// adapter; this parser accepts only bounded, local endpoint rows.
pub fn parse_wsl_ss_output(input: &str) -> Result<Vec<WslPort>, ListenerError> {
    ensure_output_bound(input.as_bytes())?;
    let mut rows = Vec::new();
    for line in input.lines().take(MAX_LISTENER_ROWS) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 5 {
            continue;
        }
        let proto = match fields[0].to_ascii_lowercase().as_str() {
            "tcp" | "tcp4" => "TCP",
            "tcp6" => "TCP6",
            "udp" | "udp4" => "UDP",
            "udp6" => "UDP6",
            _ => continue,
        };
        let state = match fields[1].to_ascii_uppercase().as_str() {
            "LISTEN" if matches!(proto, "TCP" | "TCP6") => "LISTENING",
            "UNCONN" if matches!(proto, "UDP" | "UDP6") => "UNCONN",
            _ => continue,
        };
        let local_addr = fields[4].to_owned();
        let port = extract_port(&local_addr);
        if port == 0
            || local_addr.len() > MAX_NAME_BYTES
            || local_addr.chars().any(char::is_control)
        {
            continue;
        }
        let pid = fields
            .iter()
            .skip(5)
            .find_map(|field| parse_pid_from_ss(field));
        let process_name = fields
            .iter()
            .skip(5)
            .find_map(|field| parse_name_from_ss(field));
        rows.push(WslPort {
            port: PortInfo {
                proto: proto.to_owned(),
                local_addr,
                port,
                state: state.to_owned(),
                pid,
            },
            process_name,
        });
    }
    Ok(rows)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslPort {
    pub port: PortInfo,
    pub process_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerPort {
    pub container_id: String,
    pub container_name: String,
    pub distro: String,
    pub host_addr: String,
    pub host_port: u16,
    pub proto: String,
}

/// Parse the deliberately tab-separated docker ps format used by the command
/// adapter: ID, name, and published ports. Unpublished ports are ignored.
pub fn parse_docker_ps_output(
    input: &str,
    distro: &str,
) -> Result<Vec<ContainerPort>, ListenerError> {
    ensure_output_bound(input.as_bytes())?;
    validate_distro(distro)?;
    let mut rows = Vec::new();
    for line in input.lines().take(MAX_LISTENER_ROWS) {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 3 {
            continue;
        }
        let id = fields[0].trim();
        let name = fields[1].trim();
        if id.is_empty()
            || id.len() > MAX_CONTAINER_FIELD_BYTES
            || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            continue;
        }
        let Some(name) = bounded_display(name, MAX_CONTAINER_FIELD_BYTES) else {
            continue;
        };
        for mapping in fields[2].split(',') {
            let mapping = mapping.trim();
            let Some((host, remote)) = mapping.split_once("->") else {
                continue;
            };
            let Some(proto) = remote
                .rsplit_once('/')
                .map(|(_, value)| value.trim().to_ascii_uppercase())
                .filter(|value| matches!(value.as_str(), "TCP" | "UDP"))
            else {
                continue;
            };
            let Some((host_addr, port_text)) = split_host_port(host.trim()) else {
                continue;
            };
            let Ok(host_port) = port_text.parse::<u16>() else {
                continue;
            };
            if host_port == 0 {
                continue;
            }
            rows.push(ContainerPort {
                container_id: id.to_owned(),
                container_name: name.clone(),
                distro: distro.to_owned(),
                host_addr,
                host_port,
                proto,
            });
            if rows.len() >= MAX_LISTENER_ROWS {
                return Ok(rows);
            }
        }
    }
    Ok(rows)
}

/// Fixed WSL command argv builders. The returned values exclude the executable
/// name so callers can pass them directly to wsl.exe. Dynamic data is limited
/// to a validated distro or decimal PID; no shell command string is accepted.
pub fn build_wsl_listener_argv(distro: &str) -> Result<Vec<String>, ListenerError> {
    validate_distro(distro)?;
    Ok(vec![
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "ss".to_owned(),
        "-H".to_owned(),
        "-lntup".to_owned(),
    ])
}

pub fn build_wsl_proc_stat_argv(distro: &str, pid: u32) -> Result<Vec<String>, ListenerError> {
    validate_distro(distro)?;
    let path = proc_path(pid, "stat")?;
    Ok(vec![
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "cat".to_owned(),
        path,
    ])
}

pub fn build_wsl_proc_cmdline_argv(distro: &str, pid: u32) -> Result<Vec<String>, ListenerError> {
    validate_distro(distro)?;
    let path = proc_path(pid, "cmdline")?;
    Ok(vec![
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "cat".to_owned(),
        path,
    ])
}

pub fn build_wsl_kill_argv(distro: &str, pid: u32) -> Result<Vec<String>, ListenerError> {
    validate_distro(distro)?;
    if pid == 0 {
        return Err(ListenerError::InvalidRequest);
    }
    Ok(vec![
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "kill".to_owned(),
        "-TERM".to_owned(),
        "--".to_owned(),
        pid.to_string(),
    ])
}

fn split_host_port(value: &str) -> Option<(String, &str)> {
    let value = value.trim();
    let (host, port) = if let Some(close) = value.rfind(']') {
        let (host, port) = value.split_at(close + 1);
        (
            host.trim_matches(['[', ']']).to_owned(),
            port.strip_prefix(':')?,
        )
    } else {
        let (host, port) = value.rsplit_once(':')?;
        (host.to_owned(), port)
    };
    let host = if host.is_empty() || host == "*" || host == "0.0.0.0" {
        "0.0.0.0".to_owned()
    } else if host == ":::" || host == "::" {
        "::".to_owned()
    } else {
        host
    };
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host
    };
    (host.len() <= MAX_NAME_BYTES
        && !host.is_empty()
        && !host
            .chars()
            .any(|character| character.is_control() || character.is_whitespace()))
    .then_some((host, port))
}

fn proc_path(pid: u32, file: &str) -> Result<String, ListenerError> {
    if pid == 0 || !matches!(file, "stat" | "cmdline") {
        return Err(ListenerError::InvalidRequest);
    }
    Ok(format!("/proc/{pid}/{file}"))
}

/// Parse field 22 from /proc/<pid>/stat. The command name can contain spaces
/// and parentheses, so the last closing parenthesis is the safe delimiter.
pub fn parse_proc_start_tick(input: &str) -> Option<u64> {
    if input.len() > MAX_SOURCE_OUTPUT_BYTES {
        return None;
    }
    let close = input.rfind(") ")?;
    let after_comm = input.get(close + 2..)?;
    after_comm
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
        .filter(|tick| *tick != 0)
}

/// Convert /proc/<pid>/cmdline bytes into a bounded display command.
pub fn parse_proc_cmdline(input: &[u8]) -> Option<String> {
    ensure_output_bound(input).ok()?;
    let text = String::from_utf8_lossy(input)
        .split('\0')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    sanitize_command_line(&text)
}

pub fn sanitize_process_name(value: &str) -> Option<String> {
    bounded_display(value, MAX_NAME_BYTES)
}

pub fn sanitize_executable_path(value: &str) -> Option<String> {
    bounded_display(value, MAX_EXECUTABLE_PATH_BYTES)
}

pub fn sanitize_command_line(value: &str) -> Option<String> {
    bounded_display(value, MAX_COMMAND_LINE_BYTES)
}

pub fn parse_pid_from_ss(value: &str) -> Option<u32> {
    let start = value.rfind("pid=")? + 4;
    let digits = value[start..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .take(11)
        .collect::<Vec<_>>();
    if digits.is_empty() || digits.len() > 10 {
        return None;
    }
    std::str::from_utf8(&digits)
        .ok()?
        .parse()
        .ok()
        .filter(|pid| *pid != 0)
}

fn parse_name_from_ss(value: &str) -> Option<String> {
    let start = value.find("((")? + 2;
    let end = value[start..].find(',')? + start;
    let name = value.get(start..end)?.trim_matches('"');
    sanitize_process_name(name)
}

fn validate_distro(value: &str) -> Result<(), ListenerError> {
    if value.len() > MAX_DISTRO_BYTES
        || value.trim().is_empty()
        || value.chars().any(char::is_control)
        || value.chars().any(|character| {
            matches!(
                character,
                ';' | '&'
                    | '|'
                    | '<'
                    | '>'
                    | '\x60'
                    | '$'
                    | '"'
                    | '\''
                    | '\\'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '*'
                    | '?'
                    | '['
                    | ']'
                    | '!'
                    | '~'
                    | '#'
                    | '%'
            )
        })
    {
        return Err(ListenerError::InvalidRequest);
    }
    Ok(())
}

fn validate_bounded_field(value: &str, max: usize) -> Result<(), ListenerError> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(ListenerError::InvalidRequest);
    }
    Ok(())
}

fn bounded_display(value: &str, max: usize) -> Option<String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    Some(redact_and_truncate(value, max))
}

fn redact_and_truncate(value: &str, max: usize) -> String {
    let redacted = redact_sensitive_tokens(value);
    if redacted.len() <= max {
        return redacted;
    }
    let mut output = String::with_capacity(max);
    for character in redacted.chars() {
        if output.len() + character.len_utf8() + 13 > max {
            break;
        }
        output.push(character);
    }
    output.push_str("… [truncated]");
    output
}

/// Conservative display-only redaction. It is not a parser for command
/// execution: ordinary arguments remain visible, while common credential
/// key/value forms are masked before the value reaches the UI.
fn redact_sensitive_tokens(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut redact_until_option = false;
    for token in value.split_inclusive(char::is_whitespace) {
        let trailing_space = token.chars().last().is_some_and(char::is_whitespace);
        let raw_body = token.trim_end_matches(char::is_whitespace);
        let redacted_query = redact_query_parameters(raw_body);
        let body = redacted_query.as_str();

        if redact_until_option && !raw_body.starts_with('-') {
            output.push_str("<redacted>");
            if trailing_space {
                output.push(' ');
            }
            continue;
        }
        if raw_body.starts_with('-') {
            redact_until_option = false;
        }

        if redacted_query != raw_body {
            output.push_str(body);
            if trailing_space {
                output.push(' ');
            }
            continue;
        }

        let lower = body.to_ascii_lowercase();
        let key_end = body
            .find('=')
            .or_else(|| body.find(':'))
            .unwrap_or(body.len());
        let key = lower[..key_end]
            .rsplit(['?', '&'])
            .next()
            .unwrap_or("")
            .trim_start_matches('-')
            .trim_matches(['"', '\'']);
        let has_sensitive_key = is_sensitive_key(key);
        let has_assignment = key_end < body.len();
        if has_sensitive_key && has_assignment {
            output.push_str(&body[..key_end + 1]);
            output.push_str("<redacted>");
            let value_fragment = &body[key_end + 1..];
            redact_until_option = matches!(key, "authorization" | "cookie")
                || value_fragment.is_empty()
                || value_fragment.starts_with(['"', '\'']);
        } else {
            if has_sensitive_key {
                redact_until_option = true;
            }
            output.push_str(body);
        }
        if trailing_space {
            output.push(' ');
        }
    }
    output
}

const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "token",
    "secret",
    "api_key",
    "apikey",
    "authorization",
    "cookie",
    "client_secret",
    "access_token",
    "refresh_token",
    "private_key",
];

fn is_sensitive_key(key: &str) -> bool {
    let key = key
        .trim_start_matches('-')
        .trim_matches(['"', '\''])
        .to_ascii_lowercase();
    let compact_key = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    SENSITIVE_KEYS.iter().any(|candidate| {
        let compact_candidate = candidate
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect::<String>();
        key == *candidate
            || key.ends_with(candidate)
            || key.split(['-', '_']).any(|part| part == *candidate)
            || compact_key.ends_with(&compact_candidate)
    })
}

fn redact_query_parameters(value: &str) -> String {
    let Some(query_start) = value.find('?') else {
        return value.to_owned();
    };
    let (prefix, query) = value.split_at(query_start + 1);
    let mut changed = false;
    let mut result = String::with_capacity(value.len());
    result.push_str(prefix);
    for segment in query.split_inclusive('&') {
        let separator = segment.find('&');
        let parameter = separator.map_or(segment, |index| &segment[..index]);
        if let Some(equal) = parameter.find('=') {
            if is_sensitive_key(&parameter[..equal]) {
                result.push_str(&parameter[..=equal]);
                result.push_str("<redacted>");
                changed = true;
                if let Some(index) = separator {
                    result.push_str(&segment[index..]);
                }
                continue;
            }
        }
        result.push_str(segment);
    }
    if changed {
        result
    } else {
        value.to_owned()
    }
}

fn ensure_output_bound(bytes: &[u8]) -> Result<(), ListenerError> {
    if bytes.len() > MAX_SOURCE_OUTPUT_BYTES {
        Err(ListenerError::CommandOutputTooLarge)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(state: &str) -> ListenerEndpoint {
        ListenerEndpoint {
            proto: "TCP".into(),
            local_addr: "127.0.0.1:3000".into(),
            port: 3000,
            state: state.into(),
        }
    }

    fn windows_request() -> KillListenerRequest {
        KillListenerRequest {
            endpoint: endpoint("LISTENING"),
            identity: ListenerIdentity::Windows {
                pid: 42,
                start_time: "100".into(),
            },
        }
    }

    fn windows_snapshot() -> ListenerSnapshot {
        ListenerSnapshot {
            endpoint: endpoint("LISTENING"),
            identity: ListenerIdentity::Windows {
                pid: 42,
                start_time: "100".into(),
            },
        }
    }

    #[test]
    fn windows_netstat_fixture_keeps_listener_endpoint_and_pid() {
        let rows = parse_windows_ports(
            "Proto  Local Address          Foreign Address        State           PID\n\
             TCP    127.0.0.1:3000         0.0.0.0:0              LISTENING       42\n",
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].local_addr, "127.0.0.1:3000");
        assert_eq!(rows[0].pid, Some(42));
    }

    #[test]
    fn wsl_ss_fixture_extracts_pid_and_process_name() {
        let rows = parse_wsl_ss_output(
            "tcp LISTEN 0 128 127.0.0.1:5173 0.0.0.0:* users:((\"node\",pid=123,fd=22))\n",
        )
        .unwrap();
        assert_eq!(rows[0].port.port, 5173);
        assert_eq!(rows[0].port.pid, Some(123));
        assert_eq!(rows[0].process_name.as_deref(), Some("node"));
    }

    #[test]
    fn wsl_udp_fixture_preserves_unconn_listener_state() {
        let rows = parse_wsl_ss_output(
            "udp UNCONN 0 0 0.0.0.0:5353 0.0.0.0:* users:((\"dns\",pid=9,fd=7))\n",
        )
        .unwrap();
        assert_eq!(rows[0].port.proto, "UDP");
        assert_eq!(rows[0].port.state, "UNCONN");
        assert_eq!(rows[0].port.pid, Some(9));
    }

    #[test]
    fn docker_fixture_extracts_ipv4_and_ipv6_published_ports() {
        let rows = parse_docker_ps_output(
            "aabbccdd\tapi\t0.0.0.0:8080->8080/tcp, :::8443->8443/tcp\n",
            "docker-desktop",
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].host_addr, "0.0.0.0");
        assert_eq!(rows[0].host_port, 8080);
        assert_eq!(rows[1].host_addr, "[::]");
        assert_eq!(rows[1].proto, "TCP");
    }

    #[test]
    fn docker_fixture_skips_unpublished_and_invalid_ports() {
        let rows = parse_docker_ps_output(
            "aabbccdd\tapi\t8080/tcp,0.0.0.0:not-a-port->80/tcp,0.0.0.0:80->80/sctp\n",
            "docker-desktop",
        )
        .unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn wsl_listener_and_kill_argv_are_fixed_and_numeric() {
        assert_eq!(
            build_wsl_listener_argv("Ubuntu").unwrap(),
            vec!["-d", "Ubuntu", "--", "ss", "-H", "-lntup"]
        );
        assert_eq!(
            build_wsl_kill_argv("Ubuntu", 42).unwrap(),
            vec!["-d", "Ubuntu", "--", "kill", "-TERM", "--", "42"]
        );
        assert_eq!(
            build_wsl_proc_stat_argv("Ubuntu", 42).unwrap(),
            vec!["-d", "Ubuntu", "--", "cat", "/proc/42/stat"]
        );
    }

    #[test]
    fn wsl_argv_builders_reject_injection_and_zero_pid() {
        assert_eq!(
            build_wsl_listener_argv("Ubuntu;cat /etc/passwd"),
            Err(ListenerError::InvalidRequest)
        );
        assert_eq!(
            build_wsl_kill_argv("Ubuntu", 0),
            Err(ListenerError::InvalidRequest)
        );
        assert_eq!(
            build_wsl_proc_cmdline_argv("Ubuntu", 0),
            Err(ListenerError::InvalidRequest)
        );
    }

    #[test]
    fn proc_stat_parser_uses_start_tick_after_parenthesized_command() {
        let fixture = "42 (worker (api)) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 987654 20";
        assert_eq!(parse_proc_start_tick(fixture), Some(987654));
        let zero_tick = "42 (worker) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 0 20";
        assert_eq!(parse_proc_start_tick(zero_tick), None);
        assert_eq!(parse_pid_from_ss("pid=10000000000,"), None);
    }

    #[test]
    fn proc_cmdline_is_nul_separated_and_bounded() {
        assert_eq!(
            parse_proc_cmdline(b"node\0server.js\0--port\x003000\0").as_deref(),
            Some("node server.js --port 3000")
        );
        assert!(parse_proc_cmdline(&vec![b'x'; MAX_SOURCE_OUTPUT_BYTES + 1]).is_none());
    }

    #[test]
    fn reused_pid_is_rejected_when_start_time_changes() {
        let mut observed = windows_snapshot();
        if let ListenerIdentity::Windows { start_time, .. } = &mut observed.identity {
            *start_time = "101".into();
        }
        assert_eq!(
            validate_kill_target(&windows_request(), &observed),
            Err(ListenerError::StaleTarget)
        );
    }

    #[test]
    fn changed_endpoint_is_rejected_even_with_same_identity() {
        let mut observed = windows_snapshot();
        observed.endpoint.port = 3001;
        assert_eq!(
            validate_kill_target(&windows_request(), &observed),
            Err(ListenerError::StaleTarget)
        );
    }

    #[test]
    fn established_connection_is_not_a_kill_target() {
        let mut request = windows_request();
        request.endpoint.state = "ESTABLISHED".into();
        assert_eq!(
            validate_kill_target(&request, &windows_snapshot()),
            Err(ListenerError::InvalidRequest)
        );
    }

    #[test]
    fn container_identity_returns_handoff_and_never_process_action() {
        let identity = ListenerIdentity::Container {
            engine: "docker".into(),
            container_id: "aabbccdd".into(),
            distro: "docker-desktop".into(),
        };
        let request = KillListenerRequest {
            endpoint: endpoint("LISTENING"),
            identity: identity.clone(),
        };
        let snapshot = ListenerSnapshot {
            endpoint: endpoint("LISTENING"),
            identity,
        };
        assert_eq!(
            validate_kill_target(&request, &snapshot),
            Ok(KillAction::ContainerHandoff)
        );
        assert_eq!(
            container_stop_handoff(&snapshot.identity)
                .unwrap()
                .target_app,
            "wsl-desktop"
        );
    }

    #[test]
    fn malformed_identity_and_distro_fail_closed() {
        let invalid_windows = ["", "0", "123abc", "18446744073709551616"];
        for start_time in invalid_windows {
            assert_eq!(
                (ListenerIdentity::Windows {
                    pid: 3,
                    start_time: start_time.into(),
                })
                .validate(),
                Err(ListenerError::InvalidRequest)
            );
        }
        let bad = ListenerIdentity::Wsl {
            distro: "Ubuntu;touch /tmp/pwn".into(),
            pid: 3,
            start_tick: 4,
        };
        assert_eq!(bad.validate(), Err(ListenerError::InvalidRequest));
        let bad_container = ListenerIdentity::Container {
            engine: "docker".into(),
            container_id: "id with spaces".into(),
            distro: "Ubuntu".into(),
        };
        assert_eq!(bad_container.validate(), Err(ListenerError::InvalidRequest));
    }

    #[test]
    fn display_redacts_common_credentials_without_redacting_normal_path() {
        let value = r#"node C:\work\server.js --token=s3cret --port 3000 --password hunter2"#;
        let display = sanitize_command_line(value).unwrap();
        assert!(display.contains(r#"C:\work\server.js"#));
        assert!(!display.contains("s3cret"));
        assert!(!display.contains("hunter2"));
        assert!(display.contains("<redacted>"));
    }

    #[test]
    fn display_redacts_sensitive_url_query_parameters_after_normal_parameters() {
        let display = sanitize_command_line(
            "curl http://localhost:3000/hook?name=demo&access_token=secret-value&mode=full",
        )
        .unwrap();
        assert!(display.contains("name=demo"));
        assert!(display.contains("mode=full"));
        assert!(!display.contains("secret-value"));
        assert!(display.contains("access_token=<redacted>"));
    }

    #[test]
    fn display_redacts_multi_token_authorization_and_quoted_secret_values() {
        let header = sanitize_command_line(
            r#"curl -H "Authorization: Bearer top-secret" --url http://localhost:3000"#,
        )
        .unwrap();
        assert!(!header.contains("Bearer"));
        assert!(!header.contains("top-secret"));
        assert!(header.contains("--url http://localhost:3000"));

        let password = sanitize_command_line(r#"tool --password "two words" --mode safe"#).unwrap();
        assert!(!password.contains("two words"));
        assert!(password.contains("--mode safe"));

        let api_key = sanitize_command_line(r#"curl -H "X-Api-Key: secret" --compressed"#).unwrap();
        assert!(!api_key.contains("secret"));
        assert!(api_key.contains("--compressed"));
    }

    #[test]
    fn proc_cmdline_rejects_display_control_characters() {
        assert!(parse_proc_cmdline(b"node\0safe.js\0").is_some());
        assert!(parse_proc_cmdline(b"node\0unsafe\nvalue\0").is_none());
    }

    #[test]
    fn fixed_error_does_not_echo_untrusted_values() {
        let message = ListenerError::InvalidRequest.to_string();
        assert!(!message.contains("Ubuntu"));
        assert!(!message.contains("secret"));
        assert!(!message.contains("\\"));
    }

    #[test]
    fn identity_wire_shape_keeps_start_time_and_never_contains_display_path() {
        let request = windows_request();
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(encoded.contains("\"start_time\":\"100\""));
        assert!(encoded.contains("\"local_addr\":\"127.0.0.1:3000\""));
        assert!(!encoded.contains("path"));
        assert!(!encoded.contains("command"));
    }

    #[test]
    fn kill_request_rejects_unknown_control_fields() {
        let injected = serde_json::json!({
            "endpoint": {
                "proto": "TCP",
                "local_addr": "127.0.0.1:3000",
                "port": 3000,
                "state": "LISTENING"
            },
            "identity": { "kind": "windows", "pid": 1234, "start_time": "100" },
            "executable_path": "C:\\sensitive\\different.exe"
        });
        assert!(serde_json::from_value::<KillListenerRequest>(injected).is_err());
    }

    #[test]
    fn oversized_source_output_is_rejected_before_parsing() {
        let input = "x".repeat(MAX_SOURCE_OUTPUT_BYTES + 1);
        assert_eq!(
            parse_windows_ports(&input),
            Err(ListenerError::CommandOutputTooLarge)
        );
        assert_eq!(
            parse_wsl_ss_output(&input),
            Err(ListenerError::CommandOutputTooLarge)
        );
        assert_eq!(
            parse_docker_ps_output(&input, "docker-desktop"),
            Err(ListenerError::CommandOutputTooLarge)
        );
        assert!(parse_proc_start_tick(&input).is_none());
    }
}
