//! Exact process-tree ownership for Workbench launches.
//!
//! `OwnedProcess` deliberately exposes no process handle, process-group id, or
//! persisted identity. Clones share the same platform authority through one
//! `Arc`; on Windows that authority is an exact kill-on-close Job Object and on
//! Unix it is the launch-time private process group. The root `Child` is
//! separately reaped by a weak background waiter so a dropped receipt never
//! leaves a Unix zombie, the waiter cannot keep an otherwise dropped receipt
//! alive indefinitely, and descendants are cleaned as soon as the root exits.

use std::process::{Child, Command};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const DROP_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Opaque authority for one Workbench-owned process tree.
///
/// Cloning this value shares the same launch-bound authority. It is
/// intentionally not serializable: callers must retain it in backend state
/// rather than turning a PID or native handle into an IPC credential. Windows
/// uses an exact Job handle; Unix uses a private numeric process group and
/// therefore retains the documented `setsid()`/group-ID residual boundary.
#[derive(Clone)]
pub struct OwnedProcess {
    inner: Arc<OwnedProcessInner>,
}

impl std::fmt::Debug for OwnedProcess {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OwnedProcess")
            .finish_non_exhaustive()
    }
}

impl PartialEq for OwnedProcess {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for OwnedProcess {}

impl OwnedProcess {
    /// Return the root PID for backend bookkeeping only.  Workbench never
    /// serializes this value to the renderer.
    pub fn pid(&self) -> u32 {
        self.inner.pid
    }

    /// Report whether any member of the owned process tree remains alive.
    /// A failed native accounting query is conservative and reports `true`.
    pub fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }

    /// Terminate the complete owned tree and wait for full tree disappearance
    /// within `timeout`.  Once terminal emptiness is observed, this receipt
    /// never signals the authority again.
    pub fn terminate(&self, timeout: Duration) -> bool {
        self.inner.terminate(timeout)
    }
}

struct OwnedProcessInner {
    pid: u32,
    child: Mutex<Option<Child>>,
    state: Mutex<OwnedState>,
    authority: PlatformAuthority,
}

#[derive(Default)]
struct OwnedState {
    terminal_empty: bool,
}

#[cfg(unix)]
struct PlatformAuthority {
    process_group: i32,
}

#[cfg(windows)]
struct PlatformAuthority {
    job: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
// Windows HANDLE is an opaque kernel reference.  The authority is only
// accessed while the shared state mutex is held, and its Drop runs after the
// final Arc reference, so moving it between the reaper and caller is safe.
unsafe impl Send for PlatformAuthority {}

#[cfg(windows)]
unsafe impl Sync for PlatformAuthority {}

#[cfg(not(any(unix, windows)))]
struct PlatformAuthority;

impl OwnedProcessInner {
    fn is_alive(&self) -> bool {
        let Ok(mut state) = self.state.lock() else {
            // Poisoning means another cleanup path may have made progress;
            // never claim that an unknown tree is gone.
            return true;
        };
        if state.terminal_empty {
            return false;
        }
        match self.authority_is_empty() {
            Some(true) => {
                state.terminal_empty = true;
                false
            }
            Some(false) | None => true,
        }
    }

    fn terminate(&self, timeout: Duration) -> bool {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            // A poisoned state mutex must not disable the Drop cleanup
            // backstop. Recover the last state and continue conservatively.
            Err(poisoned) => poisoned.into_inner(),
        };
        if state.terminal_empty {
            return true;
        }
        if self.authority_is_empty() == Some(true) {
            state.terminal_empty = true;
            return true;
        }

        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);
        if !self.terminate_authority() {
            if self.authority_is_empty() == Some(true) {
                state.terminal_empty = true;
                return true;
            }
            return false;
        }

        // TERM gives ordinary applications a bounded graceful exit window.
        // The second half escalates through the same exact authority.  The
        // authority is never signalled after `wait_authority_empty` observes
        // terminal emptiness because this mutex remains held throughout.
        let term_deadline = Instant::now()
            .checked_add(timeout / 2)
            .unwrap_or(deadline)
            .min(deadline);
        if self.wait_authority_empty(term_deadline) {
            state.terminal_empty = true;
            return true;
        }
        if self.authority_is_empty() == Some(true) {
            state.terminal_empty = true;
            return true;
        }
        if !self.escalate_authority() {
            if self.authority_is_empty() == Some(true) {
                state.terminal_empty = true;
                return true;
            }
            return false;
        }
        if self.wait_authority_empty(deadline) {
            state.terminal_empty = true;
            return true;
        }
        false
    }

    #[cfg(unix)]
    fn authority_is_empty(&self) -> Option<bool> {
        let result = unsafe { libc::kill(-self.authority.process_group, 0) };
        if result == 0 {
            return Some(false);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Some(true),
            // EPERM still means a member exists but is not inspectable.
            Some(libc::EPERM) => Some(false),
            _ => None,
        }
    }

    #[cfg(windows)]
    fn authority_is_empty(&self) -> Option<bool> {
        query_job_active_processes(self.authority.job).map(|active| active == 0)
    }

    #[cfg(not(any(unix, windows)))]
    fn authority_is_empty(&self) -> Option<bool> {
        Some(self.child_is_gone())
    }

    #[cfg(unix)]
    fn terminate_authority(&self) -> bool {
        signal_process_group(self.authority.process_group, libc::SIGTERM)
    }

    #[cfg(windows)]
    fn terminate_authority(&self) -> bool {
        unsafe { windows::Win32::System::JobObjects::TerminateJobObject(self.authority.job, 1) }
            .is_ok()
    }

    #[cfg(not(any(unix, windows)))]
    fn terminate_authority(&self) -> bool {
        self.kill_root()
    }

    #[cfg(unix)]
    fn escalate_authority(&self) -> bool {
        signal_process_group(self.authority.process_group, libc::SIGKILL)
    }

    #[cfg(windows)]
    fn escalate_authority(&self) -> bool {
        // Job termination is already forceful on Windows.  Keep escalation as
        // an idempotent exact-authority operation for the shared algorithm.
        unsafe { windows::Win32::System::JobObjects::TerminateJobObject(self.authority.job, 1) }
            .is_ok()
    }

    #[cfg(not(any(unix, windows)))]
    fn escalate_authority(&self) -> bool {
        self.kill_root()
    }

    fn wait_authority_empty(&self, deadline: Instant) -> bool {
        loop {
            match self.authority_is_empty() {
                Some(true) => return true,
                Some(false) | None if Instant::now() >= deadline => return false,
                Some(false) | None => thread::sleep(POLL_INTERVAL),
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn child_is_gone(&self) -> bool {
        self.child
            .lock()
            .ok()
            .and_then(|child| child.as_ref().map(|_| false))
            .unwrap_or(true)
    }

    #[cfg(not(any(unix, windows)))]
    fn kill_root(&self) -> bool {
        let Ok(mut child) = self.child.lock() else {
            return false;
        };
        let Some(child) = child.as_mut() else {
            return true;
        };
        child.kill().is_ok()
    }
}

impl Drop for OwnedProcessInner {
    fn drop(&mut self) {
        // Closing a Windows Job Object is itself a kill-on-close backstop; the
        // explicit bounded termination also gives Unix groups a final TERM /
        // KILL attempt before the authority is released.
        let _ = self.terminate(DROP_CLEANUP_TIMEOUT);
        let mut child = match self.child.lock() {
            Ok(child) => child,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(child) = child.take() {
            reap_child_after_cleanup(child);
        }
    }
}

#[cfg(windows)]
impl Drop for PlatformAuthority {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.job);
        }
    }
}

/// Spawn an owned child after the caller has applied the executable and argv
/// boundary.  Windows creates it suspended, assigns/configures the Job Object,
/// then resumes exactly its sole primary thread before returning.
pub(crate) fn spawn_owned(mut command: Command) -> Result<OwnedProcess, String> {
    #[cfg(unix)]
    command.process_group(0);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
        let job = create_kill_on_close_job()?;
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                close_job(job);
                return Err("설치된 앱 실행에 실패했습니다".into());
            }
        };
        let pid = child.id();
        let process_handle = {
            use std::os::windows::io::AsRawHandle;
            windows::Win32::Foundation::HANDLE(child.as_raw_handle())
        };
        if unsafe {
            windows::Win32::System::JobObjects::AssignProcessToJobObject(job, process_handle)
        }
        .is_err()
        {
            terminate_unassigned_child(&mut child);
            close_job(job);
            return Err("설치된 앱 실행에 실패했습니다".into());
        }
        // Before consulting a PID-based thread snapshot, prove that the Job
        // still contains exactly the newly-created suspended root. If process
        // creation failed and the PID was recycled in this narrow window,
        // this check prevents resuming an unrelated process.
        if query_job_active_processes(job) != Some(1) {
            unsafe {
                let _ = windows::Win32::System::JobObjects::TerminateJobObject(job, 1);
            }
            terminate_unassigned_child(&mut child);
            close_job(job);
            return Err("설치된 앱 실행에 실패했습니다".into());
        }
        if resume_primary_thread(pid, job).is_err() {
            unsafe {
                let _ = windows::Win32::System::JobObjects::TerminateJobObject(job, 1);
            }
            let _ = child.wait();
            close_job(job);
            return Err("설치된 앱 실행에 실패했습니다".into());
        }
        let inner = Arc::new(OwnedProcessInner {
            pid,
            child: Mutex::new(Some(child)),
            state: Mutex::new(OwnedState::default()),
            authority: PlatformAuthority { job },
        });
        if spawn_owned_reaper(&inner).is_err() {
            let _ = inner.terminate(DROP_CLEANUP_TIMEOUT);
            return Err("설치된 앱 실행에 실패했습니다".into());
        }
        Ok(OwnedProcess { inner })
    }

    #[cfg(not(windows))]
    {
        let child = command
            .spawn()
            .map_err(|_| "설치된 앱 실행에 실패했습니다".to_string())?;
        let pid = child.id();
        let process_group = match i32::try_from(pid) {
            Ok(process_group) => process_group,
            Err(_) => {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Err("설치된 앱 실행에 실패했습니다".into());
            }
        };
        let inner = Arc::new(OwnedProcessInner {
            pid,
            child: Mutex::new(Some(child)),
            state: Mutex::new(OwnedState::default()),
            authority: PlatformAuthority { process_group },
        });
        if spawn_owned_reaper(&inner).is_err() {
            let _ = inner.terminate(DROP_CLEANUP_TIMEOUT);
            return Err("설치된 앱 실행에 실패했습니다".into());
        }
        Ok(OwnedProcess { inner })
    }
}

/// Move a legacy detached child to a waiter.  Dropping a `Child` without
/// waiting is a Unix zombie leak, so all legacy launch APIs use this helper
/// while preserving their immediate PID-returning behavior.
pub(crate) fn reap_detached(child: Child) {
    let slot = Arc::new(Mutex::new(Some(child)));
    let worker_slot = Arc::clone(&slot);
    if let Err(error) = thread::Builder::new()
        .name("devbox-launch-reaper".into())
        .spawn(move || {
            let mut child = match worker_slot.lock() {
                Ok(child) => child,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(mut child) = child.take() {
                let _ = child.wait();
            }
        })
    {
        // Thread creation is exceptionally unlikely; if it fails, wait here
        // rather than returning a successful launch with an unreapable child.
        let _ = error;
        let mut child = match slot.lock() {
            Ok(child) => child,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(mut child) = child.take() {
            let _ = child.wait();
        }
    }
}

fn spawn_owned_reaper(inner: &Arc<OwnedProcessInner>) -> Result<(), ()> {
    let weak = Arc::downgrade(inner);
    thread::Builder::new()
        .name("devbox-owned-launch-reaper".into())
        .spawn(move || reap_owned_child(weak))
        .map(|_| ())
        .map_err(|_| ())
}

fn reap_owned_child(weak: Weak<OwnedProcessInner>) {
    // Upgrade only long enough to move the root Child out.  The waiter then
    // holds no strong receipt reference while it blocks in wait(), allowing a
    // dropped OwnedProcess to run its authority cleanup immediately.
    let child = weak.upgrade().and_then(|inner| {
        let mut child = match inner.child.lock() {
            Ok(child) => child,
            Err(poisoned) => poisoned.into_inner(),
        };
        child.take()
    });
    let Some(mut child) = child else {
        return;
    };
    let _ = child.wait();
    // A direct GUI root is the lifetime anchor for its helpers. Clean any
    // remaining Job/group members immediately after the root exits instead of
    // leaving a numeric Unix group idle until a later UI Stop. The weak upgrade
    // is deliberately short-lived; if every receipt was already dropped, the
    // inner Drop owns the same cleanup path.
    if let Some(inner) = weak.upgrade() {
        let _ = inner.terminate(DROP_CLEANUP_TIMEOUT);
    }
}

fn reap_child_after_cleanup(child: Child) {
    let slot = Arc::new(Mutex::new(Some(child)));
    let worker_slot = Arc::clone(&slot);
    if let Err(error) = thread::Builder::new()
        .name("devbox-launch-cleanup-reaper".into())
        .spawn(move || {
            let mut child = match worker_slot.lock() {
                Ok(child) => child,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(mut child) = child.take() {
                let _ = child.wait();
            }
        })
    {
        let _ = error;
        let mut child = match slot.lock() {
            Ok(child) => child,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(mut child) = child.take() {
            let _ = child.wait();
        }
    }
}

#[cfg(unix)]
fn signal_process_group(process_group: i32, signal: i32) -> bool {
    if process_group <= 0 {
        return false;
    }
    if unsafe { libc::kill(-process_group, signal) } == 0 {
        return true;
    }
    // ESRCH is terminal only after the caller's accounting check; report it
    // as a non-send so `terminate` performs that final full-group query.
    false
}

#[cfg(windows)]
fn query_job_active_processes(job: windows::Win32::Foundation::HANDLE) -> Option<u32> {
    use std::mem::size_of;
    use windows::Win32::System::JobObjects::{
        JobObjectBasicAccountingInformation, QueryInformationJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(job),
            JobObjectBasicAccountingInformation,
            (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            None,
        )
    }
    .ok()
    .map(|_| accounting.ActiveProcesses)
}

#[cfg(windows)]
fn create_kill_on_close_job() -> Result<windows::Win32::Foundation::HANDLE, String> {
    use std::mem::size_of;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|_| "설치된 앱 실행에 실패했습니다".to_string())?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    }
    .is_err()
    {
        unsafe {
            let _ = CloseHandle(job);
        }
        return Err("설치된 앱 실행에 실패했습니다".into());
    }
    Ok(job)
}

#[cfg(windows)]
fn close_job(job: windows::Win32::Foundation::HANDLE) {
    unsafe {
        let _ = windows::Win32::Foundation::CloseHandle(job);
    }
}

#[cfg(windows)]
fn terminate_unassigned_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn resume_primary_thread(pid: u32, job: windows::Win32::Foundation::HANDLE) -> Result<(), ()> {
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NO_MORE_FILES};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(|_| ())?;
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut thread_id = None;
    if unsafe { Thread32First(snapshot, &mut entry) }.is_err() {
        unsafe {
            let _ = CloseHandle(snapshot);
        }
        return Err(());
    }
    loop {
        if entry.th32OwnerProcessID == pid && thread_id.replace(entry.th32ThreadID).is_some() {
            unsafe {
                let _ = CloseHandle(snapshot);
            }
            return Err(());
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        if unsafe { Thread32Next(snapshot, &mut entry) }.is_err() {
            let end = unsafe { GetLastError() } == ERROR_NO_MORE_FILES;
            unsafe {
                let _ = CloseHandle(snapshot);
            }
            if !end {
                return Err(());
            }
            break;
        }
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    let thread_id = thread_id.ok_or(())?;
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, thread_id) }.map_err(|_| ())?;
    // Recheck the exact Job immediately before the one and only resume. A
    // root that disappeared between the ToolHelp snapshot and this point
    // must not turn a recycled PID into a resume target.
    if query_job_active_processes(job) != Some(1) {
        unsafe {
            let _ = CloseHandle(thread);
        }
        return Err(());
    }
    let previous_suspend_count = unsafe { ResumeThread(thread) };
    unsafe {
        let _ = CloseHandle(thread);
    }
    // The CREATE_SUSPENDED primary thread must have exactly one suspension
    // count.  A zero count or a nested/ambiguous count is fail-closed.
    (previous_suspend_count == 1).then_some(()).ok_or(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::*;

    #[cfg(unix)]
    fn fixture(command: &str) -> OwnedProcess {
        let mut command_builder = Command::new("sh");
        command_builder.args(["-c", command]);
        spawn_owned(command_builder).expect("owned fixture spawn")
    }

    #[cfg(unix)]
    #[test]
    fn clones_share_exact_authority_and_terminal_state() {
        let process = fixture("sleep 30");
        let clone = process.clone();
        assert_eq!(process, clone);
        assert!(process.is_alive());
        assert!(clone.terminate(Duration::from_secs(2)));
        assert!(!process.is_alive());
        assert!(!clone.is_alive());
        // Terminal emptiness is sticky: a second call must not signal again.
        assert!(process.terminate(Duration::ZERO));
    }

    #[cfg(unix)]
    #[test]
    fn root_reaper_does_not_leave_a_zombie_after_receipt_drop() {
        let process = fixture("exit 0");
        let pid = process.pid();
        std::thread::sleep(Duration::from_millis(50));
        drop(process);
        // The assertion is intentionally bounded and only checks the local
        // fixture's /proc entry when available; macOS has no /proc contract.
        #[cfg(target_os = "linux")]
        {
            for _ in 0..20 {
                if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn root_exit_cleans_descendants_before_a_later_stop() {
        let process = fixture("sleep 30 &");
        for _ in 0..300 {
            if !process.is_alive() {
                assert!(process.terminate(Duration::ZERO));
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !process.is_alive(),
            "root reaper must empty the owned group"
        );
    }
}
