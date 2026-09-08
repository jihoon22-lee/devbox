//! Native preflight; the snapshot and filesystem writes still handle ENOSPC.
use std::path::Path;
pub fn require(root: &Path, needed: u64) -> Result<(), String> {
    if available(root)? < needed {
        return Err("import_disk_full".into());
    }
    Ok(())
}
#[cfg(windows)]
fn available(path: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{core::PCWSTR, Win32::Storage::FileSystem::GetDiskFreeSpaceExW};
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut available = 0;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut available), None, None) }
        .map_err(|_| "import_storage_unavailable")?;
    Ok(available)
}
#[cfg(unix)]
fn available(path: &Path) -> Result<u64, String> {
    use std::os::unix::ffi::OsStrExt;
    let path =
        std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| "import_path_invalid")?;
    let mut value = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), value.as_mut_ptr()) } != 0 {
        return Err("import_storage_unavailable".into());
    }
    let value = unsafe { value.assume_init() };
    Ok((value.f_bavail as u128 * value.f_frsize as u128).min(u64::MAX as u128) as u64)
}
#[cfg(not(any(windows, unix)))]
fn available(_path: &Path) -> Result<u64, String> {
    Err("import_storage_unavailable".into())
}
