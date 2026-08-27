#[cfg(target_os = "windows")]
use crate::core::listeners::{
    build_wsl_kill_argv, build_wsl_listener_argv, build_wsl_proc_cmdline_argv,
    build_wsl_proc_stat_argv, parse_docker_ps_output, parse_proc_cmdline, parse_proc_start_tick,
    parse_windows_ports, parse_wsl_ss_output, MAX_DETAIL_LOOKUPS, MAX_DISTRO_BYTES,
    MAX_LISTENER_ROWS, MAX_SOURCE_OUTPUT_BYTES, MAX_WSL_DISTROS,
};
use crate::core::listeners::{
    container_stop_handoff, sanitize_command_line, sanitize_executable_path, sanitize_process_name,
    validate_kill_target, ContainerStopHandoff, KillAction, KillListenerRequest, ListenerEndpoint,
    ListenerError, ListenerIdentity, ListenerSnapshot, ListenerSource,
};
use serde::Serialize;
#[cfg(target_os = "windows")]
use std::collections::{HashMap, HashSet};
#[cfg(target_os = "windows")]
use std::io::Read;
use sysinfo::{Pid, System};
use tauri_plugin_opener::OpenerExt;

#[cfg(target_os = "windows")]
type WslProcessDetails = Option<(u64, Option<String>)>;
#[cfg(target_os = "windows")]
type WslProcessDetailCache = HashMap<(String, u32), WslProcessDetails>;

/// Process details retained for the detail panel and identity-safe actions.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    /// Kept for v0.4.x frontend compatibility.
    pub exe: Option<String>,
    /// Legacy v0.4.x seconds-since-Unix-epoch field. It is intentionally kept
    /// separate from the exact native identity below.
    pub start_time: u64,
    pub memory_bytes: u64,
    pub command_line: Option<String>,
    pub executable_path: Option<String>,
    /// Exact native start identity as a decimal string. Windows FILETIME is
    /// larger than JavaScript's safe integer range, so the frontend must not
    /// receive this value as a JSON number.
    pub process_start_time: Option<String>,
}

/// A port row contains display metadata and an opaque identity precondition.
/// The executable path/command line are display-only values; they are never
/// accepted as process-control input.
#[derive(Debug, Clone, Serialize)]
pub struct PortRow {
    #[serde(flatten)]
    pub port: devbox_process::PortInfo,
    pub process_name: Option<String>,
    pub source: ListenerSource,
    pub command_line: Option<String>,
    pub executable_path: Option<String>,
    /// Exact native start identity as a decimal string; display-only rows may
    /// omit it when the platform cannot provide a strong identity.
    pub process_start_time: Option<String>,
    pub wsl_distro: Option<String>,
    pub wsl_start_tick: Option<u64>,
    pub container_engine: Option<String>,
    pub container_id: Option<String>,
    pub container_name: Option<String>,
    pub identity: Option<ListenerIdentity>,
}

impl PortRow {
    fn endpoint(&self) -> ListenerEndpoint {
        ListenerEndpoint::from_port(&self.port)
    }

    fn snapshot(&self) -> Option<ListenerSnapshot> {
        Some(ListenerSnapshot {
            endpoint: self.endpoint(),
            identity: self.identity.clone()?,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ListenerActionResult {
    Terminated,
    Handoff { handoff: ContainerStopHandoff },
}

/// List native, WSL, and published-container listeners. All platform work is
/// kept off the Tauri command thread and every child output is bounded.
#[tauri::command]
pub async fn list_ports() -> Result<Vec<PortRow>, String> {
    tauri::async_runtime::spawn_blocking(collect_ports)
        .await
        .map_err(|_| ListenerError::SourceUnavailable.to_string())?
        .map_err(|error| error.to_string())
}

/// Re-query the endpoint and identity immediately before a process action.
/// A container row returns a validated handoff descriptor and never invokes a
/// process termination API.
#[tauri::command]
pub async fn kill_listener(request: KillListenerRequest) -> Result<ListenerActionResult, String> {
    tauri::async_runtime::spawn_blocking(move || kill_listener_sync(request))
        .await
        .map_err(|_| ListenerError::SourceUnavailable.to_string())?
        .map_err(|error| error.to_string())
}

fn kill_listener_sync(request: KillListenerRequest) -> Result<ListenerActionResult, ListenerError> {
    request.endpoint.validate_listener()?;
    request.identity.validate()?;

    let rows = collect_ports()?;
    let observed = rows
        .iter()
        .filter_map(PortRow::snapshot)
        .find(|snapshot| {
            snapshot.endpoint == request.endpoint && snapshot.identity == request.identity
        })
        .ok_or(ListenerError::StaleTarget)?;
    let action = validate_kill_target(&request, &observed)?;

    match action {
        KillAction::WindowsProcess => terminate_windows_process(&request.identity)?,
        KillAction::WslProcess => terminate_wsl_process(&request.identity)?,
        KillAction::ContainerHandoff => {
            return Ok(ListenerActionResult::Handoff {
                handoff: container_stop_handoff(&request.identity)?,
            });
        }
    }
    Ok(ListenerActionResult::Terminated)
}

/// Return the validated handoff descriptor for a container row. The current
/// container identity is re-read through list_ports first, so a stale
/// selection cannot be handed off silently.
#[tauri::command]
pub async fn handoff_container_stop(
    request: KillListenerRequest,
) -> Result<ContainerStopHandoff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        request.endpoint.validate_listener()?;
        request.identity.validate()?;
        let rows = collect_ports()?;
        let observed = rows
            .iter()
            .filter_map(PortRow::snapshot)
            .find(|snapshot| {
                snapshot.endpoint == request.endpoint && snapshot.identity == request.identity
            })
            .ok_or(ListenerError::StaleTarget)?;
        if validate_kill_target(
            &KillListenerRequest {
                endpoint: request.endpoint,
                identity: request.identity.clone(),
            },
            &observed,
        )? != KillAction::ContainerHandoff
        {
            return Err(ListenerError::UnsupportedSource);
        }
        container_stop_handoff(&request.identity)
    })
    .await
    .map_err(|_| ListenerError::SourceUnavailable.to_string())?
    .map_err(|error| error.to_string())
}

/// PID-only process detail lookup remains read-only. Process control uses
/// kill_listener and therefore cannot be reached with a bare PID.
#[tauri::command]
pub fn get_process_info(pid: u32) -> Result<ProcessInfo, String> {
    if pid == 0 {
        return Err(ListenerError::InvalidRequest.to_string());
    }
    let system = System::new_all();
    let process = system
        .process(Pid::from_u32(pid))
        .ok_or(ListenerError::ProcessUnavailable)
        .map_err(|error| error.to_string())?;
    let name = sanitize_process_name(&process.name().to_string_lossy())
        .unwrap_or_else(|| "unknown process".to_owned());
    let command_line = process_command_line(process);
    let fallback_exe = process
        .exe()
        .and_then(|path| sanitize_executable_path(&path.to_string_lossy()));
    let (executable_path, native_start_time) = native_process_metadata(pid);
    let executable_path = executable_path.or(fallback_exe);
    let legacy_start_time = process.start_time();
    Ok(ProcessInfo {
        pid,
        name,
        exe: executable_path.clone(),
        start_time: legacy_start_time,
        memory_bytes: process.memory(),
        command_line,
        executable_path,
        process_start_time: native_start_time.map(|value| value.to_string()),
    })
}

/// PID is resolved again by the backend; the frontend cannot provide an
/// arbitrary path to the opener. Errors intentionally use fixed text.
#[tauri::command]
pub async fn reveal_process(app: tauri::AppHandle, pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err(ListenerError::InvalidRequest.to_string());
    }
    let system = System::new_all();
    let process = system
        .process(Pid::from_u32(pid))
        .ok_or(ListenerError::ProcessUnavailable)
        .map_err(|error| error.to_string())?;
    let executable = process
        .exe()
        .ok_or(ListenerError::ProcessUnavailable)
        .map_err(|error| error.to_string())?;

    app.opener()
        .reveal_item_in_dir(executable)
        .map_err(|_| ListenerError::ProcessAccessDenied.to_string())
}

/// Open only a URL produced from a validated listener row. The command keeps
/// the existing API for browser actions but does not echo the URL on failure.
#[tauri::command]
pub async fn open_browser(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if !is_safe_browser_url(&url) {
        return Err(ListenerError::InvalidRequest.to_string());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| ListenerError::ProcessAccessDenied.to_string())
}

fn process_command_line(process: &sysinfo::Process) -> Option<String> {
    let value = process
        .cmd()
        .iter()
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    sanitize_command_line(&value)
}

fn is_safe_browser_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if url.len() > 512 || url.chars().any(char::is_control) {
        return false;
    }
    let Some(rest) = lower
        .strip_prefix("http://localhost:")
        .or_else(|| lower.strip_prefix("http://127.0.0.1:"))
        .or_else(|| lower.strip_prefix("http://[::1]:"))
    else {
        return false;
    };
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 5 {
        return false;
    }
    let Ok(port) = rest[..digits].parse::<u16>() else {
        return false;
    };
    port > 0
        && rest[digits..]
            .chars()
            .next()
            .is_none_or(|character| matches!(character, '/' | '?' | '#'))
}

#[cfg(target_os = "windows")]
fn collect_ports() -> Result<Vec<PortRow>, ListenerError> {
    let deadline = command_deadline();
    let native = collect_windows_ports(deadline)?;
    let mut rows = native;
    rows.truncate(MAX_LISTENER_ROWS);
    let distros = running_wsl_distros(deadline).unwrap_or_default();
    let remaining = MAX_LISTENER_ROWS.saturating_sub(rows.len());
    rows.extend(collect_wsl_ports(&distros, remaining, deadline));
    let remaining = MAX_LISTENER_ROWS.saturating_sub(rows.len());
    rows.extend(collect_container_ports(&distros, remaining, deadline));
    sort_rows(&mut rows);
    rows.truncate(MAX_LISTENER_ROWS);
    Ok(rows)
}

#[cfg(not(target_os = "windows"))]
fn collect_ports() -> Result<Vec<PortRow>, ListenerError> {
    Ok(Vec::new())
}

#[cfg(target_os = "windows")]
fn collect_windows_ports(deadline: std::time::Instant) -> Result<Vec<PortRow>, ListenerError> {
    let output = run_fixed_command("netstat", &["-ano"], deadline)?;
    let text = String::from_utf8_lossy(&output);
    let ports = parse_windows_ports(&text)?;
    let system = System::new_all();
    Ok(ports
        .into_iter()
        .map(|port| {
            let details = port
                .pid
                .and_then(|pid| native_process_details(&system, pid));
            let process_start_time = details.as_ref().and_then(|details| details.start_time);
            let identity = port.pid.and_then(|pid| {
                process_start_time.map(|start_time| ListenerIdentity::Windows {
                    pid,
                    start_time: start_time.to_string(),
                })
            });
            PortRow {
                port,
                process_name: details.as_ref().and_then(|details| details.name.clone()),
                source: ListenerSource::Windows,
                command_line: details
                    .as_ref()
                    .and_then(|details| details.command_line.clone()),
                executable_path: details
                    .as_ref()
                    .and_then(|details| details.executable_path.clone()),
                process_start_time: process_start_time.map(|value| value.to_string()),
                wsl_distro: None,
                wsl_start_tick: None,
                container_engine: None,
                container_id: None,
                container_name: None,
                identity,
            }
        })
        .collect())
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct NativeProcessDetails {
    name: Option<String>,
    command_line: Option<String>,
    executable_path: Option<String>,
    start_time: Option<u64>,
}

#[cfg(target_os = "windows")]
fn native_process_details(system: &System, pid: u32) -> Option<NativeProcessDetails> {
    let process = system.process(Pid::from_u32(pid))?;
    let native = query_windows_process(pid);
    let fallback_exe = process
        .exe()
        .and_then(|path| sanitize_executable_path(&path.to_string_lossy()));
    Some(NativeProcessDetails {
        name: sanitize_process_name(&process.name().to_string_lossy()),
        command_line: process_command_line(process),
        executable_path: native
            .as_ref()
            .and_then(|details| details.executable_path.clone())
            .or(fallback_exe),
        // A sysinfo seconds-resolution fallback is display-only. It is not
        // strong enough to become a kill identity, so rows without the native
        // FILETIME stay non-actionable.
        start_time: native.as_ref().map(|details| details.start_time),
    })
}

#[cfg(target_os = "windows")]
fn collect_wsl_ports(
    distros: &[String],
    max_rows: usize,
    deadline: std::time::Instant,
) -> Vec<PortRow> {
    let mut rows = Vec::new();
    if max_rows == 0 {
        return rows;
    }
    let mut detail_cache = WslProcessDetailCache::new();
    for distro in distros.iter().take(MAX_WSL_DISTROS) {
        let Ok(listener_args) = build_wsl_listener_argv(distro) else {
            continue;
        };
        let listener_args = listener_args.iter().map(String::as_str).collect::<Vec<_>>();
        let Ok(output) = run_fixed_command("wsl.exe", &listener_args, deadline) else {
            continue;
        };
        let text = devbox_wsl::output::decode_output(&output);
        let Ok(ports) = parse_wsl_ss_output(&text) else {
            continue;
        };
        for parsed in ports.into_iter().take(max_rows.saturating_sub(rows.len())) {
            let mut command_line = None;
            let mut start_tick = None;
            if let Some(pid) = parsed.port.pid {
                let key = (distro.clone(), pid);
                let details = if let Some(cached) = detail_cache.get(&key) {
                    cached.clone()
                } else if detail_cache.len() < MAX_DETAIL_LOOKUPS {
                    let queried = query_wsl_process(distro, pid, deadline);
                    detail_cache.insert(key, queried.clone());
                    queried
                } else {
                    None
                };
                if let Some((tick, command)) = details {
                    start_tick = Some(tick);
                    command_line = command;
                }
            }
            let identity =
                parsed
                    .port
                    .pid
                    .zip(start_tick)
                    .map(|(pid, tick)| ListenerIdentity::Wsl {
                        distro: distro.clone(),
                        pid,
                        start_tick: tick,
                    });
            let process_name = parsed
                .process_name
                .or_else(|| command_line.as_deref().and_then(first_command_part));
            rows.push(PortRow {
                port: parsed.port,
                process_name,
                source: ListenerSource::Wsl,
                command_line,
                executable_path: None,
                process_start_time: None,
                wsl_distro: Some(distro.clone()),
                wsl_start_tick: start_tick,
                container_engine: None,
                container_id: None,
                container_name: None,
                identity,
            });
        }
    }
    rows
}

#[cfg(target_os = "windows")]
fn collect_container_ports(
    distros: &[String],
    max_rows: usize,
    deadline: std::time::Instant,
) -> Vec<PortRow> {
    let mut rows = Vec::new();
    if max_rows == 0 {
        return rows;
    }
    for distro in distros.iter().take(MAX_WSL_DISTROS) {
        let Ok(output) = run_wsl_docker_ps(distro, deadline) else {
            continue;
        };
        let text = devbox_wsl::output::decode_output(&output);
        let Ok(ports) = parse_docker_ps_output(&text, distro) else {
            continue;
        };
        for container in ports.into_iter().take(max_rows.saturating_sub(rows.len())) {
            let identity = ListenerIdentity::Container {
                engine: "docker".to_owned(),
                container_id: container.container_id.clone(),
                distro: container.distro.clone(),
            };
            rows.push(PortRow {
                port: devbox_process::PortInfo {
                    proto: container.proto,
                    local_addr: format!("{}:{}", container.host_addr, container.host_port),
                    port: container.host_port,
                    state: "LISTENING".to_owned(),
                    pid: None,
                },
                process_name: Some(container.container_name.clone()),
                source: ListenerSource::Container,
                command_line: None,
                executable_path: None,
                process_start_time: None,
                wsl_distro: Some(container.distro),
                wsl_start_tick: None,
                container_engine: Some("docker".to_owned()),
                container_id: Some(container.container_id),
                container_name: Some(container.container_name),
                identity: Some(identity),
            });
            if rows.len() >= max_rows {
                return rows;
            }
        }
    }
    rows
}

#[cfg(target_os = "windows")]
fn running_wsl_distros(deadline: std::time::Instant) -> Result<Vec<String>, ListenerError> {
    let output = run_fixed_command("wsl.exe", &["--list", "--running", "--quiet"], deadline)?;
    let text = devbox_wsl::output::decode_output(&output);
    let mut seen = HashSet::new();
    let mut distros = Vec::new();
    for line in text.lines() {
        let name = line.trim().trim_start_matches('*').trim();
        if name.is_empty() || name.len() > MAX_DISTRO_BYTES {
            continue;
        }
        let identity = ListenerIdentity::Wsl {
            distro: name.to_owned(),
            pid: 1,
            start_tick: 1,
        };
        if identity.validate().is_ok() && seen.insert(name.to_owned()) {
            distros.push(name.to_owned());
        }
    }
    Ok(distros)
}

#[cfg(target_os = "windows")]
fn query_wsl_process(
    distro: &str,
    pid: u32,
    deadline: std::time::Instant,
) -> Option<(u64, Option<String>)> {
    if pid == 0 {
        return None;
    }
    let stat_args = build_wsl_proc_stat_argv(distro, pid).ok()?;
    let stat_args = stat_args.iter().map(String::as_str).collect::<Vec<_>>();
    let stat = run_fixed_command("wsl.exe", &stat_args, deadline).ok()?;
    let stat_text = devbox_wsl::output::decode_output(&stat);
    let start_tick = parse_proc_start_tick(&stat_text)?;
    let cmdline_args = build_wsl_proc_cmdline_argv(distro, pid).ok()?;
    let cmdline_args = cmdline_args.iter().map(String::as_str).collect::<Vec<_>>();
    let cmdline = run_fixed_command("wsl.exe", &cmdline_args, deadline)
        .ok()
        .and_then(|bytes| parse_proc_cmdline(&bytes));
    Some((start_tick, cmdline))
}

#[cfg(target_os = "windows")]
fn first_command_part(value: &str) -> Option<String> {
    value
        .split_whitespace()
        .next()
        .and_then(sanitize_process_name)
}

#[cfg(target_os = "windows")]
fn terminate_windows_process(identity: &ListenerIdentity) -> Result<(), ListenerError> {
    let ListenerIdentity::Windows { pid, start_time } = identity else {
        return Err(ListenerError::UnsupportedSource);
    };
    let expected_start_time = start_time
        .parse::<u64>()
        .map_err(|_| ListenerError::InvalidRequest)?;
    terminate_windows_process_by_identity(*pid, expected_start_time)
}

#[cfg(not(target_os = "windows"))]
fn terminate_windows_process(_identity: &ListenerIdentity) -> Result<(), ListenerError> {
    Err(ListenerError::SourceUnavailable)
}

#[cfg(target_os = "windows")]
fn terminate_windows_process_by_identity(
    pid: u32,
    expected_start_time: u64,
) -> Result<(), ListenerError> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
            false,
            pid,
        )
    }
    .map_err(|_| ListenerError::ProcessUnavailable)?;

    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result =
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    if result.is_err() {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(ListenerError::ProcessUnavailable);
    }
    let observed_start_time =
        (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    if observed_start_time != expected_start_time {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Err(ListenerError::StaleTarget);
    }
    let terminated = unsafe { TerminateProcess(handle, 1).is_ok() };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if terminated {
        Ok(())
    } else {
        Err(ListenerError::ProcessAccessDenied)
    }
}

#[cfg(target_os = "windows")]
fn terminate_wsl_process(identity: &ListenerIdentity) -> Result<(), ListenerError> {
    let ListenerIdentity::Wsl {
        distro,
        pid,
        start_tick,
    } = identity
    else {
        return Err(ListenerError::UnsupportedSource);
    };
    let deadline = command_deadline();
    let Some((observed_tick, _)) = query_wsl_process(distro, *pid, deadline) else {
        return Err(ListenerError::ProcessUnavailable);
    };
    if observed_tick != *start_tick {
        return Err(ListenerError::StaleTarget);
    }
    let kill_args = build_wsl_kill_argv(distro, *pid)?;
    let kill_args = kill_args.iter().map(String::as_str).collect::<Vec<_>>();
    run_fixed_command("wsl.exe", &kill_args, deadline).map(|_| ())
}

#[cfg(not(target_os = "windows"))]
fn terminate_wsl_process(_identity: &ListenerIdentity) -> Result<(), ListenerError> {
    Err(ListenerError::SourceUnavailable)
}

#[cfg(target_os = "windows")]
fn run_wsl_docker_ps(distro: &str, deadline: std::time::Instant) -> Result<Vec<u8>, ListenerError> {
    let identity = ListenerIdentity::Wsl {
        distro: distro.to_owned(),
        pid: 1,
        start_tick: 1,
    };
    identity.validate()?;
    let args = [
        "-d".to_owned(),
        distro.to_owned(),
        "--".to_owned(),
        "docker".to_owned(),
        "ps".to_owned(),
        "--format".to_owned(),
        "{{.ID}}\t{{.Names}}\t{{.Ports}}".to_owned(),
    ];
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    run_fixed_command("wsl.exe", &args, deadline)
}

#[cfg(target_os = "windows")]
fn command_deadline() -> std::time::Instant {
    std::time::Instant::now() + std::time::Duration::from_secs(15)
}

/// Own every descendant of a fixed discovery command. Auto-refresh invokes
/// `wsl.exe`, Docker, and Podman repeatedly; killing only the root on timeout
/// can otherwise leave a descendant holding the stdout pipe and accumulating
/// across polls.
#[cfg(target_os = "windows")]
struct FixedCommandJob {
    handle: windows::Win32::Foundation::HANDLE,
}

#[cfg(target_os = "windows")]
impl FixedCommandJob {
    fn assign_to(child: &std::process::Child) -> Result<Self, ListenerError> {
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|_| ListenerError::SourceUnavailable)?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .is_err()
        {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(ListenerError::SourceUnavailable);
        }
        if unsafe { AssignProcessToJobObject(handle, HANDLE(child.as_raw_handle())) }.is_err() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(ListenerError::SourceUnavailable);
        }
        Ok(Self { handle })
    }

    fn terminate(&self, child: &mut std::process::Child) {
        use windows::Win32::System::JobObjects::TerminateJobObject;
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
        let _ = child.wait();
    }
}

#[cfg(target_os = "windows")]
impl Drop for FixedCommandJob {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        unsafe {
            // KILL_ON_JOB_CLOSE guarantees that a successful root command
            // cannot leave a helper holding the bounded stdout pipe either.
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(target_os = "windows")]
fn run_fixed_command(
    program: &str,
    args: &[&str],
    deadline: std::time::Instant,
) -> Result<Vec<u8>, ListenerError> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{self, TryRecvError};
    use std::thread;
    use std::time::Duration;

    if std::time::Instant::now() >= deadline {
        return Err(ListenerError::CommandTimedOut);
    }

    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|_| ListenerError::SourceUnavailable)?;
    let job = match FixedCommandJob::assign_to(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let Some(mut stdout) = child.stdout.take() else {
        job.terminate(&mut child);
        return Err(ListenerError::SourceUnavailable);
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let _reader = thread::spawn(move || {
        let mut output = Vec::with_capacity(MAX_SOURCE_OUTPUT_BYTES.min(64 * 1024));
        let result = stdout
            .by_ref()
            .take((MAX_SOURCE_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut output)
            .map_err(|_| ListenerError::SourceUnavailable)
            .and_then(|read| {
                if read > MAX_SOURCE_OUTPUT_BYTES {
                    Err(ListenerError::CommandOutputTooLarge)
                } else {
                    Ok(output)
                }
            });
        let _ = sender.send(result);
    });

    let mut output = None;
    loop {
        if output.is_none() {
            match receiver.try_recv() {
                Ok(Ok(bytes)) => output = Some(bytes),
                Ok(Err(error)) => {
                    job.terminate(&mut child);
                    return Err(error);
                }
                Err(TryRecvError::Disconnected) => {
                    job.terminate(&mut child);
                    return Err(ListenerError::SourceUnavailable);
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        match child.try_wait() {
            Ok(Some(status)) if !status.success() => return Err(ListenerError::SourceUnavailable),
            Ok(Some(_)) => {
                if let Some(bytes) = output {
                    return Ok(bytes);
                }
            }
            Ok(None) => {}
            Err(_) => {
                job.terminate(&mut child);
                return Err(ListenerError::SourceUnavailable);
            }
        }

        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            job.terminate(&mut child);
            return Err(ListenerError::CommandTimedOut);
        }
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

#[cfg(target_os = "windows")]
fn query_windows_process(pid: u32) -> Option<WindowsProcessMetadata> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let start_result =
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    let result = start_result.ok().map(|_| {
        let start_time =
            (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        let mut buffer = vec![0u16; 32 * 1024];
        let mut size = buffer.len() as u32;
        let path = unsafe {
            QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            )
            .ok()
            .and_then(|_| {
                let length = usize::try_from(size).ok()?;
                String::from_utf16(&buffer[..length]).ok()
            })
        }
        .and_then(|path| sanitize_executable_path(&path));
        WindowsProcessMetadata {
            start_time,
            executable_path: path,
        }
    });
    unsafe {
        let _ = CloseHandle(handle);
    }
    result
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
struct WindowsProcessMetadata {
    start_time: u64,
    executable_path: Option<String>,
}

#[cfg(target_os = "windows")]
fn native_process_metadata(pid: u32) -> (Option<String>, Option<u64>) {
    let metadata = query_windows_process(pid);
    (
        metadata
            .as_ref()
            .and_then(|details| details.executable_path.clone()),
        metadata.map(|details| details.start_time),
    )
}

#[cfg(not(target_os = "windows"))]
fn native_process_metadata(pid: u32) -> (Option<String>, Option<u64>) {
    let _ = pid;
    (None, None)
}

#[cfg(target_os = "windows")]
fn sort_rows(rows: &mut [PortRow]) {
    rows.sort_by(|left, right| {
        source_rank(left.source)
            .cmp(&source_rank(right.source))
            .then(left.port.proto.cmp(&right.port.proto))
            .then(left.port.local_addr.cmp(&right.port.local_addr))
            .then(left.port.port.cmp(&right.port.port))
            .then(left.port.pid.cmp(&right.port.pid))
            .then(identity_sort_key(left).cmp(&identity_sort_key(right)))
    });
}

#[cfg(target_os = "windows")]
fn source_rank(source: ListenerSource) -> u8 {
    match source {
        ListenerSource::Windows => 0,
        ListenerSource::Wsl => 1,
        ListenerSource::Container => 2,
    }
}

#[cfg(target_os = "windows")]
fn identity_sort_key(row: &PortRow) -> String {
    match &row.identity {
        Some(ListenerIdentity::Windows { pid, start_time }) => {
            format!("windows:{pid}:{start_time}")
        }
        Some(ListenerIdentity::Wsl {
            distro,
            pid,
            start_tick,
        }) => format!("wsl:{distro}:{pid}:{start_tick}"),
        Some(ListenerIdentity::Container {
            engine,
            container_id,
            distro,
        }) => format!("container:{engine}:{distro}:{container_id}"),
        None => String::new(),
    }
}
