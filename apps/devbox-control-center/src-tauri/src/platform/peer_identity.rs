//! Pipe peers are identified from the OS, not a PID/product supplied in JSON.
use super::component_scope::CapturedScope;
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, sync::Arc};
use windows::{
    core::PWSTR,
    Win32::{
        Foundation::{
            CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, FILETIME, HANDLE, WAIT_TIMEOUT,
        },
        System::{
            Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
            Threading::{
                GetCurrentProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
                WaitForSingleObject, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE,
            },
        },
    },
};
type Result<T> = std::result::Result<T, &'static str>;
struct Handle(HANDLE);
// Process handles are process-wide; this wrapper only queries and closes its own handle.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

// A blocking identity task must own a duplicate. If its async caller times out,
// closing/reusing the original Tokio handle cannot redirect the native query.
pub(crate) struct PipeWitness(Handle);
impl PipeWitness {
    pub(crate) fn capture(pipe: HANDLE) -> Result<Self> {
        let process = unsafe { GetCurrentProcess() };
        let mut duplicate = HANDLE::default();
        unsafe {
            DuplicateHandle(
                process,
                pipe,
                process,
                &mut duplicate,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        }
        .map_err(|_| "peer_pipe_unavailable")?;
        Ok(Self(Handle(duplicate)))
    }
}

pub(crate) struct ProcessPeer {
    handle: Handle,
    scope: Arc<CapturedScope>,
    pub(crate) product: String,
    _process_id: u32,
    _started_at: u64,
}
impl ProcessPeer {
    pub(crate) fn from_pipe(
        scope: Arc<CapturedScope>,
        pipe: PipeWitness,
        server_side: bool,
    ) -> Result<Self> {
        let mut pid = 0;
        unsafe {
            if server_side {
                GetNamedPipeClientProcessId(pipe.0 .0, &mut pid)
            } else {
                GetNamedPipeServerProcessId(pipe.0 .0, &mut pid)
            }
        }
        .map_err(|_| "peer_process_unavailable")?;
        Self::capture(scope, pid)
    }
    fn capture(scope: Arc<CapturedScope>, process_id: u32) -> Result<Self> {
        if process_id == 0 {
            return Err("peer_process_unavailable");
        }
        let handle = Handle(
            unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    process_id,
                )
            }
            .map_err(|_| "peer_process_unavailable")?,
        );
        if unsafe { WaitForSingleObject(handle.0, 0) } != WAIT_TIMEOUT {
            return Err("peer_process_retired");
        }
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .map_err(|_| "peer_process_unavailable")?;
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        unsafe {
            QueryFullProcessImageNameW(
                handle.0,
                PROCESS_NAME_WIN32,
                PWSTR(image.as_mut_ptr()),
                &mut length,
            )
        }
        .map_err(|_| "peer_image_unavailable")?;
        if length == 0 || length as usize >= image.len() {
            return Err("peer_image_unavailable");
        }
        let path = PathBuf::from(OsString::from_wide(&image[..length as usize]));
        let product = scope.product_for_image(&path)?;
        let peer = Self {
            handle,
            scope,
            product,
            _process_id: process_id,
            _started_at: (u64::from(created.dwHighDateTime) << 32)
                | u64::from(created.dwLowDateTime),
        };
        peer.revalidate()?;
        Ok(peer)
    }
    pub(crate) fn revalidate(&self) -> Result<()> {
        if unsafe { WaitForSingleObject(self.handle.0, 0) } != WAIT_TIMEOUT {
            return Err("peer_process_retired");
        }
        self.scope.revalidate()
    }
    pub(crate) fn scope(&self) -> &CapturedScope {
        &self.scope
    }
    pub(crate) fn wire_identity(&self) -> Result<product_contract::transport::Peer> {
        self.revalidate()?;
        product_contract::transport::Peer::from_native(
            &self.product,
            &self.scope.installation_key,
            &self.scope.id,
        )
    }
}
