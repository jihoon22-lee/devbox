//! Native-selected store and Project Registry owner for the product host.
//! Call IO methods only from the dispatcher's bounded blocking workers.
use crate::{core::stores::StoreRoot, project_owner::ProjectOwner};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

type Result<T> = std::result::Result<T, &'static str>;
struct Selected {
    generation: crate::core::stores::Generation,
    projects: Arc<ProjectOwner>,
}
pub struct Host {
    stores: Arc<StoreRoot>,
    selected: Mutex<Option<Selected>>,
}
impl Host {
    pub fn open(root: &Path) -> Result<Self> {
        let stores = Arc::new(StoreRoot::open(root)?);
        let selected = stores
            .read()?
            .map(|generation| {
                let projects = Arc::new(ProjectOwner::open(
                    &stores.component(&generation, "registry")?,
                )?);
                Ok::<_, &'static str>(Selected {
                    generation,
                    projects,
                })
            })
            .transpose()?;
        Ok(Self {
            stores,
            selected: Mutex::new(selected),
        })
    }
    pub fn status(&self) -> Result<Value> {
        let selected = self.selected.try_lock().map_err(|_| "store_owner_busy")?;
        Ok(
            json!({"selected": selected.is_some(), "generation": selected.as_ref().map(|s| &s.generation.id)}),
        )
    }
    pub fn start_empty(&self) -> Result<()> {
        let mut selected = self.selected.try_lock().map_err(|_| "store_owner_busy")?;
        if selected.is_some() {
            return Err("store_already_active");
        }
        let generation = self.stores.prepare()?;
        let projects = Arc::new(ProjectOwner::open(
            &self.stores.component(&generation, "registry")?,
        )?);
        self.stores.activate_new(&generation)?;
        *selected = Some(Selected {
            generation,
            projects,
        });
        Ok(())
    }
    pub fn projects(&self) -> Result<Arc<ProjectOwner>> {
        let selected = self.selected.try_lock().map_err(|_| "store_owner_busy")?;
        let selected = selected.as_ref().ok_or("setup_required")?;
        if self.stores.read()?.as_ref() != Some(&selected.generation) {
            return Err("store_pointer_changed");
        }
        Ok(selected.projects.clone())
    }
    pub fn component(&self, name: &str) -> Result<std::path::PathBuf> {
        let selected = self.selected.try_lock().map_err(|_| "store_owner_busy")?;
        let selected = selected.as_ref().ok_or("setup_required")?;
        if self.stores.read()?.as_ref() != Some(&selected.generation) {
            return Err("store_pointer_changed");
        }
        self.stores.component(&selected.generation, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
