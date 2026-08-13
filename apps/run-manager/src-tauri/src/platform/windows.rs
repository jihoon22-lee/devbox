use crate::core::shell::{self, ShellError};
use crate::lifecycle::{complete_system_shutdown, RuntimeState};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::mem::{size_of, MaybeUninit};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::FromRawHandle;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;
use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, SetHandleInformation, FILETIME, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT, HWND,
    LPARAM, LRESULT, RPC_E_CHANGED_MODE, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, GetProcessTimes,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};
use windows::Win32::UI::Shell::{
    DefSubclassProc, IShellLinkW, RemoveWindowSubclass, SetWindowSubclass, ShellLink, SLGP_RAWPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_NCDESTROY, WM_QUERYENDSESSION};
use zeroize::{Zeroize, Zeroizing};

use super::{
    StartupShortcut, StartupShortcutStatus, STARTUP_SHORTCUT_ARGUMENTS, STARTUP_SHORTCUT_FILE_NAME,
};

const STARTUP_SHORTCUT_DESCRIPTION: &str = "Run Manager background scheduler";
const SHELL_STRING_CAPACITY: usize = 32_768;

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn enter() -> Result<Self, String> {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if result.is_ok() {
            Ok(Self { uninitialize: true })
        } else if result == RPC_E_CHANGED_MODE {
            // Tauri may already own this worker thread as an MTA. Shell Link is
            // usable from that apartment; only a successful initialization is
            // balanced with CoUninitialize.
            Ok(Self {
                uninitialize: false,
            })
        } else {
            Err(format!("failed to initialize Windows COM: {result:?}"))
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

pub fn startup_shortcut_status(
    shortcut: &StartupShortcut,
) -> Result<StartupShortcutStatus, String> {
    let enabled = if shortcut.shortcut_path().exists() {
        shortcut_matches(shortcut)?
    } else {
        false
    };
    Ok(StartupShortcutStatus {
        supported: true,
        enabled,
        shortcut_path: shortcut.shortcut_path().display().to_string(),
    })
}

pub fn set_startup_shortcut_enabled(
    shortcut: &StartupShortcut,
    enabled: bool,
) -> Result<StartupShortcutStatus, String> {
    if enabled {
        create_or_update_startup_shortcut(shortcut)?;
    } else {
        remove_startup_shortcut(shortcut)?;
    }
    startup_shortcut_status(shortcut)
}

fn create_or_update_startup_shortcut(shortcut: &StartupShortcut) -> Result<(), String> {
    let parent = shortcut
        .shortcut_path()
        .parent()
        .ok_or_else(|| "the Startup shortcut has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let _apartment = ComApartment::enter()?;
    let link = new_shell_link()?;
    let executable = wide_path(shortcut.executable());
    let arguments = wide_str(STARTUP_SHORTCUT_ARGUMENTS);
    let description = wide_str(STARTUP_SHORTCUT_DESCRIPTION);
    let working_directory = shortcut
        .executable()
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let working_directory = wide_path(working_directory);

    unsafe {
        link.SetPath(PCWSTR::from_raw(executable.as_ptr()))
            .map_err(shell_error)?;
        link.SetArguments(PCWSTR::from_raw(arguments.as_ptr()))
            .map_err(shell_error)?;
        link.SetDescription(PCWSTR::from_raw(description.as_ptr()))
            .map_err(shell_error)?;
        link.SetWorkingDirectory(PCWSTR::from_raw(working_directory.as_ptr()))
            .map_err(shell_error)?;
        link.SetIconLocation(PCWSTR::from_raw(executable.as_ptr()), 0)
            .map_err(shell_error)?;
    }

    let temporary = parent.join(format!(
        ".{}.{}.tmp.lnk",
        STARTUP_SHORTCUT_FILE_NAME.trim_end_matches(".lnk"),
        uuid::Uuid::new_v4()
    ));
    let temporary_wide = wide_path(&temporary);
    let persist: IPersistFile = link.cast().map_err(shell_error)?;
    let save_result = unsafe { persist.Save(PCWSTR::from_raw(temporary_wide.as_ptr()), true) }
        .map_err(shell_error)
        .and_then(|()| {
            replace_file_atomic(&temporary, shortcut.shortcut_path())
                .map_err(|error| error.to_string())
        });
    if save_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    save_result
}

fn remove_startup_shortcut(shortcut: &StartupShortcut) -> Result<(), String> {
    if !shortcut.shortcut_path().exists() {
        return Ok(());
    }
    if !shortcut_is_owned(shortcut.shortcut_path())? {
        return Err("the existing Startup shortcut is not owned by Run Manager".to_string());
    }
    fs::remove_file(shortcut.shortcut_path()).map_err(|error| error.to_string())
}

fn shortcut_matches(shortcut: &StartupShortcut) -> Result<bool, String> {
    let (target, arguments, description) = load_shortcut(shortcut.shortcut_path())?;
    Ok(same_windows_path(&target, shortcut.executable())
        && arguments == STARTUP_SHORTCUT_ARGUMENTS
        && description == STARTUP_SHORTCUT_DESCRIPTION)
}

fn shortcut_is_owned(path: &Path) -> Result<bool, String> {
    let (_, arguments, description) = load_shortcut(path)?;
    Ok(arguments == STARTUP_SHORTCUT_ARGUMENTS && description == STARTUP_SHORTCUT_DESCRIPTION)
}

fn load_shortcut(path: &Path) -> Result<(PathBuf, String, String), String> {
    let _apartment = ComApartment::enter()?;
    let link = new_shell_link()?;
    let persist: IPersistFile = link.cast().map_err(shell_error)?;
    let path_wide = wide_path(path);
    unsafe { persist.Load(PCWSTR::from_raw(path_wide.as_ptr()), STGM_READ) }
        .map_err(shell_error)?;

    let mut target = vec![0_u16; SHELL_STRING_CAPACITY];
    let mut arguments = vec![0_u16; SHELL_STRING_CAPACITY];
    let mut description = vec![0_u16; SHELL_STRING_CAPACITY];
    unsafe {
        link.GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .map_err(shell_error)?;
        link.GetArguments(&mut arguments).map_err(shell_error)?;
        link.GetDescription(&mut description).map_err(shell_error)?;
    }
    Ok((
        PathBuf::from(os_string_from_wide(&target)),
        os_string_from_wide(&arguments)
            .to_string_lossy()
            .into_owned(),
        os_string_from_wide(&description)
            .to_string_lossy()
            .into_owned(),
    ))
}

fn new_shell_link() -> Result<IShellLinkW, String> {
    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.map_err(shell_error)
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left.to_string_lossy()
        .to_lowercase()
        .eq(&right.to_string_lossy().to_lowercase())
}

fn os_string_from_wide(value: &[u16]) -> OsString {
    let end = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    OsString::from_wide(&value[..end])
}

fn wide_str(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn shell_error(error: windows::core::Error) -> String {
    format!("Windows Startup shortcut operation failed: {error}")
}

/// Replace the manifest in one filesystem operation. `std::fs::rename` does
/// not replace an existing destination on Windows, while `ReplaceFileW` does
/// and preserves the crash-safe temp-file + atomic-swap contract.
pub(crate) fn replace_file_atomic(replacement: &Path, destination: &Path) -> io::Result<()> {
    let replacement_wide = wide_path(replacement);
    let destination_wide = wide_path(destination);

    // ReplaceFileW requires an existing destination. The first manifest is a
    // create-only rename; if a racing writer creates the destination, retrying
    // with ReplaceFileW below preserves the replacement contract.
    if !destination.exists() {
        match fs::rename(replacement, destination) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
            Err(_) => {}
        }
    }

    unsafe {
        ReplaceFileW(
            PCWSTR::from_raw(destination_wide.as_ptr()),
            PCWSTR::from_raw(replacement_wide.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(|error| io::Error::other(error.to_string()))
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

const SESSION_END_SUBCLASS_ID: usize = 0x4456_4258;

struct SessionEndContext {
    state: Arc<RuntimeState>,
    pending: bool,
}

pub fn install_session_end_hook(
    window: &tauri::WebviewWindow,
    _app: &AppHandle,
    state: Arc<RuntimeState>,
) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    let context = Box::into_raw(Box::new(SessionEndContext {
        state,
        pending: false,
    }));

    let installed = unsafe {
        SetWindowSubclass(
            hwnd,
            Some(session_end_proc),
            SESSION_END_SUBCLASS_ID,
            context as usize,
        )
    };
    if installed.as_bool() {
        Ok(())
    } else {
        unsafe {
            drop(Box::from_raw(context));
        }
        Err("failed to install the Windows session-end hook".to_string())
    }
}

unsafe extern "system" fn session_end_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    context: usize,
) -> LRESULT {
    match message {
        WM_QUERYENDSESSION => {
            let context = &mut *(context as *mut SessionEndContext);
            // Another application may cancel the session end. A query records
            // intent but must not stop the scheduler yet.
            context.pending = true;
            LRESULT(1)
        }
        WM_ENDSESSION if wparam.0 != 0 => {
            let context = &mut *(context as *mut SessionEndContext);
            let _was_queried = context.pending;
            context.pending = false;
            // Complete cleanup before tao handles WM_ENDSESSION(TRUE) by
            // destroying the Tauri event loop.
            complete_system_shutdown(&context.state);
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        WM_ENDSESSION => {
            let context = &mut *(context as *mut SessionEndContext);
            context.pending = false;
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(session_end_proc), SESSION_END_SUBCLASS_ID);
            drop(Box::from_raw(context as *mut SessionEndContext));
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        _ => DefSubclassProc(hwnd, message, wparam, lparam),
    }
}

const WAIT_FOREVER: u32 = u32::MAX;

/// Errors returned by the direct Windows process boundary.  There is
/// intentionally no `Command` fallback variant: a run is never allowed to
/// continue outside its Job Object after this adapter has been selected.
#[derive(Debug)]
pub enum WindowsExecutionError {
    Shell(ShellError),
    Win32(String),
    Io(std::io::Error),
    ProcessStillAlive,
    InvalidHandle(&'static str),
}

impl std::fmt::Display for WindowsExecutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shell(error) => error.fmt(formatter),
            Self::Win32(error) => formatter.write_str(error),
            Self::Io(error) => write!(formatter, "Windows process I/O failed: {error}"),
            Self::ProcessStillAlive => {
                formatter.write_str("Windows process tree did not terminate before the deadline")
            }
            Self::InvalidHandle(handle) => write!(formatter, "invalid Windows {handle} handle"),
        }
    }
}

impl std::error::Error for WindowsExecutionError {}

impl From<ShellError> for WindowsExecutionError {
    fn from(error: ShellError) -> Self {
        Self::Shell(error)
    }
}

impl From<std::io::Error> for WindowsExecutionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug)]
struct OwnedWindowsHandle {
    handle: HANDLE,
}

impl OwnedWindowsHandle {
    fn empty() -> Self {
        Self {
            handle: HANDLE::default(),
        }
    }

    fn new(handle: HANDLE, name: &'static str) -> Result<Self, WindowsExecutionError> {
        if handle.is_invalid() {
            return Err(WindowsExecutionError::InvalidHandle(name));
        }
        Ok(Self { handle })
    }

    fn raw(&self) -> HANDLE {
        self.handle
    }

    fn take_raw(&mut self, name: &'static str) -> Result<HANDLE, WindowsExecutionError> {
        if self.handle.is_invalid() {
            return Err(WindowsExecutionError::InvalidHandle(name));
        }
        Ok(std::mem::take(&mut self.handle))
    }

    fn close(&mut self) {
        if !self.handle.is_invalid() {
            // Each handle is owned by exactly one OwnedWindowsHandle.  The
            // field is invalidated before calling CloseHandle so a later
            // error path cannot close it a second time.
            let handle = std::mem::take(&mut self.handle);
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
    }
}

impl Drop for OwnedWindowsHandle {
    fn drop(&mut self) {
        self.close();
    }
}

// Kernel handles can safely move to the blocking wait/termination task.  The
// wrapper owns each handle and serializes close through Drop.
unsafe impl Send for OwnedWindowsHandle {}
unsafe impl Sync for OwnedWindowsHandle {}

#[derive(Debug)]
struct ProcessAttributeList {
    storage: Vec<MaybeUninit<usize>>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl ProcessAttributeList {
    fn with_handles(handles: &[HANDLE]) -> Result<Self, WindowsExecutionError> {
        let mut size = 0usize;
        // The first call reports the required size through `size`.  Windows
        // returns ERROR_INSUFFICIENT_BUFFER here, which is expected.
        unsafe {
            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
        }
        if size == 0 {
            return Err(last_error("InitializeProcThreadAttributeList(size)"));
        }
        let words = size.div_ceil(size_of::<usize>());
        let mut storage = vec![MaybeUninit::<usize>::uninit(); words];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        unsafe {
            if let Err(error) = InitializeProcThreadAttributeList(Some(list), 1, None, &mut size) {
                return Err(win32_error("InitializeProcThreadAttributeList", error));
            }
            if let Err(error) = UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr().cast()),
                size_of_val(handles),
                None,
                None,
            ) {
                DeleteProcThreadAttributeList(list);
                return Err(win32_error("UpdateProcThreadAttribute(HANDLE_LIST)", error));
            }
        }
        Ok(Self { storage, list })
    }

    fn raw(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.list
    }
}

impl Drop for ProcessAttributeList {
    fn drop(&mut self) {
        if !self.list.is_invalid() {
            unsafe { DeleteProcThreadAttributeList(self.list) };
        }
        // Keep the backing allocation alive until the attribute list has been
        // deleted.  Reading this field also documents that it owns the list's
        // storage rather than relying on a temporary buffer.
        let _ = self.storage.len();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsProcessIdentity {
    pub pid: u32,
    /// UTC epoch milliseconds read from the process creation FILETIME.  The
    /// value is persisted with the PID so PID reuse cannot be mistaken for
    /// the original process.
    pub created_at: i64,
}

impl WindowsProcessIdentity {
    /// PID equality alone is insufficient because Windows may reuse a PID
    /// after the original process exits.  Startup/stop callers must compare
    /// both fields obtained from a fresh process handle.
    pub fn matches(&self, observed: &Self) -> bool {
        self.pid == observed.pid && self.created_at == observed.created_at
    }
}

#[derive(Debug)]
pub struct WindowsChild {
    process: OwnedWindowsHandle,
    job: OwnedWindowsHandle,
    stdout_read: OwnedWindowsHandle,
    stderr_read: OwnedWindowsHandle,
    identity: WindowsProcessIdentity,
}

unsafe impl Send for WindowsChild {}
unsafe impl Sync for WindowsChild {}

impl WindowsChild {
    pub fn identity(&self) -> WindowsProcessIdentity {
        self.identity
    }

    pub fn stdout_handle(&self) -> HANDLE {
        self.stdout_read.raw()
    }

    pub fn stderr_handle(&self) -> HANDLE {
        self.stderr_read.raw()
    }

    /// Transfer the parent read end to a blocking reader exactly once. The
    /// `File` becomes the sole owner of the kernel handle; the child wrapper
    /// is left with an invalid slot and therefore cannot double-close it.
    pub fn take_stdout_file(&mut self) -> Result<std::fs::File, WindowsExecutionError> {
        let handle = self.stdout_read.take_raw("stdout pipe")?;
        Ok(unsafe { std::fs::File::from_raw_handle(handle.0) })
    }

    pub fn take_stderr_file(&mut self) -> Result<std::fs::File, WindowsExecutionError> {
        let handle = self.stderr_read.take_raw("stderr pipe")?;
        Ok(unsafe { std::fs::File::from_raw_handle(handle.0) })
    }

    /// Wait for the process root.  The Job Object remains owned by this
    /// handle until the caller drops the child, after log readers have flushed.
    pub fn wait(&self, timeout: Option<Duration>) -> Result<Option<u32>, WindowsExecutionError> {
        let milliseconds = timeout.map(duration_to_millis).unwrap_or(WAIT_FOREVER);
        let result = unsafe { WaitForSingleObject(self.process.raw(), milliseconds) };
        if result == WAIT_TIMEOUT {
            return Ok(None);
        }
        if result != WAIT_OBJECT_0 {
            return Err(last_error("WaitForSingleObject(process)"));
        }
        let mut exit_code = 0u32;
        unsafe { GetExitCodeProcess(self.process.raw(), &mut exit_code) }
            .map_err(|error| win32_error("GetExitCodeProcess", error))?;
        Ok(Some(exit_code))
    }

    /// Terminate the complete process tree and wait for the Job Object.  A
    /// root process that exits before one of its descendants cannot make
    /// cleanup appear complete.  The kill-on-close limit is retained as the
    /// final crash/drop safety net.
    pub fn terminate_and_wait(&self, timeout: Duration) -> Result<u32, WindowsExecutionError> {
        unsafe { TerminateJobObject(self.job.raw(), 1) }
            .map_err(|error| win32_error("TerminateJobObject", error))?;
        let Some(()) = wait_for_handle(self.job.raw(), timeout)? else {
            return Err(WindowsExecutionError::ProcessStillAlive);
        };
        // A signalled job has no active processes.  Read the root exit code
        // without spending another part of the caller's deadline.
        let Some(exit_code) = self.wait(Some(Duration::ZERO))? else {
            return Err(WindowsExecutionError::ProcessStillAlive);
        };
        Ok(exit_code)
    }
}

fn wait_for_handle(handle: HANDLE, timeout: Duration) -> Result<Option<()>, WindowsExecutionError> {
    let result = unsafe { WaitForSingleObject(handle, duration_to_millis(timeout)) };
    if result == WAIT_TIMEOUT {
        return Ok(None);
    }
    if result != WAIT_OBJECT_0 {
        return Err(last_error("WaitForSingleObject"));
    }
    Ok(Some(()))
}

/// Spawn a Windows job through the explicit CreateProcessW contract:
/// suspended creation, inherited EOF stdin-read plus stdout/stderr write
/// handles only, Job Object assignment, then resume. Any failure after process
/// creation terminates and reaps the suspended child before returning; it
/// never falls back to an unsupervised process.
pub fn spawn(
    command: &str,
    cwd: Option<&Path>,
    environment: &BTreeMap<String, String>,
) -> Result<WindowsChild, WindowsExecutionError> {
    let shell_command = shell::build_windows_shell_command(command)?;
    shell::validate_environment(environment)?;
    if cwd.is_some_and(|path| path.as_os_str().encode_wide().any(|unit| unit == 0)) {
        return Err(WindowsExecutionError::Shell(ShellError::NulByte("cwd")));
    }

    let mut stdout_read = OwnedWindowsHandle::empty();
    let mut stdout_write = OwnedWindowsHandle::empty();
    let mut stderr_read = OwnedWindowsHandle::empty();
    let mut stderr_write = OwnedWindowsHandle::empty();
    let mut stdin_read = OwnedWindowsHandle::empty();
    let mut stdin_write = OwnedWindowsHandle::empty();
    // The child inherits only `stdin_read`; the parent owns `stdin_write`
    // until CreateProcessW returns and then closes it to deliver EOF.
    create_pipe_pair(&mut stdin_read, &mut stdin_write)?;
    create_pipe_pair(&mut stdout_read, &mut stdout_write)?;
    create_pipe_pair(&mut stderr_read, &mut stderr_write)?;
    unsafe {
        SetHandleInformation(stdout_read.raw(), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
            .map_err(|error| win32_error("SetHandleInformation(stdout)", error))?;
        SetHandleInformation(stderr_read.raw(), HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
            .map_err(|error| win32_error("SetHandleInformation(stderr)", error))?;
    }

    let job = create_kill_on_close_job()?;
    let child = create_suspended_process(
        &shell_command,
        cwd,
        environment,
        stdin_read.raw(),
        stdout_write.raw(),
        stderr_write.raw(),
    );
    // None of the anonymous-pipe ends used for stdio are retained by the
    // parent after CreateProcessW returns. The child owns the inherited
    // handles; each parent-side OwnedWindowsHandle is invalidated exactly once
    // here, including the stdin write end that deliberately produces EOF.
    stdin_read.close();
    stdin_write.close();
    stdout_write.close();
    stderr_write.close();
    let process_info = match child {
        Ok(value) => value,
        Err(error) => {
            drop(job);
            return Err(error);
        }
    };
    let process = match OwnedWindowsHandle::new(process_info.hProcess, "process") {
        Ok(process) => process,
        Err(error) => {
            terminate_failed_start(process_info.hProcess);
            unsafe {
                if !process_info.hThread.is_invalid() {
                    let _ = CloseHandle(process_info.hThread);
                }
            }
            drop(job);
            return Err(error);
        }
    };
    let thread = match OwnedWindowsHandle::new(process_info.hThread, "thread") {
        Ok(thread) => thread,
        Err(error) => {
            terminate_failed_start(process.raw());
            drop(process);
            drop(job);
            return Err(error);
        }
    };
    let assign_result = unsafe { AssignProcessToJobObject(job.raw(), process.raw()) };
    if let Err(error) = assign_result {
        terminate_failed_start(process.raw());
        drop(thread);
        drop(process);
        drop(job);
        return Err(win32_error("AssignProcessToJobObject", error));
    }
    let resume_result = unsafe { ResumeThread(thread.raw()) };
    drop(thread);
    if resume_result == u32::MAX {
        terminate_failed_start(process.raw());
        drop(process);
        drop(job);
        return Err(last_error("ResumeThread"));
    }

    let identity = match process_creation_identity(process_info.dwProcessId, process.raw()) {
        Ok(identity) => identity,
        Err(error) => {
            let _ = unsafe { TerminateJobObject(job.raw(), 1) };
            let _ = wait_for_handle(process.raw(), Duration::from_secs(5));
            drop(process);
            drop(job);
            return Err(error);
        }
    };
    Ok(WindowsChild {
        process,
        job,
        stdout_read,
        stderr_read,
        identity,
    })
}

fn create_pipe_pair(
    read: &mut OwnedWindowsHandle,
    write: &mut OwnedWindowsHandle,
) -> Result<(), WindowsExecutionError> {
    let mut read_handle = HANDLE::default();
    let mut write_handle = HANDLE::default();
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: true.into(),
    };
    let result = unsafe {
        CreatePipe(
            &mut read_handle,
            &mut write_handle,
            Some(&attributes as *const SECURITY_ATTRIBUTES),
            0,
        )
    };
    if let Err(error) = result {
        unsafe {
            if !read_handle.is_invalid() {
                let _ = CloseHandle(read_handle);
            }
            if !write_handle.is_invalid() {
                let _ = CloseHandle(write_handle);
            }
        }
        return Err(win32_error("CreatePipe", error));
    }
    read.handle = read_handle;
    write.handle = write_handle;
    Ok(())
}

fn create_kill_on_close_job() -> Result<OwnedWindowsHandle, WindowsExecutionError> {
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|error| win32_error("CreateJobObjectW", error))?;
    let job = OwnedWindowsHandle::new(job, "Job Object")?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .map_err(|error| win32_error("SetInformationJobObject", error))?;
    }
    Ok(job)
}

fn create_suspended_process(
    shell_command: &shell::WindowsShellCommand,
    cwd: Option<&Path>,
    environment: &BTreeMap<String, String>,
    stdin_read: HANDLE,
    stdout_write: HANDLE,
    stderr_write: HANDLE,
) -> Result<PROCESS_INFORMATION, WindowsExecutionError> {
    let mut command_line = wide_nul(&shell_command.command_line);
    let application_name = wide_nul(&shell_command.application_name);
    let current_directory = cwd.map(|path| wide_os_nul(path.as_os_str()));
    let mut environment_block = build_environment_block(environment)?;
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdOutput = stdout_write;
    startup.StartupInfo.hStdError = stderr_write;
    startup.StartupInfo.hStdInput = stdin_read;
    let inherited = [stdin_read, stdout_write, stderr_write];
    let attributes = ProcessAttributeList::with_handles(&inherited)?;
    startup.lpAttributeList = attributes.raw();
    let mut process_info = PROCESS_INFORMATION::default();
    let mut flags = CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT;
    if !environment_block.is_empty() {
        flags |= CREATE_UNICODE_ENVIRONMENT;
    }
    let current_directory = current_directory
        .as_ref()
        .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr()));
    let created = unsafe {
        CreateProcessW(
            PCWSTR(application_name.as_ptr()),
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            true,
            flags,
            if environment_block.is_empty() {
                None
            } else {
                Some(environment_block.as_ptr().cast())
            },
            current_directory,
            (&startup as *const STARTUPINFOEXW).cast(),
            &mut process_info,
        )
    };
    environment_block.zeroize();
    if let Err(error) = created {
        return Err(win32_error("CreateProcessW", error));
    }
    Ok(process_info)
}

fn build_environment_block(
    overrides: &BTreeMap<String, String>,
) -> Result<Zeroizing<Vec<u16>>, WindowsExecutionError> {
    if overrides.is_empty() {
        return Ok(Zeroizing::new(Vec::new()));
    }
    let mut environment = Vec::<(OsString, OsString)>::new();
    for (key, value) in std::env::vars_os() {
        let key_text = key.to_string_lossy();
        if !overrides
            .keys()
            .any(|override_key| key_text.eq_ignore_ascii_case(override_key))
        {
            environment.push((key, value));
        }
    }
    for (key, value) in overrides {
        environment.push((OsString::from(key), OsString::from(value)));
    }
    // CreateProcessW requires a sorted Unicode environment block. Windows
    // environment names are case-insensitive, so override removal and sort
    // order use the same folded representation.
    environment.sort_by_cached_key(|(key, _)| key.to_string_lossy().to_uppercase());
    let mut block = Zeroizing::new(Vec::new());
    for (key, value) in environment {
        let mut entry = key;
        entry.push("=");
        entry.push(value);
        if entry.to_string_lossy().contains('\0') {
            return Err(WindowsExecutionError::Shell(ShellError::NulByte(
                "environment",
            )));
        }
        block.extend(entry.encode_wide());
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

fn process_creation_identity(
    pid: u32,
    process: HANDLE,
) -> Result<WindowsProcessIdentity, WindowsExecutionError> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) }
        .map_err(|error| win32_error("GetProcessTimes", error))?;
    let ticks = (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    const WINDOWS_TO_UNIX_EPOCH_100NS: u64 = 11644473600 * 10_000_000;
    let epoch_ticks = ticks
        .checked_sub(WINDOWS_TO_UNIX_EPOCH_100NS)
        .ok_or_else(|| {
            WindowsExecutionError::Win32("process creation time precedes Unix epoch".to_owned())
        })?;
    Ok(WindowsProcessIdentity {
        pid,
        created_at: (epoch_ticks / 10_000) as i64,
    })
}

fn terminate_failed_start(process: HANDLE) {
    if process.is_invalid() {
        return;
    }
    unsafe {
        let _ = TerminateProcess(process, 1);
        let _ = WaitForSingleObject(process, 5_000);
    }
}

fn wide_nul(value: &str) -> Vec<u16> {
    OsString::from(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn wide_os_nul(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn duration_to_millis(duration: Duration) -> u32 {
    duration.as_millis().min(u128::from(u32::MAX - 1)) as u32
}

fn win32_error(context: &str, error: windows::core::Error) -> WindowsExecutionError {
    WindowsExecutionError::Win32(format!("{context} failed: {error}"))
}

fn last_error(context: &'static str) -> WindowsExecutionError {
    WindowsExecutionError::Win32(format!("{context} failed"))
}

#[cfg(test)]
mod execution_tests {
    use super::*;

    #[test]
    fn process_creation_contract_requires_suspended_hidden_extended_startup() {
        assert_eq!(CREATE_SUSPENDED.0, 0x0000_0004);
        assert_eq!(CREATE_NO_WINDOW.0, 0x0800_0000);
        assert_eq!(EXTENDED_STARTUPINFO_PRESENT.0, 0x0008_0000);
        assert_eq!(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, 0x0002_0002);
        assert_eq!(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0, 0x0000_2000);
    }

    #[test]
    fn environment_block_is_double_nul_terminated_and_overrides_inherit() {
        let block = build_environment_block(&BTreeMap::from([(
            String::from("DEVBOX_TEST_EXECUTION"),
            String::from("value"),
        )]))
        .unwrap();
        assert_eq!(block.last(), Some(&0));
        assert_eq!(block[block.len() - 2], 0);
        assert!(String::from_utf16_lossy(&block).contains("DEVBOX_TEST_EXECUTION=value"));
    }

    #[test]
    fn anonymous_stdio_pipe_parent_ends_are_invalidated_after_close() {
        let mut read = OwnedWindowsHandle::empty();
        let mut write = OwnedWindowsHandle::empty();
        create_pipe_pair(&mut read, &mut write).unwrap();
        assert!(!read.raw().is_invalid());
        assert!(!write.raw().is_invalid());

        read.close();
        write.close();
        assert!(read.raw().is_invalid());
        assert!(write.raw().is_invalid());

        // A repeated close is a no-op, which keeps every error path exactly
        // once-owned even when a caller performs defensive cleanup.
        read.close();
        write.close();
        assert!(read.raw().is_invalid());
        assert!(write.raw().is_invalid());
    }
}
