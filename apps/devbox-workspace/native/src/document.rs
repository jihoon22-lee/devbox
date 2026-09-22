//! Bounded filesystem-only entry point for the Knowledge host. No shell, child
//! process, listener or Workspace task authority is available through this mode.
use serde::{Deserialize, Serialize};
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Operation {
    pub version: u32,
    pub method: Method,
    pub paths: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    PrivateDirectory,
    Permissions,
    Create,
    Replace,
    Sync,
}

#[cfg(all(target_os = "linux", feature = "helper"))]
pub fn execute(operation: Operation) -> std::io::Result<()> {
    use std::{
        ffi::CString,
        fs, io,
        os::unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, PermissionsExt},
        },
        path::{Component, Path, PathBuf},
    };
    let invalid = || io::Error::from(io::ErrorKind::InvalidInput);
    if operation.version != 1 || operation.paths.len() > 3 {
        return Err(invalid());
    }
    let paths = operation
        .paths
        .iter()
        .map(|raw| {
            let path = PathBuf::from(raw);
            if raw.len() > 4096
                || !path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            {
                return Err(invalid());
            }
            devbox_filesystem::ensure_no_links(path.parent().ok_or_else(invalid)?)?;
            Ok(path)
        })
        .collect::<io::Result<Vec<_>>>()?;
    let private = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.strip_prefix(".devbox-save-").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit() || b == b'-')
                })
            })
    };
    if operation.method == Method::Sync {
        if paths.len() != 1 {
            return Err(invalid());
        }
        devbox_filesystem::ensure_no_links(&paths[0])?;
        return fs::File::open(&paths[0])?.sync_all();
    }
    if operation.method == Method::PrivateDirectory {
        if paths.len() != 1 || !private(&paths[0]) {
            return Err(invalid());
        }
        return fs::DirBuilder::new().mode(0o700).create(&paths[0]);
    }
    if paths.len() < 2 {
        return Err(invalid());
    }
    let staged = &paths[0];
    let target = &paths[1];
    let directory = staged.parent().ok_or_else(invalid)?;
    if !private(directory)
        || directory.parent() != target.parent()
        || staged.file_name().and_then(|n| n.to_str()) != Some("submitted.md")
    {
        return Err(invalid());
    }
    devbox_filesystem::ensure_no_links(staged)?;
    let metadata = fs::symlink_metadata(staged)?;
    if !metadata.is_file()
        || metadata.len() > 10 * 1024 * 1024
        || fs::metadata(directory)?.permissions().mode() & 0o777 != 0o700
    {
        return Err(invalid());
    }
    if operation.method == Method::Permissions {
        if paths.len() != 2 {
            return Err(invalid());
        }
        let mode = match fs::symlink_metadata(target) {
            Ok(metadata) if metadata.is_file() => metadata.permissions().mode(),
            Ok(_) => return Err(invalid()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0o600,
            Err(error) => return Err(error),
        };
        fs::set_permissions(staged, fs::Permissions::from_mode(mode))?;
        return fs::File::open(staged)?.sync_all();
    }
    let flags = match operation.method {
        Method::Create if paths.len() == 2 => libc::RENAME_NOREPLACE,
        Method::Replace
            if paths.len() == 3
                && paths[2] == directory.join("previous.md")
                && !paths[2].exists() =>
        {
            devbox_filesystem::ensure_no_links(target)?;
            if !fs::metadata(target)?.is_file() {
                return Err(invalid());
            }
            libc::RENAME_EXCHANGE
        }
        _ => return Err(invalid()),
    };
    let from = CString::new(staged.as_os_str().as_bytes())?;
    let to = CString::new(target.as_os_str().as_bytes())?;
    if unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            flags,
        )
    } != 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOSYS) || !wsl1(staged)? {
            return Err(error);
        }
        // WSL1 lacks renameat2. Move the exact displaced object first, then
        // atomically link complete new bytes only into an absent name. A reader
        // may see a brief missing path; a concurrent creator is never replaced.
        return publish_wsl1(staged, target, paths.get(2).map(PathBuf::as_path), || {});
    }
    if flags == libc::RENAME_EXCHANGE {
        fs::rename(staged, &paths[2])?;
    }
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "helper"))]
fn wsl1(path: &std::path::Path) -> std::io::Result<bool> {
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open(path)?;
    let mut info = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    if unsafe { libc::fstatfs(file.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { info.assume_init() }.f_type == 0x5346_4846)
}
#[cfg(all(target_os = "linux", feature = "helper"))]
fn publish_wsl1(
    staged: &std::path::Path,
    target: &std::path::Path,
    previous: Option<&std::path::Path>,
    after_move: impl FnOnce(),
) -> std::io::Result<()> {
    if let Some(previous) = previous {
        std::fs::rename(target, previous)?;
    }
    after_move();
    std::fs::hard_link(staged, target)?;
    std::fs::remove_file(staged)
}

#[cfg(all(target_os = "linux", feature = "helper"))]
pub fn run() -> i32 {
    use std::io::Read;
    // Includes stdin and all filesystem operations. A killed/expired helper is
    // an unknown commit to its host, which retains evidence instead of rollback.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(5));
        std::process::exit(124);
    });
    let mut bytes = Vec::new();
    if std::io::stdin()
        .take(16_385)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 16_384
    {
        return 2;
    }
    let operation = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return 2,
    };
    match execute(operation) {
        Ok(()) => 0,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => 21,
        Err(_) => 22,
    }
}

#[cfg(all(test, target_os = "linux", feature = "helper"))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};
    fn call(method: Method, paths: &[&Path]) -> std::io::Result<()> {
        execute(Operation {
            version: 1,
            method,
            paths: paths.iter().map(|p| p.to_str().unwrap().into()).collect(),
        })
    }
    #[test]
    fn private_modes_exchange_and_no_clobber_preserve_both_writers() {
        for mode in [0o600, 0o640, 0o644] {
            let root = tempfile::tempdir().unwrap();
            let target = root.path().join("Case.md");
            fs::write(&target, "external").unwrap();
            fs::set_permissions(&target, fs::Permissions::from_mode(mode)).unwrap();
            let dir = root.path().join(".devbox-save-1-1");
            call(Method::PrivateDirectory, &[&dir]).unwrap();
            assert_eq!(
                fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let staged = dir.join("submitted.md");
            let previous = dir.join("previous.md");
            fs::write(&staged, "ours").unwrap();
            call(Method::Permissions, &[&staged, &target]).unwrap();
            assert_eq!(
                fs::metadata(&staged).unwrap().permissions().mode() & 0o777,
                mode
            );
            assert!(call(Method::Create, &[&staged, &target]).is_err());
            call(Method::Replace, &[&staged, &target, &previous]).unwrap();
            assert_eq!(fs::read_to_string(&target).unwrap(), "ours");
            assert_eq!(fs::read_to_string(previous).unwrap(), "external");
            assert_eq!(
                fs::metadata(target).unwrap().permissions().mode() & 0o777,
                mode
            );
        }
    }
    #[test]
    fn wsl1_fallback_never_overwrites_a_creator_in_the_missing_path_window() {
        let root = tempfile::tempdir().unwrap();
        let staged = root.path().join("submitted.md");
        let target = root.path().join("note.md");
        let previous = root.path().join("previous.md");
        fs::write(&staged, "ours").unwrap();
        fs::write(&target, "original").unwrap();
        let result = publish_wsl1(&staged, &target, Some(&previous), || {
            fs::write(&target, "concurrent creator").unwrap()
        });
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_to_string(target).unwrap(), "concurrent creator");
        assert_eq!(fs::read_to_string(previous).unwrap(), "original");
        assert_eq!(fs::read_to_string(staged).unwrap(), "ours");
    }
    #[test]
    fn unrelated_paths_and_symlinks_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join(".devbox-save-1-1");
        call(Method::PrivateDirectory, &[&dir]).unwrap();
        let staged = dir.join("submitted.md");
        fs::write(&staged, "ours").unwrap();
        let other = tempfile::tempdir().unwrap();
        let outside = other.path().join("outside.md");
        assert!(call(Method::Create, &[&staged, &outside]).is_err());
        let link = root.path().join("link.md");
        fs::write(&outside, "external").unwrap();
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(call(Method::Replace, &[&staged, &link, &dir.join("previous.md")]).is_err());
        assert_eq!(fs::read_to_string(outside).unwrap(), "external");
    }
}
