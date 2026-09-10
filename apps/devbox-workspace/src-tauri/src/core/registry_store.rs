//! Single-writer storage for the Workspace-owned registry. A product host opens
//! only its activated generation. Reading metadata grants no execution trust.
use super::registry::Registry;
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, try_lock_exclusive,
    FilesystemIdentity,
};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type Result<T> = std::result::Result<T, &'static str>;
const MAX_BYTES: u64 = 4 * 1024 * 1024;

pub struct RegistryStore {
    directory: PathBuf,
    directory_identity: FilesystemIdentity,
    _directory_handle: File,
    lease_identity: FilesystemIdentity,
    _lease_handle: File,
    writer: Mutex<()>,
}
impl RegistryStore {
    /// No path is accepted from a renderer here. Native activation selects an
    /// existing local product directory, with no symlink/reparse ancestors.
    pub fn open(directory: &Path) -> Result<Self> {
        if !directory.is_absolute() {
            return Err("invalid_registry_directory");
        }
        ensure_no_links(directory).map_err(|_| "invalid_registry_directory")?;
        let (directory_handle, directory_identity) =
            open_filesystem_object(directory, true).map_err(|_| "invalid_registry_directory")?;
        let lock_path = directory.join("registry-owner.lock");
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&lock_path)
        {
            Ok(file) => drop(file),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err("registry_owner_unavailable"),
        }
        ensure_no_links(&lock_path).map_err(|_| "registry_owner_changed")?;
        let (lease_handle, lease_identity) =
            open_filesystem_object(&lock_path, false).map_err(|_| "registry_owner_changed")?;
        if !try_lock_exclusive(&lease_handle).map_err(|_| "registry_owner_unavailable")? {
            return Err("registry_owner_busy");
        }
        let store = Self {
            directory: directory.into(),
            directory_identity,
            _directory_handle: directory_handle,
            lease_identity,
            _lease_handle: lease_handle,
            writer: Mutex::new(()),
        };
        store.assert_owner()?;
        store.read()?; // Corrupt/future data must fail without any replacement.
        Ok(store)
    }
    pub fn read(&self) -> Result<Registry> {
        self.read_snapshot().map(|(_, registry)| registry)
    }
    pub fn update<T>(
        &self,
        expected_revision: u64,
        change: impl FnOnce(&mut Registry) -> Result<T>,
    ) -> Result<(Registry, T)> {
        let _writer = self.writer.lock().map_err(|_| "registry_store_busy")?;
        let (before, mut next) = self.read_snapshot()?;
        if next.revision != expected_revision {
            return Err("stale_registry");
        }
        let original = next.clone();
        let result = change(&mut next)?;
        if next == original {
            return Ok((next, result));
        }
        let after = next.encode()?;
        if before.as_deref() == Some(after.as_slice()) {
            return Ok((next, result));
        }
        // A no-op on a fresh store need not create a file. Every actual change
        // advances the owner revision, including imports and trust approval.
        if before.is_none() && next == Registry::default() {
            return Ok((next, result));
        }
        if next.revision <= expected_revision {
            return Err("invalid_revision");
        }
        let (current, _) = self.read_snapshot()?;
        if current != before {
            return Err("registry_changed_during_review");
        }
        devbox_filesystem::atomic_write(self.directory.join("project-registry.json"), &after)
            .map_err(|_| "registry_save_failed")?;
        self.assert_owner()?;
        let (saved, checked) = self.read_snapshot()?;
        if saved.as_deref() != Some(after.as_slice()) {
            return Err("registry_changed_after_save");
        }
        Ok((checked, result))
    }
    fn assert_owner(&self) -> Result<()> {
        ensure_no_links(&self.directory).map_err(|_| "registry_owner_changed")?;
        if filesystem_identity(&self.directory, true).map_err(|_| "registry_owner_changed")?
            != self.directory_identity
            || filesystem_identity(self.directory.join("registry-owner.lock"), false)
                .map_err(|_| "registry_owner_changed")?
                != self.lease_identity
        {
            return Err("registry_owner_changed");
        }
        Ok(())
    }
    fn read_snapshot(&self) -> Result<(Option<Vec<u8>>, Registry)> {
        self.assert_owner()?;
        let path = self.directory.join("project-registry.json");
        let (mut handle, identity) = match open_filesystem_object(&path, false) {
            Ok(opened) => opened,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok((None, Registry::default()))
            }
            Err(_) => return Err("registry_read_failed"),
        };
        let mut bytes = Vec::new();
        (&mut handle)
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "registry_read_failed")?;
        let registry = Registry::parse(&bytes)?;
        self.assert_owner()?;
        if filesystem_identity(&path, false).map_err(|_| "registry_changed_during_read")?
            != identity
        {
            return Err("registry_changed_during_read");
        }
        Ok((Some(bytes), registry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::{Binding, ObjectStamp};
    use product_contract::ExecutionTarget;
    fn add(store: &RegistryStore, revision: u64) -> Result<Registry> {
        store
            .update(revision, |registry| {
                registry.register(
                    revision,
                    "fixture",
                    Binding {
                        target: ExecutionTarget::Windows,
                        root: r"C:\synthetic\repo".into(),
                        root_object: ObjectStamp {
                            scope: "1".into(),
                            object: "2".into(),
                        },
                        repository_object: None,
                    },
                )
            })
            .map(|(registry, _)| registry)
    }
    #[test]
    fn lease_cas_and_restart_keep_one_durable_owner() {
        let directory = tempfile::tempdir().unwrap();
        let store = RegistryStore::open(directory.path()).unwrap();
        assert!(matches!(
            RegistryStore::open(directory.path()),
            Err("registry_owner_busy")
        ));
        let original = add(&store, 1).unwrap();
        assert_eq!(add(&store, 1), Err("stale_registry"));
        drop(store);
        let reopened = RegistryStore::open(directory.path()).unwrap();
        assert_eq!(reopened.read().unwrap(), original);
        assert_eq!(add(&reopened, 2).unwrap(), original);
    }
    #[test]
    fn corrupt_future_data_and_failed_mutation_preserve_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("project-registry.json");
        for invalid in [b"{broken".as_slice(), br#"{"schemaVersion":2,"revision":1,"projects":[],"worktrees":[],"legacyReferences":[]}"#] {
            std::fs::write(&path, invalid).unwrap();
            assert!(RegistryStore::open(directory.path()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), invalid);
        }
        std::fs::remove_file(&path).unwrap();
        let store = RegistryStore::open(directory.path()).unwrap();
        add(&store, 1).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(store
            .update(2, |registry| {
                registry.projects.clear();
                registry.revision += 1;
                Ok(())
            })
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(store
            .update(2, |registry| {
                registry.projects[0].name = "silent change".into();
                Ok(())
            })
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn concurrent_disk_change_during_review_is_not_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let store = RegistryStore::open(directory.path()).unwrap();
        let original = add(&store, 1).unwrap();
        let path = directory.path().join("project-registry.json");
        let mut external = original.clone();
        external
            .rename(2, &original.projects[0].id, "external fixture edit")
            .unwrap();
        let external_bytes = external.encode().unwrap();
        let result = store.update(2, |registry| {
            std::fs::write(&path, &external_bytes).unwrap();
            registry.rename(2, &original.projects[0].id, "stale review")
        });
        assert_eq!(result, Err("registry_changed_during_review"));
        assert_eq!(std::fs::read(&path).unwrap(), external_bytes);
    }
    #[cfg(unix)]
    #[test]
    fn directory_replacement_and_symlink_store_are_rejected() {
        use std::os::unix::fs::symlink;
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let store = RegistryStore::open(&root).unwrap();
        add(&store, 1).unwrap();
        std::fs::rename(&root, parent.path().join("old")).unwrap();
        std::fs::create_dir(&root).unwrap();
        assert_eq!(store.read(), Err("registry_owner_changed"));
        let outside = parent.path().join("outside.json");
        std::fs::write(&outside, b"preserve me").unwrap();
        symlink(&outside, root.join("project-registry.json")).unwrap();
        assert!(RegistryStore::open(&root).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"preserve me");
    }
}
