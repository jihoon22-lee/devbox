//! Bounded ownership for short-lived native probe processes.
//!
//! A `tokio::process::Child` only represents the root process. WSL and other
//! fixed probes may create helpers before the root exits, so killing the root
//! alone is not a sufficient timeout/cancellation boundary. Unix children are
//! put in a private process group before spawn; Windows children are created
//! suspended, assigned to a kill-on-close Job Object, and only then resumed.
//! Assignment failure is fail-closed and never runs a cleanup helper process
//! before the child has been dealt with directly.
//!
//! Unix process groups intentionally document one residual boundary: a
//! malicious descendant that calls `setsid()` can escape the private group.
//! Workbench has no authority over that new session, so the bounded group
//! disappearance check reports only what the private group proves.

use std::thread;
use std::time::{Duration, Instant};
use tokio::process::Child;

const CLEANUP_TIMEOUT: Duration = Duration::from_millis(500);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[cfg(target_os = "windows")]
use std::mem::size_of;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NO_MORE_FILES, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// Own a root process and every descendant that remains in its probe tree.
pub(crate) struct ProcessTree {
    #[cfg(target_os = "windows")]
    job: HANDLE,
    #[cfg(unix)]
    process_group: i32,
    terminal_empty: bool,
}

// A Windows Job Object handle is process-wide rather than thread-affine: its
// query, termination, and CloseHandle operations may run on a different
// executor thread from the one that created it. ProcessTree owns the sole
// handle value and exposes mutation only through `&mut self`, so moving that
// ownership between Tokio worker threads cannot create concurrent access or a
// double close. windows-rs models HANDLE as a raw pointer and therefore does
// not derive Send automatically, so record this narrower platform guarantee.
#[cfg(target_os = "windows")]
unsafe impl Send for ProcessTree {}

impl ProcessTree {
    /// Assign an already spawned child to the platform process-tree boundary.
    /// On Windows the child must have been created with CREATE_SUSPENDED; this
    /// method assigns the Job Object and resumes its sole primary thread.
    /// Callers must kill/reap the root when this returns `Err`.
    pub(crate) fn assign(child: &Child) -> Result<Self, ()> {
        #[cfg(target_os = "windows")]
        {
            let raw_handle = child.raw_handle().ok_or(())?;
            let process = HANDLE(raw_handle);
            let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|_| ())?;
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
                return Err(());
            }
            if unsafe { AssignProcessToJobObject(job, process) }.is_err() {
                unsafe {
                    let _ = TerminateJobObject(job, 1);
                    let _ = CloseHandle(job);
                }
                return Err(());
            }
            // The suspended root is the only process allowed in the Job at
            // this point. Refuse to use a PID thread snapshot if accounting
            // cannot prove that exact root is still present.
            if query_job_active_processes(job) != Some(1) {
                unsafe {
                    let _ = TerminateJobObject(job, 1);
                    let _ = CloseHandle(job);
                }
                return Err(());
            }
            let Some(pid) = child.id() else {
                unsafe {
                    let _ = TerminateJobObject(job, 1);
                }
                let _ = wait_for_job_empty(job, CLEANUP_TIMEOUT);
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(());
            };
            if resume_primary_thread(pid, job).is_err() {
                // The process is still suspended or only partially resumed;
                // terminate through the exact Job authority before closing it.
                unsafe {
                    let _ = TerminateJobObject(job, 1);
                }
                let _ = wait_for_job_empty(job, CLEANUP_TIMEOUT);
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(());
            }
            Ok(Self {
                job,
                terminal_empty: false,
            })
        }

        #[cfg(unix)]
        {
            // The corresponding Command uses `process_group(0)`, making the
            // child its own group leader. A negative group id then targets
            // only this probe and descendants which remain in that group.
            let process_group = i32::try_from(child.id().ok_or(())?).map_err(|_| ())?;
            Ok(Self {
                process_group,
                terminal_empty: false,
            })
        }

        #[cfg(not(any(unix, target_os = "windows")))]
        {
            let _ = child;
            Ok(Self {
                terminal_empty: false,
            })
        }
    }

    /// Kill the complete tree and reap the root within a finite cleanup
    /// window. The tree remains owned until this method returns so
    /// descendants cannot outlive the command's guard.
    pub(crate) async fn terminate(&mut self, child: &mut Child) -> bool {
        let tree_gone = self.terminate_descendants();
        let root_gone = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait())
            .await
            .is_ok_and(|result| result.is_ok());
        tree_gone && root_gone
    }

    /// Terminate descendants after a normal root exit as well. A helper which
    /// inherited a pipe must never survive a successful-looking probe. The
    /// return value proves full Job/group disappearance, not merely that a
    /// signal was sent.
    pub(crate) fn terminate_descendants(&mut self) -> bool {
        if self.terminal_empty {
            return true;
        }
        if self.authority_is_empty() == Some(true) {
            self.terminal_empty = true;
            return true;
        }

        #[cfg(target_os = "windows")]
        {
            if unsafe { TerminateJobObject(self.job, 1) }.is_err() {
                if self.authority_is_empty() == Some(true) {
                    self.terminal_empty = true;
                    return true;
                }
                return false;
            }
        }

        #[cfg(unix)]
        {
            if !signal_group(self.process_group, libc::SIGTERM)
                && self.authority_is_empty() != Some(true)
            {
                return false;
            }
        }

        let deadline = Instant::now()
            .checked_add(CLEANUP_TIMEOUT / 2)
            .unwrap_or_else(Instant::now);
        if self.wait_authority_empty(deadline) {
            self.terminal_empty = true;
            return true;
        }
        if self.authority_is_empty() == Some(true) {
            self.terminal_empty = true;
            return true;
        }

        #[cfg(unix)]
        if !signal_group(self.process_group, libc::SIGKILL)
            && self.authority_is_empty() != Some(true)
        {
            return false;
        }

        #[cfg(target_os = "windows")]
        if unsafe { TerminateJobObject(self.job, 1) }.is_err()
            && self.authority_is_empty() != Some(true)
        {
            return false;
        }

        if self.wait_authority_empty(
            Instant::now()
                .checked_add(CLEANUP_TIMEOUT)
                .unwrap_or_else(Instant::now),
        ) {
            self.terminal_empty = true;
            true
        } else {
            false
        }
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

    #[cfg(target_os = "windows")]
    fn authority_is_empty(&self) -> Option<bool> {
        query_job_active_processes(self.job).map(|active| active == 0)
    }

    #[cfg(unix)]
    fn authority_is_empty(&self) -> Option<bool> {
        let result = unsafe { libc::kill(-self.process_group, 0) };
        if result == 0 {
            return Some(false);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Some(true),
            Some(libc::EPERM) => Some(false),
            _ => None,
        }
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    fn authority_is_empty(&self) -> Option<bool> {
        Some(true)
    }

    /// Best-effort tree cleanup for the narrow window before a platform tree
    /// owner could be attached. This only kills/reaps the suspended root;
    /// because no user code has run yet, no helper can exist outside it.
    pub(crate) async fn terminate_unassigned(child: &mut Child) {
        let _ = child.kill().await;
        let _ = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await;
    }
}

#[cfg(unix)]
fn signal_group(process_group: i32, signal: i32) -> bool {
    process_group > 0 && unsafe { libc::kill(-process_group, signal) } == 0
}

#[cfg(target_os = "windows")]
fn query_job_active_processes(job: HANDLE) -> Option<u32> {
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

#[cfg(target_os = "windows")]
fn wait_for_job_empty(job: HANDLE, timeout: Duration) -> bool {
    let deadline = Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(Instant::now);
    loop {
        match query_job_active_processes(job) {
            Some(0) => return true,
            Some(_) | None if Instant::now() >= deadline => return false,
            Some(_) | None => thread::sleep(POLL_INTERVAL),
        }
    }
}

#[cfg(target_os = "windows")]
fn resume_primary_thread(pid: u32, job: HANDLE) -> Result<(), ()> {
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
    // Recheck the exact Job immediately before the one and only resume so a
    // terminated root cannot turn a recycled PID into a resume target.
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
    (previous_suspend_count == 1).then_some(()).ok_or(())
}

#[cfg(target_os = "windows")]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE is the final crash/drop backstop;
        // make one bounded accounting attempt before releasing the handle.
        if !self.terminal_empty {
            if self.authority_is_empty() == Some(true) {
                self.terminal_empty = true;
            } else {
                unsafe {
                    let _ = TerminateJobObject(self.job, 1);
                }
                if wait_for_job_empty(self.job, CLEANUP_TIMEOUT) {
                    self.terminal_empty = true;
                }
            }
        }
        unsafe {
            let _ = CloseHandle(self.job);
        }
    }
}

#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // A cancelled future can drop the tree before its normal cleanup
        // branch runs. Keep the private group as a bounded synchronous
        // backstop, while respecting the sticky terminal-empty state.
        if self.terminal_empty {
            return;
        }
        if self.authority_is_empty() == Some(true) {
            self.terminal_empty = true;
            return;
        }
        if signal_group(self.process_group, libc::SIGKILL)
            && self.wait_authority_empty(
                Instant::now()
                    .checked_add(CLEANUP_TIMEOUT)
                    .unwrap_or_else(Instant::now),
            )
        {
            self.terminal_empty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    #[test]
    fn process_tree_can_move_with_its_async_worker() {
        fn assert_send<T: Send>() {}
        assert_send::<super::ProcessTree>();
    }

    #[test]
    fn cleanup_window_is_finite() {
        assert!(super::CLEANUP_TIMEOUT.as_millis() > 0);
        assert!(super::CLEANUP_TIMEOUT.as_secs() < 1);
    }

    #[cfg(unix)]
    #[test]
    fn wait_for_group_empty_honors_an_expired_deadline() {
        let mut tree = super::ProcessTree {
            process_group: unsafe { libc::getpgrp() },
            terminal_empty: false,
        };
        let started = std::time::Instant::now();
        assert!(!tree.wait_authority_empty(started));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        // Do not let the Drop backstop signal this test process's own group.
        tree.terminal_empty = true;
    }
}
