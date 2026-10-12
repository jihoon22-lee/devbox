//! Own the handle before any fallible Job configuration, including assignment.
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::*,
};
pub struct Job(HANDLE);
// HANDLE is an owned kernel object; its only operation after construction is CloseHandle.
unsafe impl Send for Job {}
impl Job {
    pub fn for_child(child: &tokio::process::Child) -> std::io::Result<Self> {
        let raw = child
            .raw_handle()
            .ok_or_else(|| std::io::Error::other("child handle 없음"))?;
        // SAFETY: Job owns this new handle; the borrowed child handle remains alive through assignment.
        unsafe {
            let job = Self(CreateJobObjectW(None, None).map_err(std::io::Error::other)?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(std::io::Error::other)?;
            AssignProcessToJobObject(job.0, HANDLE(raw)).map_err(std::io::Error::other)?;
            Ok(job)
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
