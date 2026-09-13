//! Consistent copy of a closed browser store. The Windows adapter supplies
//! exclusive read handles, including the LevelDB LOCK guard. No original file
//! is created, removed, renamed or opened for writing.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_FILES: usize = 512;
const MAX_BYTES: u64 = 256 * 1024 * 1024;
const MAX_SECONDS: u64 = 30;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CopiedFile {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClosedStoreSnapshot {
    pub schema_version: u32,
    pub acquisition: String,
    pub files: Vec<CopiedFile>,
}
fn names(root: &Path) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    for entry in fs::read_dir(root).map_err(|_| "legacy_store_unreadable")? {
        let entry = entry.map_err(|_| "legacy_store_unreadable")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "legacy_store_invalid")?;
        if name.len() > 128
            || name.is_empty()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        {
            return Err("legacy_store_invalid".into());
        }
        let metadata = entry.file_type().map_err(|_| "legacy_store_unreadable")?;
        if !metadata.is_file() || metadata.is_symlink() {
            return Err("legacy_store_links_forbidden".into());
        }
        devbox_filesystem::ensure_no_links(entry.path())
            .map_err(|_| "legacy_store_links_forbidden")?;
        result.push(name);
        if result.len() > MAX_FILES {
            return Err("legacy_store_too_large".into());
        }
    }
    result.sort();
    if !result.iter().any(|name| name == "LOCK") || !result.iter().any(|name| name == "CURRENT") {
        return Err("legacy_store_invalid".into());
    }
    Ok(result)
}
fn check_cancel(cancelled: &AtomicBool, started: Instant) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("legacy_snapshot_cancelled".into());
    }
    if started.elapsed() > Duration::from_secs(MAX_SECONDS) {
        return Err("legacy_snapshot_timeout".into());
    }
    Ok(())
}
fn digest(file: &mut File) -> Result<(u64, String), String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "legacy_store_unreadable")?;
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "legacy_store_unreadable")?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .filter(|value| *value <= MAX_BYTES)
            .ok_or("legacy_store_too_large")?;
        hasher.update(&buffer[..count]);
    }
    Ok((
        bytes,
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}
/// `open_read` must enforce the platform's closed-source guard. All source file
/// handles remain alive through verification. A failed copy stays unaccepted in
/// the caller's staging namespace for cleanup; it never becomes a source.
pub fn copy_closed_store(
    source: &Path,
    target: &Path,
    cancelled: &AtomicBool,
    open_read: impl Fn(&Path) -> std::io::Result<File>,
) -> Result<ClosedStoreSnapshot, String> {
    let started = Instant::now();
    check_cancel(cancelled, started)?;
    devbox_filesystem::ensure_no_links(source).map_err(|_| "legacy_store_links_forbidden")?;
    let parent = target.parent().ok_or("legacy_snapshot_target_invalid")?;
    devbox_filesystem::ensure_no_links(parent).map_err(|_| "legacy_snapshot_target_invalid")?;
    let source = source
        .canonicalize()
        .map_err(|_| "legacy_store_unreadable")?;
    let parent = parent
        .canonicalize()
        .map_err(|_| "legacy_snapshot_target_invalid")?;
    let target = parent.join(target.file_name().ok_or("legacy_snapshot_target_invalid")?);
    if target.starts_with(&source) || source.starts_with(&target) {
        return Err("legacy_snapshot_overlap".into());
    }
    let identity = devbox_filesystem::filesystem_identity(&source, true)
        .map_err(|_| "legacy_store_unreadable")?;
    // Opening LOCK without write/create access is intentionally before listing.
    let _lock = open_read(&source.join("LOCK")).map_err(|_| "legacy_app_must_be_closed")?;
    let entries = names(&source)?;
    let mut handles: Vec<(PathBuf, File, devbox_filesystem::FilesystemIdentity)> = Vec::new();
    let mut total = 0_u64;
    for name in entries.iter().filter(|name| *name != "LOCK") {
        check_cancel(cancelled, started)?;
        let path = source.join(name);
        let before = devbox_filesystem::filesystem_identity(&path, false)
            .map_err(|_| "legacy_store_unreadable")?;
        let handle = open_read(&path).map_err(|_| "legacy_app_must_be_closed")?;
        let length = handle
            .metadata()
            .map_err(|_| "legacy_store_unreadable")?
            .len();
        total = total
            .checked_add(length)
            .filter(|size| *size <= MAX_BYTES)
            .ok_or("legacy_store_too_large")?;
        handles.push((path, handle, before));
    }
    fs::create_dir(&target).map_err(|_| "legacy_snapshot_target_must_be_new")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
            .map_err(|_| "legacy_snapshot_target_invalid")?;
    }
    let mut copied = Vec::new();
    for (path, handle, before) in &mut handles {
        check_cancel(cancelled, started)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("legacy_store_invalid")?;
        let (bytes, hash) = digest(handle)?;
        handle
            .seek(SeekFrom::Start(0))
            .map_err(|_| "legacy_store_unreadable")?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target.join(name))
            .map_err(|_| "legacy_snapshot_write_failed")?;
        let mut buffer = [0_u8; 65536];
        let mut written = 0_u64;
        loop {
            check_cancel(cancelled, started)?;
            let count = handle
                .read(&mut buffer)
                .map_err(|_| "legacy_store_unreadable")?;
            if count == 0 {
                break;
            }
            written += count as u64;
            if written > bytes {
                return Err("legacy_store_changed".into());
            }
            output
                .write_all(&buffer[..count])
                .map_err(|_| "legacy_snapshot_write_failed")?;
        }
        output
            .sync_all()
            .map_err(|_| "legacy_snapshot_write_failed")?;
        let (after_bytes, after_hash) = digest(handle)?;
        let mut copy = File::open(target.join(name)).map_err(|_| "legacy_snapshot_write_failed")?;
        if written != bytes
            || after_bytes != bytes
            || after_hash != hash
            || digest(&mut copy)? != (bytes, hash.clone())
            || devbox_filesystem::filesystem_identity(&*path, false)
                .map_err(|_| "legacy_store_changed")?
                != *before
        {
            return Err("legacy_store_changed".into());
        }
        copied.push(CopiedFile {
            name: name.to_string(),
            bytes,
            sha256: hash,
        });
    }
    if names(&source)? != entries
        || devbox_filesystem::filesystem_identity(&source, true)
            .map_err(|_| "legacy_store_changed")?
            != identity
    {
        return Err("legacy_store_changed".into());
    }
    check_cancel(cancelled, started)?;
    Ok(ClosedStoreSnapshot {
        schema_version: 1,
        acquisition: "closed-leveldb-exclusive-copy/v1".into(),
        files: copied,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) -> PathBuf {
        let source = root.join("legacy");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("LOCK"), []).unwrap();
        fs::write(source.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
        fs::write(source.join("MANIFEST-000001"), b"synthetic manifest").unwrap();
        fs::write(source.join("000003.log"), b"synthetic wal bytes").unwrap();
        source
    }
    #[test]
    fn keeps_original_bytes_and_copies_wal_without_reusing_its_lock() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(dir.path());
        let target = dir.path().join("copy");
        let snapshot = copy_closed_store(&source, &target, &AtomicBool::new(false), |path| {
            File::open(path)
        })
        .unwrap();
        assert_eq!(snapshot.files.len(), 3);
        assert_eq!(
            fs::read(source.join("000003.log")).unwrap(),
            fs::read(target.join("000003.log")).unwrap()
        );
        assert!(!target.join("LOCK").exists());
        assert!(source.join("LOCK").exists());
        assert!(copy_closed_store(
            &source,
            &target,
            &AtomicBool::new(false),
            |path| File::open(path)
        )
        .is_err());
    }
    #[test]
    fn busy_source_cancel_and_overlap_never_create_a_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(dir.path());
        let target = dir.path().join("copy");
        assert!(
            copy_closed_store(&source, &target, &AtomicBool::new(false), |_| Err(
                std::io::Error::from(std::io::ErrorKind::PermissionDenied)
            ))
            .is_err()
        );
        assert!(!target.exists());
        assert!(
            copy_closed_store(&source, &target, &AtomicBool::new(true), |path| File::open(
                path
            ))
            .is_err()
        );
        assert!(!target.exists());
        assert!(copy_closed_store(
            &source,
            &source.join("nested"),
            &AtomicBool::new(false),
            |path| File::open(path)
        )
        .is_err());
        assert!(!source.join("nested").exists());
    }
    #[cfg(unix)]
    #[test]
    fn linked_source_records_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(dir.path());
        fs::write(dir.path().join("outside"), b"outside fixture").unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside"), source.join("000004.log")).unwrap();
        assert!(copy_closed_store(
            &source,
            &dir.path().join("copy"),
            &AtomicBool::new(false),
            |path| File::open(path)
        )
        .is_err());
    }
}
