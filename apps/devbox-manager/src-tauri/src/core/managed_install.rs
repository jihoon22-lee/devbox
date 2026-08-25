//! Validation and removal for Manager-owned portable app trees.
//!
//! Paths from `registry.json` are evidence only. Every action derives the
//! expected path from the Manager root, a validated catalog id, and a bounded
//! version component, then requires the canonical registry path to match it.

use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_TREE_DEPTH: usize = 16;
const MAX_TREE_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedInstallError {
    InvalidIdentity,
    Missing,
    UnsafePath,
    RegistryMismatch,
    UnsupportedEntry,
    Io,
}

impl fmt::Display for ManagedInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentity => "managed install identity is invalid",
            Self::Missing => "managed portable install is missing",
            Self::UnsafePath => "managed portable path is unsafe",
            Self::RegistryMismatch => "managed registry path does not match the install layout",
            Self::UnsupportedEntry => "managed portable tree contains an unsupported entry",
            Self::Io => "managed portable filesystem operation failed",
        })
    }
}

impl std::error::Error for ManagedInstallError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedPortableInstall {
    pub app_root: PathBuf,
    pub executable: PathBuf,
}

impl ManagedPortableInstall {
    pub fn install_dir(&self) -> Result<&Path, ManagedInstallError> {
        self.executable
            .parent()
            .ok_or(ManagedInstallError::UnsafePath)
    }
}

fn safe_component(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.trim() == value
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\', '\0', ':'])
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn ensure_plain_path(root: &Path, target: &Path) -> Result<(), ManagedInstallError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| ManagedInstallError::UnsafePath)?;
    let mut current = root.to_path_buf();
    let root_metadata = fs::symlink_metadata(&current).map_err(|_| ManagedInstallError::Missing)?;
    if is_link_or_reparse(&root_metadata) || !root_metadata.is_dir() {
        return Err(ManagedInstallError::UnsafePath);
    }
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(ManagedInstallError::UnsafePath);
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|_| ManagedInstallError::Missing)?;
        if is_link_or_reparse(&metadata) {
            return Err(ManagedInstallError::UnsafePath);
        }
    }
    Ok(())
}

pub fn resolve_portable_install(
    manager_root: &Path,
    app_id: &str,
    version: &str,
    registry_executable: &str,
) -> Result<ManagedPortableInstall, ManagedInstallError> {
    if !safe_component(app_id, 64) || !safe_component(version, 128) {
        return Err(ManagedInstallError::InvalidIdentity);
    }

    let canonical_manager =
        fs::canonicalize(manager_root).map_err(|_| ManagedInstallError::Missing)?;
    let apps_root = canonical_manager.join("apps");
    let expected_root = apps_root.join(app_id);
    let expected_executable = expected_root
        .join("versions")
        .join(version)
        .join(format!("{app_id}.exe"));

    ensure_plain_path(&apps_root, &expected_executable)?;
    let canonical_apps = fs::canonicalize(&apps_root).map_err(|_| ManagedInstallError::Missing)?;
    let canonical_root =
        fs::canonicalize(&expected_root).map_err(|_| ManagedInstallError::Missing)?;
    let canonical_executable =
        fs::canonicalize(&expected_executable).map_err(|_| ManagedInstallError::Missing)?;
    let canonical_registry =
        fs::canonicalize(registry_executable).map_err(|_| ManagedInstallError::RegistryMismatch)?;

    if !canonical_apps.starts_with(&canonical_manager)
        || !canonical_root.starts_with(&canonical_apps)
        || !canonical_executable.starts_with(&canonical_root)
        || canonical_registry != canonical_executable
        || !canonical_executable.is_file()
    {
        return Err(ManagedInstallError::RegistryMismatch);
    }

    Ok(ManagedPortableInstall {
        app_root: canonical_root,
        executable: canonical_executable,
    })
}

fn validate_tree(
    path: &Path,
    depth: usize,
    entries: &mut usize,
) -> Result<(), ManagedInstallError> {
    if depth > MAX_TREE_DEPTH || *entries >= MAX_TREE_ENTRIES {
        return Err(ManagedInstallError::UnsupportedEntry);
    }
    *entries += 1;
    let metadata = fs::symlink_metadata(path).map_err(|_| ManagedInstallError::Missing)?;
    if is_link_or_reparse(&metadata) {
        return Err(ManagedInstallError::UnsafePath);
    }
    if metadata.is_file() {
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(ManagedInstallError::UnsupportedEntry);
    }
    let children = fs::read_dir(path).map_err(|_| ManagedInstallError::Io)?;
    for entry in children {
        let entry = entry.map_err(|_| ManagedInstallError::Io)?;
        validate_tree(&entry.path(), depth + 1, entries)?;
    }
    Ok(())
}

pub fn remove_portable_install(
    install: &ManagedPortableInstall,
) -> Result<(), ManagedInstallError> {
    let mut entries = 0;
    validate_tree(&install.app_root, 0, &mut entries)?;
    if !install.executable.is_file() || !install.executable.starts_with(&install.app_root) {
        return Err(ManagedInstallError::UnsafePath);
    }
    fs::remove_dir_all(&install.app_root).map_err(|_| ManagedInstallError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devbox-manager-managed-install-{}-{nonce}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (TestRoot, PathBuf, PathBuf) {
        let root = TestRoot::new();
        let executable = root
            .0
            .join("apps/port-manager/versions/0.4.0/port-manager.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"portable").unwrap();
        fs::write(root.0.join("apps/port-manager/current.json"), b"{}").unwrap();
        fs::create_dir_all(root.0.join("apps/keep/versions/1.0.0")).unwrap();
        fs::write(root.0.join("user-data.txt"), b"preserve").unwrap();
        (root, executable.clone(), executable)
    }

    #[test]
    fn resolves_only_the_exact_derived_registry_executable() {
        let (root, executable, registry) = fixture();
        let resolved =
            resolve_portable_install(&root.0, "port-manager", "0.4.0", registry.to_str().unwrap())
                .unwrap();

        assert_eq!(resolved.executable, fs::canonicalize(executable).unwrap());
        assert!(resolved.install_dir().unwrap().ends_with("0.4.0"));
    }

    #[test]
    fn rejects_traversal_and_registry_mismatch_without_echoing_paths() {
        let (root, _executable, _) = fixture();
        let outside = root.0.join("outside.exe");
        fs::write(&outside, b"outside").unwrap();

        let traversal = resolve_portable_install(
            &root.0,
            "../port-manager",
            "0.4.0",
            outside.to_str().unwrap(),
        )
        .unwrap_err();
        let mismatch =
            resolve_portable_install(&root.0, "port-manager", "0.4.0", outside.to_str().unwrap())
                .unwrap_err();

        assert_eq!(traversal, ManagedInstallError::InvalidIdentity);
        assert_eq!(mismatch, ManagedInstallError::RegistryMismatch);
        assert!(!mismatch.to_string().contains(outside.to_str().unwrap()));
    }

    #[test]
    fn removal_deletes_only_the_managed_app_tree() {
        let (root, _executable, registry) = fixture();
        let resolved =
            resolve_portable_install(&root.0, "port-manager", "0.4.0", registry.to_str().unwrap())
                .unwrap();

        remove_portable_install(&resolved).unwrap();

        assert!(!root.0.join("apps/port-manager").exists());
        assert!(root.0.join("apps/keep").exists());
        assert_eq!(fs::read(root.0.join("user-data.txt")).unwrap(), b"preserve");
    }

    #[cfg(unix)]
    #[test]
    fn removal_rejects_symlink_entries_before_mutation() {
        use std::os::unix::fs::symlink;

        let (root, _executable, registry) = fixture();
        let outside = root.0.join("outside.txt");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, root.0.join("apps/port-manager/escape")).unwrap();
        let resolved =
            resolve_portable_install(&root.0, "port-manager", "0.4.0", registry.to_str().unwrap())
                .unwrap();

        assert_eq!(
            remove_portable_install(&resolved),
            Err(ManagedInstallError::UnsafePath)
        );
        assert!(root.0.join("apps/port-manager").exists());
        assert_eq!(fs::read(outside).unwrap(), b"outside");
    }
}
