//! Custom install-root selection and locator commit boundary.
//!
//! This module deliberately does not move or delete an existing installation.
//! A root change is only allowed when the active manifest is empty and the
//! selected directory is an existing, canonical, empty directory.  The
//! preview is read-only; apply re-runs every check and uses the observed
//! locator revision as a compare-and-swap guard.

use devbox_launch::{parse_install_root_locator, InstallRootLocator, INSTALL_ROOT_SCHEMA_VERSION};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const MAX_INSTALL_ROOT_PATH_BYTES: usize = 4_096;
pub const MAX_LOCATOR_BYTES: u64 = 16 * 1024;
pub const MAX_MANIFEST_BYTES: u64 = 1_048_576;
pub const MAX_MANIFEST_ENTRIES: usize = 256;
pub const MAX_CANDIDATE_ENTRIES: usize = 4_096;
pub const MIN_FREE_SPACE_BYTES: u64 = 128 * 1024 * 1024;
pub const DEFAULT_ROOT_ID: &str = "devbox-manager-default";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallRootPreviewStatus {
    Ready,
    AlreadyActive,
    ExistingInstall,
    CandidateConflict,
    PermissionDenied,
    InsufficientFreeSpace,
    FreeSpaceUnavailable,
}

impl InstallRootPreviewStatus {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::AlreadyActive => "already-active",
            Self::ExistingInstall => "existing-install",
            Self::CandidateConflict => "candidate-conflict",
            Self::PermissionDenied => "permission-denied",
            Self::InsufficientFreeSpace => "insufficient-free-space",
            Self::FreeSpaceUnavailable => "free-space-unavailable",
        }
    }

    pub fn can_apply(self) -> bool {
        matches!(self, Self::Ready | Self::AlreadyActive)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveInstallLocation {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub root_id: String,
    pub registry_revision: u64,
    pub catalog_revision: u64,
    pub from_legacy_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRootPreview {
    pub status: InstallRootPreviewStatus,
    pub registry_revision: u64,
    pub catalog_revision: u64,
    pub active_root: PathBuf,
    pub candidate_root: PathBuf,
    pub candidate_manifest: PathBuf,
    pub candidate_root_id: String,
    pub free_space_bytes: Option<u64>,
    pub required_free_space_bytes: u64,
    pub active_install_count: usize,
    pub candidate_entry_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRootApply {
    pub status: InstallRootPreviewStatus,
    pub registry_revision: u64,
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub root_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomRootError {
    InvalidPath,
    PathTooLong,
    MissingDirectory,
    UnsafePath,
    ProtectedPath,
    LocatorInvalid,
    ActiveStateInvalid,
    ManifestTooLarge,
    ManifestInvalid,
    ManifestTooManyEntries,
    CandidateConflict,
    PermissionDenied,
    ExistingInstall,
    FreeSpaceUnavailable,
    InsufficientFreeSpace,
    RevisionMismatch,
    RevisionOverflow,
    InvalidCatalogRevision,
    Storage,
    Serialization,
    RollbackFailed,
    NonUtf8Path,
}

impl fmt::Display for CustomRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPath | Self::PathTooLong => "install root input is invalid",
            Self::MissingDirectory => "install root directory is unavailable",
            Self::UnsafePath | Self::ProtectedPath => "install root is unsafe",
            Self::LocatorInvalid => "install-root locator is unavailable",
            Self::ActiveStateInvalid => "active install state is invalid",
            Self::ManifestTooLarge | Self::ManifestTooManyEntries | Self::ManifestInvalid => {
                "install manifest is invalid"
            }
            Self::CandidateConflict => "install root is not empty",
            Self::PermissionDenied => "install root is not writable",
            Self::ExistingInstall => "existing installations require a separate migration",
            Self::FreeSpaceUnavailable => "free space could not be verified",
            Self::InsufficientFreeSpace => "install root has insufficient free space",
            Self::RevisionMismatch => "install root changed before confirmation",
            Self::RevisionOverflow => "install-root revision cannot advance",
            Self::InvalidCatalogRevision => "catalog revision is invalid",
            Self::Storage => "install-root state could not be stored",
            Self::Serialization => "install-root state could not be serialized",
            Self::RollbackFailed => "install-root state could not be rolled back safely",
            Self::NonUtf8Path => "install root cannot be displayed safely",
        })
    }
}

impl std::error::Error for CustomRootError {}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InstallRecord {
    pub app: String,
    pub version: String,
    pub mode: String,
    pub exe_path: String,
}

/// Read a Manager-owned manifest with bounded bytes/rows and no path/error
/// reflection. The returned records are used only to decide whether migration
/// would be required; lifecycle commands revalidate their own exact layout.
pub fn parse_install_manifest(input: &[u8]) -> Result<Vec<InstallRecord>, CustomRootError> {
    if input.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(CustomRootError::ManifestTooLarge);
    }
    let records: Vec<InstallRecord> =
        serde_json::from_slice(input).map_err(|_| CustomRootError::ManifestInvalid)?;
    if records.len() > MAX_MANIFEST_ENTRIES {
        return Err(CustomRootError::ManifestTooManyEntries);
    }

    let mut app_ids = HashSet::with_capacity(records.len());
    for record in &records {
        if !valid_component(&record.app, 64)
            || !valid_version(&record.version)
            || !matches!(record.mode.as_str(), "portable" | "installer")
            || !app_ids.insert(record.app.as_str())
        {
            return Err(CustomRootError::ManifestInvalid);
        }
        if record.mode == "installer" {
            if !record.exe_path.is_empty() {
                return Err(CustomRootError::ManifestInvalid);
            }
        } else if !valid_absolute_literal(&record.exe_path) {
            return Err(CustomRootError::ManifestInvalid);
        }
    }
    Ok(records)
}

pub fn read_install_manifest(path: &Path) -> Result<Vec<InstallRecord>, CustomRootError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CustomRootError::ActiveStateInvalid)?;
    if path_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(CustomRootError::ActiveStateInvalid);
    }
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(CustomRootError::ManifestTooLarge);
    }
    let file = File::open(path).map_err(|_| CustomRootError::ActiveStateInvalid)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CustomRootError::ActiveStateInvalid)?;
    let records = parse_install_manifest(&bytes)?;
    let root = path.parent().ok_or(CustomRootError::ActiveStateInvalid)?;
    validate_install_manifest_at_root(root, &records)?;
    Ok(records)
}

/// Validate that portable records point at the exact Manager-owned layout for
/// `root`. Shape-only parsing is intentionally separate so callers can parse
/// a proposed manifest before publishing it, while every on-disk manifest is
/// checked against its active root before it is trusted.
pub fn validate_install_manifest_at_root(
    root: &Path,
    records: &[InstallRecord],
) -> Result<(), CustomRootError> {
    if root
        .to_str()
        .is_none_or(|value| !valid_absolute_literal(value))
    {
        return Err(CustomRootError::ActiveStateInvalid);
    }
    ensure_plain_components(root).map_err(|_| CustomRootError::ActiveStateInvalid)?;
    let canonical_root =
        canonicalize_path(root).map_err(|_| CustomRootError::ActiveStateInvalid)?;
    let metadata =
        fs::symlink_metadata(&canonical_root).map_err(|_| CustomRootError::ActiveStateInvalid)?;
    if path_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(CustomRootError::ActiveStateInvalid);
    }
    for record in records {
        if record.mode != "portable" {
            continue;
        }
        let raw_executable = Path::new(&record.exe_path);
        ensure_plain_components(raw_executable).map_err(|_| CustomRootError::ManifestInvalid)?;
        let executable =
            canonicalize_path(raw_executable).map_err(|_| CustomRootError::ManifestInvalid)?;
        let expected = canonical_root
            .join("apps")
            .join(&record.app)
            .join("versions")
            .join(&record.version)
            .join(format!("{}.exe", record.app));
        let expected =
            canonicalize_path(&expected).map_err(|_| CustomRootError::ManifestInvalid)?;
        if !same_path_identity(&executable, &expected)
            || !path_within(&canonical_root, &executable)
            || !executable.is_file()
        {
            return Err(CustomRootError::ManifestInvalid);
        }
    }
    Ok(())
}

/// Resolve the current root. A missing locator is the only legacy fallback;
/// a present but malformed locator never silently falls back to the old root.
pub fn resolve_active_location(
    locator_path: &Path,
    default_root: &Path,
) -> Result<ActiveInstallLocation, CustomRootError> {
    if let Some(parent) = locator_path.parent() {
        // A missing locator is the compatibility case, but an existing
        // symlink/reparse component in its parent is still unsafe. Stop at
        // the first genuinely missing component so v0.4.x remains readable.
        ensure_plain_existing_components(parent).map_err(|_| CustomRootError::LocatorInvalid)?;
    }
    let locator_metadata = match fs::symlink_metadata(locator_path) {
        Ok(metadata) => {
            if path_is_link_or_reparse(&metadata) || !metadata.is_file() {
                return Err(CustomRootError::LocatorInvalid);
            }
            Some(metadata)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(CustomRootError::LocatorInvalid),
    };

    let Some(_locator_metadata) = locator_metadata else {
        let default_root = canonical_safe_directory(default_root, &[])?;
        let manifest = default_root.join("registry.json");
        return Ok(ActiveInstallLocation {
            root: default_root,
            manifest,
            root_id: DEFAULT_ROOT_ID.to_string(),
            registry_revision: 0,
            catalog_revision: 0,
            from_legacy_fallback: true,
        });
    };

    if let Some(parent) = locator_path.parent() {
        ensure_plain_components(parent).map_err(|_| CustomRootError::LocatorInvalid)?;
    }
    let input = read_bounded_text(locator_path, MAX_LOCATOR_BYTES)
        .map_err(|_| CustomRootError::LocatorInvalid)?;
    let locator =
        parse_install_root_locator(&input).map_err(|_| CustomRootError::LocatorInvalid)?;
    let root = canonical_safe_directory(Path::new(&locator.path), &[])
        .map_err(|_| CustomRootError::LocatorInvalid)?;
    if locator.root_id == DEFAULT_ROOT_ID {
        let default_root = canonical_safe_directory(default_root, &[])
            .map_err(|_| CustomRootError::LocatorInvalid)?;
        if !same_path_identity(&root, &default_root) {
            return Err(CustomRootError::LocatorInvalid);
        }
    }
    let manifest = canonical_manifest(&root, Path::new(&locator.manifest_path))
        .map_err(|_| CustomRootError::LocatorInvalid)?;
    Ok(ActiveInstallLocation {
        root,
        manifest,
        root_id: locator.root_id,
        registry_revision: locator.registry_revision,
        catalog_revision: locator.catalog_revision,
        from_legacy_fallback: false,
    })
}

/// Build a read-only preview. `free_space_override` exists solely for
/// deterministic tests; production passes `None` and uses the OS API.
pub fn preview_custom_root(
    locator_path: &Path,
    default_root: &Path,
    common_root: Option<&Path>,
    candidate_input: &str,
    catalog_revision: u64,
    free_space_override: Option<u64>,
) -> Result<InstallRootPreview, CustomRootError> {
    if catalog_revision == 0 {
        return Err(CustomRootError::InvalidCatalogRevision);
    }
    let active = resolve_active_location(locator_path, default_root)?;
    let active_records = if active.manifest.exists() {
        read_install_manifest(&active.manifest)?
    } else if active.from_legacy_fallback {
        Vec::new()
    } else {
        return Err(CustomRootError::ActiveStateInvalid);
    };
    let candidate = canonical_safe_directory(
        Path::new(normalize_input(candidate_input)?.as_str()),
        common_root.into_iter().collect::<Vec<_>>().as_slice(),
    )?;
    let candidate_manifest = candidate.join("registry.json");
    if candidate_manifest
        .to_str()
        .is_none_or(|path| path.len() > MAX_INSTALL_ROOT_PATH_BYTES)
    {
        return Err(CustomRootError::PathTooLong);
    }
    let candidate_root_id = if same_path_identity(&candidate, &active.root) {
        active.root_id.clone()
    } else {
        custom_root_id(&candidate)
    };
    let candidate_entry_count = bounded_entry_count(&candidate)?;
    let candidate_writable = candidate_is_writable(&candidate)?;
    let free_space_bytes = free_space_override.or_else(|| available_space_bytes(&candidate));
    let active_artifact_count = active_artifact_count(&active.root)?;

    let status = if same_path_identity(&candidate, &active.root) {
        InstallRootPreviewStatus::AlreadyActive
    } else if !active_records.is_empty() || active_artifact_count != 0 {
        InstallRootPreviewStatus::ExistingInstall
    } else if candidate_entry_count != 0 {
        InstallRootPreviewStatus::CandidateConflict
    } else if !candidate_writable {
        InstallRootPreviewStatus::PermissionDenied
    } else if free_space_bytes.is_none() {
        InstallRootPreviewStatus::FreeSpaceUnavailable
    } else if free_space_bytes.is_some_and(|free| free < MIN_FREE_SPACE_BYTES) {
        InstallRootPreviewStatus::InsufficientFreeSpace
    } else {
        InstallRootPreviewStatus::Ready
    };

    Ok(InstallRootPreview {
        status,
        registry_revision: active.registry_revision,
        catalog_revision,
        active_root: active.root,
        candidate_root: candidate,
        candidate_manifest,
        candidate_root_id,
        free_space_bytes,
        required_free_space_bytes: MIN_FREE_SPACE_BYTES,
        active_install_count: active_records.len(),
        candidate_entry_count,
    })
}

/// Apply a previously previewed root. The path is untrusted input and is
/// canonicalized again. The observed locator revision is a CAS token; no
/// filesystem mutation occurs if it no longer matches.
pub fn apply_custom_root(
    locator_path: &Path,
    default_root: &Path,
    common_root: Option<&Path>,
    candidate_input: &str,
    expected_registry_revision: u64,
    catalog_revision: u64,
    updated_at_ms: u64,
) -> Result<InstallRootApply, CustomRootError> {
    if updated_at_ms == 0 {
        return Err(CustomRootError::Serialization);
    }
    let preview = preview_custom_root(
        locator_path,
        default_root,
        common_root,
        candidate_input,
        catalog_revision,
        None,
    )?;
    if preview.registry_revision != expected_registry_revision {
        return Err(CustomRootError::RevisionMismatch);
    }
    if !preview.status.can_apply() {
        return Err(match preview.status {
            InstallRootPreviewStatus::ExistingInstall => CustomRootError::ExistingInstall,
            InstallRootPreviewStatus::CandidateConflict => CustomRootError::CandidateConflict,
            InstallRootPreviewStatus::PermissionDenied => CustomRootError::PermissionDenied,
            InstallRootPreviewStatus::InsufficientFreeSpace => {
                CustomRootError::InsufficientFreeSpace
            }
            InstallRootPreviewStatus::FreeSpaceUnavailable => CustomRootError::FreeSpaceUnavailable,
            InstallRootPreviewStatus::Ready | InstallRootPreviewStatus::AlreadyActive => {
                CustomRootError::ActiveStateInvalid
            }
        });
    }
    if preview.status == InstallRootPreviewStatus::AlreadyActive {
        return Ok(InstallRootApply {
            status: preview.status,
            registry_revision: preview.registry_revision,
            root: preview.candidate_root,
            manifest: preview.candidate_manifest,
            root_id: preview.candidate_root_id,
        });
    }

    // Re-read immediately before any write. This closes the preview/confirm
    // race even when another Manager instance changed the locator atomically.
    let current = resolve_active_location(locator_path, default_root)?;
    if current.registry_revision != expected_registry_revision
        || !same_path_identity(&current.root, &preview.active_root)
    {
        return Err(CustomRootError::RevisionMismatch);
    }
    let current_records = if current.manifest.exists() {
        read_install_manifest(&current.manifest)?
    } else if current.from_legacy_fallback {
        Vec::new()
    } else {
        return Err(CustomRootError::ActiveStateInvalid);
    };
    if !current_records.is_empty() {
        return Err(CustomRootError::ExistingInstall);
    }
    if active_artifact_count(&current.root)? != 0 {
        return Err(CustomRootError::ExistingInstall);
    }
    let registry_revision = expected_registry_revision
        .checked_add(1)
        .ok_or(CustomRootError::RevisionOverflow)?;

    let candidate = canonical_safe_directory(
        &preview.candidate_root,
        common_root.into_iter().collect::<Vec<_>>().as_slice(),
    )?;
    if bounded_entry_count(&candidate)? != 0 {
        return Err(CustomRootError::CandidateConflict);
    }
    if !candidate_is_writable(&candidate)? {
        return Err(CustomRootError::PermissionDenied);
    }
    let free = available_space_bytes(&candidate).ok_or(CustomRootError::FreeSpaceUnavailable)?;
    if free < MIN_FREE_SPACE_BYTES {
        return Err(CustomRootError::InsufficientFreeSpace);
    }

    let apps_dir = candidate.join("apps");
    let manifest = candidate.join("registry.json");
    let candidate_text = candidate
        .to_str()
        .ok_or(CustomRootError::NonUtf8Path)?
        .to_string();
    let manifest_text = manifest
        .to_str()
        .ok_or(CustomRootError::NonUtf8Path)?
        .to_string();
    ensure_new_directory(&apps_dir)?;
    if let Err((error, manifest_created)) = create_new_empty_manifest(&manifest) {
        let rolled_back = if manifest_created {
            cleanup_new_root_artifacts(&apps_dir, &manifest)
        } else {
            cleanup_empty_directory(&apps_dir)
        };
        if !rolled_back {
            return Err(CustomRootError::RollbackFailed);
        }
        return Err(error);
    }

    let locator = InstallRootLocator {
        schema_version: INSTALL_ROOT_SCHEMA_VERSION,
        registry_revision,
        catalog_revision,
        root_id: preview.candidate_root_id.clone(),
        path: candidate_text,
        manifest_path: manifest_text,
        updated_at_ms,
    };

    if let Err(error) = verify_new_root_artifacts(&candidate, &apps_dir, &manifest) {
        if !cleanup_new_root_artifacts(&apps_dir, &manifest) {
            return Err(CustomRootError::RollbackFailed);
        }
        return Err(error);
    }

    let locator_result = write_locator_if_current(
        locator_path,
        &locator,
        expected_registry_revision,
        &preview.active_root,
    );
    if let Err(error) = locator_result {
        if !cleanup_new_root_artifacts(&apps_dir, &manifest) {
            return Err(CustomRootError::RollbackFailed);
        }
        return Err(error);
    }

    Ok(InstallRootApply {
        status: InstallRootPreviewStatus::Ready,
        registry_revision,
        root: candidate,
        manifest,
        root_id: locator.root_id,
    })
}

fn write_locator_if_current(
    locator_path: &Path,
    candidate: &InstallRootLocator,
    expected_registry_revision: u64,
    expected_active_root: &Path,
) -> Result<(), CustomRootError> {
    let current = match fs::symlink_metadata(locator_path) {
        Ok(metadata) => {
            if path_is_link_or_reparse(&metadata) || !metadata.is_file() {
                return Err(CustomRootError::LocatorInvalid);
            }
            let input = read_bounded_text(locator_path, MAX_LOCATOR_BYTES)
                .map_err(|_| CustomRootError::LocatorInvalid)?;
            Some(parse_install_root_locator(&input).map_err(|_| CustomRootError::LocatorInvalid)?)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(CustomRootError::LocatorInvalid),
    };
    if current
        .as_ref()
        .map_or(0, |locator| locator.registry_revision)
        != expected_registry_revision
        || current.as_ref().is_some_and(|locator| {
            !same_path_identity(Path::new(&locator.path), expected_active_root)
        })
    {
        return Err(CustomRootError::RevisionMismatch);
    }
    let encoded =
        serde_json::to_string_pretty(candidate).map_err(|_| CustomRootError::Serialization)?;
    parse_install_root_locator(&encoded).map_err(|_| CustomRootError::Serialization)?;
    let parent = locator_path.parent().ok_or(CustomRootError::Storage)?;
    ensure_plain_components(parent).map_err(|_| CustomRootError::Storage)?;
    ensure_existing_directory(parent)?;
    devbox_filesystem::atomic_write(locator_path, encoded.as_bytes())
        .map_err(|_| CustomRootError::Storage)
}

fn normalize_input(input: &str) -> Result<String, CustomRootError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(CustomRootError::InvalidPath);
    }
    if trimmed.len() > MAX_INSTALL_ROOT_PATH_BYTES {
        return Err(CustomRootError::PathTooLong);
    }
    if trimmed.contains(['\0', '%', '$'])
        || trimmed.starts_with('~')
        || trimmed.starts_with(r"\\?\")
        || trimmed.starts_with(r"\\.\")
        || trimmed.starts_with("//?/")
        || trimmed.starts_with("//./")
        || trimmed
            .split(['/', '\\'])
            .any(|segment| matches!(segment, "." | ".."))
    {
        return Err(CustomRootError::InvalidPath);
    }
    let mut normalized = trimmed.to_string();
    while normalized.len() > 1 && normalized.ends_with(['/', '\\']) {
        // Do not turn a Windows drive root such as `C:\\` into `C:`.
        if normalized.len() == 3
            && normalized.as_bytes().get(1) == Some(&b':')
            && normalized
                .as_bytes()
                .get(2)
                .is_some_and(|byte| *byte == b'/' || *byte == b'\\')
        {
            break;
        }
        normalized.pop();
    }
    Ok(normalized)
}

fn canonical_safe_directory(path: &Path, protected: &[&Path]) -> Result<PathBuf, CustomRootError> {
    let raw = path.to_str().ok_or(CustomRootError::NonUtf8Path)?;
    let normalized = normalize_input(raw)?;
    let raw_path = Path::new(&normalized);
    if !valid_absolute_literal(&normalized) {
        return Err(CustomRootError::InvalidPath);
    }
    ensure_plain_components(raw_path)?;
    let metadata = fs::symlink_metadata(raw_path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CustomRootError::MissingDirectory
        } else {
            CustomRootError::UnsafePath
        }
    })?;
    if path_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(CustomRootError::UnsafePath);
    }
    let canonical = canonicalize_path(raw_path).map_err(|_| CustomRootError::UnsafePath)?;
    // `ensure_plain_components` already rejects symlinks and Windows reparse
    // points. Do not require textual identity: Windows canonicalization may
    // legitimately expand an 8.3 path component while preserving identity.
    if dangerous_root(&canonical) {
        return Err(CustomRootError::ProtectedPath);
    }
    for protected_root in protected {
        if let Ok(protected) = canonicalize_protected(protected_root) {
            if same_path_identity(&canonical, &protected) {
                return Err(CustomRootError::ProtectedPath);
            }
        }
    }
    Ok(canonical)
}

fn canonical_manifest(root: &Path, manifest: &Path) -> Result<PathBuf, CustomRootError> {
    let expected = root.join("registry.json");
    let raw = manifest.to_str().ok_or(CustomRootError::UnsafePath)?;
    if !valid_absolute_literal(raw) || !same_path_identity(manifest, &expected) {
        return Err(CustomRootError::UnsafePath);
    }
    ensure_plain_components(manifest)?;
    let metadata = fs::symlink_metadata(manifest).map_err(|_| CustomRootError::UnsafePath)?;
    if path_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(CustomRootError::UnsafePath);
    }
    let canonical = canonicalize_path(manifest).map_err(|_| CustomRootError::UnsafePath)?;
    if !same_path_identity(&canonical, &expected) {
        return Err(CustomRootError::UnsafePath);
    }
    Ok(canonical)
}

fn canonicalize_protected(path: &Path) -> Result<PathBuf, CustomRootError> {
    canonicalize_path(path).map_err(|_| CustomRootError::ProtectedPath)
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

fn read_bounded_text(path: &Path, max_bytes: u64) -> Result<String, CustomRootError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CustomRootError::LocatorInvalid)?;
    if path_is_link_or_reparse(&metadata) || !metadata.is_file() || metadata.len() > max_bytes {
        return Err(CustomRootError::LocatorInvalid);
    }
    let file = File::open(path).map_err(|_| CustomRootError::LocatorInvalid)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CustomRootError::LocatorInvalid)?;
    if bytes.len() as u64 > max_bytes {
        return Err(CustomRootError::LocatorInvalid);
    }
    String::from_utf8(bytes).map_err(|_| CustomRootError::LocatorInvalid)
}

fn bounded_entry_count(path: &Path) -> Result<usize, CustomRootError> {
    let mut count: usize = 0;
    let entries = fs::read_dir(path).map_err(|_| CustomRootError::UnsafePath)?;
    for entry in entries {
        let entry = entry.map_err(|_| CustomRootError::UnsafePath)?;
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|_| CustomRootError::UnsafePath)?;
        if path_is_link_or_reparse(&metadata) {
            return Err(CustomRootError::UnsafePath);
        }
        count += 1;
        if count > MAX_CANDIDATE_ENTRIES {
            return Err(CustomRootError::CandidateConflict);
        }
    }
    Ok(count)
}

fn candidate_is_writable(path: &Path) -> Result<bool, CustomRootError> {
    let metadata = fs::metadata(path).map_err(|_| CustomRootError::UnsafePath)?;
    if metadata.permissions().readonly() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o222 == 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn active_artifact_count(root: &Path) -> Result<usize, CustomRootError> {
    let entries = fs::read_dir(root).map_err(|_| CustomRootError::ActiveStateInvalid)?;
    let mut count: usize = 0;
    for entry in entries {
        let entry = entry.map_err(|_| CustomRootError::ActiveStateInvalid)?;
        let name = entry.file_name();
        if name == "registry.json" {
            continue;
        }
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|_| CustomRootError::ActiveStateInvalid)?;
        if path_is_link_or_reparse(&metadata) {
            return Err(CustomRootError::ActiveStateInvalid);
        }
        if name == "apps" && metadata.is_dir() {
            let nested = bounded_entry_count(&entry.path())?;
            count = count.saturating_add(nested);
        } else {
            count = count.saturating_add(1);
        }
        if count > MAX_CANDIDATE_ENTRIES {
            return Err(CustomRootError::ActiveStateInvalid);
        }
    }
    Ok(count)
}

fn ensure_new_directory(path: &Path) -> Result<(), CustomRootError> {
    if path.exists() {
        return Err(CustomRootError::CandidateConflict);
    }
    fs::create_dir(path).map_err(|_| CustomRootError::Storage)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            let _ = fs::remove_dir(path);
            return Err(CustomRootError::Storage);
        }
    };
    if path_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        if !path_is_link_or_reparse(&metadata) && metadata.is_dir() {
            let _ = fs::remove_dir(path);
        }
        return Err(CustomRootError::UnsafePath);
    }
    Ok(())
}

fn ensure_existing_directory(path: &Path) -> Result<(), CustomRootError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CustomRootError::Storage)?;
    if path_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(CustomRootError::UnsafePath);
    }
    Ok(())
}

/// Create the inactive candidate manifest without replacing a path that
/// appeared after the empty-directory preflight. The locator is the publish
/// boundary, so a synced exclusive file is sufficient here and avoids the
/// overwrite semantics required by normal atomic state updates.
fn create_new_empty_manifest(manifest: &Path) -> Result<(), (CustomRootError, bool)> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(manifest)
        .map_err(|error| {
            let mapped = if error.kind() == std::io::ErrorKind::AlreadyExists {
                CustomRootError::CandidateConflict
            } else {
                CustomRootError::Storage
            };
            (mapped, false)
        })?;
    let result = file
        .write_all(b"[]")
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);
    result.map_err(|_| (CustomRootError::Storage, true))
}

fn cleanup_empty_directory(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !path_is_link_or_reparse(&metadata) && metadata.is_dir() => {
            fs::read_dir(path)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false)
                && fs::remove_dir(path).is_ok()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        _ => false,
    }
}

fn verify_new_root_artifacts(
    root: &Path,
    apps_dir: &Path,
    manifest: &Path,
) -> Result<(), CustomRootError> {
    ensure_existing_directory(root)?;
    ensure_existing_directory(apps_dir)?;
    if fs::read_dir(apps_dir)
        .map_err(|_| CustomRootError::UnsafePath)?
        .next()
        .is_some()
    {
        return Err(CustomRootError::CandidateConflict);
    }

    let manifest_metadata = fs::symlink_metadata(manifest).map_err(|_| CustomRootError::Storage)?;
    if path_is_link_or_reparse(&manifest_metadata)
        || !manifest_metadata.is_file()
        || manifest_metadata.len() != 2
        || fs::read(manifest).map_err(|_| CustomRootError::Storage)? != b"[]"
    {
        return Err(CustomRootError::CandidateConflict);
    }

    let mut saw_apps = false;
    let mut saw_manifest = false;
    let entries = fs::read_dir(root).map_err(|_| CustomRootError::UnsafePath)?;
    for entry in entries {
        let entry = entry.map_err(|_| CustomRootError::UnsafePath)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| CustomRootError::UnsafePath)?;
        if path_is_link_or_reparse(&metadata) {
            return Err(CustomRootError::UnsafePath);
        }
        if same_path_identity(&path, apps_dir) && metadata.is_dir() && !saw_apps {
            saw_apps = true;
        } else if same_path_identity(&path, manifest) && metadata.is_file() && !saw_manifest {
            saw_manifest = true;
        } else {
            return Err(CustomRootError::CandidateConflict);
        }
    }
    if saw_apps && saw_manifest {
        Ok(())
    } else {
        Err(CustomRootError::CandidateConflict)
    }
}

fn cleanup_new_root_artifacts(apps_dir: &Path, manifest: &Path) -> bool {
    let manifest_removed = match fs::symlink_metadata(manifest) {
        Ok(metadata)
            if !path_is_link_or_reparse(&metadata)
                && metadata.is_file()
                && metadata.len() == 2
                && fs::read(manifest).is_ok_and(|bytes| bytes == b"[]") =>
        {
            fs::remove_file(manifest).is_ok()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        _ => false,
    };
    let apps_removed = cleanup_empty_directory(apps_dir);
    manifest_removed && apps_removed
}

fn custom_root_id(root: &Path) -> String {
    let digest = Sha256::digest(root.to_string_lossy().as_bytes());
    let mut id = String::from("custom-");
    for byte in digest.iter().take(24) {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

fn ensure_plain_components(path: &Path) -> Result<(), CustomRootError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => current.push(value),
            Component::CurDir | Component::ParentDir => return Err(CustomRootError::InvalidPath),
        }
        // A Windows disk prefix is drive-relative until RootDir is appended;
        // begin probing once the accumulated path is absolute.
        if !current.is_absolute() {
            continue;
        }
        let metadata =
            fs::symlink_metadata(&current).map_err(|_| CustomRootError::MissingDirectory)?;
        if path_is_link_or_reparse(&metadata) {
            return Err(CustomRootError::UnsafePath);
        }
    }
    Ok(())
}

/// Check only components that already exist. This is used for optional
/// versioned metadata paths where the final locator may be absent on a legacy
/// installation; a present link/reparse component must never be followed.
fn ensure_plain_existing_components(path: &Path) -> Result<(), CustomRootError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::Normal(value) => current.push(value),
            Component::CurDir | Component::ParentDir => return Err(CustomRootError::InvalidPath),
        }
        if !current.is_absolute() {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if path_is_link_or_reparse(&metadata) => {
                return Err(CustomRootError::UnsafePath)
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(CustomRootError::UnsafePath),
        }
    }
    Ok(())
}

fn dangerous_root(root: &Path) -> bool {
    if is_filesystem_root(root) {
        return true;
    }
    for variable in ["USERPROFILE", "HOME", "DEVBOX_WORKSPACE", "WORKSPACE"] {
        if std::env::var_os(variable)
            .and_then(|value| canonicalize_path(&PathBuf::from(value)).ok())
            .is_some_and(|protected| same_path_identity(&protected, root))
        {
            return true;
        }
    }
    std::env::current_dir()
        .and_then(|cwd| canonicalize_path(&cwd))
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

fn valid_absolute_literal(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_INSTALL_ROOT_PATH_BYTES
        && !value.contains(['%', '$', '\0'])
        && !value.starts_with('~')
        && !value.starts_with(r"\\?\")
        && !value.starts_with(r"\\.\")
        && !value.starts_with("//?/")
        && !value.starts_with("//./")
        && Path::new(value).is_absolute()
        && !value
            .split(['/', '\\'])
            .any(|segment| matches!(segment, "." | ".."))
        && !Path::new(value)
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

fn valid_component(value: &str, max_len: usize) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= max_len
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .copied()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_version(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && (part == &"0" || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u32>().is_ok()
        })
}

#[cfg(windows)]
fn path_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn path_is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(unix)]
fn available_space_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    let c_path = std::ffi::CString::new(bytes).ok()?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };
    u128::from(stats.f_bavail)
        .checked_mul(u128::from(stats.f_frsize))
        .and_then(|bytes| u64::try_from(bytes).ok())
}

#[cfg(windows)]
fn available_space_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut free = 0_u64;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            Some(&mut free as *mut u64),
            None,
            None,
        )
        .ok()?;
    }
    Some(free)
}

#[cfg(not(any(unix, windows)))]
fn available_space_bytes(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    struct Fixture {
        outer: PathBuf,
        default_root: PathBuf,
        candidate: PathBuf,
        common: PathBuf,
        locator: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let outer = std::env::temp_dir().join(format!(
                "devbox-manager-custom-root-{}-{id}",
                std::process::id()
            ));
            let default_root = outer.join("default");
            let candidate = outer.join("candidate");
            let common = outer.join("common");
            let locator = common.join("install-roots/v1/registry.json");
            fs::create_dir_all(&default_root).unwrap();
            fs::create_dir_all(&candidate).unwrap();
            fs::create_dir_all(locator.parent().unwrap()).unwrap();
            fs::write(default_root.join("registry.json"), b"[]").unwrap();
            Self {
                outer,
                default_root,
                candidate,
                common,
                locator,
            }
        }

        fn write_locator(&self, revision: u64, catalog_revision: u64) {
            let root = canonicalize_path(&self.default_root).unwrap();
            let locator = InstallRootLocator {
                schema_version: INSTALL_ROOT_SCHEMA_VERSION,
                registry_revision: revision,
                catalog_revision,
                root_id: DEFAULT_ROOT_ID.to_string(),
                path: root.to_string_lossy().into_owned(),
                manifest_path: canonicalize_path(&root.join("registry.json"))
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                updated_at_ms: 1,
            };
            fs::write(&self.locator, serde_json::to_vec(&locator).unwrap()).unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.outer);
        }
    }

    fn preview(fixture: &Fixture, free: u64) -> InstallRootPreview {
        preview_custom_root(
            &fixture.locator,
            &fixture.default_root,
            Some(&fixture.common),
            fixture.candidate.to_str().unwrap(),
            5,
            Some(free),
        )
        .unwrap()
    }

    #[test]
    fn empty_canonical_candidate_is_previewed_without_mutation() {
        let fixture = Fixture::new();
        fixture.write_locator(4, 5);
        let before = fs::read_dir(&fixture.candidate).unwrap().count();
        let result = preview(&fixture, MIN_FREE_SPACE_BYTES);

        assert_eq!(result.status, InstallRootPreviewStatus::Ready);
        assert_eq!(result.registry_revision, 4);
        assert_eq!(result.active_install_count, 0);
        assert_eq!(before, fs::read_dir(&fixture.candidate).unwrap().count());
        assert!(!fixture.candidate.join("registry.json").exists());
    }

    #[test]
    fn candidate_conflict_and_low_space_are_reported_without_writes() {
        let fixture = Fixture::new();
        fixture.write_locator(4, 5);
        fs::write(fixture.candidate.join("owned-by-user.txt"), b"keep").unwrap();
        assert_eq!(
            preview(&fixture, MIN_FREE_SPACE_BYTES).status,
            InstallRootPreviewStatus::CandidateConflict
        );
        fs::remove_file(fixture.candidate.join("owned-by-user.txt")).unwrap();
        assert_eq!(
            preview(&fixture, MIN_FREE_SPACE_BYTES - 1).status,
            InstallRootPreviewStatus::InsufficientFreeSpace
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_writable_empty_candidate_is_not_applyable() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::new();
        fixture.write_locator(4, 5);
        fs::set_permissions(&fixture.candidate, fs::Permissions::from_mode(0o555)).unwrap();

        let result = preview(&fixture, MIN_FREE_SPACE_BYTES);

        assert_eq!(result.status, InstallRootPreviewStatus::PermissionDenied);
        assert!(!result.status.can_apply());
        fs::set_permissions(&fixture.candidate, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn active_artifacts_block_root_switch_even_when_manifest_is_empty() {
        let fixture = Fixture::new();
        fixture.write_locator(4, 5);
        fs::create_dir_all(fixture.default_root.join("apps/partial-app")).unwrap();
        let before = fs::read_dir(&fixture.default_root).unwrap().count();

        let result = preview(&fixture, MIN_FREE_SPACE_BYTES);

        assert_eq!(result.status, InstallRootPreviewStatus::ExistingInstall);
        assert_eq!(result.active_install_count, 0);
        assert_eq!(fs::read_dir(&fixture.default_root).unwrap().count(), before);
    }

    #[test]
    fn malformed_present_locator_never_falls_back_to_default_root() {
        let fixture = Fixture::new();
        fs::write(&fixture.locator, br#"{"#).unwrap();

        assert_eq!(
            resolve_active_location(&fixture.locator, &fixture.default_root),
            Err(CustomRootError::LocatorInvalid)
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_locator_under_symlinked_parent_does_not_use_legacy_fallback() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::new();
        let linked_common = fixture.outer.join("common-link");
        symlink(&fixture.common, &linked_common).unwrap();
        let locator = linked_common.join("install-roots/v1/registry.json");

        assert_eq!(
            resolve_active_location(&locator, &fixture.default_root),
            Err(CustomRootError::LocatorInvalid)
        );
    }

    #[test]
    fn default_root_id_cannot_point_at_another_canonical_root() {
        let fixture = Fixture::new();
        fs::write(fixture.candidate.join("registry.json"), b"[]").unwrap();
        let locator = InstallRootLocator {
            schema_version: INSTALL_ROOT_SCHEMA_VERSION,
            registry_revision: 4,
            catalog_revision: 5,
            root_id: DEFAULT_ROOT_ID.to_string(),
            path: fixture.candidate.to_string_lossy().into_owned(),
            manifest_path: fixture
                .candidate
                .join("registry.json")
                .to_string_lossy()
                .into_owned(),
            updated_at_ms: 1,
        };
        fs::write(&fixture.locator, serde_json::to_vec(&locator).unwrap()).unwrap();

        assert_eq!(
            resolve_active_location(&fixture.locator, &fixture.default_root),
            Err(CustomRootError::LocatorInvalid)
        );
    }

    #[test]
    fn locator_input_is_bounded_without_reflecting_its_contents() {
        let fixture = Fixture::new();
        fs::write(&fixture.locator, vec![b'x'; MAX_LOCATOR_BYTES as usize + 1]).unwrap();

        let error = resolve_active_location(&fixture.locator, &fixture.default_root).unwrap_err();

        assert_eq!(error, CustomRootError::LocatorInvalid);
        assert!(!error.to_string().contains('x'));
    }

    #[test]
    fn existing_install_blocks_migration_and_preserves_bytes() {
        let fixture = Fixture::new();
        fixture.write_locator(4, 5);
        let executable = fixture
            .default_root
            .join("apps/code-pad/versions/0.3.2/code-pad.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"portable").unwrap();
        let manifest = json!([{
            "app": "code-pad",
            "version": "0.3.2",
            "mode": "portable",
            "exe_path": executable
        }]);
        fs::write(
            fixture.default_root.join("registry.json"),
            manifest.to_string(),
        )
        .unwrap();
        let bytes = fs::read(fixture.default_root.join("registry.json")).unwrap();
        assert_eq!(
            preview(&fixture, MIN_FREE_SPACE_BYTES).status,
            InstallRootPreviewStatus::ExistingInstall
        );
        assert_eq!(
            fs::read(fixture.default_root.join("registry.json")).unwrap(),
            bytes
        );
    }

    #[test]
    fn apply_requires_cas_and_publishes_manifest_before_locator() {
        let fixture = Fixture::new();
        fixture.write_locator(4, 5);
        let result = apply_custom_root(
            &fixture.locator,
            &fixture.default_root,
            Some(&fixture.common),
            fixture.candidate.to_str().unwrap(),
            4,
            5,
            10,
        )
        .unwrap();
        assert_eq!(result.registry_revision, 5);
        assert_eq!(fs::read(&result.manifest).unwrap(), b"[]");
        let locator: InstallRootLocator =
            serde_json::from_slice(&fs::read(&fixture.locator).unwrap()).unwrap();
        assert_eq!(locator.registry_revision, 5);
        assert_eq!(
            Path::new(&locator.path),
            canonicalize_path(&fixture.candidate).unwrap()
        );
        assert_eq!(Path::new(&locator.manifest_path), result.manifest);
        assert_eq!(
            apply_custom_root(
                &fixture.locator,
                &fixture.default_root,
                Some(&fixture.common),
                fixture.candidate.to_str().unwrap(),
                4,
                5,
                11,
            ),
            Err(CustomRootError::RevisionMismatch)
        );
    }

    #[test]
    fn revision_overflow_fails_before_candidate_mutation() {
        let fixture = Fixture::new();
        fixture.write_locator(u64::MAX, 5);

        assert_eq!(
            apply_custom_root(
                &fixture.locator,
                &fixture.default_root,
                Some(&fixture.common),
                fixture.candidate.to_str().unwrap(),
                u64::MAX,
                5,
                10,
            ),
            Err(CustomRootError::RevisionOverflow)
        );
        assert_eq!(fs::read_dir(&fixture.candidate).unwrap().count(), 0);
        assert!(!fixture.candidate.join("registry.json").exists());
    }

    #[test]
    fn rollback_never_removes_a_manifest_changed_after_publish() {
        let fixture = Fixture::new();
        let apps = fixture.candidate.join("apps");
        let manifest = fixture.candidate.join("registry.json");
        fs::create_dir(&apps).unwrap();
        fs::write(&manifest, br#"[{"app":"foreign"}]"#).unwrap();

        assert!(!cleanup_new_root_artifacts(&apps, &manifest));
        assert_eq!(fs::read(&manifest).unwrap(), br#"[{"app":"foreign"}]"#);
    }

    #[test]
    fn candidate_manifest_is_created_exclusively_without_replacing_existing_bytes() {
        let fixture = Fixture::new();
        let manifest = fixture.candidate.join("registry.json");

        create_new_empty_manifest(&manifest).unwrap();
        assert_eq!(fs::read(&manifest).unwrap(), b"[]");
        fs::write(&manifest, b"foreign-owner").unwrap();

        assert_eq!(
            create_new_empty_manifest(&manifest),
            Err((CustomRootError::CandidateConflict, false))
        );
        assert_eq!(fs::read(&manifest).unwrap(), b"foreign-owner");
    }

    #[test]
    fn candidate_is_revalidated_after_owned_artifacts_are_created() {
        let fixture = Fixture::new();
        let apps = fixture.candidate.join("apps");
        let manifest = fixture.candidate.join("registry.json");
        fs::create_dir(&apps).unwrap();
        create_new_empty_manifest(&manifest).unwrap();

        verify_new_root_artifacts(&fixture.candidate, &apps, &manifest).unwrap();
        let foreign = fixture.candidate.join("foreign.txt");
        fs::write(&foreign, b"foreign").unwrap();
        assert_eq!(
            verify_new_root_artifacts(&fixture.candidate, &apps, &manifest),
            Err(CustomRootError::CandidateConflict)
        );
        fs::remove_file(foreign).unwrap();
        fs::write(apps.join("foreign.txt"), b"foreign").unwrap();
        assert_eq!(
            verify_new_root_artifacts(&fixture.candidate, &apps, &manifest),
            Err(CustomRootError::CandidateConflict)
        );
    }

    #[test]
    fn unsafe_input_and_symlink_are_rejected_without_path_reflection() {
        let fixture = Fixture::new();
        fixture.write_locator(4, 5);
        let secret = "custom-root-secret";
        let error = preview_custom_root(
            &fixture.locator,
            &fixture.default_root,
            Some(&fixture.common),
            &format!("../{secret}"),
            5,
            Some(MIN_FREE_SPACE_BYTES),
        )
        .unwrap_err();
        assert_eq!(error, CustomRootError::InvalidPath);
        assert!(!error.to_string().contains(secret));

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&fixture.candidate, fixture.outer.join("candidate-link"))
                .unwrap();
            assert!(matches!(
                preview_custom_root(
                    &fixture.locator,
                    &fixture.default_root,
                    Some(&fixture.common),
                    fixture.outer.join("candidate-link").to_str().unwrap(),
                    5,
                    Some(MIN_FREE_SPACE_BYTES),
                ),
                Err(CustomRootError::UnsafePath | CustomRootError::ProtectedPath)
            ));
        }
    }

    #[test]
    fn manifest_parser_has_bounded_and_strict_contract() {
        let valid = br#"[{"app":"code-pad","version":"0.3.2","mode":"installer","exe_path":""}]"#;
        assert_eq!(parse_install_manifest(valid).unwrap().len(), 1);
        assert_eq!(
            parse_install_manifest(
                br#"[{"app":"../code-pad","version":"0.3.2","mode":"installer","exe_path":""}]"#
            ),
            Err(CustomRootError::ManifestInvalid)
        );
        assert_eq!(
            parse_install_manifest(br#"[{"app":"code-pad","version":"0.3.2","mode":"portable","exe_path":"relative.exe"}]"#),
            Err(CustomRootError::ManifestInvalid)
        );
    }
}
