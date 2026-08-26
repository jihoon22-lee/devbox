//! Validation and removal for Manager-owned portable app trees.
//!
//! Paths from `registry.json` are evidence only. Every action derives the
//! expected path from the Manager root, a validated catalog id, and a bounded
//! version component, then requires the canonical registry path to match it.

use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

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

/// Prepare the exact portable layout below a canonical Manager root.
///
/// `create_dir_all` is intentionally not used here: a directory component can
/// be replaced by a symlink/reparse point between root selection and the first
/// install.  Each component is created and checked independently, and the
/// returned executable slot is allowed to be an existing regular file so an
/// update can replace it after digest verification.
pub fn prepare_portable_destination(
    manager_root: &Path,
    app_id: &str,
    version: &str,
) -> Result<PathBuf, ManagedInstallError> {
    if !safe_component(app_id, 64) || !safe_component(version, 128) {
        return Err(ManagedInstallError::InvalidIdentity);
    }

    // Validate the caller-visible spelling before canonicalizing it. On
    // Windows canonicalization can legitimately expand an 8.3 component, so
    // string identity is not evidence that a path is free of reparses.
    ensure_existing_path(manager_root)?;
    let canonical_root =
        canonicalize_path(manager_root).map_err(|_| ManagedInstallError::Missing)?;
    let root_metadata =
        fs::symlink_metadata(&canonical_root).map_err(|_| ManagedInstallError::Missing)?;
    if is_link_or_reparse(&root_metadata) || !root_metadata.is_dir() {
        return Err(ManagedInstallError::UnsafePath);
    }

    let apps_root = ensure_directory_component(&canonical_root, "apps")?;
    let app_root = ensure_directory_component(&apps_root, app_id)?;
    let versions_root = ensure_directory_component(&app_root, "versions")?;
    let version_root = ensure_directory_component(&versions_root, version)?;
    let executable = version_root.join(format!("{app_id}.exe"));
    validate_download_target(&executable)?;
    Ok(executable)
}

/// Prepare the root-owned installer cache directory. The asset filename is
/// validated by the release-manifest parser; target and sibling `.partial`
/// links are still rejected by `validate_download_target` before I/O.
pub fn prepare_installer_destination(
    manager_root: &Path,
    asset_name: &str,
) -> Result<PathBuf, ManagedInstallError> {
    if !safe_component(asset_name, 256) {
        return Err(ManagedInstallError::InvalidIdentity);
    }
    ensure_existing_path(manager_root)?;
    let canonical_root =
        canonicalize_path(manager_root).map_err(|_| ManagedInstallError::Missing)?;
    let root_metadata =
        fs::symlink_metadata(&canonical_root).map_err(|_| ManagedInstallError::Missing)?;
    if is_link_or_reparse(&root_metadata) || !root_metadata.is_dir() {
        return Err(ManagedInstallError::UnsafePath);
    }
    let installers_root = ensure_directory_component(&canonical_root, "installers")?;
    let destination = installers_root.join(asset_name);
    validate_download_target(&destination)?;
    Ok(destination)
}

/// Validate an existing destination parent and optional regular-file target.
/// This is called immediately before the download creates its `.partial`
/// sibling; no caller may use an unvalidated arbitrary path for a download.
pub fn validate_download_target(destination: &Path) -> Result<(), ManagedInstallError> {
    let parent = destination
        .parent()
        .ok_or(ManagedInstallError::UnsafePath)?;
    ensure_existing_path(parent)?;
    validate_download_slot(destination)?;
    let partial = destination.with_file_name(format!(
        "{}.partial",
        destination
            .file_name()
            .ok_or(ManagedInstallError::UnsafePath)?
            .to_string_lossy()
    ));
    validate_download_slot(&partial)
}

fn validate_download_slot(path: &Path) -> Result<(), ManagedInstallError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) => Err(ManagedInstallError::UnsafePath),
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(ManagedInstallError::UnsupportedEntry),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ManagedInstallError::Io),
    }
}

fn ensure_directory_component(parent: &Path, name: &str) -> Result<PathBuf, ManagedInstallError> {
    let child = parent.join(name);
    match fs::symlink_metadata(&child) {
        Ok(metadata) => {
            if is_link_or_reparse(&metadata) || !metadata.is_dir() {
                return Err(ManagedInstallError::UnsafePath);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::create_dir(&child) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(ManagedInstallError::Io),
            }
            let metadata = fs::symlink_metadata(&child).map_err(|_| ManagedInstallError::Io)?;
            if is_link_or_reparse(&metadata) || !metadata.is_dir() {
                return Err(ManagedInstallError::UnsafePath);
            }
        }
        Err(_) => return Err(ManagedInstallError::Io),
    }
    Ok(child)
}

fn ensure_existing_path(path: &Path) -> Result<(), ManagedInstallError> {
    if !path.is_absolute() {
        return Err(ManagedInstallError::UnsafePath);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => current.push(value),
            Component::CurDir | Component::ParentDir => {
                return Err(ManagedInstallError::UnsafePath)
            }
        }
        // On Windows a disk prefix is drive-relative until RootDir follows
        // it. Probe only once the accumulated path is absolute.
        if !current.is_absolute() {
            continue;
        }
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ManagedInstallError::Missing
            } else {
                ManagedInstallError::Io
            }
        })?;
        if is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(ManagedInstallError::UnsafePath);
        }
    }
    Ok(())
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, std::io::Error> {
    path.canonicalize().map(normalize_canonical_path)
}

#[cfg(windows)]
fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(not(windows))]
fn normalize_canonical_path(path: PathBuf) -> PathBuf {
    path
}

fn same_path_identity(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        normalize_windows_identity(left) == normalize_windows_identity(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(windows)]
fn normalize_windows_identity(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        value = format!(r"\\{rest}");
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        value = rest.to_string();
    }
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    value.to_ascii_lowercase()
}

fn path_within(root: &Path, candidate: &Path) -> bool {
    #[cfg(windows)]
    {
        let root = normalize_windows_identity(root);
        let candidate = normalize_windows_identity(candidate);
        candidate == root
            || candidate
                .strip_prefix(&root)
                .is_some_and(|suffix| suffix.starts_with('\\'))
    }
    #[cfg(not(windows))]
    {
        candidate.starts_with(root)
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
        canonicalize_path(manager_root).map_err(|_| ManagedInstallError::Missing)?;
    let apps_root = canonical_manager.join("apps");
    let expected_root = apps_root.join(app_id);
    let expected_executable = expected_root
        .join("versions")
        .join(version)
        .join(format!("{app_id}.exe"));

    ensure_plain_path(&apps_root, &expected_executable)?;
    let canonical_apps = canonicalize_path(&apps_root).map_err(|_| ManagedInstallError::Missing)?;
    let canonical_root =
        canonicalize_path(&expected_root).map_err(|_| ManagedInstallError::Missing)?;
    let canonical_executable =
        canonicalize_path(&expected_executable).map_err(|_| ManagedInstallError::Missing)?;
    let canonical_registry = canonicalize_path(Path::new(registry_executable))
        .map_err(|_| ManagedInstallError::RegistryMismatch)?;

    if !path_within(&canonical_manager, &canonical_apps)
        || !path_within(&canonical_apps, &canonical_root)
        || !path_within(&canonical_root, &canonical_executable)
        || !same_path_identity(&canonical_registry, &canonical_executable)
        || !canonical_executable.is_file()
    {
        return Err(ManagedInstallError::RegistryMismatch);
    }

    Ok(ManagedPortableInstall {
        app_root: canonical_root,
        executable: canonical_executable,
    })
}

/// Compatibility wrapper for callers that still hold a resolved install.
/// New Manager removal uses the manifest-CAS command boundary directly.
#[allow(dead_code)]
pub fn remove_portable_install(
    install: &ManagedPortableInstall,
) -> Result<(), ManagedInstallError> {
    let app_id = install
        .executable
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or(ManagedInstallError::InvalidIdentity)?;
    let version = install
        .executable
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .ok_or(ManagedInstallError::InvalidIdentity)?;
    let manager_root = install
        .app_root
        .parent()
        .and_then(Path::parent)
        .ok_or(ManagedInstallError::UnsafePath)?;
    let registry_executable = install
        .executable
        .to_str()
        .ok_or(ManagedInstallError::UnsafePath)?;
    let plan = crate::core::removal::inspect_portable_removal(
        manager_root,
        app_id,
        version,
        registry_executable,
    )
    .map_err(|error| match error {
        crate::core::removal::RemovalError::InvalidIdentity => ManagedInstallError::InvalidIdentity,
        crate::core::removal::RemovalError::Missing => ManagedInstallError::Missing,
        crate::core::removal::RemovalError::UnsafePath => ManagedInstallError::UnsafePath,
        crate::core::removal::RemovalError::RegistryMismatch => {
            ManagedInstallError::RegistryMismatch
        }
        crate::core::removal::RemovalError::ForeignEntry
        | crate::core::removal::RemovalError::UnsupportedEntry => {
            ManagedInstallError::UnsupportedEntry
        }
        crate::core::removal::RemovalError::Io => ManagedInstallError::Io,
    })?;
    let outcome =
        crate::core::removal::remove_portable_tree(&plan).map_err(|error| match error {
            crate::core::removal::RemovalError::InvalidIdentity => {
                ManagedInstallError::InvalidIdentity
            }
            crate::core::removal::RemovalError::Missing => ManagedInstallError::Missing,
            crate::core::removal::RemovalError::UnsafePath => ManagedInstallError::UnsafePath,
            crate::core::removal::RemovalError::RegistryMismatch => {
                ManagedInstallError::RegistryMismatch
            }
            crate::core::removal::RemovalError::ForeignEntry
            | crate::core::removal::RemovalError::UnsupportedEntry => {
                ManagedInstallError::UnsupportedEntry
            }
            crate::core::removal::RemovalError::Io => ManagedInstallError::Io,
        })?;
    outcome
        .complete
        .then_some(())
        .ok_or(ManagedInstallError::Io)
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

        assert_eq!(resolved.executable, canonicalize_path(&executable).unwrap());
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
    fn prepares_portable_layout_one_directory_at_a_time() {
        let root = TestRoot::new();
        let destination = prepare_portable_destination(&root.0, "code-pad", "0.3.2").unwrap();

        assert_eq!(
            destination,
            canonicalize_path(&root.0)
                .unwrap()
                .join("apps/code-pad/versions/0.3.2/code-pad.exe")
        );
        assert!(destination.parent().unwrap().is_dir());
        assert!(root.0.join("apps").is_dir());
        assert!(validate_download_target(&destination).is_ok());
    }

    #[test]
    fn preparation_rejects_relative_manager_roots_before_mutation() {
        assert_eq!(
            prepare_portable_destination(Path::new("relative-root"), "code-pad", "0.3.2"),
            Err(ManagedInstallError::UnsafePath)
        );
        assert_eq!(
            prepare_installer_destination(Path::new("relative-root"), "code-pad.msi"),
            Err(ManagedInstallError::UnsafePath)
        );
    }

    #[cfg(unix)]
    #[test]
    fn preparation_rejects_symlinked_layout_component_and_download_slot() {
        use std::os::unix::fs::symlink;

        let root = TestRoot::new();
        let outside = root.0.join("outside");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(root.0.join("apps")).unwrap();
        symlink(&outside, root.0.join("apps/code-pad")).unwrap();
        assert_eq!(
            prepare_portable_destination(&root.0, "code-pad", "0.3.2"),
            Err(ManagedInstallError::UnsafePath)
        );

        let safe_root = TestRoot::new();
        let destination = prepare_portable_destination(&safe_root.0, "code-pad", "0.3.2").unwrap();
        symlink(&outside, destination.with_file_name("code-pad.exe.partial")).unwrap();
        assert_eq!(
            validate_download_target(&destination),
            Err(ManagedInstallError::UnsafePath)
        );
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
