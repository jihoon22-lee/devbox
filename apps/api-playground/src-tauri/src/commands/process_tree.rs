//! Bounded ownership for one native MCP stdio process tree.
//!
//! Windows starts the root suspended, assigns it to a kill-on-close Job
//! Object, proves that the Job contains only that root, and resumes its sole
//! primary thread. Unix uses a private process group. The group cannot own a
//! malicious descendant which deliberately calls `setsid()`; that OS authority
//! limit is documented by the stdio contract rather than hidden.

use std::thread;
use std::time::{Duration, Instant};
use tokio::process::Child;

const CLEANUP_TIMEOUT: Duration = Duration::from_millis(750);
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

pub(crate) struct ProcessTree {
    #[cfg(target_os = "windows")]
    job: HANDLE,
    #[cfg(unix)]
    process_group: i32,
    terminal_empty: bool,
}

// A Job handle is process-wide rather than thread-affine. This type owns the
// sole handle and only mutates it through `&mut self`.
#[cfg(target_os = "windows")]
unsafe impl Send for ProcessTree {}

impl ProcessTree {
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
            let process_group = i32::try_from(child.id().ok_or(())?).map_err(|_| ())?;
            if process_group <= 1 {
                return Err(());
            }
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

    pub(crate) async fn terminate(&mut self, child: &mut Child) -> bool {
        if self.terminal_empty {
            return tokio::time::timeout(CLEANUP_TIMEOUT, child.wait())
                .await
                .is_ok_and(|result| result.is_ok());
        }
        if !self.signal_termination(false) && self.authority_is_empty() != Some(true) {
            let _ = child.kill().await;
        }
        let mut root_gone = tokio::time::timeout(CLEANUP_TIMEOUT / 2, child.wait())
            .await
            .is_ok_and(|result| result.is_ok());
        if !root_gone {
            let _ = self.signal_termination(true);
            root_gone = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait())
                .await
                .is_ok_and(|result| result.is_ok());
        }
        if self.authority_is_empty() != Some(true) {
            let _ = self.signal_termination(true);
        }
        let tree_gone = self.wait_authority_empty(
            Instant::now()
                .checked_add(CLEANUP_TIMEOUT)
                .unwrap_or_else(Instant::now),
        );
        if tree_gone {
            self.terminal_empty = true;
        }
        tree_gone && root_gone
    }

    #[cfg(target_os = "windows")]
    fn signal_termination(&self, _force: bool) -> bool {
        unsafe { TerminateJobObject(self.job, 1) }.is_ok()
    }

    #[cfg(unix)]
    fn signal_termination(&self, force: bool) -> bool {
        signal_group(
            self.process_group,
            if force { libc::SIGKILL } else { libc::SIGTERM },
        )
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    fn signal_termination(&self, _force: bool) -> bool {
        true
    }

    pub(crate) fn terminate_descendants(&mut self) -> bool {
        if self.terminal_empty {
            return true;
        }
        if self.authority_is_empty() == Some(true) {
            self.terminal_empty = true;
            return true;
        }

        #[cfg(target_os = "windows")]
        if unsafe { TerminateJobObject(self.job, 1) }.is_err()
            && self.authority_is_empty() != Some(true)
        {
            return false;
        }

        #[cfg(unix)]
        if !signal_group(self.process_group, libc::SIGTERM)
            && self.authority_is_empty() != Some(true)
        {
            return false;
        }

        let deadline = Instant::now()
            .checked_add(CLEANUP_TIMEOUT / 2)
            .unwrap_or_else(Instant::now);
        if self.wait_authority_empty(deadline) {
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

    pub(crate) async fn terminate_unassigned(child: &mut Child) {
        let _ = child.kill().await;
        let _ = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await;
    }
}

#[cfg(unix)]
fn signal_group(process_group: i32, signal: i32) -> bool {
    process_group > 1 && unsafe { libc::kill(-process_group, signal) } == 0
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
        assert!(super::CLEANUP_TIMEOUT.as_secs() < 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminate_reaps_the_root_and_private_group_descendant() {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;

        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 30 & printf '%s\\n' $!; wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = command.spawn().unwrap();
        let root_pid = child.id().unwrap() as i32;
        let mut tree = super::ProcessTree::assign(&child).unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            BufReader::new(stdout).read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let descendant_pid = line.trim().parse::<i32>().unwrap();

        assert!(tree.terminate(&mut child).await);
        assert_eq!(unsafe { libc::kill(root_pid, 0) }, -1);
        assert_eq!(unsafe { libc::kill(descendant_pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}
