//! Convert an already captured local installation path for ordinary-root checks.
//! This is never a renderer or command-line input normalizer.
pub(crate) fn captured_disk_root(path: &str) -> Result<&str, &'static str> {
    let disk = path.strip_prefix(r"\\?\").unwrap_or(path);
    let bytes = disk.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || bytes[2] != b'\\' {
        return Err("bootstrap_root_unsafe");
    }
    Ok(disk)
}

/// Installer/helper command lines require an ordinary local disk path. Only a
/// native captured path may enter here, and both spellings must name one object.
#[cfg(windows)]
pub(crate) fn captured_disk_path(path: &str) -> Result<std::path::PathBuf, &'static str> {
    checked_disk_root(path, |value| {
        devbox_filesystem::filesystem_identity(value, true).map_err(|_| "bootstrap_root_changed")
    })
    .map(std::path::PathBuf::from)
}

fn checked_disk_root<T: PartialEq>(
    path: &str,
    mut identity: impl FnMut(&str) -> Result<T, &'static str>,
) -> Result<&str, &'static str> {
    let disk = captured_disk_root(path)?;
    if identity(disk)? != identity(path)? {
        return Err("bootstrap_root_changed");
    }
    Ok(disk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn installer_root_keeps_the_captured_native_directory_identity() {
        let directory = tempfile::tempdir().unwrap();
        let captured = directory.path().canonicalize().unwrap();
        let captured = captured.to_str().unwrap();
        assert!(captured.starts_with(r"\\?\"));
        let ordinary = captured_disk_path(captured).unwrap();
        assert!(!ordinary.to_str().unwrap().starts_with(r"\\?\"));
        assert_eq!(
            devbox_filesystem::filesystem_identity(&ordinary, true).unwrap(),
            devbox_filesystem::filesystem_identity(captured, true).unwrap()
        );
    }

    #[test]
    fn installer_root_requires_matching_captured_identity_before_dropping_prefix() {
        let captured = r"\\?\D:\owned fixture\Suite UI Fixture";
        let ordinary = r"D:\owned fixture\Suite UI Fixture";
        let mut inspected = Vec::new();
        assert_eq!(
            checked_disk_root(captured, |path| {
                inspected.push(path.to_owned());
                Ok((17, 42))
            }),
            Ok(ordinary)
        );
        assert_eq!(inspected, [ordinary, captured]);
        assert_eq!(
            checked_disk_root(captured, |path| Ok(path == captured)),
            Err("bootstrap_root_changed")
        );
        assert_eq!(
            checked_disk_root(captured, |_| Err::<(), _>("bootstrap_root_changed")),
            Err("bootstrap_root_changed")
        );
        for path in [
            r"\\?\UNC\server\share",
            r"\\server\share",
            r"\\?\Volume{fixture}\",
        ] {
            assert_eq!(
                checked_disk_root::<()>(path, |_| panic!("unsafe namespace must not be inspected")),
                Err("bootstrap_root_unsafe")
            );
        }
    }

    #[test]
    fn captured_verbatim_disk_root_retains_the_exact_local_path() {
        assert_eq!(
            captured_disk_root(r"\\?\D:\owned fixture\Suite UI Fixture"),
            Ok(r"D:\owned fixture\Suite UI Fixture")
        );
        assert_eq!(
            captured_disk_root(r"D:\owned fixture\Suite UI Fixture"),
            Ok(r"D:\owned fixture\Suite UI Fixture")
        );
        for path in [
            r"\\?\UNC\server\share",
            r"\\?\Volume{fixture}\",
            r"\\.\D:\fixture",
            r"\\server\share",
            r"D:relative",
            r"/tmp/fixture",
            "",
        ] {
            assert_eq!(captured_disk_root(path), Err("bootstrap_root_unsafe"));
        }
    }
}
