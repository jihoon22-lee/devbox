//! Native file identity for bounded project metadata reads.
use std::path::Path;

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
