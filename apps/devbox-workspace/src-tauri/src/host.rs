//! Native-selected store and Project Registry owner for the product host.
//! Call IO methods only from the dispatcher's bounded blocking workers.
use crate::{core::stores::StoreRoot, project_owner::ProjectOwner};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::File,
    path::Path,
    sync::{Arc, RwLock},
};

type Result<T> = std::result::Result<T, &'static str>;
struct Selected {
    generation: crate::core::stores::Generation,
    projects: Arc<ProjectOwner>,
    components: HashMap<&'static str, ComponentRoot>,
}
struct ComponentRoot {
    path: std::path::PathBuf,
    identity: devbox_filesystem::FilesystemIdentity,
    _handle: File,
}
impl Selected {
    fn open(stores: &StoreRoot, generation: crate::core::stores::Generation) -> Result<Self> {
        stores.ensure_runtime_components(&generation)?;
        let mut components = HashMap::new();
        for &name in crate::core::stores::COMPONENTS {
            let path = stores.component(&generation, name)?;
            let (handle, identity) = devbox_filesystem::open_filesystem_object(&path, true)
                .map_err(|_| "store_generation_changed")?;
            components.insert(
                name,
                ComponentRoot {
                    path,
                    identity,
                    _handle: handle,
                },
            );
        }
        let projects = Arc::new(ProjectOwner::open(&components["registry"].path)?);
        Ok(Self {
            generation,
            projects,
            components,
        })
    }
}
pub struct Host {
    stores: Arc<StoreRoot>,
    pub(crate) legacy: crate::legacy_imports::LegacyImports,
    source_environment: crate::platform::git_trust::SourceEnvironment,
    helper_directory: Option<std::path::PathBuf>,
    selected: RwLock<Option<Selected>>,
}
impl Host {
    pub(crate) fn source_environment(&self) -> &crate::platform::git_trust::SourceEnvironment {
        &self.source_environment
    }
    pub fn storage_root(&self) -> &Path {
        self.stores.root()
    }
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_inner(root, None)
    }
    pub fn open_with_resources(root: &Path, resources: std::path::PathBuf) -> Result<Self> {
        Self::open_inner(root, Some(resources.join("resources/wsl")))
    }
    pub fn helper_directory(&self) -> Result<&Path> {
        self.helper_directory
            .as_deref()
            .ok_or("wsl_helper_unavailable")
    }
    fn open_inner(root: &Path, helper_directory: Option<std::path::PathBuf>) -> Result<Self> {
        let stores = Arc::new(StoreRoot::open(root)?);
        let selected = stores
            .read()?
            .map(|generation| Selected::open(&stores, generation))
            .transpose()?;
        Ok(Self {
            legacy: crate::legacy_imports::LegacyImports::new(stores.clone())?,
            stores,
            source_environment: crate::platform::git_trust::SourceEnvironment::capture(),
            helper_directory,
            selected: RwLock::new(selected),
        })
    }
    pub fn status(&self) -> Result<Value> {
        let selected = self.selected.try_read().map_err(|_| "store_owner_busy")?;
        Ok(
            json!({"selected": selected.is_some(), "generation": selected.as_ref().map(|s| &s.generation.id)}),
        )
    }
    pub fn start_empty(&self) -> Result<()> {
        let mut selected = self.selected.try_write().map_err(|_| "store_owner_busy")?;
        if selected.is_some() {
            return Err("store_already_active");
        }
        let generation = self.stores.prepare()?;
        let next = Selected::open(&self.stores, generation.clone())?;
        self.stores.activate_new(&generation)?;
        *selected = Some(next);
        Ok(())
    }
    pub fn projects(&self) -> Result<Arc<ProjectOwner>> {
        let selected = self.selected.try_read().map_err(|_| "store_owner_busy")?;
        let selected = selected.as_ref().ok_or("setup_required")?;
        if self.stores.read()?.as_ref() != Some(&selected.generation) {
            return Err("store_pointer_changed");
        }
        Ok(selected.projects.clone())
    }
    pub fn component(&self, name: &str) -> Result<std::path::PathBuf> {
        let selected = self.selected.try_read().map_err(|_| "store_owner_busy")?;
        let selected = selected.as_ref().ok_or("setup_required")?;
        if self.stores.read()?.as_ref() != Some(&selected.generation) {
            return Err("store_pointer_changed");
        }
        let component = selected.components.get(name).ok_or("unknown_component")?;
        let current = self.stores.component(&selected.generation, name)?;
        if current != component.path
            || devbox_filesystem::filesystem_identity(&current, true)
                .map_err(|_| "store_generation_changed")?
                != component.identity
        {
            return Err("store_generation_changed");
        }
        Ok(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_b04_generation_adds_runtime_stores_and_preserves_pointer_and_user_bytes() {
        let root = tempfile::tempdir().unwrap();
        let stores = StoreRoot::open(root.path()).unwrap();
        let generation = stores.prepare().unwrap();
        for name in ["runtime", "processes", "logs"] {
            std::fs::remove_dir(stores.component(&generation, name).unwrap()).unwrap();
        }
        let file = stores
            .component(&generation, "files")
            .unwrap()
            .join("untouched.txt");
        std::fs::write(&file, "unsaved fixture bytes").unwrap();
        stores.activate_new(&generation).unwrap();
        let pointer_path = root.path().join("active-stores.json");
        let pointer = std::fs::read(&pointer_path).unwrap();
        drop(stores);
        let host = Host::open(root.path()).unwrap();
        for name in ["runtime", "processes", "logs"] {
            let path = host.component(name).unwrap();
            assert!(path.is_dir());
            std::fs::write(path.join("preserved.txt"), name).unwrap();
        }
        drop(host);
        let host = Host::open(root.path()).unwrap();
        for name in ["runtime", "processes", "logs"] {
            assert_eq!(
                std::fs::read_to_string(host.component(name).unwrap().join("preserved.txt"))
                    .unwrap(),
                name
            );
        }
        assert_eq!(std::fs::read(&pointer_path).unwrap(), pointer);
        assert_eq!(
            std::fs::read_to_string(file).unwrap(),
            "unsaved fixture bytes"
        );
    }
    #[test]
    fn malformed_additional_component_blocks_open_without_replacing_data() {
        let root = tempfile::tempdir().unwrap();
        let host = Host::open(root.path()).unwrap();
        host.start_empty().unwrap();
        let runtime = host.component("runtime").unwrap();
        drop(host);
        std::fs::remove_dir(&runtime).unwrap();
        std::fs::write(&runtime, "do not replace").unwrap();
        assert!(Host::open(root.path()).is_err());
        assert_eq!(std::fs::read_to_string(runtime).unwrap(), "do not replace");
    }
    #[test]
    #[cfg(unix)]
    fn linked_additional_component_is_never_followed_or_provisioned() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let host = Host::open(root.path()).unwrap();
        host.start_empty().unwrap();
        let logs = host.component("logs").unwrap();
        drop(host);
        std::fs::remove_dir(&logs).unwrap();
        std::os::unix::fs::symlink(outside.path(), &logs).unwrap();
        assert!(Host::open(root.path()).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }
    #[test]
    fn concurrent_selected_store_readers_do_not_reject_each_other() {
        let root = tempfile::tempdir().unwrap();
        let host = Host::open(root.path()).unwrap();
        host.start_empty().unwrap();
        // Model the read lease held while another request validates native
        // generation objects. History detail and diff issue these together.
        let reader = host.selected.try_read().unwrap();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    assert!(host.projects().unwrap().snapshot().is_ok());
                    assert!(host.component("files").is_ok());
                    assert_eq!(host.status().unwrap()["selected"], true);
                    assert_eq!(host.start_empty().unwrap_err(), "store_owner_busy");
                })
                .join()
                .unwrap();
        });
        drop(reader);
        // The change does not relax activation or pointer/identity validation.
        assert_eq!(host.start_empty().unwrap_err(), "store_already_active");
    }
    #[test]
    fn startup_is_explicit_and_restart_retains_registry_owner() {
        let root = tempfile::tempdir().unwrap();
        let host = Host::open(root.path()).unwrap();
        assert_eq!(host.status().unwrap()["selected"], false);
        assert!(host.projects().is_err());
        host.start_empty().unwrap();
        assert_eq!(host.status().unwrap()["selected"], true);
        assert!(host
            .projects()
            .unwrap()
            .snapshot()
            .unwrap()
            .projects
            .is_empty());
        assert!(host.start_empty().is_err());
        let selected = host.status().unwrap();
        drop(host);
        let reopened = Host::open(root.path()).unwrap();
        assert_eq!(reopened.status().unwrap(), selected);
        assert!(reopened
            .projects()
            .unwrap()
            .snapshot()
            .unwrap()
            .projects
            .is_empty());
    }
}
