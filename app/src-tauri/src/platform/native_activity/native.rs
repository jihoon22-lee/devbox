//! Query-only handles: no Job termination or user-process stop authority.
use devbox_protocol::windows::{ObservationAssigned, ObservationControl, RunIdentity};
use std::{
    fs::File,
    io::Read,
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
};
use windows::{
    Win32::{
        Foundation::{
            DUPLICATE_HANDLE_OPTIONS, DuplicateHandle, FILETIME, HANDLE, WAIT_OBJECT_0,
            WAIT_TIMEOUT,
        },
        Security::{
            GetSidIdentifierAuthority, GetSidSubAuthority, GetSidSubAuthorityCount,
            GetTokenInformation, IsValidSid, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::{
            JobObjects::{
                IsProcessInJob, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
                JobObjectBasicAccountingInformation, OpenJobObjectW, QueryInformationJobObject,
            },
            RemoteDesktop::ProcessIdToSessionId,
            Threading::{
                GetCurrentProcess, GetProcessId, GetProcessTimes, OpenProcessToken,
                PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
            },
        },
    },
    core::{BOOL, PCWSTR},
};

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}
fn own(handle: HANDLE) -> OwnedHandle {
    // SAFETY: only newly acquired owned kernel handles are passed here.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}
pub struct ChildProof {
    handle: OwnedHandle,
    pid: u32,
    created: u64,
    sid: String,
    session: u32,
}
pub struct JobProof(OwnedHandle);
impl ChildProof {
    pub fn capture(child: &tokio::process::Child) -> Option<Self> {
        let mut handle = HANDLE::default();
        // SAFETY: the child is borrowed until its independently owned query handle is acquired.
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                HANDLE(child.raw_handle()?),
                GetCurrentProcess(),
                &mut handle,
                (PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE).0,
                false,
                DUPLICATE_HANDLE_OPTIONS(0),
            )
        }
        .ok()?;
        let handle = own(handle);
        let pid = unsafe { GetProcessId(raw(&handle)) };
        let mut created = FILETIME::default();
        let mut exited = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let mut session = 0;
        unsafe {
            GetProcessTimes(
                raw(&handle),
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
            .ok()?;
            ProcessIdToSessionId(pid, &mut session).ok()?;
        }
        let created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        if pid == 0 || created == 0 {
            return None;
        }
        let sid = process_sid(raw(&handle))?;
        Some(Self {
            handle,
            pid,
            created,
            sid,
            session,
        })
    }
    pub fn exited(&self) -> Option<bool> {
        match unsafe { WaitForSingleObject(raw(&self.handle), 0) } {
            WAIT_OBJECT_0 => Some(true),
            WAIT_TIMEOUT => Some(false),
            _ => None,
        }
    }
    pub fn capture_job(
        &self,
        run: &RunIdentity,
        control: &ObservationControl,
    ) -> Result<Option<JobProof>, ()> {
        let Some(assigned) = assigned(Path::new(&control.assigned_path))? else {
            return Ok(None);
        };
        if !control.accepts(run, &assigned)
            || assigned.pid != self.pid
            || assigned.creation_time.parse::<u64>().ok() != Some(self.created)
            || assigned.sid != self.sid
            || assigned.session_id != self.session
        {
            return Err(());
        }
        let name: Vec<u16> = std::ffi::OsStr::new(&assigned.job_name)
            .encode_wide()
            .chain([0])
            .collect();
        // The bootstrap uses a fresh Job with no breakaway; retain this exact object for all later queries.
        let job = own(unsafe { OpenJobObjectW(4, false, PCWSTR(name.as_ptr())) }.map_err(|_| ())?);
        let mut member = BOOL::default();
        unsafe { IsProcessInJob(raw(&self.handle), Some(raw(&job)), &mut member) }
            .map_err(|_| ())?;
        if !member.as_bool() {
            return Err(());
        }
        let job = JobProof(job);
        job.active().ok_or(())?;
        Ok(Some(job))
    }
}
impl JobProof {
    pub fn active(&self) -> Option<u32> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(raw(&self.0)),
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&accounting) as u32,
                None,
            )
        }
        .ok()?;
        Some(accounting.ActiveProcesses)
    }
}
fn process_sid(process: HANDLE) -> Option<String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.ok()?;
    let token = own(token);
    let mut size = 0;
    let _ = unsafe { GetTokenInformation(raw(&token), TokenUser, None, 0, &mut size) };
    if !(std::mem::size_of::<TOKEN_USER>() as u32..=65536).contains(&size) {
        return None;
    }
    let mut buffer = vec![0_usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        GetTokenInformation(
            raw(&token),
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .ok()?;
    // SAFETY: GetTokenInformation populated the aligned TOKEN_USER and its SID within this buffer.
    unsafe {
        let sid = (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid;
        if !IsValidSid(sid).as_bool() {
            return None;
        }
        let authority = (*GetSidIdentifierAuthority(sid))
            .Value
            .iter()
            .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte));
        let count = *GetSidSubAuthorityCount(sid);
        if count > 15 {
            return None;
        }
        let mut value = format!("S-1-{authority}");
        for index in 0..u32::from(count) {
            value.push_str(&format!("-{}", *GetSidSubAuthority(sid, index)));
        }
        Some(value)
    }
}
fn assigned(path: &Path) -> Result<Option<ObservationAssigned>, ()> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_attributes() & 0x400 != 0 => return Err(()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(()),
        }
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !file.metadata().map_err(|_| ())?.is_file() {
        return Err(());
    }
    let mut bytes = Vec::new();
    file.take(16385).read_to_end(&mut bytes).map_err(|_| ())?;
    if bytes.len() > 16384 {
        return Err(());
    }
    serde_json::from_slice(&bytes).map(Some).map_err(|_| ())
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
