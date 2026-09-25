//! Update download cache housekeeping. Only the reviewed release is kept.
use std::fs;
use std::io;
use std::path::Path;

fn is_release_id(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Remove every other release directory. A directory that cannot be removed
/// (for example a setup that is still running) is skipped and retried later.
pub fn prune_releases(cache: &Path, keep: &str) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(cache)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if name != keep
            && is_release_id(name)
            && kind.is_dir()
            && !kind.is_symlink()
            && devbox_filesystem::ensure_no_links(entry.path()).is_ok()
            && fs::remove_dir_all(entry.path()).is_ok()
        {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Remove interrupted downloads. Downloads are single-flight, so no partial
/// file is in use when a download or launch starts.
pub fn prune_partials(release: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(release)? {
        let entry = entry?;
        let partial = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".partial-"));
        if partial && entry.file_type()?.is_file() && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> String {
        format!("{:064x}", n)
    }

    #[test]
    fn keeps_only_the_reviewed_release_and_unrelated_entries() {
        let cache = tempfile::tempdir().unwrap();
        for n in 1..=3 {
            fs::create_dir(cache.path().join(id(n))).unwrap();
            fs::write(cache.path().join(id(n)).join("Devbox_setup.exe"), b"x").unwrap();
        }
        fs::create_dir(cache.path().join("not-a-release")).unwrap();
        assert_eq!(prune_releases(cache.path(), &id(2)).unwrap(), 2);
        let mut names: Vec<_> = fs::read_dir(cache.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, vec![id(2), "not-a-release".to_string()]);
    }

    #[test]
    fn removes_interrupted_partials_only() {
        let release = tempfile::tempdir().unwrap();
        fs::write(release.path().join(".partial-1"), b"x").unwrap();
        fs::write(release.path().join(".partial-2"), b"x").unwrap();
        fs::write(release.path().join("Devbox_0.9.0_x64-setup.exe"), b"x").unwrap();
        assert_eq!(prune_partials(release.path()).unwrap(), 2);
        assert!(release.path().join("Devbox_0.9.0_x64-setup.exe").exists());
    }
}

#[cfg(all(test, unix))]
#[test]
fn pruning_never_follows_directory_or_partial_links() {
    use std::os::unix::fs::symlink;
    let cache = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep"), b"synthetic").unwrap();
    symlink(outside.path(), cache.path().join(format!("{:064x}", 1))).unwrap();
    symlink(
        outside.path().join("keep"),
        cache.path().join(".partial-linked"),
    )
    .unwrap();
    assert_eq!(
        prune_releases(cache.path(), &format!("{:064x}", 2)).unwrap(),
        0
    );
    assert_eq!(prune_partials(cache.path()).unwrap(), 0);
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"synthetic");
}
#[cfg(all(test, windows))]
#[test]
fn locked_installer_is_skipped_and_removed_on_the_next_attempt() {
    use std::os::windows::fs::OpenOptionsExt;
    let cache = tempfile::tempdir().unwrap();
    let release = cache.path().join(format!("{:064x}", 1));
    fs::create_dir(&release).unwrap();
    let path = release.join("setup.exe");
    fs::write(&path, b"synthetic").unwrap();
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert_eq!(
        prune_releases(cache.path(), &format!("{:064x}", 2)).unwrap(),
        0
    );
    drop(handle);
    assert_eq!(
        prune_releases(cache.path(), &format!("{:064x}", 2)).unwrap(),
        1
    );
}
