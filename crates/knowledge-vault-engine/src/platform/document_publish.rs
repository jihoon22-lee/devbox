//! Publication primitives. Unsupported filesystems fail closed; no overwrite fallback.
use std::{fs, io, path::Path};

pub fn private_directory(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    if is_wsl(path)? {
        return super::document_wsl::invoke(
            workspace_wsl::document::Method::PrivateDirectory,
            &[path],
        );
    }
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
        let encoded = wide(path)?;
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
            let result =
                CreateDirectoryW(windows::core::PCWSTR(encoded.as_ptr()), Some(&attributes))
                    .map_err(windows_error);
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            result?;
            // Some filesystem providers ignore supplied security attributes.
            // Refuse to write content unless protected access control exists.
            let confirmed = DaclSnapshot::read(path).and_then(|acl| {
                if acl.acl.is_some() && acl.info.0 & 0x8000_0000 != 0 {
                    Ok(())
                } else {
                    Err(io::ErrorKind::Unsupported.into())
                }
            });
            if confirmed.is_err() {
                let _ = fs::remove_dir(path);
            }
            confirmed
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
    #[cfg(windows)]
    if is_wsl(target)? {
        return super::document_wsl::invoke(
            workspace_wsl::document::Method::Create,
            &[staged, target],
        );
    }
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
    #[cfg(windows)]
    if is_wsl(target)? {
        return super::document_wsl::invoke(
            workspace_wsl::document::Method::Replace,
            &[staged, target, previous],
        );
    }
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
        let security = DaclSnapshot::read(target)?;
        let published_handle = security_handle(&sibling)?;
        security.apply(&published_handle)?;
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
        // ReplaceFile can materialize inherited grants as explicit ACEs. Restore
        // the original explicit ACL and inheritance on our already-open object,
        // never on a path that an external writer may have replaced meanwhile.
        security.apply(&published_handle)?;
        create(&backup, previous)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (staged, target, previous);
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(windows)]
fn security_handle(path: &Path) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL, WRITE_DAC,
    };
    // Metadata access does not reserve read/write/delete sharing. The handle
    // remains attached to the submitted file across publication and path races.
    fs::OpenOptions::new()
        .access_mode((READ_CONTROL | WRITE_DAC).0)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .open(path)
}

#[cfg(windows)]
struct DaclSnapshot {
    // u32 storage provides native ACL alignment; None represents a null DACL.
    acl: Option<Vec<u32>>,
    info: windows::Win32::Security::OBJECT_SECURITY_INFORMATION,
}
#[cfg(windows)]
impl DaclSnapshot {
    fn read(source: &Path) -> io::Result<Self> {
        use windows::{
            core::PCWSTR,
            Win32::{
                Foundation::{LocalFree, HLOCAL},
                Security::{
                    AclSizeInformation, AddAce,
                    Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
                    GetAce, GetAclInformation, GetSecurityDescriptorControl, InitializeAcl,
                    ACE_HEADER, ACE_REVISION, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION,
                    INHERITED_ACE, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
                    SE_DACL_PROTECTED, UNPROTECTED_DACL_SECURITY_INFORMATION,
                },
            },
        };
        let source = wide(source)?;
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
                let protected = control & SE_DACL_PROTECTED.0 != 0;
                let info = DACL_SECURITY_INFORMATION
                    | if protected {
                        PROTECTED_DACL_SECURITY_INFORMATION
                    } else {
                        UNPROTECTED_DACL_SECURITY_INFORMATION
                    };
                if dacl.is_null() {
                    return Ok(Self { acl: None, info });
                }
                let mut size = ACL_SIZE_INFORMATION::default();
                GetAclInformation(
                    dacl,
                    (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
                    std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                    AclSizeInformation,
                )
                .map_err(windows_error)?;
                let mut buffer = vec![0u32; (size.AclBytesInUse as usize).div_ceil(4)];
                let acl = buffer.as_mut_ptr().cast::<ACL>();
                let revision = ACE_REVISION((*dacl).AclRevision as u32);
                InitializeAcl(acl, size.AclBytesInUse, revision).map_err(windows_error)?;
                for index in 0..size.AceCount {
                    let mut ace = std::ptr::null_mut();
                    GetAce(dacl, index, &mut ace).map_err(windows_error)?;
                    let header = &*ace.cast::<ACE_HEADER>();
                    // Inherited ACEs must be regenerated from the real parent,
                    // not converted into persistent explicit grants by a setter.
                    if protected || header.AceFlags & INHERITED_ACE.0 as u8 == 0 {
                        AddAce(acl, revision, u32::MAX, ace, header.AceSize as u32)
                            .map_err(windows_error)?;
                    }
                }
                Ok(Self {
                    acl: Some(buffer),
                    info,
                })
            })();
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            result
        }
    }
    fn apply(&self, file: &fs::File) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Security::Authorization::{SetSecurityInfo, SE_FILE_OBJECT},
        };
        let acl = self
            .acl
            .as_ref()
            .map_or(std::ptr::null(), |buffer| buffer.as_ptr().cast());
        unsafe {
            SetSecurityInfo(
                HANDLE(file.as_raw_handle()),
                SE_FILE_OBJECT,
                self.info,
                None,
                None,
                Some(acl),
                None,
            )
        }
        .ok()
        .map_err(windows_error)
    }
}

#[cfg(all(test, windows))]
fn copy_dacl(source: &Path, target: &Path) -> io::Result<()> {
    DaclSnapshot::read(source)?.apply(&security_handle(target)?)
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
    fn permission_finalization_follows_our_handle_not_a_replaced_path() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private");
        private_directory(&private).unwrap();
        let security = DaclSnapshot::read(&private).unwrap();
        let target = root.path().join("target.md");
        fs::write(&target, "ours").unwrap();
        let handle = security_handle(&target).unwrap();
        let moved = root.path().join("moved.md");
        fs::rename(&target, &moved).unwrap();
        fs::write(&target, "external").unwrap();
        let external_acl = dacl(&target);
        security.apply(&handle).unwrap();
        assert_eq!(dacl(&target), external_acl);
        assert_eq!(fs::read_to_string(&target).unwrap(), "external");
        assert!(dacl(&moved).starts_with("D:P"));
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

#[cfg(windows)]
fn is_wsl(path: &Path) -> io::Result<bool> {
    devbox_wsl::path::parse_wsl_unc_path(&path.to_string_lossy())
        .map(|p| p.is_some())
        .map_err(io::Error::other)
}
pub fn staging_permissions(staged: &Path, target: &Path) -> io::Result<()> {
    #[cfg(windows)]
    if is_wsl(target)? {
        return super::document_wsl::invoke(
            workspace_wsl::document::Method::Permissions,
            &[staged, target],
        );
    }
    let _ = (staged, target);
    Ok(())
}

pub fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    if is_wsl(path)? {
        return super::document_wsl::invoke(
            workspace_wsl::document::Method::Sync,
            &[path.parent().ok_or(io::ErrorKind::InvalidInput)?],
        );
    }
    let _ = path;
    Ok(())
}
