//! Native Registry owner. Renderer requests carry one-time preview IDs, never
//! serialized filesystem evidence. Host authorization precedes these methods;
//! the host calls blocking methods only inside its bounded IO worker.
use crate::{
    core::{
        registry::{Binding, Discovery, Registry},
        registry_store::RegistryStore,
    },
    platform::project_probe::{probe_windows, ProjectLease},
};
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, &'static str>;
const PREVIEW_TTL: Duration = Duration::from_secs(180);
const MAX_PREVIEWS: usize = 8;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationPreview {
    pub preview_id: String,
    pub registry_revision: u64,
    pub binding: Binding,
    pub discovery: Discovery,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistrationAction {
    Register,
    Rebind,
}

struct Pending {
    lease: ProjectLease,
    revision: u64,
    discovery: Discovery,
    created: Instant,
}
pub struct ProjectOwner {
    store: RegistryStore,
    pending: Mutex<HashMap<String, Pending>>,
}
impl ProjectOwner {
    #[cfg(test)]
    pub(crate) fn preview_fixture(&self, root: &Path) -> Result<RegistrationPreview> {
        #[cfg(windows)]
        {
            self.preview_windows(root.to_str().ok_or("invalid_root")?)
        }
        #[cfg(not(windows))]
        {
            self.prepare(
                self.snapshot()?.revision,
                crate::platform::project_probe::probe_fixture(root)?,
            )
        }
    }
    pub fn open(generation: &Path) -> Result<Self> {
        Ok(Self {
            store: RegistryStore::open(generation)?,
            pending: Mutex::new(HashMap::new()),
        })
    }
    pub fn snapshot(&self) -> Result<Registry> {
        self.store.read()
    }
    /// Resolve the exact persisted context, then obtain fresh native evidence.
    /// A renderer-supplied root, target or object stamp cannot admit an operation.
    /// Registration metadata alone is insufficient after a root/Git replacement.
    pub fn admit(&self, context: &ProjectContext) -> Result<ProjectLease> {
        let binding = self.binding(context)?;
        if binding.target != product_contract::ExecutionTarget::Windows {
            return Err("wsl_admission_required");
        }
        self.admit_lease(context, probe_windows(&binding.root)?)
    }
    pub fn binding(&self, context: &ProjectContext) -> Result<Binding> {
        context.validate().map_err(|_| "invalid_context")?;
        self.store
            .read()?
            .worktrees
            .into_iter()
            .find(|tree| tree.context() == *context)
            .map(|tree| tree.binding)
            .ok_or("stale_context")
    }
    fn admit_lease(&self, context: &ProjectContext, lease: ProjectLease) -> Result<ProjectLease> {
        // Read again after the potentially slow probe. Rebind/removal changes
        // must not enter the previous context. Execution owners check trust separately.
        if self.binding(context)? != *lease.binding() {
            return Err("project_binding_changed");
        }
        lease.revalidate()?;
        Ok(lease)
    }
    /// Listing persisted metadata is separate from probing an external root.
    /// This function does not execute Git, start a distro, or grant trust.
    pub fn preview_windows(&self, root: &str) -> Result<RegistrationPreview> {
        let revision = self.store.read()?.revision;
        self.prepare(revision, probe_windows(root)?)
    }
    fn prepare(&self, revision: u64, lease: ProjectLease) -> Result<RegistrationPreview> {
        let registry = self.store.read()?;
        if revision != registry.revision {
            return Err("stale_registry");
        }
        lease.revalidate()?;
        let discovery = registry.discover(lease.binding())?;
        let preview_id = uuid::Uuid::new_v4().to_string();
        let preview = RegistrationPreview {
            preview_id: preview_id.clone(),
            registry_revision: revision,
            binding: lease.binding().clone(),
            discovery: discovery.clone(),
        };
        self.expire()?;
        let mut pending = self.pending.lock().map_err(|_| "registry_owner_busy")?;
        if pending.len() >= MAX_PREVIEWS {
            return Err("project_preview_limit");
        }
        pending.insert(
            preview_id,
            Pending {
                lease,
                revision,
                discovery,
                created: Instant::now(),
            },
        );
        Ok(preview)
    }
    pub fn cancel(&self, preview_id: &str) -> Result<()> {
        let removed = self
            .pending
            .lock()
            .map_err(|_| "registry_owner_busy")?
            .remove(preview_id);
        drop(removed); // Close any remote handles outside the metadata mutex.
        Ok(())
    }
    pub fn expire(&self) -> Result<()> {
        let expired = {
            let mut pending = self.pending.lock().map_err(|_| "registry_owner_busy")?;
            let keys = pending
                .iter()
                .filter(|(_, value)| value.created.elapsed() >= PREVIEW_TTL)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| pending.remove(&key))
                .collect::<Vec<_>>()
        };
        drop(expired);
        Ok(())
    }
    pub fn apply(
        &self,
        preview_id: &str,
        name: &str,
        action: RegistrationAction,
    ) -> Result<(Registry, ProjectContext)> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| "registry_owner_busy")?
            .remove(preview_id)
            .ok_or("project_preview_stale")?;
        if pending.created.elapsed() >= PREVIEW_TTL {
            return Err("project_preview_stale");
        }
        pending.lease.revalidate()?;
        self.store.update(pending.revision, |registry| {
            if registry.discover(pending.lease.binding())? != pending.discovery {
                return Err("project_preview_stale");
            }
            pending.lease.revalidate()?;
            let binding = pending.lease.binding().clone();
            let context = match (action, &pending.discovery) {
                (
                    RegistrationAction::Register,
                    Discovery::Known { .. }
                    | Discovery::NewProject
                    | Discovery::LinkedWorktree { .. },
                ) => registry.register(pending.revision, name, binding)?,
                (
                    RegistrationAction::Rebind,
                    Discovery::AliasOrMove { context } | Discovery::ReplacedRoot { context },
                ) => registry.rebind(pending.revision, context, binding)?,
                _ => return Err("project_review_action_mismatch"),
            };
            // No repository mutation occurs here; trust remains absent. Check
            // again before returning metadata to the store's final byte CAS.
            pending.lease.revalidate()?;
            Ok(context)
        })
    }
    pub fn rename(&self, revision: u64, project_id: &str, name: &str) -> Result<Registry> {
        self.store
            .update(revision, |registry| {
                registry.rename(revision, project_id, name)
            })
            .map(|(registry, ())| registry)
    }
    pub fn remove(&self, revision: u64, context: &ProjectContext) -> Result<Registry> {
        self.store
            .update(revision, |registry| registry.remove(revision, context))
            .map(|(registry, ())| registry)
    }
    pub(crate) fn review_definition_trust(
        &self,
        revision: u64,
        context: &ProjectContext,
        digest: &str,
        verify: impl FnOnce() -> Result<()>,
    ) -> Result<Registry> {
        self.store
            .update(revision, |registry| {
                verify()?;
                registry.review_trust(revision, context, digest)
            })
            .map(|(registry, _)| registry)
    }
    pub(crate) fn revoke_definition_trust(
        &self,
        revision: u64,
        context: &ProjectContext,
    ) -> Result<Registry> {
        self.store
            .update(revision, |registry| {
                registry.revoke_trust(revision, context)
            })
            .map(|(registry, ())| registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn preview_root(owner: &ProjectOwner, root: &Path) -> Result<RegistrationPreview> {
        #[cfg(windows)]
        {
            owner.preview_windows(root.to_str().unwrap())
        }
        #[cfg(unix)]
        {
            owner.prepare(
                owner.snapshot()?.revision,
                crate::platform::project_probe::probe_fixture(root)?,
            )
        }
    }
    #[test]
    fn registration_is_reviewed_one_time_and_rejects_replaced_root() {
        let generation = tempfile::tempdir().unwrap();
        let roots = tempfile::tempdir().unwrap();
        let root = roots.path().join("프로젝트 space");
        fs::create_dir(&root).unwrap();
        let owner = ProjectOwner::open(generation.path()).unwrap();
        let preview = preview_root(&owner, &root).unwrap();
        assert!(owner.snapshot().unwrap().projects.is_empty());
        owner.cancel(&preview.preview_id).unwrap();
        assert!(owner
            .apply(&preview.preview_id, "name", RegistrationAction::Register)
            .is_err());
        let preview = preview_root(&owner, &root).unwrap();
        fs::rename(&root, roots.path().join("old")).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(owner
            .apply(&preview.preview_id, "name", RegistrationAction::Register)
            .is_err());
        assert!(owner.snapshot().unwrap().projects.is_empty());
        let preview = preview_root(&owner, &root).unwrap();
        let (registry, context) = owner
            .apply(&preview.preview_id, "name", RegistrationAction::Register)
            .unwrap();
        assert_eq!(registry.worktrees[0].context(), context);
        assert_eq!(registry.worktrees[0].trusted_digest, None);
        assert!(owner
            .apply(&preview.preview_id, "again", RegistrationAction::Register)
            .is_err());
        drop(owner);
        assert_eq!(
            ProjectOwner::open(generation.path())
                .unwrap()
                .snapshot()
                .unwrap(),
            registry
        );
    }
    fn admitted(
        owner: &ProjectOwner,
        context: &ProjectContext,
        root: &Path,
    ) -> Result<ProjectLease> {
        #[cfg(windows)]
        {
            let _ = root;
            owner.admit(context)
        }
        #[cfg(unix)]
        {
            owner.admit_lease(
                context,
                crate::platform::project_probe::probe_fixture(root)?,
            )
        }
    }
    #[test]
    fn runtime_admission_rechecks_context_and_native_binding_after_registration() {
        let data = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("프로젝트 space");
        fs::create_dir(&root).unwrap();
        let owner = ProjectOwner::open(data.path()).unwrap();
        let preview = preview_root(&owner, &root).unwrap();
        let (registered, context) = owner
            .apply(&preview.preview_id, "name", RegistrationAction::Register)
            .unwrap();
        drop(admitted(&owner, &context, &root).unwrap());
        let mut other = context.clone();
        other.worktree_id = "unknown-worktree".into();
        assert!(admitted(&owner, &other, &root).is_err());
        other = context.clone();
        other.revision += 1;
        assert!(admitted(&owner, &other, &root).is_err());
        fs::rename(&root, parent.path().join("previous")).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(admitted(&owner, &context, &root).is_err());
        assert_eq!(owner.snapshot().unwrap(), registered);
        let preview = preview_root(&owner, &root).unwrap();
        let (rebound, next) = owner
            .apply(&preview.preview_id, "name", RegistrationAction::Rebind)
            .unwrap();
        assert_ne!(next.revision, context.revision);
        assert!(admitted(&owner, &context, &root).is_err());
        drop(admitted(&owner, &next, &root).unwrap());
        owner.remove(rebound.revision, &next).unwrap();
        assert!(admitted(&owner, &next, &root).is_err());
        assert!(root.is_dir());
    }
    #[test]
    fn review_checks_registry_revision_and_expiry_before_persistence() {
        let generation = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let owner = ProjectOwner::open(generation.path()).unwrap();
        let first = preview_root(&owner, root.path()).unwrap();
        let stale = preview_root(&owner, root.path()).unwrap();
        owner
            .apply(&first.preview_id, "first", RegistrationAction::Register)
            .unwrap();
        let before = owner.snapshot().unwrap();
        assert!(owner
            .apply(&stale.preview_id, "stale", RegistrationAction::Register)
            .is_err());
        assert_eq!(owner.snapshot().unwrap(), before);
        let expired = preview_root(&owner, root.path()).unwrap();
        owner
            .pending
            .lock()
            .unwrap()
            .get_mut(&expired.preview_id)
            .unwrap()
            .created = Instant::now() - PREVIEW_TTL;
        assert!(owner
            .apply(&expired.preview_id, "expired", RegistrationAction::Register)
            .is_err());
        assert_eq!(owner.snapshot().unwrap(), before);
    }
}
