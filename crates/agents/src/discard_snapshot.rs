//! Fingerprint everything force removal can delete, without following links. Files are hashed
//! by content, except that Git-ignored ones are compared by size, mtime, mode and inode and never
//! read. Hashing runs outside the lifecycle lock; under it, a discard only compares metadata.
use crate::{
    model::*,
    service::{AgentsService, Result},
};
use devbox_protocol::{Fail, Internal};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::{
    fd::AsRawFd,
    unix::fs::{MetadataExt, OpenOptionsExt},
};
use std::{
    collections::HashSet,
    fs::{self, File, Metadata, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
struct Limits {
    file: u64,
    total: u64,
    entries: usize,
    depth: usize,
    deadline: Duration,
}
const LIMITS: Limits = Limits {
    file: 2 * 1024 * 1024 * 1024,
    total: 8 * 1024 * 1024 * 1024,
    entries: 1_000_000,
    depth: 256,
    deadline: Duration::from_secs(120),
};
struct Budget {
    limits: Limits,
    started: Instant,
    bytes: u64,
    entries: usize,
    buffer: Vec<u8>,
    #[cfg(test)]
    root: PathBuf,
}
type ScanResult<T> = std::result::Result<T, &'static str>;
impl Budget {
    fn check(&self) -> ScanResult<()> {
        if self.started.elapsed() > self.limits.deadline {
            Err("deadline")
        } else {
            Ok(())
        }
    }
    fn entry(&mut self, depth: usize) -> ScanResult<()> {
        self.check()?;
        self.entries += 1;
        if self.entries > self.limits.entries || depth > self.limits.depth {
            return Err("entries_or_depth");
        }
        Ok(())
    }
}
fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
fn mode(meta: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        meta.mode()
    }
    #[cfg(not(unix))]
    {
        u32::from(meta.permissions().readonly())
    }
}
fn same(before: &Metadata, after: &Metadata) -> bool {
    let common = before.len() == after.len()
        && mode(before) == mode(after)
        && before.modified().ok() == after.modified().ok();
    #[cfg(unix)]
    {
        common
            && before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec()
    }
    #[cfg(not(unix))]
    {
        common
    }
}
fn open(path: &Path, directory: bool) -> ScanResult<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        options.custom_flags(
            nix::libc::O_NOFOLLOW
                | nix::libc::O_NONBLOCK
                | if directory { nix::libc::O_DIRECTORY } else { 0 },
        );
    }
    let file = options.open(path).map_err(|_| "unreadable_or_changed")?;
    let meta = file.metadata().map_err(|_| "unreadable")?;
    if directory != meta.is_dir() || (!directory && !meta.is_file()) {
        return Err("changed_or_special_file");
    }
    Ok(file)
}
/// What `git ls-files --others --ignored --exclude-standard --directory` lists: ignored files,
/// and directories ignored with everything below them.
#[derive(Default)]
struct Ignored {
    files: HashSet<PathBuf>,
    directories: HashSet<PathBuf>,
}
impl Ignored {
    fn parse(listing: &str) -> Self {
        let mut ignored = Self::default();
        for path in listing.split_terminator('\0') {
            match path.strip_suffix('/') {
                Some(directory) => ignored.directories.insert(directory.into()),
                None => ignored.files.insert(path.into()),
            };
        }
        ignored
    }
}
/// One walk of a worktree.
struct Walk<'a> {
    budget: Budget,
    /// The fingerprint the user confirms.
    hash: Sha256,
    /// Every entry's metadata, compared again under the lifecycle lock.
    stamp: Sha256,
    /// `false` compares metadata only: no fingerprint, and no file is opened.
    read: bool,
    ignored: &'a Ignored,
}
struct Walked {
    fingerprint: String,
    stamp: String,
}
/// Metadata that changes whenever an entry's bytes, mode, name or identity change.
fn stamp(stamp: &mut Sha256, relative: &Path, meta: &Metadata) {
    field(stamp, relative.as_os_str().as_encoded_bytes());
    stamp.update(meta.len().to_le_bytes());
    stamp.update(mode(meta).to_le_bytes());
    #[cfg(unix)]
    for value in [
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    ] {
        stamp.update(value.to_le_bytes());
    }
    #[cfg(unix)]
    for value in [meta.dev(), meta.ino()] {
        stamp.update(value.to_le_bytes());
    }
    #[cfg(not(unix))]
    field(stamp, format!("{:?}", meta.modified().ok()).as_bytes());
}
fn tree(
    path: &Path,
    relative: &Path,
    walk: &mut Walk,
    depth: usize,
    ignored: bool,
) -> ScanResult<()> {
    walk.budget.entry(depth)?;
    let directory = open(path, true)?;
    let before = directory.metadata().map_err(|_| "unreadable")?;
    stamp(&mut walk.stamp, relative, &before);
    if walk.read {
        field(&mut walk.hash, relative.as_os_str().as_encoded_bytes());
        field(&mut walk.hash, b"directory");
        walk.hash.update(mode(&before).to_le_bytes());
    }
    // Pin each parent directory while opening children. A replaced ancestor cannot redirect reads.
    #[cfg(unix)]
    let scan = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    #[cfg(not(unix))]
    let scan = path.to_owned();
    let mut entries = Vec::new();
    for entry in fs::read_dir(&scan).map_err(|_| "unreadable")? {
        walk.budget.check()?;
        let entry = entry.map_err(|_| "unreadable")?;
        if depth == 0 && entry.file_name() == ".git" {
            continue;
        }
        if entries.len() + walk.budget.entries >= walk.budget.limits.entries {
            return Err("entries_or_depth");
        }
        entries.push(entry.file_name());
    }
    entries.sort();
    for name in entries {
        walk.budget.check()?;
        let child = scan.join(&name);
        let rel = relative.join(&name);
        let meta = fs::symlink_metadata(&child).map_err(|_| "unreadable_or_changed")?;
        #[cfg(unix)]
        if meta.dev() != before.dev() {
            return Err("mounted_entry");
        }
        if meta.is_dir() {
            let ignored = ignored || walk.ignored.directories.contains(&rel);
            tree(&child, &rel, walk, depth + 1, ignored)?;
            continue;
        }
        walk.budget.entry(depth + 1)?;
        if !meta.is_file() && !meta.is_symlink() {
            return Err("special_file");
        }
        stamp(&mut walk.stamp, &rel, &meta);
        if !walk.read {
            continue;
        }
        let hash = &mut walk.hash;
        field(hash, rel.as_os_str().as_encoded_bytes());
        hash.update(mode(&meta).to_le_bytes());
        if ignored || walk.ignored.files.contains(&rel) {
            // Build output, dependencies and the like: compared by metadata, never opened.
            field(hash, b"ignored");
            hash.update(meta.len().to_le_bytes());
            #[cfg(unix)]
            for value in [meta.mtime(), meta.mtime_nsec()] {
                hash.update(value.to_le_bytes());
            }
            #[cfg(unix)]
            hash.update(meta.ino().to_le_bytes());
            #[cfg(not(unix))]
            field(hash, format!("{:?}", meta.modified().ok()).as_bytes());
        } else if meta.is_symlink() {
            field(hash, b"symlink");
            let target = fs::read_link(&child).map_err(|_| "unreadable_or_changed")?;
            field(hash, target.as_os_str().as_encoded_bytes());
            let after = fs::symlink_metadata(&child).map_err(|_| "changed")?;
            if !same(&meta, &after) {
                return Err("changed");
            }
        } else {
            let budget = &mut walk.budget;
            if meta.len() > budget.limits.file {
                return Err("file_bytes");
            }
            if budget.bytes.saturating_add(meta.len()) > budget.limits.total {
                return Err("total_bytes");
            }
            #[cfg(test)]
            probe::read(&budget.root, &rel);
            let mut file = open(&child, false)?;
            let opened = file.metadata().map_err(|_| "unreadable")?;
            if !same(&meta, &opened) {
                return Err("changed");
            }
            field(hash, b"file");
            hash.update(meta.len().to_le_bytes());
            let mut bytes = 0_u64;
            loop {
                budget.check()?;
                let count = file.read(&mut budget.buffer).map_err(|_| "unreadable")?;
                if count == 0 {
                    break;
                }
                bytes += count as u64;
                budget.bytes += count as u64;
                if bytes > budget.limits.file || budget.bytes > budget.limits.total {
                    return Err("bytes");
                }
                hash.update(&budget.buffer[..count]);
            }
            let after = fs::symlink_metadata(&child).map_err(|_| "changed")?;
            if bytes != meta.len()
                || !same(&meta, &after)
                || !same(&meta, &file.metadata().map_err(|_| "unreadable")?)
            {
                return Err("changed");
            }
        }
    }
    if !same(&before, &directory.metadata().map_err(|_| "unreadable")?) {
        return Err("changed");
    }
    walk.budget.check()
}
fn walk(
    dir: &Path,
    seed: &[u8],
    ignored: &Ignored,
    read: bool,
    limits: Limits,
) -> ScanResult<Walked> {
    let mut walk = Walk {
        budget: Budget {
            limits,
            started: Instant::now(),
            bytes: 0,
            entries: 0,
            buffer: vec![0_u8; if read { 64 * 1024 } else { 0 }],
            #[cfg(test)]
            root: dir.to_owned(),
        },
        hash: Sha256::new(),
        stamp: Sha256::new(),
        read,
        ignored,
    };
    field(&mut walk.hash, b"devbox-discard-v2");
    field(&mut walk.hash, seed);
    tree(dir, Path::new(""), &mut walk, 0, false)?;
    Ok(Walked {
        fingerprint: format!("{:x}", walk.hash.finalize()),
        stamp: format!("{:x}", walk.stamp.finalize()),
    })
}
/// The fingerprint and metadata stamp of `dir`. Reads every file Git does not ignore.
fn hash(dir: &Path, seed: &[u8], ignored: &Ignored, limits: Limits) -> ScanResult<Walked> {
    walk(dir, seed, ignored, true, limits)
}
/// The metadata stamp of `dir` alone: opens its directories, never a file.
fn metadata_stamp(dir: &Path, limits: Limits) -> ScanResult<String> {
    walk(dir, b"", &Ignored::default(), false, limits).map(|walked| walked.stamp)
}
/// A discard's hash, made before it took the lifecycle lock.
pub(crate) struct DiscardSnapshot {
    dir: PathBuf,
    head: String,
    base: String,
    index: String,
    fingerprint: String,
    stamp: String,
}
fn unavailable(reason: &str) -> Fail<AgentsError> {
    AgentsError::DiscardSnapshotUnavailable {
        reason: reason.into(),
    }
    .into()
}
/// The user confirms again: the worktree, its index, head or base changed.
async fn changed(dir: &Path, head: &str, base: &str) -> Fail<AgentsError> {
    match devbox_git::mutate::unmerged_count(dir, head, base).await {
        Ok(unmerged) => AgentsError::StateChanged {
            head: head.into(),
            base: base.into(),
            unmerged,
        }
        .into(),
        Err(error) => git_failure(error),
    }
}
async fn index(dir: &Path) -> Result<String> {
    devbox_git::run::git(dir, &["ls-files", "--stage", "-z"])
        .await
        .map_err(git_failure)
}
impl AgentsService {
    async fn snapshot(&self, dir: &Path, head: &str, base: &str) -> Result<DiscardSnapshot> {
        let index_before = index(dir).await?;
        let listing = devbox_git::run::git(
            dir,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
                "-z",
            ],
        )
        .await
        .map_err(git_failure)?;
        let mut seed = format!("{head}\0{base}\0").into_bytes();
        seed.extend_from_slice(index_before.as_bytes());
        let directory = dir.to_owned();
        let walked = tokio::task::spawn_blocking(move || {
            hash(&directory, &seed, &Ignored::parse(&listing), LIMITS)
        })
        .await
        .internal("hash discard snapshot")?
        .map_err(unavailable)?;
        if index(dir).await? != index_before {
            return Err(unavailable("index_changed"));
        }
        Ok(DiscardSnapshot {
            dir: dir.to_owned(),
            head: head.into(),
            base: base.into(),
            index: index_before,
            fingerprint: walked.fingerprint,
            stamp: walked.stamp,
        })
    }
    pub(crate) async fn discard_fingerprint(
        &self,
        dir: &Path,
        head: &str,
        base: &str,
    ) -> Result<String> {
        Ok(self.snapshot(dir, head, base).await?.fingerprint)
    }
    /// Hashes the worktree a discard would remove, before the lifecycle lock. `None` when the
    /// request has nothing to hash yet; errors wait for their usual place under the lock.
    pub(crate) async fn hash_for_discard(
        &self,
        p: &DiscardAgent,
    ) -> Option<Result<DiscardSnapshot>> {
        p.expect.discard_fingerprint.as_ref()?;
        let task = self.get(&p.id).await.ok()?;
        crate::actions::open(&task).ok()?;
        let (project, dir, _) = self.owned_worktree(&task).await.ok()?;
        let base = devbox_git::run::resolve(Path::new(&project.path), &task.base_branch)
            .await
            .ok()?;
        Some(self.snapshot(&dir, &p.expect.head_oid, &base).await)
    }
    /// Under the lifecycle lock, in place of hashing: the hash made before the lock matches
    /// what the user confirmed, and the worktree has not changed since it was made.
    pub(crate) async fn confirm_discard_snapshot(
        &self,
        hashed: Option<Result<DiscardSnapshot>>,
        dir: &Path,
        head: &str,
        base: &str,
        expect: &ReviewExpect,
    ) -> Result<DiscardSnapshot> {
        let expected = expect
            .discard_fingerprint
            .as_ref()
            .ok_or(AgentsError::InvalidInput)?;
        // No hash: the task changed while the request waited for the lock.
        let Some(snapshot) = hashed.transpose()? else {
            return Err(changed(dir, head, base).await);
        };
        if &snapshot.fingerprint != expected {
            return Err(changed(dir, head, base).await);
        }
        self.verify_discard_snapshot(&snapshot, dir, head, base)
            .await?;
        Ok(snapshot)
    }
    /// Under the lifecycle lock: the worktree, index, head and base are still the hashed ones.
    /// Compares metadata only and never opens a file, so any change since refuses the discard.
    pub(crate) async fn verify_discard_snapshot(
        &self,
        snapshot: &DiscardSnapshot,
        dir: &Path,
        head: &str,
        base: &str,
    ) -> Result<()> {
        if snapshot.dir != dir
            || snapshot.head != head
            || snapshot.base != base
            || index(dir).await? != snapshot.index
        {
            return Err(changed(dir, head, base).await);
        }
        let directory = dir.to_owned();
        let stamp = tokio::task::spawn_blocking(move || metadata_stamp(&directory, LIMITS))
            .await
            .internal("verify discard snapshot")?
            .map_err(unavailable)?;
        if stamp != snapshot.stamp {
            return Err(changed(dir, head, base).await);
        }
        Ok(())
    }
}
/// Test observation of each file whose bytes a discard hash reads.
#[cfg(test)]
pub(crate) mod probe {
    use std::{
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };
    type Watch = Arc<dyn Fn(&Path) + Send + Sync>;
    /// Keyed by the hashed directory, so each test sees only its own worktree.
    static WATCHES: Mutex<Vec<(PathBuf, Watch)>> = Mutex::new(Vec::new());
    /// Watching stops when this is dropped.
    pub(crate) struct Guard(PathBuf);
    /// Calls `watch` with each file, relative to `root`, just before its bytes are read.
    pub(crate) fn watch(root: &Path, watch: impl Fn(&Path) + Send + Sync + 'static) -> Guard {
        let mut watches = WATCHES.lock().expect("discard probe");
        watches.push((root.to_owned(), Arc::new(watch)));
        Guard(root.to_owned())
    }
    pub(super) fn read(root: &Path, relative: &Path) {
        let watches: Vec<Watch> = WATCHES
            .lock()
            .expect("discard probe")
            .iter()
            .filter(|(watched, _)| watched == root)
            .map(|(_, watch)| watch.clone())
            .collect();
        for watch in watches {
            watch(relative);
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Ok(mut watches) = WATCHES.lock() {
                watches.retain(|(watched, _)| watched != &self.0);
            }
        }
    }
}
#[cfg(test)]
#[path = "discard_scan_tests.rs"]
mod tests;
