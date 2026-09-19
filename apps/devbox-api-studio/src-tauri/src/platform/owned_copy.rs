//! Remove only a retired exporter’s owned scratch copy, rechecking identity on retry.
// Each application compiles the same adapter but uses a different entry point.
#![allow(dead_code)]
use std::{fs, path::Path};
pub fn remove_owned_directory(directory: &Path) -> Result<(), String> {
    remove_owned_directory_with(directory, |path| fs::remove_dir_all(path))
}
fn remove_owned_directory_with(
    directory: &Path,
    remove: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<(), String> {
    remove_owned_directory_checked(directory, remove, |path| fs::symlink_metadata(path))
}
pub fn remove_owned_directory_matching(
    directory: &Path,
    expected: (u64, u64),
) -> Result<(), String> {
    remove_owned_directory_against(
        directory,
        |path| fs::remove_dir_all(path),
        |path| fs::symlink_metadata(path),
        Some(expected),
    )
}
fn remove_owned_directory_checked(
    directory: &Path,
    remove: impl FnMut(&Path) -> std::io::Result<()>,
    read_metadata: impl FnMut(&Path) -> std::io::Result<fs::Metadata>,
) -> Result<(), String> {
    remove_owned_directory_against(directory, remove, read_metadata, None)
}
fn remove_owned_directory_against(
    directory: &Path,
    mut remove: impl FnMut(&Path) -> std::io::Result<()>,
    mut read_metadata: impl FnMut(&Path) -> std::io::Result<fs::Metadata>,
    expected: Option<(u64, u64)>,
) -> Result<(), String> {
    fn inspect(
        path: &Path,
        remaining: &mut usize,
        read_metadata: &mut impl FnMut(&Path) -> std::io::Result<fs::Metadata>,
    ) -> Result<bool, String> {
        *remaining = remaining
            .checked_sub(1)
            .ok_or("migration_store_too_large")?;
        let metadata = match read_metadata(path) {
            Ok(value) => value,
            // An owned scratch entry may finish a pending deletion after it
            // was enumerated. Its absence is already the desired state.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err("migration_cleanup_entry_metadata".into()),
        };
        let reparse_point = {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            }
            #[cfg(not(windows))]
            {
                false
            }
        };
        // This is an owned scratch tree, not the legacy source. Links created
        // inside it are leaves: never inspect their targets. std::remove_dir_all
        // unlinks them without following them on supported Windows/Linux hosts.
        if metadata.file_type().is_symlink() || reparse_point {
            return Ok(false);
        }
        if let Err(error) = devbox_filesystem::ensure_no_links(path) {
            return match error.kind() {
                std::io::ErrorKind::NotFound => Ok(false),
                std::io::ErrorKind::InvalidInput => Err("migration_cleanup_child_path".into()),
                _ => Err("migration_cleanup_entry_metadata".into()),
            };
        }
        let mut readonly = metadata.is_file() && metadata.permissions().readonly();
        if metadata.is_dir() {
            let entries = match fs::read_dir(path) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(_) => return Err("migration_cleanup_directory_read".into()),
            };
            for entry in entries {
                let entry = match entry {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => return Err("migration_cleanup_directory_read".into()),
                };
                readonly |= inspect(&entry.path(), remaining, read_metadata)?;
            }
        }
        Ok(readonly)
    }
    let identity = devbox_filesystem::filesystem_identity(directory, true)
        .map_err(|_| "migration_cleanup_root_identity")?;
    if expected.is_some_and(|expected| identity.components() != expected) {
        return Err("migration_cleanup_changed".into());
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        // Root/ancestor links remain forbidden, including between retries.
        devbox_filesystem::ensure_no_links(directory).map_err(|_| "migration_cleanup_root_path")?;
        if devbox_filesystem::filesystem_identity(directory, true)
            .map_err(|_| "migration_cleanup_changed")?
            != identity
        {
            return Err("migration_cleanup_changed".into());
        }
        let readonly = match inspect(directory, &mut 20_000, &mut read_metadata) {
            Ok(value) => value,
            Err(error)
                if matches!(
                    error.as_str(),
                    "migration_cleanup_entry_metadata" | "migration_cleanup_directory_read"
                ) && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
            Err(error) => return Err(error),
        };
        match remove(directory) {
            Ok(()) => return Ok(()),
            Err(error) => {
                if std::time::Instant::now() >= deadline {
                    return Err(match error.kind() {
                        std::io::ErrorKind::PermissionDenied if readonly => {
                            "migration_cleanup_readonly"
                        }
                        std::io::ErrorKind::PermissionDenied => "migration_cleanup_access_denied",
                        std::io::ErrorKind::NotFound => "migration_cleanup_changed",
                        _ => "migration_cleanup_io",
                    }
                    .into());
                }
                // The worker Job is already empty. Antivirus/indexing can
                // briefly retain a delete-incompatible handle to this owned copy.
                // Never follow links, replace the root or alter source permissions.
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_copy_cleanup_rechecks_identity_before_retrying_a_failed_removal() {
        let root = tempfile::tempdir().unwrap();
        let copy = root.path().join("copy");
        let moved = root.path().join("moved");
        fs::create_dir(&copy).unwrap();
        fs::write(copy.join("data"), b"owned copy").unwrap();
        let mut attempts = 0;
        let result = remove_owned_directory_with(&copy, |path| {
            attempts += 1;
            fs::rename(path, &moved)?;
            fs::create_dir(path)?;
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert_eq!(result.unwrap_err(), "migration_cleanup_changed");
        assert_eq!(attempts, 1);
        assert!(copy.is_dir());
        assert_eq!(fs::read(moved.join("data")).unwrap(), b"owned copy");
    }
    #[test]
    fn owned_copy_cleanup_retries_transient_delete_failure_without_touching_original() {
        let root = tempfile::tempdir().unwrap();
        let copy = root.path().join("copy");
        let original = root.path().join("original");
        fs::create_dir(&copy).unwrap();
        fs::write(copy.join("data"), b"owned copy").unwrap();
        fs::write(&original, b"original").unwrap();
        let mut attempts = 0;
        remove_owned_directory_with(&copy, |path| {
            attempts += 1;
            if attempts == 1 {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            } else {
                fs::remove_dir_all(path)
            }
        })
        .unwrap();
        assert_eq!(attempts, 2);
        assert!(!copy.exists());
        assert_eq!(fs::read(original).unwrap(), b"original");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn owned_copy_cleanup_unlinks_child_links_without_reading_or_removing_targets() {
        let root = tempfile::tempdir().unwrap();
        let copy = root.path().join("copy");
        let original = root.path().join("original");
        fs::create_dir(&copy).unwrap();
        fs::create_dir(&original).unwrap();
        fs::write(original.join("preserved"), b"source").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&original, copy.join("outside")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&original, copy.join("outside")).unwrap();
        remove_owned_directory(&copy).unwrap();
        assert!(!copy.exists());
        assert_eq!(fs::read(original.join("preserved")).unwrap(), b"source");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn owned_copy_cleanup_rejects_a_linked_root_and_preserves_its_target() {
        let root = tempfile::tempdir().unwrap();
        let copy = root.path().join("copy");
        let original = root.path().join("original");
        fs::create_dir(&original).unwrap();
        fs::write(original.join("preserved"), b"source").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&original, &copy).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&original, &copy).unwrap();
        assert!(remove_owned_directory(&copy).is_err());
        assert!(fs::symlink_metadata(copy).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(original.join("preserved")).unwrap(), b"source");
    }

    #[test]
    fn owned_copy_cleanup_accepts_an_entry_disappearing_after_enumeration() {
        let root = tempfile::tempdir().unwrap();
        let copy = root.path().join("copy");
        fs::create_dir(&copy).unwrap();
        let transient = copy.join("transient");
        fs::write(&transient, b"owned scratch").unwrap();
        let mut disappeared = false;
        remove_owned_directory_checked(
            &copy,
            |path| fs::remove_dir_all(path),
            |path| {
                if path == transient && !disappeared {
                    fs::remove_file(path)?;
                    disappeared = true;
                }
                fs::symlink_metadata(path)
            },
        )
        .unwrap();
        assert!(disappeared);
        assert!(!copy.exists());
    }

    #[test]
    fn owned_copy_cleanup_retries_transient_metadata_access_failure() {
        let root = tempfile::tempdir().unwrap();
        let copy = root.path().join("copy");
        let original = root.path().join("original");
        fs::create_dir(&copy).unwrap();
        let transient = copy.join("transient");
        fs::write(&transient, b"owned scratch").unwrap();
        fs::write(&original, b"source").unwrap();
        let mut attempts = 0;
        remove_owned_directory_checked(
            &copy,
            |path| fs::remove_dir_all(path),
            |path| {
                if path == transient {
                    attempts += 1;
                    if attempts == 1 {
                        return Err(std::io::ErrorKind::PermissionDenied.into());
                    }
                }
                fs::symlink_metadata(path)
            },
        )
        .unwrap();
        assert_eq!(attempts, 2);
        assert!(!copy.exists());
        assert_eq!(fs::read(original).unwrap(), b"source");
    }
}
