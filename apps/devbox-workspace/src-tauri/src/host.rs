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
        let mut components = HashMap::new();
        for name in ["registry", "overview", "files", "common"] {
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
    selected: RwLock<Option<Selected>>,
}
impl Host {
    pub fn storage_root(&self) -> &Path {
        self.stores.root()
    }
    pub fn open(root: &Path) -> Result<Self> {
        let stores = Arc::new(StoreRoot::open(root)?);
        let selected = stores
            .read()?
            .map(|generation| Selected::open(&stores, generation))
            .transpose()?;
        Ok(Self {
            stores,
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
