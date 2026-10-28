//! Content-addressed files that tool sessions load from `agent-hooks/`.
use sha2::{Digest, Sha256};
use std::{
    io::{Error, ErrorKind, Write},
    path::{Path, PathBuf},
};

/// Writes `<dir>/<stem>-<first 16 hex of sha256(body)>.<ext>` (0600) once.
/// A running session keeps reading the file it started with, so a new body gets a new
/// name and an existing name is reused only when it is a regular file with the same
/// bytes; anything else is `AlreadyExists`.
/// The body is synced in a temporary file in `dir` and published by a rename that never
/// replaces an existing name. Concurrent writers therefore need no lock, and an
/// interrupted write leaves at most a stray temporary file, never a partial file at the
/// final name. Existing callers still hold `AgentsService::hook_files`.
pub(crate) fn write_artifact(
    dir: &Path,
    stem: &str,
    ext: &str,
    body: &str,
) -> std::io::Result<PathBuf> {
    let digest = Sha256::digest(body.as_bytes());
    let hash: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("{stem}-{hash}.{ext}"));
    crate::prepare::no_symlink_ancestors(dir)?;
    let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let private = std::fs::Permissions::from_mode(0o600);
        temporary.as_file().set_permissions(private)?;
    }
    temporary.write_all(body.as_bytes())?;
    temporary.as_file().sync_all()?;
    match temporary.persist_noclobber(&path) {
        Ok(_) => Ok(path),
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
            // Dropping the unpublished temporary file deletes it.
            drop(error.file);
            let same = std::fs::symlink_metadata(&path)?.file_type().is_file()
                && std::fs::read(&path)? == body.as_bytes();
            if same {
                Ok(path)
            } else {
                Err(Error::new(
                    ErrorKind::AlreadyExists,
                    "existing file differs",
                ))
            }
        }
        // Converting drops, and so deletes, the temporary file.
        Err(error) => Err(error.into()),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::write_artifact;
    use std::{
        fs,
        io::ErrorKind,
        os::unix::fs::{PermissionsExt, symlink},
        sync::atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn same_body_reuses_the_same_file() {
        let dir = tempfile::tempdir().expect("owned artifact directory");
        let first = write_artifact(dir.path(), "claude", "json", "abc").expect("first write");
        assert_eq!(first, dir.path().join("claude-ba7816bf8f01cfea.json"));
        let again = write_artifact(dir.path(), "claude", "json", "abc").expect("same body");
        assert_eq!(again, first);
        assert_eq!(fs::read_to_string(&first).expect("artifact"), "abc");
        let mode = fs::metadata(&first).expect("artifact").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(fs::read_dir(dir.path()).expect("artifacts").count(), 1);
    }

    #[test]
    fn different_body_gets_a_new_name_and_keeps_the_old_file() {
        let dir = tempfile::tempdir().expect("owned artifact directory");
        let old = write_artifact(dir.path(), "claude", "json", "old body").expect("old");
        let new = write_artifact(dir.path(), "claude", "json", "new body").expect("new");
        assert_ne!(old, new);
        assert_eq!(fs::read(&old).expect("old artifact"), b"old body");
        assert_eq!(fs::read(&new).expect("new artifact"), b"new body");
    }

    #[test]
    fn symlink_or_changed_file_is_rejected() {
        let dir = tempfile::tempdir().expect("owned artifact directory");
        let changed = write_artifact(dir.path(), "claude", "json", "body").expect("write");
        fs::write(&changed, "changed").expect("change owned artifact");
        let error = write_artifact(dir.path(), "claude", "json", "body").expect_err("changed");
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&changed).expect("changed artifact"), b"changed");

        // Same bytes behind a symlink are still not ours to hand to a session.
        let other = tempfile::tempdir().expect("owned artifact directory");
        let target = other.path().join("target.json");
        fs::write(&target, "body").expect("owned symlink target");
        let link = other
            .path()
            .join(changed.file_name().expect("artifact name"));
        symlink(&target, &link).expect("owned symlink");
        let error = write_artifact(other.path(), "claude", "json", "body").expect_err("symlink");
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert!(
            fs::symlink_metadata(&link)
                .expect("link")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn write_artifact_rejects_a_symlinked_directory() {
        let root = tempfile::tempdir().expect("owned artifact directory");
        let target = root.path().join("elsewhere");
        fs::create_dir(&target).expect("owned link target");
        let link = root.path().join("agent-hooks");
        symlink(&target, &link).expect("owned symlink");
        assert!(write_artifact(&link, "opencode-plugin", "ts", "body").is_err());
        // Below a symlinked ancestor too.
        let nested = target.join("nested");
        fs::create_dir(&nested).expect("owned nested directory");
        assert!(write_artifact(&link.join("nested"), "claude", "json", "body").is_err());
        assert_eq!(fs::read_dir(&target).expect("link target").count(), 1);
        assert_eq!(fs::read_dir(&nested).expect("nested target").count(), 0);
    }

    #[test]
    fn concurrent_writers_of_the_same_body_all_succeed() {
        let dir = tempfile::tempdir().expect("owned artifact directory");
        let body = "devbox hook artifact body\n".repeat(1 << 16);
        assert!(body.len() >= 1 << 20);
        // No lock around the calls; the writers spin on one flag so they start together.
        let go = AtomicBool::new(false);
        let results: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        while !go.load(Ordering::Acquire) {
                            std::hint::spin_loop();
                        }
                        write_artifact(dir.path(), "claude", "json", &body)
                    })
                })
                .collect();
            go.store(true, Ordering::Release);
            handles
                .into_iter()
                .map(|writer| writer.join().expect("writer thread"))
                .collect()
        });
        let first = results[0].as_ref().expect("first writer").clone();
        for result in &results {
            assert_eq!(result.as_ref().expect("every writer succeeds"), &first);
        }
        assert_eq!(fs::read(&first).expect("artifact"), body.as_bytes());
        assert_eq!(fs::read_dir(dir.path()).expect("artifacts").count(), 1);
    }

    #[test]
    fn stray_temp_file_does_not_block_a_write() {
        let dir = tempfile::tempdir().expect("owned artifact directory");
        let body = "full hook settings body";
        // What an interrupted write may leave: part of the body under a temporary name.
        let stray = dir.path().join(".tmpABC123");
        fs::write(&stray, &body[..4]).expect("owned stray file");
        let path = write_artifact(dir.path(), "claude", "json", body).expect("write");
        assert_eq!(fs::read(&path).expect("artifact"), body.as_bytes());
        assert_eq!(fs::read(&stray).expect("stray file"), &body.as_bytes()[..4]);
        assert_eq!(fs::read_dir(dir.path()).expect("artifacts").count(), 2);
    }
}
