//! Bounded ownership for short-lived native probe processes.
//!
//! A `tokio::process::Child` only represents the root process. WSL and other
//! fixed probes may create helpers before the root exits, so killing the root
//! alone is not a sufficient timeout/cancellation boundary. Unix children are
//! put in a private process group before spawn; Windows children are assigned
//! to a kill-on-close Job Object immediately after spawn. Assignment failure
//! is fail-closed at the command boundary rather than silently degrading to
//! root-only cleanup.

#[cfg(target_os = "windows")]
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Child;
#[cfg(target_os = "windows")]
use tokio::process::Command;

const CLEANUP_TIMEOUT: Duration = Duration::from_millis(500);

#[cfg(target_os = "windows")]
use std::mem::size_of;

#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// Own a root process and every descendant that remains in its probe tree.
pub(crate) struct ProcessTree {
    #[cfg(target_os = "windows")]
    job: HANDLE,
    #[cfg(unix)]
    process_group: i32,
}

impl ProcessTree {
    /// Assign an already spawned child to the platform process-tree boundary.
    /// The caller must kill/reap the root when this returns `Err`.
    pub(crate) fn assign(child: &Child) -> Result<Self, ()> {
        #[cfg(target_os = "windows")]
        {
            let raw_handle = child.raw_handle().ok_or(())?;
            let process = HANDLE(raw_handle);
            let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|_| ())?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if configured.is_err() {
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(());
            }
            if unsafe { AssignProcessToJobObject(job, process) }.is_err() {
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(());
            }
            return Ok(Self { job });
        }

        #[cfg(unix)]
        {
            // The corresponding Command uses `process_group(0)`, making the
            // child its own group leader. A negative group id then targets
            // only this probe and its descendants.
            let process_group = i32::try_from(child.id().ok_or(())?).map_err(|_| ())?;
            Ok(Self { process_group })
        }

        #[cfg(not(any(unix, target_os = "windows")))]
        {
            let _ = child;
            Ok(Self {})
        }
    }

    /// Kill the complete tree and reap the root within a finite cleanup
    /// window. The tree remains owned until this method returns so descendants
    /// cannot outlive the command's guard.
    pub(crate) async fn terminate(&mut self, child: &mut Child) {
        self.terminate_descendants();
        let _ = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await;
    }

    /// Terminate descendants after a normal root exit as well. A helper which
    /// inherited a pipe must never survive a successful-looking probe.
    pub(crate) fn terminate_descendants(&mut self) {
        #[cfg(target_os = "windows")]
        unsafe {
            let _ = TerminateJobObject(self.job, 1);
        }

        #[cfg(unix)]
        {
            // Ignore ESRCH because it means the root and all descendants are
            // already gone. The group is private to this short-lived command.
            let _ = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
        }
    }

    /// Best-effort tree cleanup for the narrow window before a platform tree
    /// owner could be attached. This is only used when `assign` fails; callers
    /// still report the operation unavailable and never continue as if the
    /// child were safely owned.
    pub(crate) async fn terminate_unassigned(child: &mut Child) {
        #[cfg(target_os = "windows")]
        {
            if let Some(pid) = child.id() {
                let mut taskkill = Command::new("taskkill");
                taskkill
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .creation_flags(0x0800_0000)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(true);
                if let Ok(mut taskkill) = taskkill.spawn() {
                    let _ = tokio::time::timeout(CLEANUP_TIMEOUT, taskkill.wait()).await;
                }
            }
        }

        #[cfg(unix)]
        if let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()) {
            // All Unix callers set process_group(0) before spawn. Even if the
            // owner object could not be constructed, the private group is
            // still the safest available tree boundary.
            let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
        }

        let _ = child.kill().await;
        let _ = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await;
    }
}

#[cfg(target_os = "windows")]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE is the final crash/drop backstop.
        unsafe {
            let _ = CloseHandle(self.job);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cleanup_window_is_finite() {
        assert!(super::CLEANUP_TIMEOUT.as_millis() > 0);
        assert!(super::CLEANUP_TIMEOUT.as_secs() < 1);
    }
}
