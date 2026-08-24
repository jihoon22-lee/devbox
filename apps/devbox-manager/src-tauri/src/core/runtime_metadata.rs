use devbox_catalog::parse_catalog;
use devbox_launch::{parse_install_root_locator, InstallRootLocator, INSTALL_ROOT_SCHEMA_VERSION};
use std::fmt;
use std::path::{Path, PathBuf};

pub const DEFAULT_ROOT_ID: &str = "devbox-manager-default";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDisposition {
    Written,
    Current,
    PreservedNewer,
    PreservedOtherRoot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeMetadataSync {
    pub catalog: SyncDisposition,
    pub locator: SyncDisposition,
    pub catalog_revision: u64,
    pub registry_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMetadataError {
    InvalidBuildCatalog,
    MissingBuildRevision,
    UnsafeManagerRoot,
    RevisionOverflow,
    Serialization,
    Storage,
}

impl fmt::Display for RuntimeMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidBuildCatalog => "build-time catalog is invalid",
            Self::MissingBuildRevision => "build-time catalog revision is missing",
            Self::UnsafeManagerRoot => "manager install root is unsafe",
            Self::RevisionOverflow => "install-root revision cannot advance",
            Self::Serialization => "runtime metadata serialization failed",
            Self::Storage => "runtime metadata storage failed",
        })
    }
}

impl std::error::Error for RuntimeMetadataError {}

pub fn sync_runtime_metadata(
    manager_root: &Path,
    common_root: &Path,
    build_catalog: &str,
    updated_at_ms: u64,
) -> Result<RuntimeMetadataSync, RuntimeMetadataError> {
    let build =
        parse_catalog(build_catalog).map_err(|_| RuntimeMetadataError::InvalidBuildCatalog)?;
    let catalog_revision = build
        .catalog_revision
        .ok_or(RuntimeMetadataError::MissingBuildRevision)?;
    if updated_at_ms == 0 {
        return Err(RuntimeMetadataError::Serialization);
    }

    std::fs::create_dir_all(manager_root).map_err(|_| RuntimeMetadataError::Storage)?;
    std::fs::create_dir_all(common_root).map_err(|_| RuntimeMetadataError::Storage)?;
    let manager_root = manager_root
        .canonicalize()
        .map_err(|_| RuntimeMetadataError::UnsafeManagerRoot)?;
    if dangerous_canonical_root(&manager_root) {
        return Err(RuntimeMetadataError::UnsafeManagerRoot);
    }

    let manifest_path = manager_root.join("registry.json");
    if !manifest_path.exists() {
        devbox_filesystem::atomic_write(&manifest_path, b"[]")
            .map_err(|_| RuntimeMetadataError::Storage)?;
    }
    let manifest_path = manifest_path
        .canonicalize()
        .map_err(|_| RuntimeMetadataError::UnsafeManagerRoot)?;
    if !manifest_path.starts_with(&manager_root) || !manifest_path.is_file() {
        return Err(RuntimeMetadataError::UnsafeManagerRoot);
    }

    let catalog_path = common_root.join("catalog.json");
    let (catalog, effective_catalog_revision) =
        sync_runtime_catalog(&catalog_path, build_catalog, catalog_revision)?;
    let locator_path = common_root.join("install-roots/v1/registry.json");
    let (locator, registry_revision) = sync_default_locator(
        &locator_path,
        &manager_root,
        &manifest_path,
        effective_catalog_revision,
        updated_at_ms,
    )?;

    Ok(RuntimeMetadataSync {
        catalog,
        locator,
        catalog_revision: effective_catalog_revision,
        registry_revision,
    })
}

fn sync_runtime_catalog(
    path: &Path,
    build_catalog: &str,
    build_revision: u64,
) -> Result<(SyncDisposition, u64), RuntimeMetadataError> {
    let current = std::fs::read_to_string(path)
        .ok()
        .and_then(|input| parse_catalog(&input).ok());
    if let Some(revision) = current.and_then(|catalog| catalog.catalog_revision) {
        if revision > build_revision {
            return Ok((SyncDisposition::PreservedNewer, revision));
        }
        if revision == build_revision {
            return Ok((SyncDisposition::Current, revision));
        }
    }
    devbox_filesystem::atomic_write(path, build_catalog.as_bytes())
        .map_err(|_| RuntimeMetadataError::Storage)?;
    Ok((SyncDisposition::Written, build_revision))
}

fn sync_default_locator(
    path: &Path,
    manager_root: &Path,
    manifest_path: &Path,
    catalog_revision: u64,
    updated_at_ms: u64,
) -> Result<(SyncDisposition, u64), RuntimeMetadataError> {
    let current = read_locator(path);
    if let Some(locator) = &current {
        if locator.root_id != DEFAULT_ROOT_ID {
            return Ok((
                SyncDisposition::PreservedOtherRoot,
                locator.registry_revision,
            ));
        }
        if Path::new(&locator.path) == manager_root
            && Path::new(&locator.manifest_path) == manifest_path
            && locator.catalog_revision == catalog_revision
        {
            return Ok((SyncDisposition::Current, locator.registry_revision));
        }
    }
    let registry_revision = current.map_or(Ok(1), |locator| {
        locator
            .registry_revision
            .checked_add(1)
            .ok_or(RuntimeMetadataError::RevisionOverflow)
    })?;
    let locator = InstallRootLocator {
        schema_version: INSTALL_ROOT_SCHEMA_VERSION,
        registry_revision,
        catalog_revision,
        root_id: DEFAULT_ROOT_ID.into(),
        path: manager_root.to_string_lossy().into_owned(),
        manifest_path: manifest_path.to_string_lossy().into_owned(),
        updated_at_ms,
    };
    let disposition = write_locator_if_newer(path, &locator)?;
    Ok((disposition, registry_revision))
}

pub fn write_locator_if_newer(
    path: &Path,
    candidate: &InstallRootLocator,
) -> Result<SyncDisposition, RuntimeMetadataError> {
    let encoded =
        serde_json::to_string_pretty(candidate).map_err(|_| RuntimeMetadataError::Serialization)?;
    parse_install_root_locator(&encoded).map_err(|_| RuntimeMetadataError::Serialization)?;
    if let Some(current) = read_locator(path) {
        if current.registry_revision >= candidate.registry_revision {
            return Ok(SyncDisposition::PreservedNewer);
        }
    }
    let parent = path.parent().ok_or(RuntimeMetadataError::Storage)?;
    std::fs::create_dir_all(parent).map_err(|_| RuntimeMetadataError::Storage)?;
    devbox_filesystem::atomic_write(path, encoded.as_bytes())
        .map_err(|_| RuntimeMetadataError::Storage)?;
    Ok(SyncDisposition::Written)
}

pub fn runtime_metadata_consistent(
    manager_root: &Path,
    common_root: &Path,
    build_catalog: &str,
) -> bool {
    let Ok(build) = parse_catalog(build_catalog) else {
        return false;
    };
    let Some(build_revision) = build.catalog_revision else {
        return false;
    };
    let runtime = std::fs::read_to_string(common_root.join("catalog.json"))
        .ok()
        .and_then(|input| parse_catalog(&input).ok());
    let Some(runtime_revision) = runtime.and_then(|catalog| catalog.catalog_revision) else {
        return false;
    };
    if runtime_revision < build_revision {
        return false;
    }
    let Some(locator) = read_locator(&common_root.join("install-roots/v1/registry.json")) else {
        return false;
    };
    if locator.catalog_revision != runtime_revision {
        return false;
    }
    let Ok(manager_root) = manager_root.canonicalize() else {
        return false;
    };
    let locator_root = PathBuf::from(&locator.path);
    let Ok(canonical_root) = locator_root.canonicalize() else {
        return false;
    };
    if canonical_root != locator_root {
        return false;
    }
    if dangerous_canonical_root(&canonical_root) {
        return false;
    }
    if locator.root_id == DEFAULT_ROOT_ID && locator_root != manager_root {
        return false;
    }
    let manifest = PathBuf::from(&locator.manifest_path);
    let Ok(canonical_manifest) = manifest.canonicalize() else {
        return false;
    };
    canonical_manifest == manifest
        && canonical_manifest.is_file()
        && canonical_manifest.starts_with(&canonical_root)
        && devbox_launch::validate_installation_metadata_from_paths(
            build_catalog,
            Some(&common_root.join("catalog.json")),
            &common_root.join("install-roots/v1/registry.json"),
        )
        .is_ok()
}

fn dangerous_canonical_root(root: &Path) -> bool {
    if root.parent().is_none() {
        return true;
    }
    if std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .and_then(|home| PathBuf::from(home).canonicalize().ok())
        .is_some_and(|home| root == home)
    {
        return true;
    }
    std::env::current_dir()
        .and_then(|cwd| cwd.canonicalize())
        .is_ok_and(|cwd| root == cwd)
}

fn read_locator(path: &Path) -> Option<InstallRootLocator> {
    let input = std::fs::read_to_string(path).ok()?;
    parse_install_root_locator(&input).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestRoots {
        outer: PathBuf,
        manager: PathBuf,
        common: PathBuf,
    }

    impl TestRoots {
        fn new() -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let outer = std::env::temp_dir().join(format!(
                "manager-runtime-metadata-test-{}-{id}",
                std::process::id()
            ));
            let manager = outer.join("manager");
            let common = outer.join("common");
            fs::create_dir_all(&manager).unwrap();
            Self {
                outer,
                manager,
                common,
            }
        }

        fn locator_path(&self) -> PathBuf {
            self.common.join("install-roots/v1/registry.json")
        }
    }

    impl Drop for TestRoots {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.outer);
        }
    }

    fn catalog(revision: u64) -> String {
        json!({
            "schemaVersion": 2,
            "catalogRevision": revision,
            "apps": [{
                "id": "code-pad",
                "displayName": "Code Pad",
                "productName": "Code Pad",
                "identifier": "com.devbox.codepad",
                "cargoPackage": "code-pad",
                "appDir": "apps/code-pad",
                "release": true,
                "managerVisible": true,
                "selfManaged": false,
                "accepts": ["path", "workspace"],
                "produces": [],
                "actions": []
            }]
        })
        .to_string()
    }

    #[test]
    fn initial_sync_atomically_publishes_catalog_manifest_and_locator() {
        let roots = TestRoots::new();
        let first = sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();

        assert_eq!(first.catalog, SyncDisposition::Written);
        assert_eq!(first.locator, SyncDisposition::Written);
        assert_eq!(first.catalog_revision, 5);
        assert_eq!(first.registry_revision, 1);
        assert_eq!(
            parse_catalog(&fs::read_to_string(roots.common.join("catalog.json")).unwrap())
                .unwrap()
                .catalog_revision,
            Some(5)
        );
        assert_eq!(
            fs::read_to_string(roots.manager.join("registry.json")).unwrap(),
            "[]"
        );
        let locator = read_locator(&roots.locator_path()).unwrap();
        assert_eq!(locator.registry_revision, 1);
        assert_eq!(locator.catalog_revision, 5);
        assert_eq!(
            Path::new(&locator.path),
            roots.manager.canonicalize().unwrap()
        );
        assert!(runtime_metadata_consistent(
            &roots.manager,
            &roots.common,
            &catalog(5)
        ));

        let second =
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 200).unwrap();
        assert_eq!(second.catalog, SyncDisposition::Current);
        assert_eq!(second.locator, SyncDisposition::Current);
        assert_eq!(second.registry_revision, 1);
    }

    #[test]
    fn corrupt_or_stale_runtime_catalog_is_replaced_without_downgrading_newer_data() {
        let roots = TestRoots::new();
        fs::create_dir_all(&roots.common).unwrap();
        fs::write(roots.common.join("catalog.json"), b"{corrupt").unwrap();
        let corrupt =
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        assert_eq!(corrupt.catalog, SyncDisposition::Written);

        fs::write(roots.common.join("catalog.json"), catalog(4)).unwrap();
        let stale = sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 200).unwrap();
        assert_eq!(stale.catalog, SyncDisposition::Written);

        fs::write(roots.common.join("catalog.json"), catalog(6)).unwrap();
        let newer = sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 300).unwrap();
        assert_eq!(newer.catalog, SyncDisposition::PreservedNewer);
        assert_eq!(newer.catalog_revision, 6);
        assert_eq!(
            read_locator(&roots.locator_path())
                .unwrap()
                .catalog_revision,
            6
        );
    }

    #[test]
    fn catalog_provenance_change_advances_registry_revision_once() {
        let roots = TestRoots::new();
        let first = sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        let second =
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(6), 200).unwrap();
        let third = sync_runtime_metadata(&roots.manager, &roots.common, &catalog(6), 300).unwrap();

        assert_eq!(first.registry_revision, 1);
        assert_eq!(second.registry_revision, 2);
        assert_eq!(second.locator, SyncDisposition::Written);
        assert_eq!(third.registry_revision, 2);
        assert_eq!(third.locator, SyncDisposition::Current);
    }

    #[test]
    fn lower_or_equal_locator_revision_never_overwrites_current_registry() {
        let roots = TestRoots::new();
        sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        let current = read_locator(&roots.locator_path()).unwrap();
        let mut stale = current.clone();
        stale.updated_at_ms = 999;

        assert_eq!(
            write_locator_if_newer(&roots.locator_path(), &stale).unwrap(),
            SyncDisposition::PreservedNewer
        );
        assert_eq!(read_locator(&roots.locator_path()).unwrap(), current);
    }

    #[test]
    fn consistency_rejects_catalog_provenance_or_manifest_path_mismatch() {
        let roots = TestRoots::new();
        sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        let mut locator = read_locator(&roots.locator_path()).unwrap();

        locator.catalog_revision = 4;
        fs::write(roots.locator_path(), serde_json::to_vec(&locator).unwrap()).unwrap();
        assert!(!runtime_metadata_consistent(
            &roots.manager,
            &roots.common,
            &catalog(5)
        ));

        locator.catalog_revision = 5;
        locator.manifest_path = roots
            .outer
            .join("outside.json")
            .to_string_lossy()
            .into_owned();
        fs::write(&locator.manifest_path, b"[]").unwrap();
        fs::write(roots.locator_path(), serde_json::to_vec(&locator).unwrap()).unwrap();
        assert!(!runtime_metadata_consistent(
            &roots.manager,
            &roots.common,
            &catalog(5)
        ));
    }

    #[test]
    fn consistency_rejects_manifest_apps_absent_from_the_selected_catalog() {
        let roots = TestRoots::new();
        sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        fs::write(
            roots.manager.join("registry.json"),
            json!([{
                "app": "unknown-app",
                "version": "0.5.0",
                "mode": "installer",
                "exe_path": ""
            }])
            .to_string(),
        )
        .unwrap();

        assert!(!runtime_metadata_consistent(
            &roots.manager,
            &roots.common,
            &catalog(5)
        ));
    }

    #[test]
    fn another_valid_root_is_preserved_for_future_custom_root_support() {
        let roots = TestRoots::new();
        sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        let mut custom = read_locator(&roots.locator_path()).unwrap();
        custom.registry_revision = 2;
        custom.root_id = "custom-root-1".into();
        fs::write(roots.locator_path(), serde_json::to_vec(&custom).unwrap()).unwrap();

        let synced =
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 200).unwrap();
        assert_eq!(synced.locator, SyncDisposition::PreservedOtherRoot);
        assert_eq!(synced.registry_revision, 2);
        assert_eq!(
            read_locator(&roots.locator_path()).unwrap().root_id,
            "custom-root-1"
        );
    }

    #[test]
    fn invalid_build_catalog_and_storage_errors_do_not_echo_untrusted_paths() {
        let roots = TestRoots::new();
        let secret = "runtime-metadata-secret";
        let error = sync_runtime_metadata(
            &roots.manager,
            &roots.common,
            &format!("{{broken:{secret}}}"),
            100,
        )
        .unwrap_err()
        .to_string();

        assert_eq!(error, "build-time catalog is invalid");
        assert!(!error.contains(secret));
    }
}
