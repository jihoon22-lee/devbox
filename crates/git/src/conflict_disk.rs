//! Linux daemon: pinned directories, exclusive index lock and exchange/verify/undo.
//! The file and index cannot be committed atomically against arbitrary external writers.
use crate::{hunks::digest, model::SourceGitError as E};
use rustix::{
    fs::{self, AtFlags, FileType, Mode, OFlags, RenameFlags},
    io::Errno,
};
use std::{
    fs::File,
    io::{Read, Write},
    os::fd::OwnedFd,
    path::{Component, Path, PathBuf},
};
pub const TEXT_LIMIT: usize = 128 * 1024;
type R<T> = Result<T, E>;
fn fail<T>(_: T) -> E {
    E::UnsafeSource
}
pub struct Pinned {
    root: PathBuf,
    path: String,
    pub parent: OwnedFd,
    pub name: String,
}
#[derive(Clone)]
pub struct Snapshot {
    pub hash: String,
    pub bytes: Option<Vec<u8>>,
    pub mode: u32,
    pub exists: bool,
}
impl Pinned {
    pub fn open(root: &Path, path: &str) -> R<Self> {
        if path.is_empty()
            || path.contains('\0')
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == ".." || p.eq_ignore_ascii_case(".git"))
            || Path::new(path)
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
        {
            return Err(E::InvalidSourceInput);
        }
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut parent = fs::openat(fs::CWD, root, flags, Mode::empty()).map_err(fail)?;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return Ok(Self {
                    root: root.into(),
                    path: path.into(),
                    parent,
                    name: part.into(),
                });
            }
            parent = fs::openat(&parent, part, flags, Mode::empty()).map_err(fail)?;
        }
        Err(E::InvalidSourceInput)
    }
    pub fn valid(&self) -> bool {
        let Ok(now) = Self::open(&self.root, &self.path) else {
            return false;
        };
        let (Ok(a), Ok(b)) = (fs::fstat(&self.parent), fs::fstat(&now.parent)) else {
            return false;
        };
        a.st_dev == b.st_dev && a.st_ino == b.st_ino
    }
    pub fn read(&self) -> R<Snapshot> {
        read(&self.parent, &self.name)
    }
}
fn read(parent: &OwnedFd, name: &str) -> R<Snapshot> {
    let fd = match fs::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => {
            return Ok(Snapshot {
                hash: digest(b"absent"),
                bytes: None,
                mode: 0o100644,
                exists: false,
            });
        }
        Err(e) => return Err(fail(e)),
    };
    let stat = fs::fstat(&fd).map_err(fail)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(E::UnsafeSource);
    }
    let mode = stat.st_mode;
    // Oversize files are display-only; their metadata token cannot authorize an apply.
    let bytes = if stat.st_size > TEXT_LIMIT as i64 {
        None
    } else {
        let mut bytes = Vec::new();
        File::from(fd)
            .take((TEXT_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(fail)?;
        (bytes.len() <= TEXT_LIMIT).then_some(bytes)
    };
    let hash = if let Some(bytes) = &bytes {
        let mut fingerprint = mode.to_le_bytes().to_vec();
        fingerprint.extend(bytes);
        digest(&fingerprint)
    } else {
        digest(
            format!(
                "oversize:{}:{}:{}:{}:{mode}",
                stat.st_ino, stat.st_size, stat.st_mtime, stat.st_mtime_nsec
            )
            .as_bytes(),
        )
    };
    Ok(Snapshot {
        hash,
        bytes,
        mode,
        exists: true,
    })
}
fn unlink(parent: &OwnedFd, name: &str) {
    let _ = fs::unlinkat(parent, name, AtFlags::empty());
}
/// All operations after publish are synchronous: dropping the RPC future cannot interrupt undo.
pub struct Replacement<'a> {
    pinned: &'a Pinned,
    backup: String,
    original: Snapshot,
    written: Option<String>,
    published: bool,
}
impl<'a> Replacement<'a> {
    pub fn prepare(pinned: &'a Pinned, original: Snapshot, bytes: Option<&[u8]>) -> R<Self> {
        let backup = format!(".devbox-conflict-{}", uuid::Uuid::new_v4());
        let Some(bytes) = bytes else {
            return Ok(Self {
                pinned,
                backup,
                original,
                written: None,
                published: false,
            });
        };
        let fd = fs::openat(
            &pinned.parent,
            backup.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(fail)?;
        let result = (|| {
            fs::fchmod(&fd, Mode::from_raw_mode(original.mode & 0o777)).map_err(fail)?;
            let mut file = File::from(fd);
            file.write_all(bytes).map_err(fail)?;
            file.sync_all().map_err(fail)?;
            read(&pinned.parent, &backup)
        })();
        match result {
            Ok(written) => Ok(Self {
                pinned,
                backup,
                original,
                written: Some(written.hash),
                published: false,
            }),
            Err(e) => {
                unlink(&pinned.parent, &backup);
                Err(e)
            }
        }
    }
    pub fn publish(&mut self) -> R<()> {
        if !self.pinned.valid() {
            return Err(E::UnsafeSource);
        }
        if self.written.is_none() {
            if self.original.exists {
                fs::renameat_with(
                    &self.pinned.parent,
                    self.pinned.name.as_str(),
                    &self.pinned.parent,
                    self.backup.as_str(),
                    RenameFlags::NOREPLACE,
                )
                .map_err(fail)?;
            }
            self.published = true;
            return Ok(());
        }
        let flag = if self.original.exists {
            RenameFlags::EXCHANGE
        } else {
            RenameFlags::NOREPLACE
        };
        fs::renameat_with(
            &self.pinned.parent,
            self.backup.as_str(),
            &self.pinned.parent,
            self.pinned.name.as_str(),
            flag,
        )
        .map_err(fail)?;
        self.published = true;
        Ok(())
    }
    fn current_is_written(&self) -> bool {
        self.pinned.read().is_ok_and(|s| match &self.written {
            Some(hash) => s.hash == *hash,
            None => !s.exists,
        })
    }
    pub fn verified(&self) -> bool {
        self.pinned.valid()
            && self.current_is_written()
            && (!self.original.exists
                || read(&self.pinned.parent, &self.backup)
                    .is_ok_and(|s| s.hash == self.original.hash))
    }
    /// Restore the displaced bytes even when they differ from the preview. Never overwrite
    /// a new external current file; keep the displaced copy as a visible recovery file.
    pub fn rollback(&mut self) -> bool {
        if !self.published {
            return true;
        }
        if !self.current_is_written() {
            return false;
        }
        if self.written.is_none() {
            if self.original.exists
                && fs::renameat_with(
                    &self.pinned.parent,
                    self.backup.as_str(),
                    &self.pinned.parent,
                    self.pinned.name.as_str(),
                    RenameFlags::NOREPLACE,
                )
                .is_err()
            {
                return false;
            }
            self.published = false;
            return true;
        }
        let result = if self.original.exists {
            fs::renameat_with(
                &self.pinned.parent,
                self.backup.as_str(),
                &self.pinned.parent,
                self.pinned.name.as_str(),
                RenameFlags::EXCHANGE,
            )
        } else {
            fs::renameat_with(
                &self.pinned.parent,
                self.pinned.name.as_str(),
                &self.pinned.parent,
                self.backup.as_str(),
                RenameFlags::NOREPLACE,
            )
        };
        if result.is_err() {
            return false;
        }
        // A second external writer may have raced undo; only delete our own displaced bytes.
        if read(&self.pinned.parent, &self.backup)
            .is_ok_and(|s| Some(&s.hash) == self.written.as_ref())
        {
            unlink(&self.pinned.parent, &self.backup);
            self.published = false;
            true
        } else {
            false
        }
    }
    pub fn finish(mut self) {
        if self.original.exists {
            unlink(&self.pinned.parent, &self.backup);
        }
        self.published = false;
    }
}
impl Drop for Replacement<'_> {
    fn drop(&mut self) {
        if !self.published {
            unlink(&self.pinned.parent, &self.backup);
        }
    }
}

pub struct IndexLock {
    path: PathBuf,
    lock: PathBuf,
    file: File,
    original: Vec<u8>,
    committed: bool,
}
impl IndexLock {
    pub fn acquire(path: PathBuf) -> R<Self> {
        let lock = path.with_extension("lock");
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    E::SourceBusy
                } else {
                    E::SourceFailed
                }
            })?;
        let mut owned = Self {
            path,
            lock,
            file,
            original: Vec::new(),
            committed: false,
        };
        owned.original = read_index(&owned.path)?;
        Ok(owned)
    }

    pub fn original(&self) -> &[u8] {
        &self.original
    }
    pub fn unchanged(&self) -> bool {
        read_index(&self.path).is_ok_and(|b| b == self.original) && self.owned()
    }
    fn owned(&self) -> bool {
        use std::os::unix::fs::MetadataExt;
        self.file
            .metadata()
            .ok()
            .zip(std::fs::symlink_metadata(&self.lock).ok())
            .is_some_and(|(a, b)| a.dev() == b.dev() && a.ino() == b.ino() && b.is_file())
    }
    pub fn commit(&mut self, bytes: &[u8]) -> R<()> {
        if !self.unchanged() {
            return Err(E::SourceFailed);
        }
        self.file.write_all(bytes).map_err(fail)?;
        self.file.sync_all().map_err(fail)?;
        if !self.unchanged() {
            return Err(E::SourceFailed);
        }
        std::fs::rename(&self.lock, &self.path).map_err(fail)?;
        self.committed = true;
        Ok(())
    }
}
impl Drop for IndexLock {
    fn drop(&mut self) {
        if !self.committed && self.owned() {
            let _ = std::fs::remove_file(&self.lock);
        }
    }
}

fn read_index(path: &Path) -> R<Vec<u8>> {
    let fd = fs::openat(
        fs::CWD,
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(fail)?;
    if FileType::from_raw_mode(fs::fstat(&fd).map_err(fail)?.st_mode) != FileType::RegularFile {
        return Err(E::UnsafeSource);
    }
    const LIMIT: usize = 32 * 1024 * 1024;
    let mut bytes = Vec::new();
    File::from(fd)
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    if bytes.len() > LIMIT {
        return Err(E::SourceLimitExceeded);
    }
    Ok(bytes)
}
