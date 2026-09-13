//! B04/B07 native registry projection. No renderer command installs this state.
//! Registry aliases are metadata; search separately pins the actual root object.
use product_contract::ProjectContext;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

const INVALID: &str = "project_provider_invalid";
const MAX_BYTES: usize = 64 * 1024;
pub use product_contract::project_provider::{Availability, Project, Snapshot};
#[derive(Clone)]
pub struct Selection {
    pub project: Project,
    pub epoch: String,
    pub valid: Arc<AtomicBool>,
}
struct State {
    snapshot: Snapshot,
    epoch: String,
    valid: Arc<AtomicBool>,
}
#[derive(Default, Clone)]
pub struct Registry(Arc<Mutex<Option<State>>>);

fn path_identity(path: &str) -> Option<String> {
    devbox_filesystem::parse_safe_project_path(path).map(|p| p.identity().to_owned())
}
trait Validate {
    fn validate(&mut self) -> Result<(), &'static str>;
}
impl Validate for Snapshot {
    fn validate(&mut self) -> Result<(), &'static str> {
        if self.schema_version != 1
            || self.revision == 0
            || self.revision > 9_007_199_254_740_991
            || self.projects.len() > 256
        {
            return Err(INVALID);
        }
        for (i, project) in self.projects.iter().enumerate() {
            project.context.validate().map_err(|_| INVALID)?;
            if project.activity_paths.len() > 16
                || project
                    .activity_paths
                    .iter()
                    .any(|p| p.len() > 32768 || path_identity(p).is_none())
                || self.projects[..i].iter().any(|prior| {
                    prior.context.project_id == project.context.project_id
                        && prior.context.worktree_id == project.context.worktree_id
                        && prior.context.target == project.context.target
                })
            {
                return Err(INVALID);
            }
            let wsl = devbox_wsl::path::parse_wsl_unc_path(&project.root).map_err(|_| INVALID)?;
            if matches!(
                project.context.target,
                product_contract::ExecutionTarget::Wsl { .. }
            ) != wsl.is_some()
            {
                return Err(INVALID);
            }
        }
        for project in &mut self.projects {
            project.root = everything_plus_lib::component::normalize_import_root(&project.root)
                .map_err(|_| INVALID)?;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|current| !self.projects.iter().any(|p| &p.context == current))
        {
            return Err(INVALID);
        }
        Ok(())
    }
}
impl Registry {
    /// The authenticated registry owner supplies a complete bounded snapshot.
    /// A same-revision redelivery is idempotent only for identical content.
    pub fn replace(&self, bytes: &[u8]) -> Result<bool, &'static str> {
        if bytes.len() > MAX_BYTES {
            return Err(INVALID);
        }
        let mut snapshot: Snapshot = serde_json::from_slice(bytes).map_err(|_| INVALID)?;
        snapshot.validate()?;
        let mut state = self.0.lock().map_err(|_| "provider_unavailable")?;
        if let Some(previous) = state.as_ref() {
            if snapshot == previous.snapshot {
                return Ok(false);
            }
            if snapshot.revision <= previous.snapshot.revision {
                return Err("project_provider_stale");
            }
            previous.valid.store(false, Ordering::Release);
        }
        *state = Some(State {
            snapshot,
            epoch: uuid::Uuid::new_v4().to_string(),
            valid: Arc::new(AtomicBool::new(true)),
        });
        Ok(true)
    }
    /// Provider disconnect revokes current references. No filesystem work runs
    /// under this mutex, including when the disconnected root is remote.
    pub fn disconnect(&self) {
        if let Ok(mut state) = self.0.lock() {
            if let Some(state) = state.take() {
                state.valid.store(false, Ordering::Release);
            }
        }
    }
    pub fn current(&self) -> Option<Selection> {
        let state = self.0.lock().ok()?;
        let state = state.as_ref()?;
        let context = state.snapshot.current.as_ref()?;
        let project = state
            .snapshot
            .projects
            .iter()
            .find(|p| &p.context == context)?
            .clone();
        Some(Selection {
            project,
            epoch: state.epoch.clone(),
            valid: state.valid.clone(),
        })
    }
    pub fn matches(&self, epoch: &str) -> bool {
        self.current().is_some_and(|current| current.epoch == epoch)
    }
    pub fn association(&self, source_path: &str) -> serde_json::Value {
        use serde_json::json;
        let Ok(state) = self.0.lock() else {
            return json!({"state":"unavailable"});
        };
        let Some(state) = state.as_ref() else {
            return json!({"state":"unavailable"});
        };
        let Some(identity) = path_identity(source_path) else {
            return json!({"state":"unmapped"});
        };
        let matches: Vec<_> = state
            .snapshot
            .projects
            .iter()
            .filter(|p| {
                p.activity_paths
                    .iter()
                    .any(|alias| path_identity(alias).as_deref() == Some(&identity))
            })
            .take(2)
            .collect();
        match matches.as_slice() {
            [] => json!({"state":"unmapped"}),
            [project] => {
                json!({"state": match project.availability { Availability::Available=>"mapped",Availability::Offline=>"offline",Availability::Missing=>"missing",Availability::Unverified=>"unverified" }, "context":project.context})
            }
            _ => json!({"state":"ambiguous"}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Snapshot {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/project-provider-v1.json"
        ))
        .unwrap()
    }
    #[test]
    fn provider_revisions_cancel_old_selections_and_keep_unmapped_offline_rows_distinct() {
        let registry = Registry::default();
        assert_eq!(
            registry.association("C:/old/project")["state"],
            "unavailable"
        );
        let mut snapshot = fixture();
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        assert!(registry.replace(&bytes).unwrap());
        let old = registry.current().unwrap();
        assert!(!registry.replace(&bytes).unwrap());
        assert!(old.valid.load(Ordering::Acquire));
        assert_eq!(registry.association("c:\\OLD\\project")["state"], "mapped");
        assert_eq!(registry.association("C:/elsewhere")["state"], "unmapped");
        snapshot.projects[0].availability = Availability::Offline;
        assert_eq!(
            registry.replace(&serde_json::to_vec(&snapshot).unwrap()),
            Err("project_provider_stale")
        );
        snapshot.revision += 1;
        registry
            .replace(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        assert!(!old.valid.load(Ordering::Acquire));
        assert!(!registry.matches(&old.epoch));
        assert_eq!(registry.association("C:/old/project")["state"], "offline");
        let mut another = snapshot.projects[0].clone();
        another.context.worktree_id = "second".into();
        snapshot.projects.push(another);
        snapshot.revision += 1;
        registry
            .replace(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        assert_eq!(registry.association("C:/old/project")["state"], "ambiguous");
        registry.disconnect();
        assert!(registry.current().is_none());
    }
    #[test]
    fn unverified_associations_remain_metadata_and_cannot_advertise_available_roots() {
        let mut snapshot = fixture();
        snapshot.projects[0].availability = Availability::Unverified;
        let registry = Registry::default();
        registry
            .replace(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        assert_eq!(
            registry.association("C:/old/project")["state"],
            "unverified"
        );
        assert_ne!(
            registry.current().unwrap().project.availability,
            Availability::Available
        );
    }
    #[test]
    fn wsl_association_normalizes_the_unc_alias_without_folding_linux_case() {
        let registry = Registry::default();
        let mut snapshot = fixture();
        let project = &mut snapshot.projects[0];
        project.context.target = product_contract::ExecutionTarget::Wsl {
            distro_id: "registered-distro".into(),
        };
        project.root = "//wsl.localhost/Ubuntu/home/Project".into();
        project.activity_paths = vec!["//wsl$/Ubuntu/home/Project".into()];
        snapshot.current = Some(project.context.clone());
        registry
            .replace(&serde_json::to_vec(&snapshot).unwrap())
            .unwrap();
        assert_eq!(
            registry.current().unwrap().project.root,
            "//wsl$/ubuntu/home/Project"
        );
        assert_eq!(
            registry.association("//wsl.localhost/ubuntu/home/Project")["state"],
            "mapped"
        );
        assert_eq!(
            registry.association("//wsl$/Ubuntu/home/project")["state"],
            "unmapped"
        );
    }

    #[test]
    fn rejects_unknown_fields_root_escape_duplicate_contexts_and_unregistered_current() {
        let registry = Registry::default();
        let original = fixture();
        for mutate in 0..4 {
            let mut snapshot = original.clone();
            match mutate {
                0 => snapshot.projects[0].root = "C:/indexed/../elsewhere".into(),
                1 => snapshot.projects.push(snapshot.projects[0].clone()),
                2 => snapshot.current.as_mut().unwrap().revision += 1,
                _ => snapshot.schema_version = 99,
            }
            assert!(registry
                .replace(&serde_json::to_vec(&snapshot).unwrap())
                .is_err());
        }
        let mut value = serde_json::to_value(original).unwrap();
        value["command"] = "synthetic".into();
        assert!(registry
            .replace(&serde_json::to_vec(&value).unwrap())
            .is_err());
        assert!(registry.replace(&vec![b' '; MAX_BYTES + 1]).is_err());
        assert!(registry.current().is_none());
    }
}
