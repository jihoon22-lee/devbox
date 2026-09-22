//! Publication primitives. Unsupported filesystems fail closed; no overwrite fallback.
use std::{fs, io, path::Path};

pub fn private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(windows)]
    {
        use windows::{
            core::w,
            Win32::{
                Foundation::{LocalFree, HLOCAL},
                Security::{
                    Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
                },
                Storage::FileSystem::CreateDirectoryW,
            },
        };
        let path = wide(path)?;
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w!("D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)"),
                1,
                &mut descriptor,
                None,
            )
            .map_err(windows_error)?;
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            };
            let result = CreateDirectoryW(windows::core::PCWSTR(path.as_ptr()), Some(&attributes))
                .map_err(windows_error);
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            result
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Publish complete bytes only if the target does not exist. Do not require
/// hard-link support and never fall back to an overwrite-capable rename.
pub fn create(staged: &Path, target: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        rename_linux(staged, target, libc::RENAME_NOREPLACE)
    }
    #[cfg(windows)]
    {
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH},
        };
        let staged = wide(staged)?;
        let target = wide(target)?;
        unsafe {
            MoveFileExW(
                PCWSTR(staged.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(windows_error)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (staged, target);
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(target_os = "linux")]
fn rename_linux(from: &Path, to: &Path, flags: libc::c_uint) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            flags,
        )
    };
    if result != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Preserve the exact displaced object, including a replacement inserted by an
/// external writer after revision validation. The caller must inspect it before
/// deleting any evidence. Errors may leave an intermediate state on Windows.
pub fn replace(staged: &Path, target: &Path, previous: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        rename_linux(staged, target, libc::RENAME_EXCHANGE)?;
        fs::rename(staged, previous)
    }
    #[cfg(windows)]
    {
        use windows::{
            core::PCWSTR,
            Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS},
        };
        let staged = wide(staged)?;
        let target = wide(target)?;
        let previous = wide(previous)?;
        unsafe {
            ReplaceFileW(
                PCWSTR(target.as_ptr()),
                PCWSTR(staged.as_ptr()),
                PCWSTR(previous.as_ptr()),
                REPLACE_FILE_FLAGS(0),
                None,
                None,
            )
        }
        .map_err(windows_error)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (staged, target, previous);
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(windows)]
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let parent = fs::canonicalize(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    Ok(parent
        .join(path.file_name().ok_or(io::ErrorKind::InvalidInput)?)
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
                    SE_FILE_OBJECT,
                },
                DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            },
        },
    };
    fn dacl(path: &Path) -> String {
        let path = wide(path).unwrap();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let mut encoded = PWSTR::null();
        unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(path.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                &mut descriptor,
            )
            .ok()
            .unwrap();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                1,
                DACL_SECURITY_INFORMATION,
                &mut encoded,
                None,
            )
            .unwrap();
            let result = encoded.to_string().unwrap();
            let _ = LocalFree(Some(HLOCAL(encoded.0.cast())));
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            result
        }
    }
    #[test]
    fn windows_replacement_preserves_target_dacl_and_recovery_is_private() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("note.md");
        fs::write(&target, "original").unwrap();
        let original = dacl(&target);
        let directory = root.path().join("recovery");
        private_directory(&directory).unwrap();
        let security = dacl(&directory);
        assert!(security.starts_with("D:P"));
        assert!(security.contains(";;;OW)"));
        assert!(!security.contains(";;;WD)"));
        let staged = directory.join("submitted.md");
        fs::write(&staged, "replacement").unwrap();
        let previous = directory.join("previous.md");
        replace(&staged, &target, &previous).unwrap();
        assert_eq!(dacl(&target), original);
        assert_eq!(fs::read_to_string(previous).unwrap(), "original");
    }
}

#[cfg(windows)]
fn windows_error(error: windows::core::Error) -> io::Error {
    windows::Win32::Foundation::WIN32_ERROR::from_error(&error)
        .map(|code| io::Error::from_raw_os_error(code.0 as i32))
        .unwrap_or_else(|| io::Error::other(error))
}
