//! Bounded ownership for one native child process tree.
//!
//! Windows starts the root suspended, assigns it to a kill-on-close Job
//! Object, proves that the Job contains only that root, and resumes its sole
//! primary thread. Unix uses a private process group. The group cannot own a
//! malicious descendant which deliberately calls `setsid()`; that OS authority
//! limit is documented by the stdio contract rather than hidden.

use std::time::Duration;
use tokio::process::Child;
use tokio::time::Instant;

pub(crate) const CLEANUP_TIMEOUT: Duration = Duration::from_millis(750);
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
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(());
            };
            if resume_primary_thread(pid, job).is_err() {
                unsafe {
                    let _ = TerminateJobObject(job, 1);
                }
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
        self.terminate_until(child, Instant::now() + CLEANUP_TIMEOUT)
            .await
    }

    pub(crate) async fn terminate_until(&mut self, child: &mut Child, deadline: Instant) -> bool {
        if self.terminal_empty {
            return tokio::time::timeout_at(deadline, child.wait())
                .await
                .is_ok_and(|result| result.is_ok());
        }
        if !self.signal_termination(false) && self.authority_is_empty() != Some(true) {
            // start_kill only sends a signal; all waits share the original deadline.
            let _ = child.start_kill();
        }
        let now = Instant::now();
        let grace = now + deadline.saturating_duration_since(now) / 2;
        let mut root_gone = tokio::time::timeout_at(grace, child.wait())
            .await
            .is_ok_and(|result| result.is_ok());
        if !root_gone || self.authority_is_empty() != Some(true) {
            let _ = self.signal_termination(true);
            if !root_gone {
                let _ = child.start_kill();
                root_gone = tokio::time::timeout_at(deadline, child.wait())
                    .await
                    .is_ok_and(|result| result.is_ok());
            }
        }
        let tree_gone = wait_for_empty(deadline, {
            let tree = &mut *self;
            move || tree.authority_is_empty()
        })
        .await;
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

    pub(crate) async fn terminate_unassigned(child: &mut Child) -> bool {
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        let _ = child.start_kill();
        tokio::time::timeout_at(deadline, child.wait())
            .await
            .is_ok_and(|result| result.is_ok())
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

async fn wait_for_empty(deadline: Instant, mut probe: impl FnMut() -> Option<bool>) -> bool {
    loop {
        if probe() == Some(true) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep_until((Instant::now() + POLL_INTERVAL).min(deadline)).await;
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
        // Drop is only the signal/kill-on-close fallback, never proof of cleanup.
        if !self.terminal_empty {
            let _ = self.signal_termination(true);
        }
        unsafe {
            let _ = CloseHandle(self.job);
        }
    }
}

#[cfg(unix)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        if !self.terminal_empty {
            let _ = self.signal_termination(true);
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

    #[tokio::test]
    async fn unconfirmed_authority_yields_and_obeys_one_deadline() {
        use super::*;
        for state in [None, Some(false)] {
            let started = Instant::now();
            let deadline = started + Duration::from_millis(100);
            let (empty, heartbeat) = tokio::join!(wait_for_empty(deadline, || state), async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Instant::now()
            });
            assert!(!empty);
            assert!(heartbeat < deadline, "cleanup blocked the executor");
            assert!(started.elapsed() < Duration::from_millis(500));
        }
        assert!(wait_for_empty(Instant::now(), || Some(true)).await);
        assert!(!wait_for_empty(Instant::now(), || None).await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn force_escalation_keeps_time_to_reap_before_the_same_deadline() {
        use super::*;
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "trap '' TERM; printf 'ready\\n'; exec sleep 30"])
            .stdout(std::process::Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut tree = ProcessTree::assign(&child).unwrap();
        let mut ready = String::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            BufReader::new(child.stdout.take().unwrap()).read_line(&mut ready),
        )
        .await
        .unwrap()
        .unwrap();
        let started = Instant::now();
        assert!(tree.terminate(&mut child).await);
        assert!(started.elapsed() < CLEANUP_TIMEOUT + Duration::from_millis(500));
        assert!(child.try_wait().unwrap().is_some());
        assert!(tree.terminate(&mut child).await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unassigned_cleanup_reaps_and_drop_does_not_poll() {
        use super::*;
        let mut child = tokio::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let tree = ProcessTree::assign(&child).unwrap();
        let started = Instant::now();
        drop(tree);
        assert!(started.elapsed() < Duration::from_millis(200));
        assert!(ProcessTree::terminate_unassigned(&mut child).await);
        assert!(child.try_wait().unwrap().is_some());
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
