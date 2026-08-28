//! Native, bounded integration with the reviewed Related Tools list.
//!
//! The public command surface accepts only a curated opaque id.  Detection
//! and launch never return a resolved path, and WinGet output is discarded so
//! local paths, account names, and package-manager diagnostics cannot become
//! Manager UI data.

#[cfg(any(windows, test))]
use crate::core::related_tools::classify_detection;
use crate::core::related_tools::{
    curated_tools, find_tool, is_valid_tool_id, DetectionSource, RelatedToolSpec,
};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::process::{Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::ffi::{OsStr, OsString};
#[cfg(windows)]
use std::mem::size_of;
#[cfg(windows)]
use std::os::windows::ffi::{OsStrExt, OsStringExt};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::path::{Component, Path, PathBuf};
#[cfg(windows)]
use windows::core::{PCWSTR, PWSTR};
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(windows)]
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
#[cfg(windows)]
use windows::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, ResumeThread, TerminateProcess, WaitForSingleObject,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
    STARTUPINFOW,
};

#[cfg(windows)]
const WINGET_TIMEOUT_MS: u64 = 120_000;
#[cfg(windows)]
const MAX_PATH_ENV_BYTES: usize = 32 * 1024;
#[cfg(windows)]
const MAX_ENVIRONMENT_BLOCK_UNITS: usize = 64 * 1024;
#[cfg(windows)]
const MAX_PATH_ENTRIES: usize = 128;
#[cfg(windows)]
const MAX_PATH_ENTRY_BYTES: usize = 4 * 1024;
#[cfg(windows)]
const MAX_PATH_COMPONENTS: usize = 128;
#[cfg(windows)]
const MAX_KNOWN_PATHS: usize = 32;

/// All Related Tools actions share one native single-flight boundary.  This
/// prevents an install, launch, and detection refresh from observing or
/// changing the same executable set at the same time.
static RELATED_ACTION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelatedToolView {
    pub id: String,
    pub display_name: String,
    pub summary: String,
    pub winget_id: String,
    pub official_url: String,
    pub license_url: String,
    pub license: String,
    pub platform_supported: bool,
    pub installed: bool,
    pub detection: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelatedToolInstallRequest {
    pub tool_id: String,
    /// The confirmation is collected by the UI immediately before this
    /// command.  Requiring it at the command boundary prevents a future UI
    /// caller from accidentally turning this into an automatic installer.
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelatedToolActionView {
    pub tool_id: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
enum InstallOutcome {
    Installed,
    UnsupportedPlatform,
    WinGetUnavailable,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
enum LaunchOutcome {
    Launched,
    UnsupportedPlatform,
    NotInstalled,
}

/// Own the complete WinGet process tree on Windows.  WinGet can hand work to
/// an installer or helper process; killing only the root process on timeout
/// would leave an unbounded external mutation running after Manager reports
/// failure.  The job's kill-on-close limit is also the crash/drop fallback.
#[cfg(windows)]
struct ProcessTree {
    handle: HANDLE,
}

#[cfg(windows)]
impl ProcessTree {
    fn new() -> Result<Self, ()> {
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|_| ())?;
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
            return Err(());
        }

        Ok(Self { handle })
    }

    fn assign(&self, process: HANDLE) -> Result<(), ()> {
        unsafe { AssignProcessToJobObject(self.handle, process) }.map_err(|_| ())
    }

    fn terminate(&self) {
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
    }

    fn active_processes(&self) -> Option<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(self.handle),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
        }
        .ok()?;
        Some(accounting.ActiveProcesses)
    }

    fn wait_until_empty(&self, deadline: Instant) -> bool {
        loop {
            match self.active_processes() {
                Some(0) => return true,
                Some(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Some(_) | None => return false,
            }
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// WinGet is created suspended, assigned to the kill-on-close Job Object, and
/// only then resumed. This closes the spawn/assignment gap where an installer
/// helper could otherwise escape before the root process entered the job.
#[cfg(windows)]
struct WingetProcess {
    process: HANDLE,
    tree: ProcessTree,
}

#[cfg(windows)]
impl WingetProcess {
    fn wait(&self, timeout: Duration) -> Option<bool> {
        let deadline = Instant::now().checked_add(timeout)?;
        loop {
            let wait = unsafe { WaitForSingleObject(self.process, 0) };
            if wait == WAIT_OBJECT_0 {
                let mut exit_code = 1_u32;
                if unsafe { GetExitCodeProcess(self.process, &mut exit_code) }.is_err() {
                    self.terminate_and_reap();
                    return None;
                }
                if self.tree.wait_until_empty(deadline) {
                    return Some(exit_code == 0);
                }
                self.terminate_and_reap();
                return None;
            }
            if wait != WAIT_TIMEOUT || Instant::now() >= deadline {
                self.terminate_and_reap();
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn terminate_and_reap(&self) {
        self.tree.terminate();
        let cleanup_deadline = Instant::now() + Duration::from_secs(1);
        let _ = self.tree.wait_until_empty(cleanup_deadline);
        let _ = unsafe { WaitForSingleObject(self.process, 0) };
    }
}

#[cfg(windows)]
impl Drop for WingetProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.process);
        }
        // `tree` drops afterwards; KILL_ON_JOB_CLOSE is the final fallback if
        // the bounded cleanup above could not observe an empty job.
    }
}

fn acquire_related_action() -> Result<MutexGuard<'static, ()>, String> {
    RELATED_ACTION_LOCK
        .get_or_init(|| Mutex::new(()))
        .try_lock()
        .map_err(|_| "다른 관련 도구 작업이 진행 중입니다. 잠시 후 다시 시도하세요.".to_string())
}

/// Return the bounded status of every reviewed tool.  The blocking probes run
/// off the Tauri command thread and expose only stable metadata plus a coarse
/// detection source.
#[tauri::command]
pub async fn related_tools() -> Result<Vec<RelatedToolView>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let _guard = acquire_related_action()?;
        Ok::<Vec<RelatedToolView>, String>(detect_related_tools())
    })
    .await
    .map_err(|_| "관련 도구 감지를 완료할 수 없습니다.".to_string())?
}

/// Start one exact WinGet install only after an explicit user confirmation.
/// No arbitrary package search, installer URL, or shell command is accepted.
#[tauri::command]
pub async fn install_related_tool(
    request: RelatedToolInstallRequest,
) -> Result<RelatedToolActionView, String> {
    let spec = validated_tool(&request.tool_id)?;
    if !request.confirmed {
        return Err("관련 도구 설치는 사용자 확인이 필요합니다.".to_string());
    }

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let _guard = acquire_related_action()?;
        Ok::<InstallOutcome, String>(run_winget_install(spec))
    })
    .await
    .map_err(|_| "관련 도구 설치를 시작할 수 없습니다.".to_string())??;

    match outcome {
        InstallOutcome::Installed => Ok(RelatedToolActionView {
            tool_id: spec.id.to_string(),
            status: "installed".to_string(),
            message: "WinGet 설치가 완료되었습니다.".to_string(),
        }),
        InstallOutcome::UnsupportedPlatform => {
            Err("Related Tools는 Windows에서만 사용할 수 있습니다.".to_string())
        }
        InstallOutcome::WinGetUnavailable => Err(
            "WinGet을 사용할 수 없습니다. Windows App Installer를 설치한 뒤 다시 시도하세요."
                .to_string(),
        ),
        InstallOutcome::Failed => Err(
            "WinGet 설치가 실패했거나 취소되었습니다. 네트워크와 패키지 상태를 확인하세요."
                .to_string(),
        ),
        InstallOutcome::TimedOut => Err(
            "WinGet 설치가 제한 시간 안에 끝나지 않았습니다. 설치 창과 앱 상태를 확인하세요."
                .to_string(),
        ),
    }
}

/// Launch an installed reviewed tool through a direct process spawn.  The
/// command id is revalidated and the executable is selected from fixed names
/// or fixed vendor installation layouts; no path or argument comes from the
/// frontend.
#[tauri::command]
pub async fn launch_related_tool(tool_id: String) -> Result<RelatedToolActionView, String> {
    let spec = validated_tool(&tool_id)?;
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let _guard = acquire_related_action()?;
        Ok::<LaunchOutcome, String>(launch_tool(spec))
    })
    .await
    .map_err(|_| "관련 도구를 실행할 수 없습니다.".to_string())??;

    match outcome {
        LaunchOutcome::Launched => Ok(RelatedToolActionView {
            tool_id: spec.id.to_string(),
            status: "launched".to_string(),
            message: "관련 도구를 실행했습니다.".to_string(),
        }),
        LaunchOutcome::UnsupportedPlatform => {
            Err("Related Tools는 Windows에서만 사용할 수 있습니다.".to_string())
        }
        LaunchOutcome::NotInstalled => {
            Err("설치된 실행 파일을 찾을 수 없습니다. 먼저 확인 후 설치하세요.".to_string())
        }
    }
}

fn validated_tool(id: &str) -> Result<&'static RelatedToolSpec, String> {
    if !is_valid_tool_id(id) {
        return Err("관련 도구 식별자가 올바르지 않습니다.".to_string());
    }
    find_tool(id).ok_or_else(|| "지원하는 관련 도구가 아닙니다.".to_string())
}

fn detect_related_tools() -> Vec<RelatedToolView> {
    curated_tools()
        .iter()
        .map(|spec| {
            let source = detect_tool(spec);
            RelatedToolView {
                id: spec.id.to_string(),
                display_name: spec.display_name.to_string(),
                summary: spec.summary.to_string(),
                winget_id: spec.winget_id.to_string(),
                official_url: spec.official_url.to_string(),
                license_url: spec.license_url.to_string(),
                license: spec.license_summary.to_string(),
                platform_supported: cfg!(windows),
                installed: matches!(
                    source,
                    DetectionSource::Path | DetectionSource::KnownLocation
                ),
                detection: source.as_code().to_string(),
            }
        })
        .collect()
}

fn detect_tool(spec: &RelatedToolSpec) -> DetectionSource {
    #[cfg(not(windows))]
    {
        let _ = spec;
        DetectionSource::Unavailable
    }

    #[cfg(windows)]
    {
        let probe_available = path_probe_available();
        if spec
            .executable_names
            .iter()
            .any(|executable| path_executable(executable).is_some())
        {
            return DetectionSource::Path;
        }
        let known_location_found = known_executable_paths(spec)
            .iter()
            .any(|path| safe_existing_executable(path));
        classify_detection(false, known_location_found, probe_available)
    }
}

fn run_winget_install(spec: &RelatedToolSpec) -> InstallOutcome {
    #[cfg(not(windows))]
    {
        let _ = spec;
        InstallOutcome::UnsupportedPlatform
    }

    #[cfg(windows)]
    {
        let Some(winget_path) = winget_executable() else {
            return InstallOutcome::WinGetUnavailable;
        };
        let Ok(process) = spawn_guarded_winget(&winget_path, spec) else {
            return InstallOutcome::Failed;
        };
        classify_winget_result(
            true,
            true,
            process.wait(Duration::from_millis(WINGET_TIMEOUT_MS)),
        )
    }
}

#[cfg(windows)]
fn spawn_guarded_winget(path: &Path, spec: &RelatedToolSpec) -> Result<WingetProcess, ()> {
    // The resolver already checked this path, but resolution and process
    // creation are separate operations. Re-check immediately before building
    // the native process request so a replaced executable is not silently
    // trusted after a long environment/argument preparation step.
    if !safe_existing_executable(path) {
        return Err(());
    }

    // Both application name and argv originate from the fixed catalog. The
    // mutable command-line buffer is still quoted with the documented Windows
    // argv rules so an installation path containing spaces cannot alter argv.
    let application_name = wide_nul(path.as_os_str())?;
    let mut command_line = Vec::<u16>::new();
    append_windows_arg(&mut command_line, path.as_os_str())?;
    for argument in winget_install_args(spec) {
        append_windows_arg(&mut command_line, OsStr::new(argument))?;
    }
    command_line.push(0);
    let environment = build_safe_environment_block()?;

    let tree = ProcessTree::new()?;
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut process_info = PROCESS_INFORMATION::default();
    if !safe_existing_executable(path) {
        return Err(());
    }
    let created = unsafe {
        CreateProcessW(
            PCWSTR(application_name.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            Some(environment.as_ptr().cast()),
            PCWSTR::null(),
            &startup,
            &mut process_info,
        )
    };
    if created.is_err() {
        return Err(());
    }

    if tree.assign(process_info.hProcess).is_err() {
        terminate_suspended_process(&process_info);
        return Err(());
    }
    let resume_result = unsafe { ResumeThread(process_info.hThread) };
    unsafe {
        let _ = CloseHandle(process_info.hThread);
    }
    if resume_result == u32::MAX {
        let _ = unsafe { TerminateProcess(process_info.hProcess, 1) };
        let _ = unsafe { WaitForSingleObject(process_info.hProcess, 1_000) };
        unsafe {
            let _ = CloseHandle(process_info.hProcess);
        }
        return Err(());
    }

    Ok(WingetProcess {
        process: process_info.hProcess,
        tree,
    })
}

#[cfg(windows)]
fn terminate_suspended_process(process_info: &PROCESS_INFORMATION) {
    let _ = unsafe { TerminateProcess(process_info.hProcess, 1) };
    let _ = unsafe { WaitForSingleObject(process_info.hProcess, 1_000) };
    unsafe {
        let _ = CloseHandle(process_info.hThread);
        let _ = CloseHandle(process_info.hProcess);
    }
}

#[cfg(windows)]
fn wide_nul(value: &OsStr) -> Result<Vec<u16>, ()> {
    let mut encoded = value.encode_wide().collect::<Vec<_>>();
    if encoded.contains(&0) {
        return Err(());
    }
    encoded.push(0);
    Ok(encoded)
}

#[cfg(windows)]
fn append_windows_arg(command_line: &mut Vec<u16>, value: &OsStr) -> Result<(), ()> {
    let units = value.encode_wide().collect::<Vec<_>>();
    if units.contains(&0) {
        return Err(());
    }
    append_windows_arg_units(command_line, &units);
    Ok(())
}

/// Append one argument using the CommandLineToArgvW-compatible quoting rules.
/// The pure UTF-16 helper is tested on WSL while the OsStr adapter above keeps
/// arbitrary valid Windows paths lossless.
#[cfg(any(windows, test))]
fn append_windows_arg_units(command_line: &mut Vec<u16>, value: &[u16]) {
    const SPACE: u16 = b' ' as u16;
    const TAB: u16 = b'\t' as u16;
    const QUOTE: u16 = b'"' as u16;
    const BACKSLASH: u16 = b'\\' as u16;

    if !command_line.is_empty() {
        command_line.push(SPACE);
    }
    let quote = value.is_empty()
        || value
            .iter()
            .any(|unit| matches!(*unit, SPACE | TAB | QUOTE));
    if !quote {
        command_line.extend_from_slice(value);
        return;
    }

    command_line.push(QUOTE);
    let mut backslashes = 0_usize;
    for unit in value {
        if *unit == BACKSLASH {
            backslashes += 1;
            continue;
        }
        if *unit == QUOTE {
            command_line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2 + 1));
            command_line.push(QUOTE);
        } else {
            command_line.extend(std::iter::repeat_n(BACKSLASH, backslashes));
            command_line.push(*unit);
        }
        backslashes = 0;
    }
    command_line.extend(std::iter::repeat_n(BACKSLASH, backslashes * 2));
    command_line.push(QUOTE);
}

fn launch_tool(spec: &RelatedToolSpec) -> LaunchOutcome {
    #[cfg(not(windows))]
    {
        let _ = spec;
        LaunchOutcome::UnsupportedPlatform
    }

    #[cfg(windows)]
    {
        for executable in spec.executable_names {
            if let Some(path) = path_executable(executable) {
                if spawn_program(&path) {
                    return LaunchOutcome::Launched;
                }
            }
        }
        for path in known_executable_paths(spec) {
            if safe_existing_executable(&path) && spawn_program(&path) {
                return LaunchOutcome::Launched;
            }
        }
        LaunchOutcome::NotInstalled
    }
}

#[cfg(windows)]
fn spawn_program(program: &Path) -> bool {
    // Re-check immediately before spawning.  The path resolver and this
    // check are deliberately separate so a changed PATH cannot redirect a
    // launch to a different executable between lookup and spawn.
    if !safe_existing_executable(program) {
        return false;
    }
    let mut command = Command::new(program);
    sanitize_external_environment(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.creation_flags(CREATE_NO_WINDOW.0);
    command.spawn().is_ok()
}

#[cfg(windows)]
fn path_executable(executable: &str) -> Option<PathBuf> {
    if !safe_executable_name(executable) {
        return None;
    }
    let raw_path = std::env::var_os("PATH")?;
    if raw_path.to_string_lossy().len() > MAX_PATH_ENV_BYTES {
        return None;
    }
    for directory in std::env::split_paths(&raw_path).take(MAX_PATH_ENTRIES) {
        if directory.to_string_lossy().len() > MAX_PATH_ENTRY_BYTES || !safe_path_entry(&directory)
        {
            continue;
        }
        let candidate = directory.join(executable);
        if safe_existing_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn path_probe_available() -> bool {
    std::env::var_os("PATH")
        .map(|path| path.to_string_lossy().len() <= MAX_PATH_ENV_BYTES)
        .unwrap_or(false)
}

#[cfg(windows)]
fn winget_executable() -> Option<PathBuf> {
    let mut known_paths = Vec::with_capacity(2);
    if let Some(local_app_data) = dirs::data_local_dir().and_then(safe_root_path) {
        known_paths.push(
            local_app_data
                .join("Microsoft")
                .join("WindowsApps")
                .join("winget.exe"),
        );
    }
    if let Some(system_directory) = system_directory() {
        known_paths.push(system_directory.join("winget.exe"));
    }
    known_paths
        .into_iter()
        .find(|path| trusted_winget_path(path) && safe_existing_executable(path))
        .or_else(|| {
            let path = path_executable("winget.exe")?;
            trusted_winget_path(&path).then_some(path)
        })
}

#[cfg(windows)]
fn trusted_winget_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !file_name.eq_ignore_ascii_case("winget.exe") {
        return false;
    }

    let local_alias = dirs::data_local_dir()
        .and_then(safe_root_path)
        .map(|root| root.join("Microsoft").join("WindowsApps"));
    let system_alias = system_directory();
    local_alias
        .as_ref()
        .is_some_and(|root| path_is_under(path, root))
        || system_alias
            .as_ref()
            .is_some_and(|root| path_is_under(path, root))
}

#[cfg(windows)]
fn system_directory() -> Option<PathBuf> {
    let mut buffer = vec![0_u16; MAX_PATH_ENV_BYTES];
    let length = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if length == 0 || length >= buffer.len() {
        return None;
    }
    safe_root_path(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

#[cfg(windows)]
fn path_is_under(path: &Path, root: &Path) -> bool {
    let path = path.to_string_lossy();
    let root = root.to_string_lossy().trim_end_matches(['\\', '/']);
    path.eq_ignore_ascii_case(root)
        || path
            .get(root.len()..)
            .is_some_and(|suffix| suffix.starts_with('\\') || suffix.starts_with('/'))
}

#[cfg(windows)]
fn safe_executable_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PATH_ENTRY_BYTES
        && value.to_ascii_lowercase().ends_with(".exe")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'-' | b'_'))
}

#[cfg(windows)]
fn safe_path_entry(path: &Path) -> bool {
    safe_path_shape(path) && plain_existing_components(path)
}

#[cfg(any(windows, test))]
fn classify_winget_result(
    command_available: bool,
    process_started: bool,
    process_status: Option<bool>,
) -> InstallOutcome {
    if !command_available || !process_started {
        return InstallOutcome::WinGetUnavailable;
    }
    match process_status {
        Some(true) => InstallOutcome::Installed,
        Some(false) => InstallOutcome::Failed,
        None => InstallOutcome::TimedOut,
    }
}

#[cfg(windows)]
fn sanitize_external_environment(command: &mut Command) {
    // Related Tools are optional complements and must not receive arbitrary
    // application environment values (which may contain credentials).  Keep
    // only the standard Windows process variables needed for PATH lookup and
    // normal desktop application startup.
    const SAFE_ENVIRONMENT: &[&str] = &[
        "ALLUSERSPROFILE",
        "APPDATA",
        "ComSpec",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "OS",
        "PATH",
        "PATHEXT",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "PUBLIC",
        "SystemDrive",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERDOMAIN",
        "USERNAME",
        "USERPROFILE",
        "windir",
        "WINDIR",
    ];
    let values = SAFE_ENVIRONMENT
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (*name, value)))
        .collect::<Vec<_>>();
    command.env_clear();
    for (name, value) in values {
        command.env(name, value);
    }
}

#[cfg(windows)]
fn build_safe_environment_block() -> Result<Vec<u16>, ()> {
    const SAFE_ENVIRONMENT: &[&str] = &[
        "ALLUSERSPROFILE",
        "APPDATA",
        "ComSpec",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "OS",
        "PATH",
        "PATHEXT",
        "ProgramData",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "PUBLIC",
        "SystemDrive",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERDOMAIN",
        "USERNAME",
        "USERPROFILE",
        "windir",
        "WINDIR",
    ];
    let mut values = SAFE_ENVIRONMENT
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|value| (OsString::from(*name), value)))
        .collect::<Vec<_>>();
    values.sort_by_cached_key(|(name, _)| name.to_string_lossy().to_ascii_uppercase());
    values.dedup_by(|left, right| {
        left.0
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.0.to_string_lossy())
    });

    let mut block = Vec::<u16>::new();
    for (name, value) in values {
        let name = name.encode_wide().collect::<Vec<_>>();
        let value = value.encode_wide().collect::<Vec<_>>();
        if name.contains(&0)
            || value.contains(&0)
            || value.len() > MAX_PATH_ENV_BYTES
            || block
                .len()
                .saturating_add(name.len())
                .saturating_add(value.len())
                .saturating_add(3)
                > MAX_ENVIRONMENT_BLOCK_UNITS
        {
            return Err(());
        }
        block.extend(name);
        block.push(b'=' as u16);
        block.extend(value);
        block.push(0);
    }
    // CreateProcessW requires an additional terminator after the last entry;
    // an empty environment is represented by two NUL units.
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

#[cfg(windows)]
fn winget_install_args(spec: &RelatedToolSpec) -> Vec<&'static str> {
    vec![
        "install",
        "--id",
        spec.winget_id,
        "--exact",
        "--source",
        "winget",
        "--accept-source-agreements",
        "--accept-package-agreements",
    ]
}

#[cfg(windows)]
fn known_executable_paths(spec: &RelatedToolSpec) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let add = |paths: &mut Vec<PathBuf>, env: &str, components: &[&str], executable: &str| {
        if paths.len() >= MAX_KNOWN_PATHS {
            return;
        }
        let Some(root) = safe_environment_root(env) else {
            return;
        };
        let mut path = root;
        for component in components {
            path.push(component);
        }
        path.push(executable);
        paths.push(path);
    };
    let add_program_files = |paths: &mut Vec<PathBuf>, components: &[&str], executable: &str| {
        add(paths, "ProgramFiles", components, executable);
        add(paths, "ProgramFiles(x86)", components, executable);
    };

    match spec.id {
        "power-toys" => add_program_files(&mut paths, &["PowerToys"], "PowerToys.exe"),
        "vs-code" => {
            add(
                &mut paths,
                "LOCALAPPDATA",
                &["Programs", "Microsoft VS Code"],
                "Code.exe",
            );
            add_program_files(&mut paths, &["Microsoft VS Code"], "Code.exe");
        }
        "bruno" => {
            add(
                &mut paths,
                "LOCALAPPDATA",
                &["Programs", "Bruno"],
                "Bruno.exe",
            );
            add(&mut paths, "LOCALAPPDATA", &["Bruno"], "Bruno.exe");
        }
        "dbeaver" => {
            add_program_files(&mut paths, &["DBeaver"], "dbeaver.exe");
            add(&mut paths, "LOCALAPPDATA", &["DBeaver"], "dbeaver.exe");
        }
        "db-browser" => {
            add_program_files(
                &mut paths,
                &["DB Browser for SQLite"],
                "DB Browser for SQLite.exe",
            );
            add_program_files(&mut paths, &["DB Browser for SQLite"], "sqlitebrowser.exe");
        }
        "github-desktop" => {
            add(
                &mut paths,
                "LOCALAPPDATA",
                &["GitHubDesktop"],
                "GitHubDesktop.exe",
            );
            add_program_files(&mut paths, &["GitHub Desktop"], "GitHubDesktop.exe");
        }
        "podman-desktop" => {
            add(
                &mut paths,
                "LOCALAPPDATA",
                &["Programs", "Podman Desktop"],
                "podman-desktop.exe",
            );
            add_program_files(&mut paths, &["Podman Desktop"], "podman-desktop.exe");
        }
        "docker-desktop" => {
            add_program_files(&mut paths, &["Docker", "Docker"], "Docker Desktop.exe")
        }
        // Windows Terminal is normally exposed as the `wt.exe` app execution
        // alias and is intentionally detected through PATH only.
        "windows-terminal" => {}
        _ => {}
    }
    paths
}

#[cfg(windows)]
fn safe_environment_root(name: &str) -> Option<PathBuf> {
    safe_root_path(PathBuf::from(std::env::var_os(name)?))
}

#[cfg(windows)]
fn safe_root_path(path: PathBuf) -> Option<PathBuf> {
    if !safe_path_shape(&path) {
        return None;
    }
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return None;
    }
    Some(path)
}

#[cfg(windows)]
fn safe_existing_executable(path: &Path) -> bool {
    if !safe_path_shape(path) {
        return false;
    }
    let Some(metadata) = std::fs::symlink_metadata(path).ok() else {
        return false;
    };
    if !metadata.is_file() || (is_link_or_reparse(&metadata) && !is_trusted_windows_alias(path)) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    plain_existing_components(parent) && std::fs::canonicalize(path).is_ok()
}

#[cfg(windows)]
fn is_trusted_windows_alias(path: &Path) -> bool {
    // Windows Store applications expose a small, OS-managed alias in this
    // exact directory.  Allow only the two aliases this module resolves; all
    // other reparse-point executables remain rejected.
    let Some(local_app_data) = dirs::data_local_dir().and_then(safe_root_path) else {
        return false;
    };
    let trusted_parent = local_app_data.join("Microsoft").join("WindowsApps");
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    parent
        .to_string_lossy()
        .eq_ignore_ascii_case(&trusted_parent.to_string_lossy())
        && matches!(
            file_name.to_ascii_lowercase().as_str(),
            "wt.exe" | "winget.exe"
        )
}

#[cfg(windows)]
fn safe_path_shape(path: &Path) -> bool {
    let mut components = path.components();
    let local_drive = matches!(
        components.next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_))
    );
    let rooted = matches!(components.next(), Some(Component::RootDir));
    path.is_absolute()
        && local_drive
        && rooted
        && path.to_string_lossy().len() <= MAX_PATH_ENV_BYTES
        && path.components().count() <= MAX_PATH_COMPONENTS
        && !path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

#[cfg(windows)]
fn plain_existing_components(path: &Path) -> bool {
    if !safe_path_shape(path) {
        return false;
    }

    // Walk existing parents instead of rebuilding a Windows path from its
    // Prefix/RootDir components.  Rebuilding can accidentally turn a
    // drive-qualified root into a drive-relative path; parent() preserves
    // drive and UNC prefixes exactly as the OS represented them.
    let mut current = path;
    loop {
        let Ok(metadata) = std::fs::symlink_metadata(current) else {
            return false;
        };
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return false;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent;
    }
    true
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::related_tools::{curated_tools, DetectionSource};

    #[test]
    fn detection_view_contains_no_path_or_process_output() {
        let views = detect_related_tools();
        assert_eq!(views.len(), curated_tools().len());
        for view in views {
            assert_eq!(view.platform_supported, cfg!(windows));
            assert!(!view.id.contains('/') && !view.id.contains('\\'));
            assert!(matches!(
                view.detection.as_str(),
                "path" | "known-location" | "not-found" | "unavailable"
            ));
            assert!(!view.display_name.contains("C:"));
            assert!(!view.summary.contains("C:"));
            assert!(!view.winget_id.contains('\\') && !view.winget_id.contains('/'));
            assert!(!view.official_url.contains("C:"));
            assert!(!view.license_url.contains("C:"));
            assert!(!view.license.contains("C:"));
        }
    }

    #[test]
    fn validates_only_curated_opaque_ids() {
        assert!(validated_tool("power-toys").is_ok());
        assert!(validated_tool("unknown").is_err());
        assert!(validated_tool("C:\\secret\\tool.exe").is_err());
        assert!(validated_tool("vs-code --id attacker").is_err());
    }

    #[test]
    fn install_request_is_camel_case_and_strict() {
        let request: RelatedToolInstallRequest =
            serde_json::from_str(r#"{"toolId":"vs-code","confirmed":true}"#).unwrap();
        assert_eq!(request.tool_id, "vs-code");
        assert!(request.confirmed);
        assert!(serde_json::from_str::<RelatedToolInstallRequest>(
            r#"{"toolId":"vs-code","confirmed":true,"path":"C:\\secret"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<RelatedToolInstallRequest>(
            r#"{"tool_id":"vs-code","confirmed":true}"#
        )
        .is_err());
    }

    #[test]
    fn detection_codes_are_stable() {
        assert_eq!(DetectionSource::Path.as_code(), "path");
        assert_eq!(DetectionSource::KnownLocation.as_code(), "known-location");
        assert_eq!(DetectionSource::NotFound.as_code(), "not-found");
        assert_eq!(DetectionSource::Unavailable.as_code(), "unavailable");
        assert_eq!(
            classify_detection(false, false, false).as_code(),
            "unavailable"
        );
    }

    #[test]
    fn winget_arguments_are_exact_and_have_no_user_slot() {
        #[cfg(windows)]
        {
            let args = winget_install_args(find_tool("vs-code").unwrap());
            assert_eq!(
                args,
                vec![
                    "install",
                    "--id",
                    "Microsoft.VisualStudioCode",
                    "--exact",
                    "--source",
                    "winget",
                    "--accept-source-agreements",
                    "--accept-package-agreements",
                ]
            );
        }
    }

    #[test]
    fn winget_result_mapping_is_fail_closed_for_offline_and_timeout_cases() {
        assert_eq!(
            classify_winget_result(false, false, None),
            InstallOutcome::WinGetUnavailable
        );
        assert_eq!(
            classify_winget_result(true, false, None),
            InstallOutcome::WinGetUnavailable
        );
        assert_eq!(
            classify_winget_result(true, true, Some(false)),
            InstallOutcome::Failed
        );
        assert_eq!(
            classify_winget_result(true, true, None),
            InstallOutcome::TimedOut
        );
        assert_eq!(
            classify_winget_result(true, true, Some(true)),
            InstallOutcome::Installed
        );
    }

    #[test]
    fn windows_argument_quoting_preserves_spaces_quotes_and_trailing_slashes() {
        let mut command_line = Vec::new();
        append_windows_arg_units(
            &mut command_line,
            &"C:\\Program Files\\winget.exe"
                .encode_utf16()
                .collect::<Vec<_>>(),
        );
        append_windows_arg_units(
            &mut command_line,
            &"value\\\"quoted".encode_utf16().collect::<Vec<_>>(),
        );
        append_windows_arg_units(
            &mut command_line,
            &"C:\\Program Files\\".encode_utf16().collect::<Vec<_>>(),
        );
        append_windows_arg_units(&mut command_line, &[]);

        assert_eq!(
            String::from_utf16(&command_line).unwrap(),
            r#""C:\Program Files\winget.exe" "value\\\"quoted" "C:\Program Files\\" """#
        );
    }

    #[test]
    fn related_actions_use_one_single_flight_lock() {
        let _guard = acquire_related_action().unwrap();
        assert!(acquire_related_action().is_err());
    }

    #[cfg(windows)]
    #[test]
    fn executable_name_policy_rejects_paths_and_non_executables() {
        assert!(safe_executable_name("code.exe"));
        assert!(safe_executable_name("DB Browser for SQLite.exe"));
        assert!(!safe_executable_name("..\\code.exe"));
        assert!(!safe_executable_name("code"));
        assert!(!safe_executable_name("code.exe --flag"));
    }

    #[cfg(not(windows))]
    #[test]
    fn installation_is_not_attempted_on_non_windows() {
        assert_eq!(
            run_winget_install(find_tool("vs-code").unwrap()),
            InstallOutcome::UnsupportedPlatform
        );
    }
}
