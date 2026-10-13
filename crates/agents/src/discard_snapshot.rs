//! Hash every byte force removal can delete, including ignored files, without following links.
use crate::{
    model::*,
    service::{AgentsService, Result},
};
use devbox_protocol::Internal;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::{
    fd::AsRawFd,
    unix::fs::{MetadataExt, OpenOptionsExt},
};
use std::{
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
fn tree(
    path: &Path,
    relative: &Path,
    hash: &mut Sha256,
    budget: &mut Budget,
    depth: usize,
) -> ScanResult<()> {
    budget.entry(depth)?;
    let directory = open(path, true)?;
    let before = directory.metadata().map_err(|_| "unreadable")?;
    field(hash, relative.as_os_str().as_encoded_bytes());
    field(hash, b"directory");
    hash.update(mode(&before).to_le_bytes());
    // Pin each parent directory while opening children. A replaced ancestor cannot redirect reads.
    #[cfg(unix)]
    let scan = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    #[cfg(not(unix))]
    let scan = path.to_owned();
    let mut entries = Vec::new();
    for entry in fs::read_dir(&scan).map_err(|_| "unreadable")? {
        budget.check()?;
        let entry = entry.map_err(|_| "unreadable")?;
        if depth == 0 && entry.file_name() == ".git" {
            continue;
        }
        if entries.len() + budget.entries >= budget.limits.entries {
            return Err("entries_or_depth");
        }
        entries.push(entry.file_name());
    }
    entries.sort();
    for name in entries {
        budget.check()?;
        let child = scan.join(&name);
        let rel = relative.join(&name);
        let meta = fs::symlink_metadata(&child).map_err(|_| "unreadable_or_changed")?;
        #[cfg(unix)]
        if meta.dev() != before.dev() {
            return Err("mounted_entry");
        }
        if meta.is_dir() {
            tree(&child, &rel, hash, budget, depth + 1)?;
            continue;
        }
        budget.entry(depth + 1)?;
        field(hash, rel.as_os_str().as_encoded_bytes());
        hash.update(mode(&meta).to_le_bytes());
        if meta.is_symlink() {
            field(hash, b"symlink");
            let target = fs::read_link(&child).map_err(|_| "unreadable_or_changed")?;
            field(hash, target.as_os_str().as_encoded_bytes());
            let after = fs::symlink_metadata(&child).map_err(|_| "changed")?;
            if !same(&meta, &after) {
                return Err("changed");
            }
        } else if meta.is_file() {
            if meta.len() > budget.limits.file {
                return Err("file_bytes");
            }
            if budget.bytes.saturating_add(meta.len()) > budget.limits.total {
                return Err("total_bytes");
            }
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
        } else {
            return Err("special_file");
        }
    }
    if !same(&before, &directory.metadata().map_err(|_| "unreadable")?) {
        return Err("changed");
    }
    budget.check()
}
fn scan(dir: &Path, seed: &[u8], limits: Limits) -> ScanResult<String> {
    let mut hash = Sha256::new();
    field(&mut hash, b"devbox-discard-v1");
    field(&mut hash, seed);
    let mut budget = Budget {
        limits,
        started: Instant::now(),
        bytes: 0,
        entries: 0,
        buffer: vec![0_u8; 64 * 1024],
    };
    tree(dir, Path::new(""), &mut hash, &mut budget, 0)?;
    Ok(format!("{:x}", hash.finalize()))
}
impl AgentsService {
    pub(crate) async fn discard_fingerprint(
        &self,
        dir: &Path,
        head: &str,
        base: &str,
    ) -> Result<String> {
        let index = devbox_git::run::git(dir, &["ls-files", "--stage", "-z"])
            .await
            .map_err(git_failure)?;
        let mut seed = format!("{head}\0{base}\0").into_bytes();
        seed.extend_from_slice(index.as_bytes());
        let directory = dir.to_owned();
        let fingerprint = tokio::task::spawn_blocking(move || scan(&directory, &seed, LIMITS))
            .await
            .internal("hash discard snapshot")?
            .map_err(|reason| AgentsError::DiscardSnapshotUnavailable {
                reason: reason.into(),
            })?;
        let after = devbox_git::run::git(dir, &["ls-files", "--stage", "-z"])
            .await
            .map_err(git_failure)?;
        if after != index {
            return Err(AgentsError::DiscardSnapshotUnavailable {
                reason: "index_changed".into(),
            }
            .into());
        }
        Ok(fingerprint)
    }
    pub(crate) async fn check_discard_fingerprint(
        &self,
        dir: &Path,
        head: &str,
        base: &str,
        expect: &ReviewExpect,
    ) -> Result<()> {
        let expected = expect
            .discard_fingerprint
            .as_ref()
            .ok_or(AgentsError::InvalidInput)?;
        let actual = self.discard_fingerprint(dir, head, base).await?;
        if expected != &actual {
            let unmerged = devbox_git::mutate::unmerged_count(dir, head, base)
                .await
                .map_err(git_failure)?;
            return Err(AgentsError::StateChanged {
                head: head.into(),
                base: base.into(),
                unmerged,
            }
            .into());
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn deep_tree_keeps_streaming_buffer_off_the_recursive_stack() {
        const CHILD: &str = "DEVBOX_TEST_DEEP_HASH_CHILD";
        if let Some(path) = std::env::var_os(CHILD) {
            scan(Path::new(&path), b"index", LIMITS).expect("deep tree hash");
            return;
        }
        let root = tempfile::tempdir().expect("fixture");
        let mut directory = root.path().to_owned();
        for _ in 0..40 {
            directory = directory.join("d");
            fs::create_dir(&directory).expect("owned directory");
        }
        fs::write(directory.join("file"), "owned bytes").expect("file");
        // Disable only the owned child's core dump; a failing regression must leave no host artifact.
        let status = std::process::Command::new("bash")
            .args(["-c", "ulimit -c 0\nexec \"$@\"", "--"])
            .arg(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "discard_snapshot::tests::deep_tree_keeps_streaming_buffer_off_the_recursive_stack",
                "--nocapture",
            ])
            .env(CHILD, root.path())
            .status()
            .expect("owned child");
        assert!(
            status.success(),
            "a small deep tree must not exhaust the scanner's stack: {status}"
        );
    }
    #[cfg(unix)]
    #[test]
    fn fifo_replacement_cannot_block_the_open_before_deadline_checks() {
        const CHILD: &str = "DEVBOX_TEST_FIFO_OPEN_CHILD";
        if let Some(path) = std::env::var_os(CHILD) {
            assert!(
                open(Path::new(&path), false).is_err(),
                "fstat must reject a FIFO after nonblocking open"
            );
            return;
        }
        let root = tempfile::tempdir().expect("fixture");
        let path = root.path().join("replacement");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&path)
                .status()
                .expect("owned FIFO")
                .success()
        );
        struct OwnedChild(std::process::Child);
        impl Drop for OwnedChild {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = OwnedChild(std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "discard_snapshot::tests::fifo_replacement_cannot_block_the_open_before_deadline_checks", "--nocapture"])
            .env(CHILD, &path).spawn().expect("owned child"));
        let started = Instant::now();
        loop {
            if let Some(status) = child.0.try_wait().expect("child state") {
                assert!(status.success());
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "regular-file open blocked on a raced FIFO before any deadline check"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn fingerprint_reads_bytes_modes_links_and_fails_closed_on_limits() {
        let root = tempfile::tempdir().expect("fixture");
        fs::write(root.path().join("file"), "one").expect("file");
        let first = scan(root.path(), b"index", LIMITS).expect("hash");
        fs::write(root.path().join("file"), "two").expect("file");
        assert_ne!(first, scan(root.path(), b"index", LIMITS).expect("hash"));
        assert_ne!(
            first,
            scan(root.path(), b"different-index", LIMITS).expect("hash")
        );
        let mut small = LIMITS;
        small.file = 2;
        assert_eq!(scan(root.path(), b"", small), Err("file_bytes"));
        small = LIMITS;
        small.deadline = Duration::ZERO;
        assert_eq!(scan(root.path(), b"", small), Err("deadline"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let before = scan(root.path(), b"index", LIMITS).expect("hash");
            fs::set_permissions(root.path().join("file"), fs::Permissions::from_mode(0o700))
                .expect("mode");
            assert_ne!(before, scan(root.path(), b"index", LIMITS).expect("hash"));
            std::os::unix::fs::symlink("/unreadable-external-target", root.path().join("link"))
                .expect("link");
            assert!(
                scan(root.path(), b"index", LIMITS).is_ok(),
                "link target must never be opened"
            );
        }
    }
}
