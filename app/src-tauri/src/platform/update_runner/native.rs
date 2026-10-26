//! Windows handle operations used only by the internal update runner.
use crate::core::{
    channel::CHANNEL,
    update::runner::{ProcessIdentity, Result, RunnerError},
};
use std::{
    ffi::OsString,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{
            DUPLICATE_HANDLE_OPTIONS, DuplicateHandle, FILETIME, GetHandleInformation, HANDLE,
            HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::{
            Com::CoTaskMemFree,
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            JobObjects::IsProcessInJob,
            RemoteDesktop::ProcessIdToSessionId,
            Threading::*,
        },
        UI::{
            Shell::{FOLDERID_LocalAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
            WindowsAndMessaging::{MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MessageBoxW},
        },
    },
    core::{BOOL, PCWSTR, PWSTR},
};

pub fn raw(handle: &impl AsRawHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}
fn own(handle: HANDLE) -> OwnedHandle {
    // SAFETY: callers pass a newly-created, valid owned kernel handle exactly once.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}
pub fn duplicate(handle: HANDLE, access: u32, inherit: bool) -> Result<OwnedHandle> {
    let mut result = HANDLE::default();
    // SAFETY: source is borrowed for this call; the result becomes independently owned.
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut result,
            access,
            inherit,
            DUPLICATE_HANDLE_OPTIONS(0),
        )
    }
    .map_err(|_| RunnerError::Identity)?;
    Ok(own(result))
}
pub fn event() -> Result<OwnedHandle> {
    // Unnamed, initially unsignaled events are not discoverable by name or inherited by default.
    unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
        .map(own)
        .map_err(|_| RunnerError::Spawn)
}
pub fn signal(event: &OwnedHandle) -> Result<()> {
    unsafe { SetEvent(raw(event)) }.map_err(|_| RunnerError::Unknown)
}
pub fn current_process() -> Result<OwnedHandle> {
    unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            GetCurrentProcessId(),
        )
    }
    .map(own)
    .map_err(|_| RunnerError::Identity)
}
pub fn identity(handle: HANDLE) -> Result<ProcessIdentity> {
    let pid = unsafe { GetProcessId(handle) };
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let mut session = 0;
    if pid == 0 {
        return Err(RunnerError::Identity);
    }
    unsafe {
        GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)
            .map_err(|_| RunnerError::Identity)?;
        ProcessIdToSessionId(pid, &mut session).map_err(|_| RunnerError::Identity)?;
    }
    let started = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    if started == 0 {
        return Err(RunnerError::Identity);
    }
    Ok(ProcessIdentity {
        pid,
        started,
        session,
    })
}
pub fn image(handle: HANDLE) -> Result<PathBuf> {
    let mut value = vec![0_u16; 32768];
    let mut size = value.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(value.as_mut_ptr()),
            &mut size,
        )
    }
    .map_err(|_| RunnerError::Identity)?;
    std::fs::canonicalize(PathBuf::from(OsString::from_wide(&value[..size as usize])))
        .map_err(|_| RunnerError::Identity)
}
pub fn parent_pid() -> Result<u32> {
    let snapshot = own(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|_| RunnerError::Identity)?);
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    unsafe { Process32FirstW(raw(&snapshot), &mut entry) }.map_err(|_| RunnerError::Identity)?;
    let current = unsafe { GetCurrentProcessId() };
    loop {
        if entry.th32ProcessID == current {
            return Ok(entry.th32ParentProcessID);
        }
        if unsafe { Process32NextW(raw(&snapshot), &mut entry) }.is_err() {
            return Err(RunnerError::Identity);
        }
    }
}
pub fn exited(handle: HANDLE) -> Result<bool> {
    match unsafe { WaitForSingleObject(handle, 0) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(RunnerError::Unknown),
    }
}
pub fn wait(handle: HANDLE, timeout: Duration) -> Result<bool> {
    match unsafe { WaitForSingleObject(handle, timeout.as_millis().min(u32::MAX as u128) as u32) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(RunnerError::Unknown),
    }
}
pub fn outside_job(handle: HANDLE) -> Result<bool> {
    let mut contained = BOOL::default();
    unsafe { IsProcessInJob(handle, None, &mut contained) }.map_err(|_| RunnerError::Identity)?;
    Ok(!contained.as_bool())
}
/// Used only for the exact child created by prepare, before its commit event is signaled.
pub fn abort_prepared(child: HANDLE, disarm: &OwnedHandle) -> Result<bool> {
    let _ = signal(disarm);
    if wait(child, Duration::from_secs(2))? {
        return Ok(true);
    }
    unsafe { TerminateProcess(child, 125) }.map_err(|_| RunnerError::Unknown)?;
    wait(child, Duration::from_secs(2))
}
pub fn terminate_prepared(child: HANDLE) -> Result<()> {
    unsafe { TerminateProcess(child, 125) }.map_err(|_| RunnerError::Unknown)
}
/// Query failure retains quarantine; a reused PID is not the old process.
pub fn process_live(expected: ProcessIdentity) -> Result<bool> {
    let process = match unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            expected.pid,
        )
    } {
        Ok(handle) => own(handle),
        Err(error) if error.code().0 as u32 == 0x80070057 => return Ok(false), // ERROR_INVALID_PARAMETER: PID does not exist.
        Err(_) => return Err(RunnerError::Unknown),
    };
    if identity(raw(&process))? != expected {
        return Ok(false);
    }
    Ok(!exited(raw(&process))?)
}
/// Modal notice without an owner window; startup has not created any window yet.
pub fn notice(title: &str, body: &str) {
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (title, body) = (wide(title), wide(body));
    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive this blocking call.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND,
        )
    };
}
pub fn local_root() -> Result<PathBuf> {
    let pointer = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, KF_FLAG_DEFAULT, None) }
        .map_err(|_| RunnerError::Storage)?;
    // SAFETY: Shell returns a NUL-terminated CoTaskMem buffer; free it after copying.
    let value = unsafe { OsString::from_wide(pointer.as_wide()) };
    unsafe { CoTaskMemFree(Some(pointer.0.cast())) };
    Ok(PathBuf::from(value)
        .join(CHANNEL.config().identifier)
        .join("updates"))
}

struct Attributes {
    _memory: Vec<usize>,
    pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
}
impl Attributes {
    fn new(handles: &[HANDLE]) -> Result<Self> {
        let mut size = 0;
        let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut size) };
        if size == 0 || size > 65536 {
            return Err(RunnerError::Spawn);
        }
        let mut memory = vec![0_usize; size.div_ceil(std::mem::size_of::<usize>())];
        let pointer = LPPROC_THREAD_ATTRIBUTE_LIST(memory.as_mut_ptr().cast());
        unsafe { InitializeProcThreadAttributeList(Some(pointer), 1, None, &mut size) }
            .map_err(|_| RunnerError::Spawn)?;
        let result = Self {
            _memory: memory,
            pointer,
        };
        unsafe {
            UpdateProcThreadAttribute(
                pointer,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr().cast()),
                std::mem::size_of_val(handles),
                None,
                None,
            )
        }
        .map_err(|_| RunnerError::Spawn)?;
        Ok(result)
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.pointer) };
    }
}
pub fn spawn_runner(exe: &Path, inherited: &[OwnedHandle]) -> Result<OwnedHandle> {
    let handles: Vec<_> = inherited.iter().map(raw).collect();
    let attributes = Attributes::new(&handles)?;
    let application: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let mut command = vec![b'"' as u16];
    command.extend(exe.as_os_str().encode_wide());
    command.extend("\" --update-runner".encode_utf16());
    for handle in &handles {
        command.extend(format!(" {}", handle.0 as usize).encode_utf16());
    }
    command.push(0);
    let startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
            ..Default::default()
        },
        lpAttributeList: attributes.pointer,
    };
    let mut process = PROCESS_INFORMATION::default();
    // Only six least-privilege duplicated handles are inherited. Breakaway failure aborts preparation.
    unsafe {
        CreateProcessW(
            PCWSTR(application.as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            true,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &startup.StartupInfo,
            &mut process,
        )
    }
    .map_err(|_| RunnerError::Spawn)?;
    let child = own(process.hProcess);
    let _thread = own(process.hThread);
    Ok(child)
}
pub fn receive_handles(values: &[OsString]) -> Result<Vec<OwnedHandle>> {
    if values.len() != 6 {
        return Err(RunnerError::Identity);
    }
    let mut parsed = Vec::new();
    for value in values {
        let value = value
            .to_str()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v != 0 && *v != usize::MAX)
            .ok_or(RunnerError::Identity)?;
        if parsed.contains(&value) {
            return Err(RunnerError::Identity);
        }
        let mut flags = 0;
        unsafe { GetHandleInformation(HANDLE(value as *mut _), &mut flags) }
            .map_err(|_| RunnerError::Identity)?;
        if flags & HANDLE_FLAG_INHERIT.0 == 0 {
            return Err(RunnerError::Identity);
        }
        parsed.push(value);
    }
    let mut owned = Vec::new();
    for value in parsed {
        let handle = HANDLE(value as *mut _);
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) }
            .map_err(|_| RunnerError::Identity)?;
        owned.push(own(handle));
    }
    Ok(owned)
}
pub fn wait_any(handles: &[HANDLE], timeout: Duration) -> Result<Option<usize>> {
    let value = unsafe {
        WaitForMultipleObjects(
            handles,
            false,
            timeout.as_millis().min(u32::MAX as u128) as u32,
        )
    };
    if value == WAIT_TIMEOUT {
        return Ok(None);
    }
    let index = value.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
    if index < handles.len() {
        Ok(Some(index))
    } else {
        Err(RunnerError::Unknown)
    }
}
pub fn bounded_child(
    child: &mut std::process::Child,
    duration: Duration,
) -> Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + duration;
    loop {
        if let Some(status) = child.try_wait().map_err(|_| RunnerError::Unknown)? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_commit_handle_can_wait_but_cannot_create_installation_authority() {
        let parent = event().unwrap();
        let child_view = duplicate(raw(&parent), PROCESS_SYNCHRONIZE.0, false).unwrap();
        assert!(signal(&child_view).is_err());
        assert!(!wait(raw(&child_view), Duration::ZERO).unwrap());
        signal(&parent).unwrap();
        assert!(wait(raw(&child_view), Duration::ZERO).unwrap());
    }
    #[test]
    fn retained_process_identity_distinguishes_reused_pid_from_actual_owner() {
        let handle = current_process().unwrap();
        let expected = identity(raw(&handle)).unwrap();
        assert!(process_live(expected).unwrap());
        assert!(
            !process_live(ProcessIdentity {
                started: expected.started + 1,
                ..expected
            })
            .unwrap()
        );
        assert!(!exited(raw(&handle)).unwrap());
    }
    #[test]
    fn runner_cli_rejects_missing_repeated_or_noninherited_handles() {
        assert!(receive_handles(&[]).is_err());
        let parent = event().unwrap();
        let value = OsString::from((raw(&parent).0 as usize).to_string());
        assert!(receive_handles(&vec![value; 6]).is_err());
        let inherited = duplicate(
            raw(&parent),
            (PROCESS_SYNCHRONIZE.0) | EVENT_MODIFY_STATE.0,
            true,
        )
        .unwrap();
        let value = OsString::from((raw(&inherited).0 as usize).to_string());
        assert!(receive_handles(&vec![value; 6]).is_err());
        // Rejected lists never adopt/close handles belonging to the caller.
        signal(&parent).unwrap();
        assert!(wait(raw(&inherited), Duration::ZERO).unwrap());
    }
}
