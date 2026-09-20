//! Safe, Manager-owned portable removal.
//!
//! A registry entry is evidence, not a deletion path.  This module derives
//! the only removable tree from the active Manager root, catalog-safe app id,
//! and bounded version component.  It accepts the exact layout that Manager
//! creates and refuses links, reparse points, traversal, special files, and
//! foreign entries before touching anything.  The caller owns the manifest
//! compare-and-swap; this module only plans and removes the binary tree.

use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_TREE_DEPTH: usize = 16;
const MAX_TREE_ENTRIES: usize = 10_000;
const MAX_CURRENT_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalError {
    InvalidIdentity,
    Missing,
    UnsafePath,
    RegistryMismatch,
    ForeignEntry,
    UnsupportedEntry,
    Io,
}

impl fmt::Display for RemovalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidIdentity => "removal identity is invalid",
            Self::Missing => "managed removal target is missing",
            Self::UnsafePath => "managed removal path is unsafe",
            Self::RegistryMismatch => "managed removal registry path does not match",
            Self::ForeignEntry => "managed removal tree contains a foreign entry",
            Self::UnsupportedEntry => "managed removal tree contains an unsupported entry",
            Self::Io => "managed removal filesystem operation failed",
        })
    }
}

impl std::error::Error for RemovalError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalState {
    Ready,
    Partial,
    Missing,
}

impl RemovalState {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Partial => "partial",
            Self::Missing => "missing",
        }
    }
}

/// A fully validated deletion plan.  The private paths are all derived from
/// `manager_root`, `app_id`, and `version`; callers cannot supply arbitrary
/// paths to the destructive operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalPlan {
    pub manager_root: PathBuf,
    pub app_id: String,
    pub version: String,
    pub registry_executable: String,
    pub app_root: PathBuf,
    pub executable: PathBuf,
    pub state: RemovalState,
    pub owned_entry_count: usize,
    pub owned_bytes: u64,
    owned_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalFailure {
    UnsafePath,
    UnsupportedEntry,
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemovalOutcome {
    pub complete: bool,
    pub removed_entry_count: usize,
    pub remaining_entry_count: usize,
    pub failure: Option<RemovalFailure>,
}

/// Derive and inspect the exact Manager portable layout for one registry
/// record.  Missing executable/tree state is represented as `Partial` or
/// `Missing` so a prior interrupted removal can be retried safely.
pub fn inspect_portable_removal(
    manager_root: &Path,
    app_id: &str,
    version: &str,
    registry_executable: &str,
) -> Result<RemovalPlan, RemovalError> {
    if !safe_component(app_id, 64) || !safe_component(version, 128) {
        return Err(RemovalError::InvalidIdentity);
    }
    if !valid_absolute_literal(registry_executable) {
        return Err(RemovalError::RegistryMismatch);
    }

    let canonical_root = canonicalize_existing_directory(manager_root)?;
    if protected_root(&canonical_root) {
        return Err(RemovalError::UnsafePath);
    }

    let apps_root = canonical_root.join("apps");
    let expected_app_root = apps_root.join(app_id);
    let expected_executable = expected_app_root
        .join("versions")
        .join(version)
        .join(format!("{app_id}.exe"));
    // Validate the registry spelling even when a previous interrupted removal
    // has already removed one or more parent directories.
    validate_registry_path(&expected_executable, registry_executable)?;
    if !existing_directory_is_plain(&apps_root)? {
        return Ok(missing_plan(
            &canonical_root,
            app_id,
            version,
            registry_executable,
        ));
    }
    let Some(app_root) = existing_directory(&expected_app_root)? else {
        return Ok(missing_plan(
            &canonical_root,
            app_id,
            version,
            registry_executable,
        ));
    };
    let mut owned_paths = Vec::new();
    let mut owned_bytes = 0_u64;
    let mut target_present = false;
    inspect_app_root(
        &app_root,
        app_id,
        version,
        &expected_executable,
        &mut owned_paths,
        &mut owned_bytes,
        &mut target_present,
    )?;
    add_directory(&app_root, &mut owned_paths)?;

    // A path can disappear after inspection; retaining the derived path in the
    // plan lets the recovery invocation clear the manifest without guessing a
    // new location.  Directories are removed after files, deepest first.
    owned_paths.sort_by(|left, right| {
        path_depth(right)
            .cmp(&path_depth(left))
            .then_with(|| right.as_os_str().cmp(left.as_os_str()))
    });
    let state = if target_present {
        RemovalState::Ready
    } else if owned_paths.is_empty() {
        RemovalState::Missing
    } else {
        RemovalState::Partial
    };
    Ok(RemovalPlan {
        manager_root: canonical_root,
        app_id: app_id.to_string(),
        version: version.to_string(),
        registry_executable: registry_executable.to_string(),
        app_root,
        executable: expected_executable,
        state,
        owned_entry_count: owned_paths.len(),
        owned_bytes,
        owned_paths,
    })
}

/// Remove only the paths collected by a validated plan.  No recursive delete
/// primitive is used: an unexpected link/foreign file or an I/O failure stops
/// the walk and returns a bounded recovery report.  A caller can restore its
/// manifest and retry the exact plan after the blocking condition is fixed.
pub fn remove_portable_tree(plan: &RemovalPlan) -> Result<RemovalOutcome, RemovalError> {
    // Re-run all path and tree checks immediately before destructive I/O.  The
    // caller's manifest CAS is a separate boundary; this closes the local
    // preview-to-remove gap and rejects a changed tree before deletion.
    let current = inspect_portable_removal(
        &plan.manager_root,
        &plan.app_id,
        &plan.version,
        &plan.registry_executable,
    )?;
    if current.app_root != plan.app_root
        || current.executable != plan.executable
        || current.state != plan.state
    {
        return Err(RemovalError::UnsafePath);
    }
    if current.state == RemovalState::Missing {
        return Ok(RemovalOutcome {
            complete: true,
            removed_entry_count: 0,
            remaining_entry_count: 0,
            failure: None,
        });
    }

    let mut removed = 0_usize;
    for path in &current.owned_paths {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Ok(partial_outcome(&current, removed, Some(RemovalFailure::Io))),
        };
        if is_link_or_reparse(&metadata) {
            return Ok(partial_outcome(
                &current,
                removed,
                Some(RemovalFailure::UnsafePath),
            ));
        }
        let result = if metadata.is_file() {
            fs::remove_file(path)
        } else if metadata.is_dir() {
            fs::remove_dir(path)
        } else {
            return Ok(partial_outcome(
                &current,
                removed,
                Some(RemovalFailure::UnsupportedEntry),
            ));
        };
        match result {
            Ok(()) => removed = removed.saturating_add(1),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Ok(partial_outcome(&current, removed, Some(RemovalFailure::Io))),
        }
    }

    let remaining = remaining_entry_count(&current.app_root);
    Ok(RemovalOutcome {
        complete: !current.app_root.exists() && remaining == 0,
        removed_entry_count: removed,
        remaining_entry_count: remaining,
        failure: (remaining != 0).then_some(RemovalFailure::Io),
    })
}

fn partial_outcome(
    plan: &RemovalPlan,
    removed: usize,
    failure: Option<RemovalFailure>,
) -> RemovalOutcome {
    RemovalOutcome {
        complete: false,
        removed_entry_count: removed,
        remaining_entry_count: remaining_entry_count(&plan.app_root),
        failure,
    }
}

fn remaining_entry_count(path: &Path) -> usize {
    match bounded_entry_count(path) {
        Ok(count) => count,
        Err(RemovalError::Missing) => 0,
        Err(_) => 1,
    }
}

fn missing_plan(
    manager_root: &Path,
    app_id: &str,
    version: &str,
    registry_executable: &str,
) -> RemovalPlan {
    let app_root = manager_root.join("apps").join(app_id);
    let executable = app_root
        .join("versions")
        .join(version)
        .join(format!("{app_id}.exe"));
    RemovalPlan {
        manager_root: manager_root.to_path_buf(),
        app_id: app_id.to_string(),
        version: version.to_string(),
        registry_executable: registry_executable.to_string(),
        app_root,
        executable,
        state: RemovalState::Missing,
        owned_entry_count: 0,
        owned_bytes: 0,
        owned_paths: Vec::new(),
    }
}

fn inspect_app_root(
    app_root: &Path,
    app_id: &str,
    target_version: &str,
    expected_executable: &Path,
    owned_paths: &mut Vec<PathBuf>,
    owned_bytes: &mut u64,
    target_present: &mut bool,
) -> Result<(), RemovalError> {
    let root_metadata = fs::symlink_metadata(app_root).map_err(|_| RemovalError::Missing)?;
    if is_link_or_reparse(&root_metadata) || !root_metadata.is_dir() {
        return Err(RemovalError::UnsafePath);
    }
    let entries = fs::read_dir(app_root).map_err(|_| RemovalError::Io)?;
    for entry in entries {
        let entry = entry.map_err(|_| RemovalError::Io)?;
        let path = entry.path();
        let name = entry.file_name();
        let metadata = fs::symlink_metadata(&path).map_err(|_| RemovalError::Io)?;
        if is_link_or_reparse(&metadata) {
            return Err(RemovalError::UnsafePath);
        }
        if name == "current.json" {
            if !metadata.is_file() || metadata.len() > MAX_CURRENT_BYTES {
                return Err(RemovalError::UnsupportedEntry);
            }
            add_file(&path, &metadata, owned_paths, owned_bytes)?;
        } else if name == "versions" {
            if !metadata.is_dir() {
                return Err(RemovalError::UnsupportedEntry);
            }
            inspect_versions(
                &path,
                app_id,
                target_version,
                expected_executable,
                owned_paths,
                owned_bytes,
                target_present,
            )?;
        } else {
            // This is a user/foreign entry inside the Manager layout.  Never
            // infer ownership from a filename or delete it as a side effect.
            return Err(RemovalError::ForeignEntry);
        }
    }
    Ok(())
}

fn inspect_versions(
    versions_root: &Path,
    app_id: &str,
    target_version: &str,
    expected_executable: &Path,
    owned_paths: &mut Vec<PathBuf>,
    owned_bytes: &mut u64,
    target_present: &mut bool,
) -> Result<(), RemovalError> {
    let versions_metadata = fs::symlink_metadata(versions_root).map_err(|_| RemovalError::Io)?;
    if is_link_or_reparse(&versions_metadata) || !versions_metadata.is_dir() {
        return Err(RemovalError::UnsafePath);
    }
    let mut version_count = 0_usize;
    let entries = fs::read_dir(versions_root).map_err(|_| RemovalError::Io)?;
    for entry in entries {
        let entry = entry.map_err(|_| RemovalError::Io)?;
        version_count = version_count
            .checked_add(1)
            .ok_or(RemovalError::UnsupportedEntry)?;
        if version_count > MAX_TREE_ENTRIES {
            return Err(RemovalError::UnsupportedEntry);
        }
        let version_name = entry
            .file_name()
            .into_string()
            .map_err(|_| RemovalError::InvalidIdentity)?;
        if !safe_component(&version_name, 128) {
            return Err(RemovalError::InvalidIdentity);
        }
        let version_dir = entry.path();
        let metadata = fs::symlink_metadata(&version_dir).map_err(|_| RemovalError::Io)?;
        if is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(RemovalError::UnsafePath);
        }
        let expected_name = format!("{app_id}.exe");
        let expected_partial_name = format!("{app_id}.exe.partial");
        let mut saw_file = false;
        let files = fs::read_dir(&version_dir).map_err(|_| RemovalError::Io)?;
        for file in files {
            let file = file.map_err(|_| RemovalError::Io)?;
            let path = file.path();
            let name = file.file_name();
            let name = name.to_str().ok_or(RemovalError::InvalidIdentity)?;
            let metadata = fs::symlink_metadata(&path).map_err(|_| RemovalError::Io)?;
            if is_link_or_reparse(&metadata) {
                return Err(RemovalError::UnsafePath);
            }
            let valid_name = name == expected_name || name == expected_partial_name;
            if !valid_name {
                return Err(RemovalError::ForeignEntry);
            }
            if !metadata.is_file() {
                return Err(RemovalError::UnsupportedEntry);
            }
            saw_file = true;
            add_file(&path, &metadata, owned_paths, owned_bytes)?;
            if name == expected_name {
                if !same_path_identity(&path, expected_executable) {
                    return Err(RemovalError::RegistryMismatch);
                }
                if version_name == target_version {
                    *target_present = true;
                }
            }
        }
        if !saw_file && version_name != target_version {
            return Err(RemovalError::ForeignEntry);
        }
        add_directory(&version_dir, owned_paths)?;
    }
    add_directory(versions_root, owned_paths)?;
    Ok(())
}

fn add_file(
    path: &Path,
    metadata: &fs::Metadata,
    owned_paths: &mut Vec<PathBuf>,
    owned_bytes: &mut u64,
) -> Result<(), RemovalError> {
    if owned_paths.len() >= MAX_TREE_ENTRIES {
        return Err(RemovalError::UnsupportedEntry);
    }
    owned_paths.push(path.to_path_buf());
    *owned_bytes = owned_bytes
        .checked_add(metadata.len())
        .ok_or(RemovalError::UnsupportedEntry)?;
    Ok(())
}

fn add_directory(path: &Path, owned_paths: &mut Vec<PathBuf>) -> Result<(), RemovalError> {
    if owned_paths.len() >= MAX_TREE_ENTRIES {
        return Err(RemovalError::UnsupportedEntry);
    }
    owned_paths.push(path.to_path_buf());
    Ok(())
}

fn path_depth(path: &Path) -> usize {
    path.components().count()
}

fn bounded_entry_count(path: &Path) -> Result<usize, RemovalError> {
    bounded_entry_count_at(path, 0)
}

fn bounded_entry_count_at(path: &Path, depth: usize) -> Result<usize, RemovalError> {
    if depth > MAX_TREE_DEPTH {
        return Err(RemovalError::UnsupportedEntry);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| RemovalError::Missing)?;
    if is_link_or_reparse(&metadata) {
        return Err(RemovalError::UnsafePath);
    }
    if metadata.is_file() {
        return Ok(1);
    }
    if !metadata.is_dir() {
        return Err(RemovalError::UnsupportedEntry);
    }
    let mut count = 1_usize;
    let entries = fs::read_dir(path).map_err(|_| RemovalError::Io)?;
    for entry in entries {
        let entry = entry.map_err(|_| RemovalError::Io)?;
        count = count
            .checked_add(bounded_entry_count_at(&entry.path(), depth + 1)?)
            .ok_or(RemovalError::UnsupportedEntry)?;
        if count > MAX_TREE_ENTRIES {
            return Err(RemovalError::UnsupportedEntry);
        }
    }
    Ok(count)
}

fn canonicalize_existing_directory(path: &Path) -> Result<PathBuf, RemovalError> {
    if !path.is_absolute() || !plain_existing_components(path) {
        return Err(RemovalError::UnsafePath);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| RemovalError::Missing)?;
    if is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(RemovalError::UnsafePath);
    }
    path.canonicalize()
        .map(normalize_canonical_path)
        .map_err(|_| RemovalError::Missing)
}

fn existing_directory_is_plain(path: &Path) -> Result<bool, RemovalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) => Err(RemovalError::UnsafePath),
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err(RemovalError::UnsafePath),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(RemovalError::Io),
    }
}

fn existing_directory(path: &Path) -> Result<Option<PathBuf>, RemovalError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_link_or_reparse(&metadata) => Err(RemovalError::UnsafePath),
        Ok(metadata) if metadata.is_dir() => path
            .canonicalize()
            .map(normalize_canonical_path)
            .map(Some)
            .map_err(|_| RemovalError::Missing),
        Ok(_) => Err(RemovalError::UnsafePath),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(RemovalError::Io),
    }
}

fn validate_registry_path(expected: &Path, registry: &str) -> Result<(), RemovalError> {
    let raw = Path::new(registry);
    if !plain_existing_components_allow_missing_final(raw)
        || !plain_existing_components_allow_missing_final(expected)
    {
        return Err(RemovalError::UnsafePath);
    }
    // Windows may spell the same app-owned root through an 8.3 alias while
    // canonicalization returns its long form. Resolve the deepest existing
    // plain ancestor and append only the still-missing tail, then compare the
    // complete derived identity. This also keeps interrupted-removal recovery
    // strict when the version directory or executable no longer exists.
    let normalized_raw = canonicalize_allow_missing(raw)?;
    let normalized_expected = canonicalize_allow_missing(expected)?;
    if !same_path_identity(&normalized_raw, &normalized_expected) {
        return Err(RemovalError::RegistryMismatch);
    }
    match fs::symlink_metadata(raw) {
        Ok(metadata) if is_link_or_reparse(&metadata) => Err(RemovalError::UnsafePath),
        Ok(metadata) if metadata.is_file() => {
            let canonical = raw
                .canonicalize()
                .map(normalize_canonical_path)
                .map_err(|_| RemovalError::RegistryMismatch)?;
            same_path_identity(&canonical, &normalized_expected)
                .then_some(())
                .ok_or(RemovalError::RegistryMismatch)
        }
        Ok(_) => Err(RemovalError::UnsupportedEntry),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(RemovalError::Io),
    }
}

fn canonicalize_allow_missing(path: &Path) -> Result<PathBuf, RemovalError> {
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if is_link_or_reparse(&metadata) {
                    return Err(RemovalError::UnsafePath);
                }
                let suffix = path
                    .strip_prefix(ancestor)
                    .map_err(|_| RemovalError::RegistryMismatch)?;
                let canonical = ancestor
                    .canonicalize()
                    .map(normalize_canonical_path)
                    .map_err(|_| RemovalError::RegistryMismatch)?;
                return Ok(canonical.join(suffix));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or(RemovalError::RegistryMismatch)?;
            }
            Err(_) => return Err(RemovalError::Io),
        }
    }
}

fn safe_component(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.trim() == value
        && !value.contains(['/', '\\', '\0', ':'])
        && value != "."
        && value != ".."
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn valid_absolute_literal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4_096
        && !value.contains(['%', '$', '\0'])
        && !value.starts_with('~')
        && !value.starts_with(r"\\?\")
        && !value.starts_with(r"\\.\")
        && !value.starts_with("//?/")
        && !value.starts_with("//./")
        && Path::new(value).is_absolute()
        && !Path::new(value)
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

fn plain_existing_components(path: &Path) -> bool {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => current.push(value),
            Component::CurDir | Component::ParentDir => return false,
        }
        if !current.is_absolute() {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(&current) else {
            return false;
        };
        if is_link_or_reparse(&metadata) {
            return false;
        }
    }
    true
}

fn plain_existing_components_allow_missing_final(path: &Path) -> bool {
    let mut current = PathBuf::new();
    let mut missing = false;
    let components = path.components().collect::<Vec<_>>();
    for component in &components {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => current.push(value),
            Component::CurDir | Component::ParentDir => return false,
        }
        if !current.is_absolute() {
            continue;
        }
        if missing {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link_or_reparse(&metadata) => return false,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
            }
            Err(_) => return false,
        }
    }
    true
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

fn protected_root(root: &Path) -> bool {
    if is_filesystem_root(root) {
        return true;
    }
    for variable in ["USERPROFILE", "HOME", "DEVBOX_WORKSPACE", "WORKSPACE"] {
        if std::env::var_os(variable)
            .and_then(|value| PathBuf::from(value).canonicalize().ok())
            .is_some_and(|protected| same_path_identity(&protected, root))
        {
            return true;
        }
    }
    std::env::current_dir()
        .and_then(|cwd| cwd.canonicalize())
        .is_ok_and(|cwd| same_path_identity(&cwd, root))
}

fn is_filesystem_root(path: &Path) -> bool {
    let mut saw_root = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) => {}
            Component::RootDir => saw_root = true,
            Component::CurDir | Component::ParentDir | Component::Normal(_) => return false,
        }
    }
    saw_root
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    struct TestRoot {
        root: PathBuf,
    }

    impl TestRoot {
        fn new() -> Self {
            let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "devbox-manager-safe-removal-{}-{nonce}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn install(&self) -> PathBuf {
            let executable = self.root.join("apps/code-pad/versions/0.5.0/code-pad.exe");
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(&executable, b"portable").unwrap();
            fs::write(
                executable
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("current.json"),
                b"{}",
            )
            .unwrap();
            executable
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn exact_plan_deletes_binary_tree_but_not_user_data_or_sibling_app() {
        let fixture = TestRoot::new();
        let executable = fixture.install();
        fs::create_dir_all(fixture.root.join("apps/keep/versions/1.0.0")).unwrap();
        fs::write(fixture.root.join("user-data.db"), b"keep").unwrap();
        let plan = inspect_portable_removal(
            &fixture.root,
            "code-pad",
            "0.5.0",
            executable.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(plan.state, RemovalState::Ready);
        assert!(remove_portable_tree(&plan).unwrap().complete);
        assert!(!fixture.root.join("apps/code-pad").exists());
        assert!(fixture.root.join("apps/keep").exists());
        assert_eq!(
            fs::read(fixture.root.join("user-data.db")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn traversal_and_foreign_entries_are_rejected_before_mutation() {
        let fixture = TestRoot::new();
        let executable = fixture.install();
        let outside = fixture.root.join("outside.txt");
        fs::write(&outside, b"outside").unwrap();
        fs::write(
            fixture
                .root
                .join("apps/code-pad/versions/0.5.0/foreign.txt"),
            b"keep",
        )
        .unwrap();
        assert_eq!(
            inspect_portable_removal(
                &fixture.root,
                "../code-pad",
                "0.5.0",
                executable.to_str().unwrap(),
            ),
            Err(RemovalError::InvalidIdentity)
        );
        assert_eq!(
            inspect_portable_removal(
                &fixture.root,
                "code-pad",
                "0.5.0",
                executable.to_str().unwrap(),
            ),
            Err(RemovalError::ForeignEntry)
        );
        assert!(outside.exists());
        assert!(executable.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_entry_is_rejected_without_following_or_deleting_target() {
        use std::os::unix::fs::symlink;

        let fixture = TestRoot::new();
        let executable = fixture.install();
        let outside = fixture.root.join("outside.txt");
        fs::write(&outside, b"outside").unwrap();
        symlink(
            &outside,
            fixture
                .root
                .join("apps/code-pad/versions/0.5.0/foreign.txt"),
        )
        .unwrap();
        assert_eq!(
            inspect_portable_removal(
                &fixture.root,
                "code-pad",
                "0.5.0",
                executable.to_str().unwrap(),
            ),
            Err(RemovalError::UnsafePath)
        );
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
    }

    #[test]
    fn missing_tree_is_a_retryable_manifest_cleanup_state() {
        let fixture = TestRoot::new();
        let executable = fixture
            .root
            .join("apps/code-pad/versions/0.5.0/code-pad.exe");
        let plan = inspect_portable_removal(
            &fixture.root,
            "code-pad",
            "0.5.0",
            executable.to_str().unwrap(),
        )
        .unwrap();
        assert_eq!(plan.state, RemovalState::Missing);
        assert!(remove_portable_tree(&plan).unwrap().complete);
    }

    #[cfg(unix)]
    #[test]
    fn permission_failure_returns_a_bounded_partial_outcome() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = TestRoot::new();
        let executable = fixture.install();
        let version_dir = executable.parent().unwrap();
        let original_mode = fs::metadata(version_dir).unwrap().permissions().mode() & 0o777;
        fs::set_permissions(
            version_dir,
            fs::Permissions::from_mode(original_mode & !0o222),
        )
        .unwrap();

        let plan = inspect_portable_removal(
            &fixture.root,
            "code-pad",
            "0.5.0",
            executable.to_str().unwrap(),
        )
        .unwrap();
        let outcome = remove_portable_tree(&plan).unwrap();
        fs::set_permissions(version_dir, fs::Permissions::from_mode(original_mode)).unwrap();

        assert!(!outcome.complete);
        assert_eq!(outcome.removed_entry_count, 0);
        assert!(outcome.remaining_entry_count > 0);
        assert_eq!(outcome.failure, Some(RemovalFailure::Io));
        assert!(executable.exists());
    }
}
