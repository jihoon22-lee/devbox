//! Windows process-tree ownership for local language-server children.
//!
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is set before the child is assigned.
//! The wrapper owns the job handle for the entire wait-task lifetime and also
//! exposes explicit termination for stop/crash paths.  Assignment failures are
//! returned to the caller; they never silently degrade into a process-only
//! session.

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::process::Child;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_TIMEOUT};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectAssociateCompletionPortInformation,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus};

use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

const JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO: u32 = 4;

pub(crate) struct WindowsJobObject {
    handle: HANDLE,
    completion_port: HANDLE,
    exited: AtomicBool,
}

// Kernel handles are process-wide and the Job Object APIs are safe to invoke
// from the async wait/stop tasks that share this ownership wrapper.
unsafe impl Send for WindowsJobObject {}
unsafe impl Sync for WindowsJobObject {}

impl WindowsJobObject {
    pub(crate) fn assign_to(child: &Child) -> Result<Self, String> {
        let process_handle = child
            .raw_handle()
            .ok_or_else(|| "child exited before its process handle was available".to_owned())?;
        let process_handle = HANDLE(process_handle);
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| format!("CreateJobObjectW failed: {error}"))?;

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configure = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if let Err(error) = configure {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(format!("SetInformationJobObject failed: {error}"));
        }

        let completion_port =
            // Pollers may migrate between Tokio threads. This port has no
            // blocking workers, so it must not reserve a single active thread.
            match unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, u32::MAX) } {
                Ok(port) => port,
                Err(error) => {
                    unsafe {
                        let _ = CloseHandle(handle);
                    }
                    return Err(format!("CreateIoCompletionPort failed: {error}"));
                }
            };
        let job = Self {
            handle,
            completion_port,
            exited: AtomicBool::new(false),
        };
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: handle.0,
            CompletionPort: completion_port,
        };
        unsafe {
            SetInformationJobObject(
                handle,
                JobObjectAssociateCompletionPortInformation,
                (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
            )
        }
        .map_err(|error| format!("job completion association failed: {error}"))?;
        if let Err(error) = unsafe { AssignProcessToJobObject(handle, process_handle) } {
            return Err(format!("AssignProcessToJobObject failed: {error}"));
        }
        // Command does not expose the primary thread handle. The suspended
        // child cannot create another thread before this assignment/resume.
        resume_suspended_process(child.id().ok_or("child process ID unavailable")?)?;
        Ok(job)
    }

    pub(crate) fn is_empty(&self) -> Result<bool, String> {
        // Accounting can reach zero before process handles become signaled.
        // A private port is associated before the suspended root is admitted;
        // ACTIVE_PROCESS_ZERO is the Windows tree-completion notification.
        if self.exited.load(Ordering::Acquire) {
            return Ok(true);
        }
        for _ in 0..256 {
            let mut message = 0;
            let mut key = 0;
            let mut overlapped = std::ptr::null_mut();
            match unsafe {
                GetQueuedCompletionStatus(
                    self.completion_port,
                    &mut message,
                    &mut key,
                    &mut overlapped,
                    0,
                )
            } {
                Ok(())
                    if key == self.handle.0 as usize
                        && message == JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO =>
                {
                    self.exited.store(true, Ordering::Release);
                    return Ok(true);
                }
                Ok(()) => {}
                Err(error)
                    if error.code() == windows::core::HRESULT::from_win32(WAIT_TIMEOUT.0) =>
                {
                    break
                }
                Err(error) => return Err(format!("job completion unavailable: {error}")),
            }
        }
        Ok(self.exited.load(Ordering::Acquire))
    }

    pub(crate) async fn terminate_and_wait(&self) -> Result<(), String> {
        self.terminate()?;
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if self.is_empty()? {
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| "process tree termination unconfirmed".to_owned())?
    }

    pub(crate) fn terminate(&self) -> Result<(), String> {
        unsafe { TerminateJobObject(self.handle, 1) }
            .map_err(|error| format!("TerminateJobObject failed: {error}"))
    }
}

fn resume_suspended_process(process_id: u32) -> Result<(), String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|_| "suspended child thread unavailable".to_owned())?;
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut thread_id = None;
    if unsafe { Thread32First(snapshot, &mut entry) }.is_ok() {
        loop {
            if entry.th32OwnerProcessID == process_id
                && thread_id.replace(entry.th32ThreadID).is_some()
            {
                unsafe {
                    let _ = CloseHandle(snapshot);
                }
                return Err("ambiguous suspended child threads".into());
            }
            if unsafe { Thread32Next(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    let thread = unsafe {
        OpenThread(
            THREAD_SUSPEND_RESUME,
            false,
            thread_id.ok_or("suspended child thread missing")?,
        )
    }
    .map_err(|_| "suspended child thread unavailable".to_owned())?;
    let previous_suspend_count = unsafe { ResumeThread(thread) };
    unsafe {
        let _ = CloseHandle(thread);
    }
    if previous_suspend_count == 1 {
        Ok(())
    } else {
        Err("unexpected child suspend count".into())
    }
}

impl Drop for WindowsJobObject {
    fn drop(&mut self) {
        // The kill-on-close limit is the final cleanup path if the owning
        // process/session is dropped while a descendant is still alive.
        unsafe {
            let _ = CloseHandle(self.handle);
            let _ = CloseHandle(self.completion_port);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "spawned only by the owned suspended-child test"]
    fn suspended_child_fixture() {
        let Some(marker) = std::env::var_os("DEVBOX_SUSPENDED_CHILD_MARKER") else {
            return;
        };
        std::fs::write(marker, b"executed").unwrap();
    }

    #[tokio::test]
    async fn suspended_child_cannot_execute_before_job_assignment() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("owned-marker");
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "lsp::process::windows_job::tests::suspended_child_fixture",
            ])
            .env("DEVBOX_SUSPENDED_CHILD_MARKER", &marker)
            .creation_flags(0x0800_0000 | 0x0000_0004)
            .kill_on_drop(true);
        let mut child = command.spawn().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        assert!(!marker.exists());
        let job = WindowsJobObject::assign_to(&child).unwrap();
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read(marker).unwrap(), b"executed");
        job.terminate_and_wait().await.unwrap();
        assert!(job.is_empty().unwrap());
    }
}
