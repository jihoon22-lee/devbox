//! One native-owned generation pointer selects Workspace metadata stores.
//! Repository files and .git directories never live inside these generations.
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, try_lock_exclusive,
    FilesystemIdentity,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::Mutex,
};

type Result<T> = std::result::Result<T, &'static str>;
const POINTER: &str = "active-stores.json";
const COMPONENTS: &[&str] = &["registry", "overview", "files", "common"];
const MAX_GENERATIONS: usize = 32;
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Generation {
    pub schema_version: u32,
    pub id: String,
}
impl Generation {
    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err("unsupported_store_version");
        }
        if !uuid::Uuid::parse_str(&self.id).is_ok_and(|id| id.to_string() == self.id) {
            return Err("invalid_store_generation");
        }
        Ok(())
    }
}

pub struct StoreRoot {
    root: PathBuf,
    identity: FilesystemIdentity,
    _root_handle: File,
    lock_identity: FilesystemIdentity,
    _lock_handle: File,
    writer: Mutex<()>,
}
impl StoreRoot {
    /// Native startup supplies this existing product-local directory. No
    /// renderer path selects an activation owner or legacy namespace.
    pub fn open(root: &Path) -> Result<Self> {
        if !root.is_absolute() {
            return Err("invalid_store_root");
        }
        ensure_no_links(root).map_err(|_| "invalid_store_root")?;
        let (handle, identity) =
            open_filesystem_object(root, true).map_err(|_| "invalid_store_root")?;
        let lock_path = root.join("store-activation.lock");
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => drop(file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err("store_owner_unavailable"),
        }
        ensure_no_links(&lock_path).map_err(|_| "store_owner_changed")?;
        let (lock_handle, lock_identity) =
            open_filesystem_object(&lock_path, false).map_err(|_| "store_owner_changed")?;
        if !try_lock_exclusive(&lock_handle).map_err(|_| "store_owner_unavailable")? {
            return Err("store_owner_busy");
        }
        let store = Self {
            root: root.into(),
            identity,
            _root_handle: handle,
            lock_identity,
            _lock_handle: lock_handle,
            writer: Mutex::new(()),
        };
        store.assert_owner()?;
        if let Some(generation) = store.read()? {
            store.validate_generation(&generation)?;
        }
        Ok(store)
    }
    fn assert_owner(&self) -> Result<()> {
        ensure_no_links(&self.root).map_err(|_| "store_owner_changed")?;
        if filesystem_identity(&self.root, true).map_err(|_| "store_owner_changed")?
            != self.identity
            || filesystem_identity(self.root.join("store-activation.lock"), false)
                .map_err(|_| "store_owner_changed")?
                != self.lock_identity
        {
            return Err("store_owner_changed");
        }
        Ok(())
    }
    pub fn read(&self) -> Result<Option<Generation>> {
        self.assert_owner()?;
        let path = self.root.join(POINTER);
        let (handle, identity) = match open_filesystem_object(&path, false) {
            Ok(value) => value,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("store_pointer_unavailable"),
        };
        let mut bytes = Vec::new();
        handle
            .take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| "store_pointer_unavailable")?;
        if bytes.len() > 4096 {
            return Err("invalid_store_generation");
        }
        let generation: Generation =
            serde_json::from_slice(&bytes).map_err(|_| "invalid_store_generation")?;
        generation.validate()?;
        self.assert_owner()?;
        if filesystem_identity(&path, false).map_err(|_| "store_pointer_changed")? != identity {
            return Err("store_pointer_changed");
        }
        Ok(Some(generation))
    }
    pub fn component(&self, generation: &Generation, component: &str) -> Result<PathBuf> {
        self.assert_owner()?;
        generation.validate()?;
        if !COMPONENTS.contains(&component) {
            return Err("invalid_store_component");
        }
        let path = self
            .root
            .join("stores")
            .join(&generation.id)
            .join(component);
        ensure_no_links(&path).map_err(|_| "store_generation_unavailable")?;
        open_filesystem_object(&path, true).map_err(|_| "store_generation_unavailable")?;
        Ok(path)
    }
    fn validate_generation(&self, generation: &Generation) -> Result<()> {
        for component in COMPONENTS {
            self.component(generation, component)?;
        }
        // Future/corrupt Registry metadata cannot become an empty project list.
        // The activation owner validates without acquiring the live Registry
        // writer, which is held separately by the runtime ProjectOwner.
        let path = self
            .component(generation, "registry")?
            .join("project-registry.json");
        match open_filesystem_object(&path, false) {
            Ok((handle, identity)) => {
                let mut bytes = Vec::new();
                handle
                    .take(4 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "invalid_registry")?;
                super::registry::Registry::parse(&bytes)?;
                if filesystem_identity(path, false).map_err(|_| "store_generation_changed")?
                    != identity
                {
                    return Err("store_generation_changed");
                }
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err("invalid_registry"),
        }
        'files: for (component, name, limit) in [
            ("overview", "project-profiles.json", 4 * 1024 * 1024),
            ("overview", "profile-templates.json", 1024 * 1024),
            ("files", "session.json", 8 * 1024 * 1024),
            ("files", "recovery.json", 8 * 1024 * 1024),
            ("files", "lsp/config.json", 8 * 1024 * 1024),
        ] {
            let mut path = self.component(generation, component)?;
            for segment in name.split('/') {
                path.push(segment);
                match fs::symlink_metadata(&path) {
                    Ok(_) => ensure_no_links(&path).map_err(|_| "invalid_component_store")?,
                    Err(error) if error.kind() == ErrorKind::NotFound => continue 'files,
                    Err(_) => return Err("invalid_component_store"),
                }
            }
            let (mut handle, identity) = match open_filesystem_object(&path, false) {
                Ok(value) => value,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(_) => return Err("invalid_component_store"),
            };
            let mut bytes = Vec::new();
            (&mut handle)
                .take(limit + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "invalid_component_store")?;
            if bytes.len() as u64 > limit {
                return Err("invalid_component_store");
            }
            if component == "overview" {
                workbench_lib::component::validate_persistent_file(name, &bytes)?;
            } else {
                code_pad_lib::component::validate_persistent_file(name, &bytes)?;
            }
            if filesystem_identity(&path, false).map_err(|_| "store_generation_changed")?
                != identity
            {
                return Err("store_generation_changed");
            }
        }
        self.assert_owner()
    }
    /// A prepared directory is unselected until activation; cancellation or
    /// importer failure can preserve it for recovery without changing startup.
    pub fn prepare(&self) -> Result<Generation> {
        let _writer = self.writer.lock().map_err(|_| "store_owner_busy")?;
        self.assert_owner()?;
        let stores = self.root.join("stores");
        match fs::create_dir(&stores) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err("store_prepare_failed"),
        }
        ensure_no_links(&stores).map_err(|_| "store_owner_changed")?;
        // Failed/unselected generations remain recoverable. Bound preparation
        // instead of silently deleting a generation that may contain edits.
        let entries = fs::read_dir(&stores).map_err(|_| "store_prepare_failed")?;
        for (index, entry) in entries.enumerate() {
            entry.map_err(|_| "store_prepare_failed")?;
            if index >= MAX_GENERATIONS - 1 {
                return Err("store_generation_limit");
            }
        }
        let generation = Generation {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
        };
        let directory = stores.join(&generation.id);
        fs::create_dir(&directory).map_err(|_| "store_prepare_failed")?;
        for component in COMPONENTS {
            fs::create_dir(directory.join(component)).map_err(|_| "store_prepare_failed")?;
        }
        self.validate_generation(&generation)?;
        Ok(generation)
    }
    /// New-user activation cannot replace an existing selected generation.
    /// Import/recovery must use its separately reviewed transition protocol.
    pub fn activate_new(&self, generation: &Generation) -> Result<()> {
        let _writer = self.writer.lock().map_err(|_| "store_owner_busy")?;
        self.validate_generation(generation)?;
        if self.read()?.is_some() {
            return Err("store_already_active");
        }
        let bytes = serde_json::to_vec(generation).map_err(|_| "invalid_store_generation")?;
        devbox_filesystem::atomic_write(self.root.join(POINTER), &bytes)
            .map_err(|_| "store_activation_failed")?;
        self.assert_owner()?;
        if self.read()?.as_ref() != Some(generation) {
            return Err("store_pointer_changed");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_is_unselected_and_activation_preserves_private_component_ownership() {
        let root = tempfile::tempdir().unwrap();
        let store = StoreRoot::open(root.path()).unwrap();
        let abandoned = store.prepare().unwrap();
        assert_eq!(store.read().unwrap(), None);
        let selected = store.prepare().unwrap();
        assert_ne!(selected, abandoned);
        let paths = COMPONENTS
            .iter()
            .map(|component| store.component(&selected, component).unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(paths.len(), COMPONENTS.len());
        store.activate_new(&selected).unwrap();
        assert_eq!(store.activate_new(&abandoned), Err("store_already_active"));
        assert_eq!(store.read().unwrap(), Some(selected.clone()));
        assert!(store.component(&selected, "../legacy").is_err());
        drop(store);
        assert_eq!(
            StoreRoot::open(root.path()).unwrap().read().unwrap(),
            Some(selected)
        );
    }
    #[test]
    fn competing_owner_and_corrupt_future_pointer_are_not_silently_replaced() {
        let root = tempfile::tempdir().unwrap();
        let owner = StoreRoot::open(root.path()).unwrap();
        assert!(matches!(
            StoreRoot::open(root.path()),
            Err("store_owner_busy")
        ));
        let generation = owner.prepare().unwrap();
        let mut future = generation.clone();
        future.schema_version = 2;
        for bytes in [b"{broken".to_vec(), serde_json::to_vec(&future).unwrap()] {
            fs::write(root.path().join(POINTER), &bytes).unwrap();
            assert!(owner.activate_new(&generation).is_err());
            assert_eq!(fs::read(root.path().join(POINTER)).unwrap(), bytes);
        }
    }
    #[test]
    fn malformed_registry_or_linked_component_prevents_activation() {
        let root = tempfile::tempdir().unwrap();
        let owner = StoreRoot::open(root.path()).unwrap();
        let generation = owner.prepare().unwrap();
        let path = owner
            .component(&generation, "registry")
            .unwrap()
            .join("project-registry.json");
        fs::write(&path, "{broken").unwrap();
        assert!(owner.activate_new(&generation).is_err());
        assert_eq!(owner.read().unwrap(), None);
        fs::remove_file(path).unwrap();
        let component = owner.component(&generation, "files").unwrap();
        fs::remove_dir(&component).unwrap();
        fs::write(&component, "not a directory").unwrap();
        assert!(owner.activate_new(&generation).is_err());
    }
    #[test]
    fn invalid_user_metadata_blocks_activation_without_losing_original_bytes() {
        let root = tempfile::tempdir().unwrap();
        let owner = StoreRoot::open(root.path()).unwrap();
        let generation = owner.prepare().unwrap();
        for (component, name, bytes) in [
            (
                "overview",
                "project-profiles.json",
                br#"{"version":2,"profiles":[]}"#.as_slice(),
            ),
            ("overview", "profile-templates.json", b"{broken".as_slice()),
            ("files", "session.json", br#"{"version":2}"#.as_slice()),
            (
                "files",
                "recovery.json",
                br#"{"version":2,"entries":[]}"#.as_slice(),
            ),
            ("files", "lsp/config.json", b"{broken".as_slice()),
        ] {
            let path = owner.component(&generation, component).unwrap().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            assert!(owner.activate_new(&generation).is_err(), "{name}");
            assert_eq!(owner.read().unwrap(), None);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            fs::remove_file(&path).unwrap();
        }
        let recovery = owner
            .component(&generation, "files")
            .unwrap()
            .join("recovery.json");
        let bytes = r#"{"version":1,"entries":[{"path":"C:\\fixture\\한글.rs","content":"unsaved\r\n","snapshot_at_ms":1}]}"#.as_bytes();
        fs::write(&recovery, bytes).unwrap();
        owner.activate_new(&generation).unwrap();
        drop(owner);
        let reopened = StoreRoot::open(root.path()).unwrap();
        assert_eq!(reopened.read().unwrap(), Some(generation));
        assert_eq!(fs::read(recovery).unwrap(), bytes);
    }
    #[test]
    fn unselected_generation_limit_preserves_recoverable_data() {
        let root = tempfile::tempdir().unwrap();
        let owner = StoreRoot::open(root.path()).unwrap();
        let first = owner.prepare().unwrap();
        let path = owner
            .component(&first, "files")
            .unwrap()
            .join("preserved.txt");
        fs::write(&path, "unselected recovery bytes").unwrap();
        for _ in 1..MAX_GENERATIONS {
            owner.prepare().unwrap();
        }
        assert_eq!(owner.prepare(), Err("store_generation_limit"));
        assert_eq!(owner.read().unwrap(), None);
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "unselected recovery bytes"
        );
    }
    #[test]
    #[cfg(unix)]
    fn replacing_owner_directory_invalidates_prepared_activation() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("owner");
        fs::create_dir(&root).unwrap();
        let owner = StoreRoot::open(&root).unwrap();
        let generation = owner.prepare().unwrap();
        fs::rename(&root, base.path().join("moved")).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(owner.activate_new(&generation).is_err());
        assert!(!root.join(POINTER).exists());
        assert!(!base.path().join("moved").join(POINTER).exists());
    }
}
