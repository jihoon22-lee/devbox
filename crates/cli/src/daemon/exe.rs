//! The devbox path that agent hooks and start scripts run.
use std::path::{Path, PathBuf};

/// Installed daemons run `bin/devbox-<sha256>` behind the `bin/devbox` symlink that every
/// update repoints. Returns that link when it resolves to `exe`, so a session started now
/// still reaches a daemon after an update; any other layout (development builds,
/// `pnpm dev`, a link to another binary) keeps `exe`.
pub(crate) fn stable_devbox_path(exe: &Path) -> PathBuf {
    let hashed = exe
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("devbox-"))
        .is_some_and(|hash| {
            hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
    let link = exe.with_file_name("devbox");
    let linked = hashed
        && std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink())
        && matches!(
            (std::fs::canonicalize(&link), std::fs::canonicalize(exe)),
            (Ok(target), Ok(current)) if target == current
        );
    if linked { link } else { exe.to_path_buf() }
}

#[cfg(all(test, unix))]
mod tests {
    use super::stable_devbox_path;
    use std::{
        fs,
        os::unix::fs::symlink,
        path::{Path, PathBuf},
    };

    fn binary(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::create_dir_all(dir).expect("owned binary directory");
        fs::write(&path, name).expect("owned binary");
        path
    }

    #[test]
    fn uses_the_symlink_when_it_points_to_current_exe() {
        let dir = tempfile::tempdir().expect("owned bin directory");
        let name = format!("devbox-{}", "0123456789abcdef".repeat(4));
        let exe = binary(dir.path(), &name);
        // The installer links by relative name: `ln -s devbox-<sha256> devbox`.
        symlink(&name, dir.path().join("devbox")).expect("owned link");
        assert_eq!(stable_devbox_path(&exe), dir.path().join("devbox"));
    }

    #[test]
    fn falls_back_when_the_symlink_points_elsewhere() {
        let dir = tempfile::tempdir().expect("owned bin directory");
        let exe = binary(dir.path(), &format!("devbox-{}", "a".repeat(64)));
        let newer = format!("devbox-{}", "b".repeat(64));
        binary(dir.path(), &newer);
        symlink(&newer, dir.path().join("devbox")).expect("owned link");
        assert_eq!(stable_devbox_path(&exe), exe);

        let plain = tempfile::tempdir().expect("owned bin directory");
        let exe = binary(plain.path(), &format!("devbox-{}", "a".repeat(64)));
        fs::hard_link(&exe, plain.path().join("devbox")).expect("owned non-link");
        assert_eq!(stable_devbox_path(&exe), exe);
    }

    #[test]
    fn falls_back_for_unhashed_names() {
        let dir = tempfile::tempdir().expect("owned target directory");
        let exe = binary(&dir.path().join("target/debug"), "devbox");
        assert_eq!(stable_devbox_path(&exe), exe);

        for name in [
            format!("devbox-{}", "A".repeat(64)),
            format!("devbox-{}", "a".repeat(63)),
            format!("devbox-{}", "g".repeat(64)),
        ] {
            let dir = tempfile::tempdir().expect("owned bin directory");
            let exe = binary(dir.path(), &name);
            symlink(&name, dir.path().join("devbox")).expect("owned link");
            assert_eq!(stable_devbox_path(&exe), exe, "{name}");
        }
    }
}
