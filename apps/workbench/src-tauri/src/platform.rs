//! Workbench's platform secret boundary.
//!
//! `core::environment` owns parsing and metadata.  This module is the only
//! place that turns a secret reference's current `.env` value into an
//! execution-time value.  It seals and immediately unseals the value through
//! `crates/secrets`; the resulting `Zeroizing<String>` is borrowed only while
//! the child process is spawned and is never serialized or logged.

use devbox_secrets::SealError;
use std::path::Path;
use zeroize::Zeroizing;

/// Stable identity captured from an already-open filesystem handle. Paths and
/// timestamps are deliberately excluded: Unix uses device/inode and Windows
/// uses volume/file-index from `GetFileInformationByHandle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    scope: u64,
    object: u64,
}

/// Open a path without following its final link/reparse component and return
/// the identity of that exact handle. Callers keep the `File` alive across a
/// read, then compare it with a newly opened path identity to detect swaps.
pub(crate) fn open_readonly_with_identity(
    path: &Path,
    directory: bool,
) -> std::io::Result<(std::fs::File, FileIdentity)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

        #[cfg(target_os = "linux")]
        const NO_FOLLOW: i32 = 0x20000;
        #[cfg(target_os = "macos")]
        const NO_FOLLOW: i32 = 0x100;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        const NO_FOLLOW: i32 = 0;

        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(NO_FOLLOW)
            .open(path)?;
        let metadata = file.metadata()?;
        if metadata.is_dir() != directory {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unexpected file type",
            ));
        }
        let identity = FileIdentity {
            scope: metadata.dev(),
            object: metadata.ino(),
        };
        Ok((file, identity))
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::FromRawHandle;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::GENERIC_READ;
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
            FILE_ATTRIBUTE_DIRECTORY, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            OPEN_EXISTING,
        };

        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let flags = FILE_FLAG_OPEN_REPARSE_POINT
            | if directory {
                FILE_FLAG_BACKUP_SEMANTICS
            } else {
                Default::default()
            };
        let desired_access = FILE_READ_ATTRIBUTES.0 | if directory { 0 } else { GENERIC_READ.0 };
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                desired_access,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                flags,
                None,
            )
        }
        .map_err(|_| std::io::Error::last_os_error())?;
        // Transfer ownership immediately; every later error path closes the
        // handle through `File::drop` exactly once.
        let file = unsafe { std::fs::File::from_raw_handle(handle.0) };
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(handle, &mut information) }
            .map_err(|_| std::io::Error::last_os_error())?;
        let is_directory = information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
        if is_directory != directory {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unexpected file type",
            ));
        }
        let identity = FileIdentity {
            scope: u64::from(information.dwVolumeSerialNumber),
            object: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        };
        Ok((file, identity))
    }

    #[cfg(not(any(unix, windows)))]
    {
        use std::time::UNIX_EPOCH;

        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if metadata.is_dir() != directory {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unexpected file type",
            ));
        }
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid time"))?;
        Ok((
            file,
            FileIdentity {
                scope: metadata.len(),
                object: u64::try_from(modified.as_nanos()).unwrap_or(u64::MAX),
            },
        ))
    }
}

pub(crate) fn path_identity(path: &Path, directory: bool) -> std::io::Result<FileIdentity> {
    open_readonly_with_identity(path, directory).map(|(_, identity)| identity)
}

pub fn resolve_secret_for_execution(value: &str) -> Result<Zeroizing<String>, &'static str> {
    if value.is_empty() {
        return Ok(Zeroizing::new(String::new()));
    }
    let sealer = platform_sealer();
    let sealed = Zeroizing::new(
        devbox_secrets::seal_v1(sealer.as_ref(), value).map_err(|_| "secret unavailable")?,
    );
    devbox_secrets::unseal_v1(sealer.as_ref(), sealed.as_slice()).map_err(|_| "secret unavailable")
}

fn platform_sealer() -> Box<dyn devbox_secrets::Sealer> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows_impl::DpapiSealer)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(UnsupportedSealer)
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    use zeroize::Zeroize;

    // Distinct application entropy prevents a Workbench blob from being
    // mistaken for an API Playground or Run Manager blob while DPAPI still
    // scopes it to the current Windows user.
    const ENTROPY: &[u8] = b"devbox.workbench.project-environment.v1";

    pub(super) struct DpapiSealer;

    impl devbox_secrets::Sealer for DpapiSealer {
        fn seal(&self, plaintext: &str) -> Result<Vec<u8>, SealError> {
            let input = blob(plaintext.as_bytes())?;
            let entropy = blob(ENTROPY)?;
            let mut output = CRYPT_INTEGER_BLOB::default();
            unsafe {
                CryptProtectData(
                    &input,
                    PCWSTR::null(),
                    Some(&entropy as *const _),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
            .map_err(|_| SealError::CryptoFailure)?;
            copy_and_free(output)
        }

        fn unseal(&self, ciphertext: &[u8]) -> Result<Zeroizing<String>, SealError> {
            let input = blob(ciphertext)?;
            let entropy = blob(ENTROPY)?;
            let mut output = CRYPT_INTEGER_BLOB::default();
            unsafe {
                CryptUnprotectData(
                    &input,
                    None,
                    Some(&entropy as *const _),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
            .map_err(|_| SealError::CryptoFailure)?;
            let bytes = copy_and_zeroize_free(output)?;
            // `String::from_utf8` takes ownership of a plain Vec and keeps it
            // inside `FromUtf8Error` on failure. Zeroize that error-owned copy
            // explicitly so malformed/tampered DPAPI output has the same
            // transient-memory guarantee as the success path.
            let text = match String::from_utf8(bytes.to_vec()) {
                Ok(text) => text,
                Err(error) => {
                    let mut invalid = error.into_bytes();
                    invalid.zeroize();
                    return Err(SealError::InvalidInput);
                }
            };
            Ok(Zeroizing::new(text))
        }
    }

    fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, SealError> {
        let cb_data = u32::try_from(bytes.len()).map_err(|_| SealError::InvalidInput)?;
        Ok(CRYPT_INTEGER_BLOB {
            cbData: cb_data,
            pbData: bytes.as_ptr() as *mut u8,
        })
    }

    fn copy_and_free(blob: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>, SealError> {
        if blob.pbData.is_null() {
            return Err(SealError::CryptoFailure);
        }
        let mut copied =
            unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() };
        unsafe {
            // DPAPI allocates this buffer with LocalAlloc.  Even though the
            // caller immediately wraps the copied ciphertext in Zeroizing,
            // clear the provider-owned transient before releasing it too.
            std::ptr::write_bytes(blob.pbData, 0, blob.cbData as usize);
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
        if blob.cbData == 0 || copied.is_empty() {
            copied.zeroize();
            Err(SealError::CryptoFailure)
        } else {
            Ok(copied)
        }
    }

    fn copy_and_zeroize_free(blob: CRYPT_INTEGER_BLOB) -> Result<Zeroizing<Vec<u8>>, SealError> {
        if blob.pbData.is_null() || blob.cbData == 0 {
            return Err(SealError::CryptoFailure);
        }
        let mut copied =
            unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() };
        unsafe {
            std::ptr::write_bytes(blob.pbData, 0, blob.cbData as usize);
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
        if copied.is_empty() {
            copied.zeroize();
            Err(SealError::CryptoFailure)
        } else {
            Ok(Zeroizing::new(copied))
        }
    }
}

#[cfg(not(target_os = "windows"))]
struct UnsupportedSealer;

#[cfg(not(target_os = "windows"))]
impl devbox_secrets::Sealer for UnsupportedSealer {
    fn seal(&self, _plaintext: &str) -> Result<Vec<u8>, SealError> {
        Err(SealError::CryptoFailure)
    }

    fn unseal(&self, _ciphertext: &[u8]) -> Result<Zeroizing<String>, SealError> {
        Err(SealError::CryptoFailure)
    }
}
