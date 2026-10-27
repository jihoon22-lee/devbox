//! Content-addressed files that tool sessions load from `agent-hooks/`.
use sha2::{Digest, Sha256};
use std::{
    io::{Error, ErrorKind},
    path::{Path, PathBuf},
};

/// Writes `<dir>/<stem>-<first 16 hex of sha256(body)>.<ext>` once (0600, create_new).
/// A running session keeps reading the file it started with, so a new body gets a new
/// name and an existing name is reused only when it is a regular file with the same
/// bytes; anything else is `AlreadyExists`. Callers hold `AgentsService::hook_files`.
pub(crate) fn write_artifact(
    dir: &Path,
    stem: &str,
    ext: &str,
    body: &str,
) -> std::io::Result<PathBuf> {
    let digest = Sha256::digest(body.as_bytes());
    let hash: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let path = dir.join(format!("{stem}-{hash}.{ext}"));
    match crate::prepare::private_file(&path, body, false) {
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
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
        written => written.map(|()| path),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::write_artifact;
    use std::{
        fs,
        io::ErrorKind,
        os::unix::fs::{PermissionsExt, symlink},
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
}
