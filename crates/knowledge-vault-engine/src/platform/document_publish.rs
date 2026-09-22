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
        // Keep both replacement participants under the original parent while
        // Windows merges DACLs. A private backup parent must not inject its
        // inheritable OWNER RIGHTS grant into the published document.
        let recovery = staged.parent().ok_or(io::ErrorKind::InvalidInput)?;
        let name = recovery
            .file_name()
            .ok_or(io::ErrorKind::InvalidInput)?
            .to_string_lossy();
        let parent = target.parent().ok_or(io::ErrorKind::InvalidInput)?;
        let sibling = parent.join(format!("{name}.submitted.md"));
        let backup = parent.join(format!("{name}.previous.md"));
        create(staged, &sibling)?;
        copy_dacl(target, &sibling)?;
        // Reserve our backup name without replacing an unrelated existing file.
        drop(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&backup)?,
        );
        let from = wide(&sibling)?;
        let to = wide(target)?;
        let saved = wide(&backup)?;
        unsafe {
            ReplaceFileW(
                PCWSTR(to.as_ptr()),
                PCWSTR(from.as_ptr()),
                PCWSTR(saved.as_ptr()),
                REPLACE_FILE_FLAGS(0),
                None,
                None,
            )
        }
        .map_err(windows_error)?;
        create(&backup, previous)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (staged, target, previous);
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(windows)]
fn copy_dacl(source: &Path, target: &Path) -> io::Result<()> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::{GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT},
                GetSecurityDescriptorControl, DACL_SECURITY_INFORMATION,
                PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
                UNPROTECTED_DACL_SECURITY_INFORMATION,
            },
        },
    };
    let source = wide(source)?;
    let target = wide(target)?;
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let mut dacl = std::ptr::null_mut();
    unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(source.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut dacl),
            None,
            &mut descriptor,
        )
        .ok()
        .map_err(windows_error)?;
        let result = (|| {
            let mut control = 0;
            let mut revision = 0;
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision)
                .map_err(windows_error)?;
            let inheritance = if control & SE_DACL_PROTECTED.0 != 0 {
                PROTECTED_DACL_SECURITY_INFORMATION
            } else {
                UNPROTECTED_DACL_SECURITY_INFORMATION
            };
            SetNamedSecurityInfoW(
                PCWSTR(target.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | inheritance,
                None,
                None,
                Some(dacl),
                None,
            )
            .ok()
            .map_err(windows_error)
        })();
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        result
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
        for protected in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("note.md");
            fs::write(&target, "original").unwrap();
            let directory = root.path().join("recovery");
            private_directory(&directory).unwrap();
            let security = dacl(&directory);
            assert!(security.starts_with("D:P"));
            assert!(security.contains(";;;OW)"));
            assert!(!security.contains(";;;WD)"));
            if protected {
                copy_dacl(&directory, &target).unwrap();
                assert!(dacl(&target).starts_with("D:P"));
                assert!(!dacl(&target).contains(";;;BA)"));
            }
            let original = dacl(&target);
            let staged = directory.join("submitted.md");
            fs::write(&staged, "replacement").unwrap();
            let previous = directory.join("previous.md");
            replace(&staged, &target, &previous).unwrap();
            // AI records that inheritance was processed. Compare every ACE,
            // including its inherited bit, and retain the protection flag.
            assert_eq!(dacl(&target).replace("AI", ""), original.replace("AI", ""));
            assert_eq!(fs::read_to_string(previous).unwrap(), "original");
        }
    }
}

#[cfg(windows)]
fn windows_error(error: windows::core::Error) -> io::Error {
    windows::Win32::Foundation::WIN32_ERROR::from_error(&error)
        .map(|code| io::Error::from_raw_os_error(code.0 as i32))
        .unwrap_or_else(|| io::Error::other(error))
}
