use devbox_catalog::{capable_targets, select_catalog, Catalog, CatalogError};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

pub const INSTALL_ROOT_SCHEMA_VERSION: u32 = 1;
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
}

enum LocatorState {
    MissingOrInvalid,
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
    let locator: InstallRootLocator =
        serde_json::from_str(input).map_err(|_| InstallLookupError::InvalidLocator)?;
    if locator.schema_version != INSTALL_ROOT_SCHEMA_VERSION
        || locator.registry_revision == 0
        || locator.catalog_revision == 0
        || !valid_root_id(&locator.root_id)
        || locator.updated_at_ms == 0
        || !valid_absolute_literal(&locator.path)
        || !valid_absolute_literal(&locator.manifest_path)
    {
        return Err(InstallLookupError::InvalidLocator);
    }
    Ok(locator)
}

/// Resolve through a valid versioned locator. A missing or malformed locator
/// uses the v0.4.x Manager location as a read-only migration fallback. Once a
/// valid locator exists, an invalid manifest or executable fails closed and
/// never falls back around that registry boundary.
pub fn resolve_installed_from_paths(
    locator_path: Option<&Path>,
    legacy_base: Option<&Path>,
    app_id: &str,
) -> Option<PathBuf> {
    if !valid_app_id(app_id) {
        return None;
    }
    match read_locator_state(locator_path) {
        LocatorState::MissingOrInvalid => {
            legacy_base.and_then(|base| crate::resolve_legacy_from_base(base, app_id))
        }
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
    let runtime = runtime_catalog_path.and_then(|path| std::fs::read_to_string(path).ok());
    let selected = select_catalog(build_catalog, runtime.as_deref()).map_err(map_catalog_error)?;
    let targets = capable_targets(&selected.catalog, capability);
    let locator = read_locator_state(locator_path);
    let manifest = match &locator {
        LocatorState::Valid(locator) => Some(load_manifest(locator)?),
        LocatorState::MissingOrInvalid => None,
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
    let runtime = runtime_catalog_path.and_then(|path| std::fs::read_to_string(path).ok());
    let selected = select_catalog(build_catalog, runtime.as_deref()).map_err(map_catalog_error)?;
    let LocatorState::Valid(locator) = read_locator_state(Some(locator_path)) else {
        return Err(InstallLookupError::InvalidLocator);
    };
    let manifest = load_manifest(&locator)?;
    validate_manifest_apps(&manifest, &selected.catalog)
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
        return LocatorState::MissingOrInvalid;
    };
    let Ok(input) = std::fs::read_to_string(path) else {
        return LocatorState::MissingOrInvalid;
    };
    match parse_install_root_locator(&input) {
        Ok(locator) => LocatorState::Valid(locator),
        Err(_) => LocatorState::MissingOrInvalid,
    }
}

fn load_manifest(locator: &InstallRootLocator) -> Result<InstalledManifest, InstallLookupError> {
    let raw_root = PathBuf::from(&locator.path);
    let raw_manifest = PathBuf::from(&locator.manifest_path);
    let root = canonical_non_symlink(&raw_root).map_err(|_| InstallLookupError::UnsafeRoot)?;
    if root != raw_root || dangerous_canonical_root(&root) {
        return Err(InstallLookupError::UnsafeRoot);
    }
    let manifest =
        canonical_non_symlink(&raw_manifest).map_err(|_| InstallLookupError::UnsafeManifest)?;
    if manifest != raw_manifest || !manifest.starts_with(&root) || !manifest.is_file() {
        return Err(InstallLookupError::UnsafeManifest);
    }

    let input =
        std::fs::read_to_string(&manifest).map_err(|_| InstallLookupError::InvalidManifest)?;
    let rows: Vec<InstalledManifestEntry> =
        serde_json::from_str(&input).map_err(|_| InstallLookupError::InvalidManifest)?;
    let mut app_ids = HashSet::new();
    let mut installed = HashMap::new();
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
            continue;
        }
        let raw_executable = PathBuf::from(&row.exe_path);
        if !valid_absolute_literal(&row.exe_path) {
            return Err(InstallLookupError::UnsafeExecutable);
        }
        let executable = canonical_non_symlink(&raw_executable)
            .map_err(|_| InstallLookupError::UnsafeExecutable)?;
        let expected = root
            .join("apps")
            .join(&row.app)
            .join("versions")
            .join(&row.version)
            .join(format!("{}.exe", row.app));
        let expected = expected
            .canonicalize()
            .map_err(|_| InstallLookupError::UnsafeExecutable)?;
        if executable != expected || !executable.starts_with(&root) || !executable.is_file() {
            return Err(InstallLookupError::UnsafeExecutable);
        }
        installed.insert(row.app, executable);
    }
    Ok(InstalledManifest {
        app_ids,
        executables: installed,
    })
}

fn canonical_non_symlink(path: &Path) -> Result<PathBuf, ()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if metadata.file_type().is_symlink() {
        return Err(());
    }
    path.canonicalize().map_err(|_| ())
}

fn dangerous_canonical_root(root: &Path) -> bool {
    if root.parent().is_none() {
        return true;
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        if Path::new(&home)
            .canonicalize()
            .is_ok_and(|canonical_home| root == canonical_home)
        {
            return true;
        }
    }
    if std::env::current_dir()
        .and_then(|cwd| cwd.canonicalize())
        .is_ok_and(|cwd| root == cwd)
    {
        return true;
    }
    false
}

fn valid_absolute_literal(value: &str) -> bool {
    !value.is_empty()
        && !value.contains(['%', '$', '\0'])
        && !value.starts_with('~')
        && Path::new(value).is_absolute()
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
                root: root.canonicalize().unwrap(),
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
            executable.canonicalize().unwrap()
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
                manifest_path: self
                    .manifest
                    .canonicalize()
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
    fn missing_or_corrupt_locator_uses_read_only_legacy_fallback() {
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
            Some(executable)
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
        let dotted = executable
            .parent()
            .unwrap()
            .join("ignored")
            .join("..")
            .join("code-pad.exe");
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
}
