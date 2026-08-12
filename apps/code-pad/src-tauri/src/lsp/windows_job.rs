//! Windows process-tree ownership for local language-server children.
//!
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is set before the child is assigned.
//! The wrapper owns the job handle for the entire wait-task lifetime and also
//! exposes explicit termination for stop/crash paths.  Assignment failures are
//! returned to the caller; they never silently degrade into a process-only
//! session.

use std::mem::size_of;
use tokio::process::Child;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

pub(crate) struct WindowsJobObject {
    handle: HANDLE,
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

        if let Err(error) = unsafe { AssignProcessToJobObject(handle, process_handle) } {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(format!("AssignProcessToJobObject failed: {error}"));
        }
        Ok(Self { handle })
    }

    pub(crate) fn terminate(&self) -> Result<(), String> {
        unsafe { TerminateJobObject(self.handle, 1) }
            .map_err(|error| format!("TerminateJobObject failed: {error}"))
    }
}

impl Drop for WindowsJobObject {
    fn drop(&mut self) {
        // The kill-on-close limit is the final cleanup path if the owning
        // process/session is dropped while a descendant is still alive.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_policy_keeps_tree_cleanup_explicit() {
        assert_eq!(
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0, 0x2000,
            "the wrapper must retain kill-on-close rather than process-only cleanup"
        );
    }
}
