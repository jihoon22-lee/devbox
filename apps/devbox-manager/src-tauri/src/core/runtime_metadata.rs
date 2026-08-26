use devbox_catalog::{parse_catalog, select_catalog};
use devbox_launch::{parse_install_root_locator, InstallRootLocator, INSTALL_ROOT_SCHEMA_VERSION};
use std::fmt;
use std::fs::File;
use std::io::Read;
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

    let locator_path = common_root.join("install-roots/v1/registry.json");
    ensure_plain_existing_components(common_root).map_err(|_| RuntimeMetadataError::Storage)?;
    if let Some(locator_parent) = locator_path.parent() {
        ensure_plain_existing_components(locator_parent)
            .map_err(|_| RuntimeMetadataError::Storage)?;
    }
    // A present but corrupt locator is an explicit failure, not permission to
    // silently recreate the default pointer. Inspect it before any metadata
    // write so startup remains fail-closed for a damaged custom-root state.
    let current_locator = read_locator_state(&locator_path)?;
    let runtime_catalog = std::fs::read_to_string(common_root.join("catalog.json")).ok();
    let selected_catalog_revision = select_catalog(build_catalog, runtime_catalog.as_deref())
        .map_err(|_| RuntimeMetadataError::InvalidBuildCatalog)?
        .catalog
        .catalog_revision
        .ok_or(RuntimeMetadataError::MissingBuildRevision)?;
    if current_locator
        .as_ref()
        .is_some_and(|locator| locator.catalog_revision > selected_catalog_revision)
    {
        return Err(RuntimeMetadataError::Storage);
    }
    if current_locator
        .as_ref()
        .is_some_and(|locator| locator.root_id != DEFAULT_ROOT_ID)
    {
        devbox_launch::validate_installation_metadata_from_paths(
            build_catalog,
            Some(&common_root.join("catalog.json")),
            &locator_path,
        )
        .map_err(|_| RuntimeMetadataError::Storage)?;
    }

    ensure_plain_existing_components(manager_root)
        .map_err(|_| RuntimeMetadataError::UnsafeManagerRoot)?;
    ensure_plain_existing_components(common_root).map_err(|_| RuntimeMetadataError::Storage)?;
    std::fs::create_dir_all(manager_root).map_err(|_| RuntimeMetadataError::Storage)?;
    std::fs::create_dir_all(common_root).map_err(|_| RuntimeMetadataError::Storage)?;
    let manager_metadata =
        std::fs::symlink_metadata(manager_root).map_err(|_| RuntimeMetadataError::Storage)?;
    if metadata_is_link_or_reparse(&manager_metadata) || !manager_metadata.is_dir() {
        return Err(RuntimeMetadataError::UnsafeManagerRoot);
    }
    let manager_root =
        canonicalize_path(manager_root).map_err(|_| RuntimeMetadataError::UnsafeManagerRoot)?;
    if dangerous_canonical_root(&manager_root) {
        return Err(RuntimeMetadataError::UnsafeManagerRoot);
    }

    let manifest_path = manager_root.join("registry.json");
    match std::fs::symlink_metadata(&manifest_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            devbox_filesystem::atomic_write(&manifest_path, b"[]")
                .map_err(|_| RuntimeMetadataError::Storage)?;
        }
        Ok(metadata) if metadata_is_link_or_reparse(&metadata) || !metadata.is_file() => {
            return Err(RuntimeMetadataError::UnsafeManagerRoot);
        }
        Ok(_) => {}
        Err(_) => return Err(RuntimeMetadataError::Storage),
    }
    let manifest_path =
        canonicalize_path(&manifest_path).map_err(|_| RuntimeMetadataError::UnsafeManagerRoot)?;
    if !path_within(&manager_root, &manifest_path) || !manifest_path.is_file() {
        return Err(RuntimeMetadataError::UnsafeManagerRoot);
    }

    let catalog_path = common_root.join("catalog.json");
    let (catalog, effective_catalog_revision) =
        sync_runtime_catalog(&catalog_path, build_catalog, catalog_revision)?;
    let (locator, registry_revision) = sync_default_locator(
        &locator_path,
        &manager_root,
        &manifest_path,
        effective_catalog_revision,
        updated_at_ms,
        current_locator,
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
    current: Option<InstallRootLocator>,
) -> Result<(SyncDisposition, u64), RuntimeMetadataError> {
    if let Some(locator) = &current {
        if locator.root_id != DEFAULT_ROOT_ID {
            if locator.catalog_revision == catalog_revision {
                return Ok((
                    SyncDisposition::PreservedOtherRoot,
                    locator.registry_revision,
                ));
            }
            let mut updated = locator.clone();
            updated.registry_revision = updated
                .registry_revision
                .checked_add(1)
                .ok_or(RuntimeMetadataError::RevisionOverflow)?;
            updated.catalog_revision = catalog_revision;
            updated.updated_at_ms = updated_at_ms;
            let revision = updated.registry_revision;
            let disposition = write_locator_if_newer(path, &updated)?;
            return Ok((disposition, revision));
        }
        if same_path_identity(Path::new(&locator.path), manager_root)
            && same_path_identity(Path::new(&locator.manifest_path), manifest_path)
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
    if let Some(current) = read_locator_state(path)? {
        if current.registry_revision >= candidate.registry_revision {
            return Ok(SyncDisposition::PreservedNewer);
        }
    }
    let parent = path.parent().ok_or(RuntimeMetadataError::Storage)?;
    ensure_plain_existing_components(parent).map_err(|_| RuntimeMetadataError::Storage)?;
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
    let Ok(manager_root) = canonicalize_path(manager_root) else {
        return false;
    };
    let locator_root = PathBuf::from(&locator.path);
    let Ok(canonical_root) = canonicalize_path(&locator_root) else {
        return false;
    };
    if !same_path_identity(&canonical_root, &locator_root) {
        return false;
    }
    if dangerous_canonical_root(&canonical_root) {
        return false;
    }
    if locator.root_id == DEFAULT_ROOT_ID && !same_path_identity(&locator_root, &manager_root) {
        return false;
    }
    let manifest = PathBuf::from(&locator.manifest_path);
    let Ok(canonical_manifest) = canonicalize_path(&manifest) else {
        return false;
    };
    same_path_identity(&canonical_manifest, &manifest)
        && canonical_manifest.is_file()
        && path_within(&canonical_root, &canonical_manifest)
        && devbox_launch::validate_installation_metadata_from_paths(
            build_catalog,
            Some(&common_root.join("catalog.json")),
            &common_root.join("install-roots/v1/registry.json"),
        )
        .is_ok()
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

fn dangerous_canonical_root(root: &Path) -> bool {
    if is_filesystem_root(root) {
        return true;
    }
    if std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .and_then(|home| canonicalize_path(&PathBuf::from(home)).ok())
        .is_some_and(|home| same_path_identity(root, &home))
    {
        return true;
    }
    std::env::current_dir()
        .and_then(|cwd| canonicalize_path(&cwd))
        .is_ok_and(|cwd| same_path_identity(root, &cwd))
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

/// Check existing path components before a later `create_dir_all`. Missing
/// components are allowed because the caller may create them, but a present
/// symlink/reparse component is never followed by metadata sync.
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
            Ok(metadata) if metadata_is_link_or_reparse(&metadata) => return Err(()),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

fn read_locator(path: &Path) -> Option<InstallRootLocator> {
    read_locator_state(path).ok().flatten()
}

fn read_locator_state(path: &Path) -> Result<Option<InstallRootLocator>, RuntimeMetadataError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RuntimeMetadataError::Storage),
    };
    if metadata_is_link_or_reparse(&metadata)
        || !metadata.is_file()
        || metadata.len() > devbox_launch::MAX_INSTALL_ROOT_LOCATOR_BYTES
    {
        return Err(RuntimeMetadataError::Storage);
    }
    let file = File::open(path).map_err(|_| RuntimeMetadataError::Storage)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(devbox_launch::MAX_INSTALL_ROOT_LOCATOR_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RuntimeMetadataError::Storage)?;
    if bytes.len() as u64 > devbox_launch::MAX_INSTALL_ROOT_LOCATOR_BYTES {
        return Err(RuntimeMetadataError::Storage);
    }
    let input = String::from_utf8(bytes).map_err(|_| RuntimeMetadataError::Storage)?;
    let locator = parse_install_root_locator(&input).map_err(|_| RuntimeMetadataError::Storage)?;
    Ok(Some(locator))
}

#[cfg(windows)]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
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
            canonicalize_path(&roots.manager).unwrap()
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
    fn another_valid_root_is_preserved_and_tracks_catalog_revision() {
        let roots = TestRoots::new();
        sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        let custom_root = roots.outer.join("custom-root");
        fs::create_dir(&custom_root).unwrap();
        let custom_manifest = custom_root.join("registry.json");
        fs::write(&custom_manifest, b"[]").unwrap();
        let mut custom = read_locator(&roots.locator_path()).unwrap();
        custom.registry_revision = 2;
        custom.root_id = "custom-root-1".into();
        custom.path = canonicalize_path(&custom_root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        custom.manifest_path = canonicalize_path(&custom_manifest)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        fs::write(roots.locator_path(), serde_json::to_vec(&custom).unwrap()).unwrap();

        let synced =
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(6), 200).unwrap();
        assert_eq!(synced.locator, SyncDisposition::Written);
        assert_eq!(synced.registry_revision, 3);
        let updated = read_locator(&roots.locator_path()).unwrap();
        assert_eq!(updated.root_id, "custom-root-1");
        assert_eq!(updated.catalog_revision, 6);
        assert_eq!(updated.path, custom.path);
        assert_eq!(updated.manifest_path, custom.manifest_path);

        let current =
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(6), 300).unwrap();
        assert_eq!(current.locator, SyncDisposition::PreservedOtherRoot);
        assert_eq!(current.registry_revision, 3);
    }

    #[test]
    fn unsafe_custom_root_is_not_preserved_or_rewritten_during_startup() {
        let roots = TestRoots::new();
        sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        let mut custom = read_locator(&roots.locator_path()).unwrap();
        custom.registry_revision = 2;
        custom.root_id = "custom-root-unsafe".into();
        custom.path = roots
            .outer
            .join("missing-root")
            .to_string_lossy()
            .into_owned();
        custom.manifest_path = roots
            .outer
            .join("missing-root/registry.json")
            .to_string_lossy()
            .into_owned();
        let original = serde_json::to_vec(&custom).unwrap();
        fs::write(roots.locator_path(), &original).unwrap();

        assert_eq!(
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(6), 200),
            Err(RuntimeMetadataError::Storage)
        );
        assert_eq!(fs::read(roots.locator_path()).unwrap(), original);
    }

    #[test]
    fn locator_catalog_revision_is_never_downgraded() {
        let roots = TestRoots::new();
        sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100).unwrap();
        let mut locator = read_locator(&roots.locator_path()).unwrap();
        locator.registry_revision = 2;
        locator.catalog_revision = 7;
        let original = serde_json::to_vec(&locator).unwrap();
        fs::write(roots.locator_path(), &original).unwrap();

        assert_eq!(
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(6), 200),
            Err(RuntimeMetadataError::Storage)
        );
        assert_eq!(fs::read(roots.locator_path()).unwrap(), original);
    }

    #[test]
    fn present_corrupt_locator_is_not_replaced_during_startup_sync() {
        let roots = TestRoots::new();
        fs::create_dir_all(roots.locator_path().parent().unwrap()).unwrap();
        let original = br#"{"broken":true}"#;
        fs::write(roots.locator_path(), original).unwrap();

        assert_eq!(
            sync_runtime_metadata(&roots.manager, &roots.common, &catalog(5), 100),
            Err(RuntimeMetadataError::Storage)
        );
        assert_eq!(fs::read(roots.locator_path()).unwrap(), original);
        assert!(!roots.manager.join("registry.json").exists());
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
