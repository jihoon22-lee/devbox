//! Data-folder records for same-folder temp files (`.devbox-tmp-*`) and for the
//! folders that held claimed documents, so startup can sweep crash leftovers
//! without walking whole project trees.
use crate::scope::WriteScope;
use crate::sys::{SysError, hex, open_dir};
use rustix::fs::{AtFlags, FileType, statat, unlinkat};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

pub const TMP_PREFIX: &str = ".devbox-tmp-";
pub const STALE: Duration = Duration::from_secs(3600);
const FORGET_UNREACHABLE: Duration = Duration::from_secs(7 * 24 * 3600);
const FORGET_DIR: Duration = Duration::from_secs(30 * 24 * 3600);
const WALK_ENTRIES: usize = 200_000;
const WALK_DEPTH: usize = 64;

fn write_durable(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), SysError> {
    private_dir(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(dir.join(name))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    rustix::fs::fsync(open_dir(dir)?)?;
    Ok(())
}
/// Creates missing folders as 0700 (journals and ledgers hold private paths/text).
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}
fn age(meta: &std::fs::Metadata) -> Duration {
    meta.modified()
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .unwrap_or_default()
}
fn entries(dir: &Path) -> Vec<(PathBuf, Duration)> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return vec![];
    };
    read.flatten()
        .filter_map(|e| Some((e.path(), age(&e.metadata().ok()?))))
        .collect()
}

pub struct TmpLedger {
    dir: PathBuf,
}
pub struct Entry(PathBuf);
impl TmpLedger {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    pub fn record(&self, tmp: &Path) -> Result<Entry, SysError> {
        let name = uuid::Uuid::new_v4().simple().to_string();
        write_durable(&self.dir, &name, tmp.as_os_str().as_bytes())?;
        Ok(Entry(self.dir.join(name)))
    }
    pub fn remove(&self, entry: Entry) -> Result<(), SysError> {
        match std::fs::remove_file(&entry.0) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
    /// Removes recorded temp files (and their records) older than `older`.
    pub fn sweep(&self, scope: &WriteScope, older: Duration) -> usize {
        let mut removed = 0;
        for (entry, entry_age) in entries(&self.dir) {
            if entry_age < older {
                continue;
            }
            let Ok(bytes) = std::fs::read(&entry) else {
                continue;
            };
            let tmp = PathBuf::from(std::ffi::OsString::from_vec(bytes));
            match remove_tmp(scope, &tmp, None) {
                Reach::Removed => removed += 1,
                Reach::Unreachable if entry_age < FORGET_UNREACHABLE => continue,
                _ => {}
            }
            let _ = std::fs::remove_file(&entry);
        }
        removed
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Reach {
    Removed,
    Kept,
    Unreachable,
}
/// Unlinks one `.devbox-tmp-*` regular file inside a registered root.
fn remove_tmp(scope: &WriteScope, path: &Path, min_age: Option<Duration>) -> Reach {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return Reach::Kept;
    };
    if !name.starts_with(TMP_PREFIX) {
        return Reach::Kept;
    }
    let canonical = std::fs::canonicalize(parent).ok();
    if canonical.as_deref() != Some(parent) || !scope.roots().any(|(_, r)| parent.starts_with(r)) {
        return Reach::Unreachable;
    }
    let Ok(dir) = open_dir(parent) else {
        return Reach::Unreachable;
    };
    match statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(st) if FileType::from_raw_mode(st.st_mode) == FileType::RegularFile => {
            let modified = st.st_mtime.max(0) as u64;
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if min_age.is_some_and(|min| now.saturating_sub(modified) < min.as_secs()) {
                return Reach::Kept;
            }
            match unlinkat(&dir, name, AtFlags::empty()) {
                Ok(()) => Reach::Removed,
                Err(_) => Reach::Kept,
            }
        }
        _ => Reach::Kept,
    }
}

/// Sweeps unrecorded stale temp files in `dir` (recursively for the vault).
pub fn sweep_dir(scope: &WriteScope, dir: &Path, recursive: bool) -> usize {
    let mut removed = 0;
    let mut budget = WALK_ENTRIES;
    let mut stack = vec![(dir.to_path_buf(), 0_usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            if budget == 0 {
                return removed;
            }
            budget -= 1;
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name();
            if kind.is_file() && name.as_bytes().starts_with(TMP_PREFIX.as_bytes()) {
                if remove_tmp(scope, &entry.path(), Some(STALE)) == Reach::Removed {
                    removed += 1;
                }
            } else if kind.is_dir() && recursive && depth < WALK_DEPTH && name != ".git" {
                stack.push((entry.path(), depth + 1));
            }
        }
    }
    removed
}

/// Folders that held claimed documents, persisted for the next startup sweep.
pub struct ClaimedDirs {
    dir: PathBuf,
    seen: Mutex<HashSet<PathBuf>>,
}
impl ClaimedDirs {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            seen: Mutex::default(),
        }
    }
    pub fn note(&self, folder: &Path) -> Result<(), SysError> {
        if !self
            .seen
            .lock()
            .expect("claimed dirs")
            .insert(folder.to_path_buf())
        {
            return Ok(());
        }
        let name = hex(&Sha256::digest(folder.as_os_str().as_bytes()));
        write_durable(&self.dir, &name, folder.as_os_str().as_bytes())
    }
    pub fn sweep(&self, scope: &WriteScope) -> usize {
        let mut removed = 0;
        for (entry, entry_age) in entries(&self.dir) {
            if entry_age > FORGET_DIR {
                let _ = std::fs::remove_file(&entry);
                continue;
            }
            if let Ok(bytes) = std::fs::read(&entry) {
                let folder = PathBuf::from(std::ffi::OsString::from_vec(bytes));
                removed += sweep_dir(scope, &folder, false);
            }
        }
        removed
    }
}
