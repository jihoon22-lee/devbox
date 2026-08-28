use crate::canonicalize_path;
use devbox_catalog::{capable_targets, select_catalog, Catalog, CatalogError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const INSTALL_ROOT_SCHEMA_VERSION: u32 = 1;
pub const MAX_INSTALL_ROOT_PATH_BYTES: usize = 4_096;
pub const MAX_INSTALL_ROOT_LOCATOR_BYTES: u64 = 16 * 1024;
pub const MAX_INSTALL_MANIFEST_BYTES: u64 = 1_048_576;
pub const MAX_INSTALL_MANIFEST_ENTRIES: usize = 256;
const MAX_RUNTIME_CATALOG_BYTES: u64 = 1_048_576;
const BUILD_CATALOG: &str = include_str!("../../../apps/catalog.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallRootLocator {
    pub schema_version: u32,
    pub registry_revision: u64,
    pub catalog_revision: u64,
    pub root_id: String,
    pub path: String,
    pub manifest_path: String,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledTarget {
    pub id: String,
    pub display_name: String,
    pub executable: PathBuf,
}

/// Canonical, read-only evidence for an installed app. Portable entries have
/// an executable and an install root because the Manager manifest owns both.
/// Installer entries deliberately leave both paths absent: spawning an
/// installer does not prove where its wizard ultimately placed the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPathDetails {
    pub app_id: String,
    pub mode: String,
    pub executable: Option<PathBuf>,
    pub install_root: Option<PathBuf>,
    pub source_manifest: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallLookupError {
    InvalidLocator,
    UnsafeRoot,
    UnsafeManifest,
    InvalidManifest,
    UnsafeExecutable,
    InvalidBuildCatalog,
}

impl fmt::Display for InstallLookupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLocator => "install-root locator is invalid",
            Self::UnsafeRoot => "install root is unsafe",
            Self::UnsafeManifest => "install manifest path is unsafe",
            Self::InvalidManifest => "install manifest is invalid",
            Self::UnsafeExecutable => "installed executable path is unsafe",
            Self::InvalidBuildCatalog => "build-time catalog is invalid",
        })
    }
}

impl std::error::Error for InstallLookupError {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledManifestEntry {
    app: String,
    version: String,
    mode: String,
    exe_path: String,
}

struct InstalledManifest {
    app_ids: HashSet<String>,
    executables: HashMap<String, PathBuf>,
    modes: HashMap<String, String>,
    root: PathBuf,
    source_manifest: PathBuf,
}

enum LocatorState {
    Missing,
    Invalid,
    Valid(InstallRootLocator),
}

pub fn runtime_catalog_path() -> Option<PathBuf> {
    common_root().map(|root| root.join("catalog.json"))
}

pub fn install_root_registry_path() -> Option<PathBuf> {
    common_root().map(|root| root.join("install-roots/v1/registry.json"))
}

fn common_root() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("devbox"))
}

pub fn parse_install_root_locator(input: &str) -> Result<InstallRootLocator, InstallLookupError> {
    if input.len() > MAX_INSTALL_ROOT_LOCATOR_BYTES as usize {
        return Err(InstallLookupError::InvalidLocator);
    }
    let locator: InstallRootLocator =
        serde_json::from_str(input).map_err(|_| InstallLookupError::InvalidLocator)?;
    if locator.schema_version != INSTALL_ROOT_SCHEMA_VERSION
        || locator.registry_revision == 0
        || locator.catalog_revision == 0
        || !valid_root_id(&locator.root_id)
        || locator.updated_at_ms == 0
        || locator.path.len() > MAX_INSTALL_ROOT_PATH_BYTES
        || locator.manifest_path.len() > MAX_INSTALL_ROOT_PATH_BYTES
        || !valid_absolute_literal(&locator.path)
        || !valid_absolute_literal(&locator.manifest_path)
    {
        return Err(InstallLookupError::InvalidLocator);
    }
    Ok(locator)
}

/// Resolve through a valid versioned locator. Only an absent locator uses the
/// v0.4.x Manager location as a read-only migration fallback. A present but
/// malformed locator, or an invalid manifest/executable behind a valid one,
/// fails closed and never falls back around that registry boundary.
pub fn resolve_installed_from_paths(
    locator_path: Option<&Path>,
    legacy_base: Option<&Path>,
    app_id: &str,
) -> Option<PathBuf> {
    if !valid_app_id(app_id) {
        return None;
    }
    match read_locator_state(locator_path) {
        LocatorState::Missing => {
            legacy_base.and_then(|base| crate::resolve_legacy_from_base(base, app_id))
        }
        LocatorState::Invalid => None,
        LocatorState::Valid(locator) => load_manifest(&locator)
            .ok()
            .and_then(|manifest| manifest.executables.get(app_id).cloned()),
    }
}

pub fn installed_targets(capability: &str) -> Vec<InstalledTarget> {
    let runtime_path = runtime_catalog_path();
    let locator_path = install_root_registry_path();
    let legacy_base = crate::manager_base();
    installed_targets_from_paths(
        BUILD_CATALOG,
        runtime_path.as_deref(),
        locator_path.as_deref(),
        legacy_base.as_deref(),
        capability,
    )
    .unwrap_or_default()
}

pub fn installed_targets_from_paths(
    build_catalog: &str,
    runtime_catalog_path: Option<&Path>,
    locator_path: Option<&Path>,
    legacy_base: Option<&Path>,
    capability: &str,
) -> Result<Vec<InstalledTarget>, InstallLookupError> {
    let runtime = runtime_catalog_path
        .and_then(|path| read_bounded_text(path, MAX_RUNTIME_CATALOG_BYTES).ok());
    let selected = select_catalog(build_catalog, runtime.as_deref()).map_err(map_catalog_error)?;
    let targets = capable_targets(&selected.catalog, capability);
    let locator = read_locator_state(locator_path);
    let manifest = match &locator {
        LocatorState::Valid(locator) => Some(load_manifest(locator)?),
        LocatorState::Missing => None,
        LocatorState::Invalid => return Err(InstallLookupError::InvalidLocator),
    };

    Ok(targets
        .into_iter()
        .filter_map(|app| {
            let executable = match &manifest {
                Some(manifest) => manifest.executables.get(&app.id).cloned(),
                None => legacy_base.and_then(|base| crate::resolve_legacy_from_base(base, &app.id)),
            }?;
            Some(InstalledTarget {
                id: app.id,
                display_name: app.display_name,
                executable,
            })
        })
        .collect())
}

/// Validate the versioned locator, its app-owned manifest, and catalog app IDs
/// without launching a process. This is intended for read-only doctor checks.
pub fn validate_installation_metadata_from_paths(
    build_catalog: &str,
    runtime_catalog_path: Option<&Path>,
    locator_path: &Path,
) -> Result<(), InstallLookupError> {
    let runtime = runtime_catalog_path
        .and_then(|path| read_bounded_text(path, MAX_RUNTIME_CATALOG_BYTES).ok());
    let selected = select_catalog(build_catalog, runtime.as_deref()).map_err(map_catalog_error)?;
    let LocatorState::Valid(locator) = read_locator_state(Some(locator_path)) else {
        return Err(InstallLookupError::InvalidLocator);
    };
    let manifest = load_manifest(&locator)?;
    validate_manifest_apps(&manifest, &selected.catalog)
}

/// Return display-only installation evidence after validating the selected
/// catalog, versioned locator, source manifest, and every portable executable
/// in that manifest. The function reads filesystem state only.
pub fn installed_path_details_from_paths(
    build_catalog: &str,
    runtime_catalog_path: Option<&Path>,
    locator_path: &Path,
    expected_source_manifest: &Path,
    app_id: &str,
) -> Result<Option<InstalledPathDetails>, InstallLookupError> {
    if !valid_app_id(app_id) {
        return Err(InstallLookupError::InvalidManifest);
    }
    let locator_metadata =
        std::fs::symlink_metadata(locator_path).map_err(|_| InstallLookupError::InvalidLocator)?;
    if path_is_link_or_reparse(&locator_metadata) || !locator_metadata.is_file() {
        return Err(InstallLookupError::InvalidLocator);
    }
    let runtime = runtime_catalog_path
        .and_then(|path| read_bounded_text(path, MAX_RUNTIME_CATALOG_BYTES).ok());
    let selected = select_catalog(build_catalog, runtime.as_deref()).map_err(map_catalog_error)?;
    if !selected
        .catalog
        .apps
        .iter()
        .any(|app| app.id == app_id && app.manager_visible && !app.self_managed)
    {
        return Ok(None);
    }
    let LocatorState::Valid(locator) = read_locator_state(Some(locator_path)) else {
        return Err(InstallLookupError::InvalidLocator);
    };
    if selected.catalog.catalog_revision != Some(locator.catalog_revision) {
        return Err(InstallLookupError::InvalidLocator);
    }
    let manifest = load_manifest(&locator)?;
    let expected_source_manifest = canonical_non_symlink(expected_source_manifest)
        .map_err(|_| InstallLookupError::UnsafeManifest)?;
    if manifest.source_manifest != expected_source_manifest {
        return Err(InstallLookupError::UnsafeManifest);
    }
    validate_manifest_apps(&manifest, &selected.catalog)?;
    let Some(mode) = manifest.modes.get(app_id) else {
        return Ok(None);
    };
    let portable = mode == "portable";
    let executable = portable
        .then(|| manifest.executables.get(app_id).cloned())
        .flatten();
    if portable && executable.is_none() {
        return Err(InstallLookupError::UnsafeExecutable);
    }
    Ok(Some(InstalledPathDetails {
        app_id: app_id.to_string(),
        mode: mode.clone(),
        executable,
        install_root: portable.then(|| manifest.root.clone()),
        source_manifest: manifest.source_manifest.clone(),
    }))
}

fn validate_manifest_apps(
    manifest: &InstalledManifest,
    catalog: &Catalog,
) -> Result<(), InstallLookupError> {
    let known = catalog
        .apps
        .iter()
        .map(|app| app.id.as_str())
        .collect::<HashSet<_>>();
    manifest
        .app_ids
        .iter()
        .all(|app_id| known.contains(app_id.as_str()))
        .then_some(())
        .ok_or(InstallLookupError::InvalidManifest)
}

fn map_catalog_error(_error: CatalogError) -> InstallLookupError {
    InstallLookupError::InvalidBuildCatalog
}

fn read_locator_state(path: Option<&Path>) -> LocatorState {
    let Some(path) = path else {
        return LocatorState::Missing;
    };
    if let Some(parent) = path.parent() {
        // Keep the v0.4.x missing-locator fallback, but never treat a
        // locator hidden behind an existing symlink/reparse parent as absent.
        if ensure_plain_existing_components(parent).is_err() {
            return LocatorState::Invalid;
        }
    }
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => LocatorState::Missing,
        Err(_) => LocatorState::Invalid,
        Ok(metadata) if path_is_link_or_reparse(&metadata) || !metadata.is_file() => {
            LocatorState::Invalid
        }
        Ok(_) => {
            if ensure_plain_components(path).is_err() {
                return LocatorState::Invalid;
            }
            let Ok(input) = read_bounded_text(path, MAX_INSTALL_ROOT_LOCATOR_BYTES) else {
                return LocatorState::Invalid;
            };
            match parse_install_root_locator(&input) {
                Ok(locator) => LocatorState::Valid(locator),
                Err(_) => LocatorState::Invalid,
            }
        }
    }
}

fn load_manifest(locator: &InstallRootLocator) -> Result<InstalledManifest, InstallLookupError> {
    let raw_root = PathBuf::from(&locator.path);
    let raw_manifest = PathBuf::from(&locator.manifest_path);
    let root = canonical_non_symlink(&raw_root).map_err(|_| InstallLookupError::UnsafeRoot)?;
    if !same_path_identity(&root, &raw_root) || dangerous_canonical_root(&root) {
        return Err(InstallLookupError::UnsafeRoot);
    }
    let manifest =
        canonical_non_symlink(&raw_manifest).map_err(|_| InstallLookupError::UnsafeManifest)?;
    if !same_path_identity(&manifest, &raw_manifest)
        || !path_within(&root, &manifest)
        || !manifest.is_file()
    {
        return Err(InstallLookupError::UnsafeManifest);
    }

    let input = read_bounded_text(&manifest, MAX_INSTALL_MANIFEST_BYTES)
        .map_err(|_| InstallLookupError::InvalidManifest)?;
    let rows: Vec<InstalledManifestEntry> =
        serde_json::from_str(&input).map_err(|_| InstallLookupError::InvalidManifest)?;
    if rows.len() > MAX_INSTALL_MANIFEST_ENTRIES {
        return Err(InstallLookupError::InvalidManifest);
    }
    let mut app_ids = HashSet::new();
    let mut installed = HashMap::new();
    let mut modes = HashMap::new();
    for row in rows {
        if !valid_app_id(&row.app)
            || !valid_version(&row.version)
            || !matches!(row.mode.as_str(), "portable" | "installer")
            || !app_ids.insert(row.app.clone())
        {
            return Err(InstallLookupError::InvalidManifest);
        }
        if row.mode == "installer" {
            if !row.exe_path.is_empty() {
                return Err(InstallLookupError::InvalidManifest);
            }
            modes.insert(row.app, row.mode);
            continue;
        }
        let raw_executable = PathBuf::from(&row.exe_path);
        if !valid_absolute_literal(&row.exe_path) {
            return Err(InstallLookupError::UnsafeExecutable);
        }
        ensure_plain_components(&raw_executable)
            .map_err(|_| InstallLookupError::UnsafeExecutable)?;
        let executable = canonical_non_symlink(&raw_executable)
            .map_err(|_| InstallLookupError::UnsafeExecutable)?;
        let expected = root
            .join("apps")
            .join(&row.app)
            .join("versions")
            .join(&row.version)
            .join(format!("{}.exe", row.app));
        let expected =
            canonicalize_path(&expected).map_err(|_| InstallLookupError::UnsafeExecutable)?;
        if !same_path_identity(&executable, &expected)
            || !path_within(&root, &executable)
            || !executable.is_file()
        {
            return Err(InstallLookupError::UnsafeExecutable);
        }
        modes.insert(row.app.clone(), row.mode);
        installed.insert(row.app, executable);
    }
    Ok(InstalledManifest {
        app_ids,
        executables: installed,
        modes,
        root,
        source_manifest: manifest,
    })
}

#[cfg(windows)]
fn path_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn path_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn canonical_non_symlink(path: &Path) -> Result<PathBuf, ()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if path_is_link_or_reparse(&metadata) {
        return Err(());
    }
    canonicalize_path(path).map_err(|_| ())
}

/// Reject a symlink/reparse point in every component, not only the final
/// executable entry. A link that resolves to another directory inside the
/// managed root must not be accepted as an alternate install layout.
fn ensure_plain_components(path: &Path) -> Result<(), ()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            std::path::Component::Normal(value) => current.push(value),
            std::path::Component::CurDir | std::path::Component::ParentDir => return Err(()),
        }
        // On Windows a disk prefix (`C:`) is not an absolute path until the
        // following root component has been appended. Do not probe the
        // drive-relative spelling; start metadata checks at the absolute root.
        if !current.is_absolute() {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| ())?;
        if path_is_link_or_reparse(&metadata) {
            return Err(());
        }
    }
    Ok(())
}

/// Check existing components up to (but not including) a potentially missing
/// locator. A legacy installation may not have created `install-roots/v1`;
/// missing components therefore stop the check, while an existing link or
/// reparse point is invalid even when the final locator is absent.
fn ensure_plain_existing_components(path: &Path) -> Result<(), ()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            std::path::Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            std::path::Component::Normal(value) => current.push(value),
            std::path::Component::CurDir | std::path::Component::ParentDir => return Err(()),
        }
        if !current.is_absolute() {
            continue;
        }
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if path_is_link_or_reparse(&metadata) => return Err(()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(()),
        }
    }
    Ok(())
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

fn read_bounded_text(path: &Path, max_bytes: u64) -> Result<String, ()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if path_is_link_or_reparse(&metadata) || !metadata.is_file() || metadata.len() > max_bytes {
        return Err(());
    }
    let file = File::open(path).map_err(|_| ())?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > max_bytes {
        return Err(());
    }
    String::from_utf8(bytes).map_err(|_| ())
}

fn dangerous_canonical_root(root: &Path) -> bool {
    if is_filesystem_root(root) {
        return true;
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        if canonicalize_path(Path::new(&home))
            .is_ok_and(|canonical_home| same_path_identity(root, &canonical_home))
        {
            return true;
        }
    }
    if std::env::current_dir()
        .and_then(|cwd| canonicalize_path(&cwd))
        .is_ok_and(|cwd| same_path_identity(root, &cwd))
    {
        return true;
    }
    false
}

fn is_filesystem_root(path: &Path) -> bool {
    let mut saw_root = false;
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) => {}
            std::path::Component::RootDir => saw_root = true,
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::Normal(_) => return false,
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
        && !Path::new(value).components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
}

fn valid_root_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 96
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .copied()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_app_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestLayout {
        outer: PathBuf,
        root: PathBuf,
        manifest: PathBuf,
        locator: PathBuf,
    }

    impl TestLayout {
        fn new() -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let outer = std::env::temp_dir()
                .join(format!("launch-locator-test-{}-{id}", std::process::id()));
            let root = outer.join("manager-root");
            let manifest = root.join("registry.json");
            let locator = outer.join("common/install-roots/v1/registry.json");
            fs::create_dir_all(locator.parent().unwrap()).unwrap();
            fs::create_dir_all(&root).unwrap();
            Self {
                outer,
                root: canonicalize_path(&root).unwrap(),
                manifest,
                locator,
            }
        }

        fn install(&self, app_id: &str, version: &str) -> PathBuf {
            let executable = self
                .root
                .join("apps")
                .join(app_id)
                .join("versions")
                .join(version)
                .join(format!("{app_id}.exe"));
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(&executable, b"fixture executable").unwrap();
            canonicalize_path(&executable).unwrap()
        }

        fn write_manifest(&self, rows: serde_json::Value) {
            fs::write(&self.manifest, rows.to_string()).unwrap();
        }

        fn write_locator(&self, registry_revision: u64, catalog_revision: u64) {
            let locator = InstallRootLocator {
                schema_version: INSTALL_ROOT_SCHEMA_VERSION,
                registry_revision,
                catalog_revision,
                root_id: "devbox-manager-default".into(),
                path: self.root.to_string_lossy().into_owned(),
                manifest_path: canonicalize_path(&self.manifest)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                updated_at_ms: 1,
            };
            fs::write(&self.locator, serde_json::to_vec(&locator).unwrap()).unwrap();
        }
    }

    impl Drop for TestLayout {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.outer);
        }
    }

    fn app(id: &str, accepts: serde_json::Value) -> serde_json::Value {
        json!({
            "id": id,
            "displayName": id,
            "productName": id,
            "identifier": format!("com.devbox.{}", id.replace('-', "")),
            "cargoPackage": id,
            "appDir": format!("apps/{id}"),
            "release": true,
            "managerVisible": true,
            "selfManaged": false,
            "accepts": accepts,
            "produces": [],
            "actions": []
        })
    }

    fn catalog(revision: u64, apps: Vec<serde_json::Value>) -> String {
        json!({
            "schemaVersion": 2,
            "catalogRevision": revision,
            "apps": apps,
        })
        .to_string()
    }

    #[test]
    fn valid_locator_and_manifest_resolve_only_installed_capable_targets() {
        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.5.0","mode":"portable","exe_path":executable},
            {"app":"wsl-desktop","version":"0.5.0","mode":"installer","exe_path":""}
        ]));
        layout.write_locator(3, 5);
        let build = catalog(
            5,
            vec![
                app("code-pad", json!(["path", "workspace"])),
                app("wsl-desktop", json!(["path"])),
            ],
        );

        assert_eq!(
            resolve_installed_from_paths(Some(&layout.locator), None, "code-pad"),
            Some(executable.clone())
        );
        let targets =
            installed_targets_from_paths(&build, None, Some(&layout.locator), None, "path")
                .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].id, "code-pad");
        assert_eq!(targets[0].executable, executable);
        assert!(installed_targets_from_paths(
            &build,
            None,
            Some(&layout.locator),
            None,
            "handoff:missing/v1"
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn install_path_details_are_canonical_and_read_only() {
        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.5.0","mode":"portable","exe_path":executable},
            {"app":"wsl-desktop","version":"0.5.0","mode":"installer","exe_path":""}
        ]));
        layout.write_locator(3, 5);
        let build = catalog(
            5,
            vec![
                app("code-pad", json!(["path"])),
                app("wsl-desktop", json!(["path"])),
            ],
        );
        let manifest_before = fs::read(&layout.manifest).unwrap();
        let locator_before = fs::read(&layout.locator).unwrap();
        let executable_before = fs::read(&executable).unwrap();

        let portable = installed_path_details_from_paths(
            &build,
            None,
            &layout.locator,
            &layout.manifest,
            "code-pad",
        )
        .unwrap()
        .unwrap();
        assert_eq!(portable.app_id, "code-pad");
        assert_eq!(portable.mode, "portable");
        assert_eq!(portable.executable, Some(executable.clone()));
        assert_eq!(portable.install_root, Some(layout.root.clone()));
        assert_eq!(
            portable.source_manifest,
            canonicalize_path(&layout.manifest).unwrap()
        );

        let installer = installed_path_details_from_paths(
            &build,
            None,
            &layout.locator,
            &layout.manifest,
            "wsl-desktop",
        )
        .unwrap()
        .unwrap();
        assert_eq!(installer.mode, "installer");
        assert_eq!(installer.executable, None);
        assert_eq!(installer.install_root, None);
        assert_eq!(
            installer.source_manifest,
            canonicalize_path(&layout.manifest).unwrap()
        );

        assert_eq!(fs::read(&layout.manifest).unwrap(), manifest_before);
        assert_eq!(fs::read(&layout.locator).unwrap(), locator_before);
        assert_eq!(fs::read(&executable).unwrap(), executable_before);
    }

    #[test]
    fn install_path_details_fail_closed_on_catalog_revision_mismatch() {
        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.5.0","mode":"portable","exe_path":executable}
        ]));
        layout.write_locator(1, 4);
        let build = catalog(5, vec![app("code-pad", json!(["path"]))]);

        assert_eq!(
            installed_path_details_from_paths(
                &build,
                None,
                &layout.locator,
                &layout.manifest,
                "code-pad"
            ),
            Err(InstallLookupError::InvalidLocator)
        );
    }

    #[test]
    fn install_path_details_reject_a_different_active_manifest() {
        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.5.0","mode":"portable","exe_path":executable}
        ]));
        layout.write_locator(1, 5);
        let other_manifest = layout.outer.join("other-registry.json");
        fs::write(&other_manifest, b"[]").unwrap();
        let build = catalog(5, vec![app("code-pad", json!(["path"]))]);

        let error = installed_path_details_from_paths(
            &build,
            None,
            &layout.locator,
            &other_manifest,
            "code-pad",
        )
        .unwrap_err();
        assert_eq!(error, InstallLookupError::UnsafeManifest);
        assert!(!error
            .to_string()
            .contains(&other_manifest.to_string_lossy()[..]));
    }

    #[cfg(unix)]
    #[test]
    fn install_path_details_reject_a_symlinked_locator_without_path_reflection() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.5.0","mode":"portable","exe_path":executable}
        ]));
        layout.write_locator(1, 5);
        let actual_locator = layout.locator.with_file_name("actual-locator.json");
        fs::rename(&layout.locator, &actual_locator).unwrap();
        symlink(&actual_locator, &layout.locator).unwrap();
        let build = catalog(5, vec![app("code-pad", json!(["path"]))]);

        let error = installed_path_details_from_paths(
            &build,
            None,
            &layout.locator,
            &layout.manifest,
            "code-pad",
        )
        .unwrap_err();
        assert_eq!(error, InstallLookupError::InvalidLocator);
        assert!(!error
            .to_string()
            .contains(&layout.outer.to_string_lossy()[..]));
    }

    #[test]
    fn missing_locator_uses_read_only_legacy_fallback_but_corrupt_does_not() {
        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        fs::write(
            layout.root.join("apps/code-pad/current.json"),
            json!({"version":"0.5.0","exePath":executable}).to_string(),
        )
        .unwrap();

        assert_eq!(
            resolve_installed_from_paths(None, Some(&layout.root), "code-pad"),
            Some(executable.clone())
        );
        fs::write(&layout.locator, b"{broken").unwrap();
        assert_eq!(
            resolve_installed_from_paths(Some(&layout.locator), Some(&layout.root), "code-pad"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_locator_under_symlinked_parent_does_not_use_legacy_fallback() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new();
        let linked_common = layout.outer.join("linked-common");
        symlink(
            layout
                .locator
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap(),
            &linked_common,
        )
        .unwrap();
        let locator = linked_common.join("install-roots/v1/registry.json");

        assert_eq!(
            resolve_installed_from_paths(Some(&locator), Some(&layout.root), "code-pad"),
            None
        );
    }

    #[test]
    fn valid_locator_with_corrupt_manifest_fails_closed_without_legacy_bypass() {
        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        fs::write(
            layout.root.join("apps/code-pad/current.json"),
            json!({"version":"0.5.0","exePath":executable}).to_string(),
        )
        .unwrap();
        layout.write_manifest(json!({"not":"an installed-app array"}));
        layout.write_locator(1, 1);

        assert_eq!(
            resolve_installed_from_paths(Some(&layout.locator), Some(&layout.root), "code-pad"),
            None
        );
    }

    #[test]
    fn stale_or_corrupt_runtime_catalog_falls_back_but_newer_runtime_wins() {
        let layout = TestLayout::new();
        let code_pad = layout.install("code-pad", "0.5.0");
        let fake = layout.install("fake-sixteenth", "0.5.0");
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.5.0","mode":"portable","exe_path":code_pad},
            {"app":"fake-sixteenth","version":"0.5.0","mode":"portable","exe_path":fake}
        ]));
        layout.write_locator(2, 6);
        let build = catalog(5, vec![app("code-pad", json!(["path"]))]);
        let runtime_path = layout.outer.join("runtime-catalog.json");

        fs::write(
            &runtime_path,
            catalog(4, vec![app("fake-sixteenth", json!(["path"]))]),
        )
        .unwrap();
        let stale = installed_targets_from_paths(
            &build,
            Some(&runtime_path),
            Some(&layout.locator),
            None,
            "path",
        )
        .unwrap();
        assert_eq!(stale[0].id, "code-pad");

        fs::write(&runtime_path, b"{corrupt").unwrap();
        let corrupt = installed_targets_from_paths(
            &build,
            Some(&runtime_path),
            Some(&layout.locator),
            None,
            "path",
        )
        .unwrap();
        assert_eq!(corrupt[0].id, "code-pad");

        fs::write(
            &runtime_path,
            catalog(6, vec![app("fake-sixteenth", json!(["path"]))]),
        )
        .unwrap();
        let newer = installed_targets_from_paths(
            &build,
            Some(&runtime_path),
            Some(&layout.locator),
            None,
            "path",
        )
        .unwrap();
        assert_eq!(newer[0].id, "fake-sixteenth");

        fs::write(
            &runtime_path,
            vec![b'x'; (MAX_RUNTIME_CATALOG_BYTES as usize).saturating_add(1)],
        )
        .unwrap();
        let oversized = installed_targets_from_paths(
            &build,
            Some(&runtime_path),
            Some(&layout.locator),
            None,
            "path",
        )
        .unwrap();
        assert_eq!(oversized[0].id, "code-pad");
    }

    #[test]
    fn version_or_path_mismatch_and_traversal_app_ids_are_hidden() {
        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.4.9","mode":"portable","exe_path":executable}
        ]));
        layout.write_locator(1, 1);

        assert_eq!(
            resolve_installed_from_paths(Some(&layout.locator), None, "code-pad"),
            None
        );
        assert_eq!(
            resolve_installed_from_paths(Some(&layout.locator), None, "../code-pad"),
            None
        );
    }

    #[test]
    fn executable_dot_segments_are_rejected_even_when_they_canonicalize_inside_root() {
        let layout = TestLayout::new();
        let executable = layout.install("code-pad", "0.5.0");
        let separator = std::path::MAIN_SEPARATOR;
        let dotted = format!(
            "{}{separator}ignored{separator}..{separator}code-pad.exe",
            executable.parent().unwrap().display()
        );
        fs::create_dir_all(executable.parent().unwrap().join("ignored")).unwrap();
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.5.0","mode":"portable","exe_path":dotted}
        ]));
        layout.write_locator(1, 1);

        assert_eq!(
            resolve_installed_from_paths(Some(&layout.locator), None, "code-pad"),
            None
        );
    }

    #[test]
    fn installed_target_validation_rejects_manifest_apps_absent_from_catalog() {
        let layout = TestLayout::new();
        let executable = layout.install("unknown-app", "0.5.0");
        layout.write_manifest(json!([
            {"app":"unknown-app","version":"0.5.0","mode":"portable","exe_path":executable}
        ]));
        layout.write_locator(1, 5);
        let build = catalog(5, vec![app("code-pad", json!(["path"]))]);

        assert_eq!(
            validate_installation_metadata_from_paths(&build, None, &layout.locator),
            Err(InstallLookupError::InvalidManifest)
        );
        assert!(
            installed_targets_from_paths(&build, None, Some(&layout.locator), None, "path")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn locator_contract_rejects_unknown_fields_zero_revisions_and_untrusted_values_safely() {
        let secret = "locator-secret-must-not-appear";
        let invalid = json!({
            "schemaVersion": 1,
            "registryRevision": 0,
            "catalogRevision": 1,
            "rootId": secret,
            "path": "/tmp/root",
            "manifestPath": "/tmp/root/registry.json",
            "updatedAtMs": 1,
            "extra": true
        })
        .to_string();
        let error = parse_install_root_locator(&invalid)
            .unwrap_err()
            .to_string();

        assert_eq!(error, "install-root locator is invalid");
        assert!(!error.contains(secret));

        let oversized = "x".repeat(MAX_INSTALL_ROOT_LOCATOR_BYTES as usize + 1);
        assert_eq!(
            parse_install_root_locator(&oversized),
            Err(InstallLookupError::InvalidLocator)
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_executable_cannot_escape_the_manifest_root() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new();
        let outside = layout.outer.join("outside.exe");
        fs::write(&outside, b"outside").unwrap();
        let executable = layout
            .root
            .join("apps/code-pad/versions/0.5.0/code-pad.exe");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        symlink(&outside, &executable).unwrap();
        layout.write_manifest(json!([
            {"app":"code-pad","version":"0.5.0","mode":"portable","exe_path":executable}
        ]));
        layout.write_locator(1, 1);

        assert_eq!(
            resolve_installed_from_paths(Some(&layout.locator), None, "code-pad"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_layout_component_is_rejected_even_when_it_resolves_inside_root() {
        use std::os::unix::fs::symlink;

        let layout = TestLayout::new();
        let real_app_root = layout.root.join("apps/real-code-pad");
        let executable = real_app_root
            .join("versions/0.5.0/code-pad.exe")
            .canonicalize()
            .unwrap_or_else(|_| real_app_root.join("versions/0.5.0/code-pad.exe"));
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"inside root").unwrap();
        symlink(&real_app_root, layout.root.join("apps/code-pad")).unwrap();
        layout.write_manifest(json!([{
            "app": "code-pad",
            "version": "0.5.0",
            "mode": "portable",
            "exe_path": layout.root.join("apps/code-pad/versions/0.5.0/code-pad.exe")
        }]));
        layout.write_locator(1, 1);

        assert_eq!(
            resolve_installed_from_paths(Some(&layout.locator), None, "code-pad"),
            None
        );
    }
}
