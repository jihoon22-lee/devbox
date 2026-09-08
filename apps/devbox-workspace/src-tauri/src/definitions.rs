//! Native project definition inspection and one-time execution-definition trust.
//! This does not start Git/tasks/LSP or resolve any secret reference.
use crate::{
    core::{
        manifest::{self, LocalOverlay, Manifest},
        registry::Registry,
    },
    host::Host,
    platform::project_files::ProjectFiles,
    private_metadata::MetadataRoot,
};
use product_contract::ProjectContext;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, &'static str>;
const MANIFEST: &str = ".devbox/project.json";
const OVERLAY: &str = "local-overlay.json";
const TTL: Duration = Duration::from_secs(180);
const MAX_PREVIEWS: usize = 4;
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionView {
    context: ProjectContext,
    registry_revision: u64,
    project: Manifest,
    local: LocalOverlay,
    effective: Manifest,
    sources: Vec<String>,
    unavailable_sources: Vec<String>,
    definitions_trusted: bool,
    has_approval: bool,
}
struct Snapshot {
    context: ProjectContext,
    registry_revision: u64,
    project: Manifest,
    local: LocalOverlay,
    effective: Manifest,
    sources: BTreeMap<String, String>,
    unavailable_sources: Vec<String>,
    digest: String,
    files: ProjectFiles,
    private: MetadataRoot,
    overlay_bytes: Option<Vec<u8>>,
}
impl Snapshot {
    fn capture(
        context: &ProjectContext,
        registry_revision: u64,
        mut files: ProjectFiles,
        private: MetadataRoot,
        deadline: u64,
    ) -> Result<Self> {
        crate::files_host::current_deadline(deadline)?;
        let project = files
            .read(MANIFEST, true)?
            .map(|bytes| Manifest::parse(&bytes))
            .transpose()?
            .unwrap_or_default();
        let overlay_bytes = private.read(OVERLAY)?;
        let local = overlay_bytes
            .as_ref()
            .map(|bytes| LocalOverlay::parse(bytes, context))
            .transpose()?
            .unwrap_or_else(|| LocalOverlay::empty(context.clone()));
        let effective = manifest::effective(&Manifest::default(), &project, &local, context)?;
        let paths: BTreeSet<_> = effective
            .tasks
            .values()
            .map(|task| task.source.clone())
            .chain(
                effective
                    .toolchains
                    .values()
                    .filter_map(|tool| tool.source.clone()),
            )
            .collect();
        let mut sources = BTreeMap::new();
        let mut unavailable_sources = Vec::new();
        for path in paths {
            crate::files_host::current_deadline(deadline)?;
            match files.read(&path, false) {
                Ok(Some(bytes)) => {
                    sources.insert(path, digest(&bytes));
                }
                _ => unavailable_sources.push(path),
            }
        }
        let digest = if unavailable_sources.is_empty() {
            effective.execution_digest(&sources)?
        } else {
            String::new()
        };
        let snapshot = Self {
            context: context.clone(),
            registry_revision,
            project,
            local,
            effective,
            sources,
            unavailable_sources,
            digest,
            files,
            private,
            overlay_bytes,
        };
        snapshot.revalidate()?;
        crate::files_host::current_deadline(deadline)?;
        Ok(snapshot)
    }
    fn revalidate(&self) -> Result<()> {
        self.files.revalidate()?;
        if self.private.read(OVERLAY)? != self.overlay_bytes {
            return Err("project_definition_changed");
        }
        Ok(())
    }
    fn view(&self, registry: &Registry) -> Result<DefinitionView> {
        Ok(DefinitionView {
            context: self.context.clone(),
            registry_revision: registry.revision,
            project: self.project.clone(),
            local: self.local.clone(),
            effective: self.effective.clone(),
            sources: self.sources.keys().cloned().collect(),
            unavailable_sources: self.unavailable_sources.clone(),
            definitions_trusted: registry.trusted(&self.context, &self.digest)?,
            has_approval: registry
                .worktrees
                .iter()
                .any(|tree| tree.id == self.context.worktree_id && tree.trusted_digest.is_some()),
        })
    }
}
struct Pending {
    snapshot: Snapshot,
    created: Instant,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustPreview {
    preview_id: String,
    definition: DefinitionView,
}
#[derive(Default)]
pub struct Definitions {
    data: Option<MetadataRoot>,
    views: Option<MetadataRoot>,
    pending: HashMap<String, Pending>,
}
impl Definitions {
    fn private(&mut self, host: &Host, context: &ProjectContext) -> Result<MetadataRoot> {
        let path = host.component("overview")?;
        if self.data.is_none() {
            self.data = Some(MetadataRoot::open(&path)?);
        }
        self.data
            .as_ref()
            .ok_or("invalid_overview_store")?
            .revalidate()?;
        if self.views.is_none() {
            self.views = Some(
                self.data
                    .as_ref()
                    .ok_or("invalid_overview_store")?
                    .child("definitions")?,
            );
        }
        self.views
            .as_ref()
            .ok_or("invalid_overview_store")?
            .child(&format!("worktree-{}", context.worktree_id))
    }
    fn snapshot(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<Snapshot> {
        let projects = host.projects()?;
        let registry = projects.snapshot()?;
        let lease = projects.admit(context)?;
        crate::files_host::current_deadline(deadline)?;
        let private = self.private(host, context)?;
        let snapshot = Snapshot::capture(
            context,
            registry.revision,
            ProjectFiles::new(lease)?,
            private,
            deadline,
        )?;
        if projects.snapshot()?.revision != registry.revision {
            return Err("stale_registry");
        }
        Ok(snapshot)
    }
    pub fn load(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<DefinitionView> {
        let snapshot = self.snapshot(host, context, deadline)?;
        snapshot.view(&host.projects()?.snapshot()?)
    }
    pub fn preview_trust(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<TrustPreview> {
        self.expire();
        if self.pending.len() >= MAX_PREVIEWS {
            return Err("project_preview_limit");
        }
        let snapshot = self.snapshot(host, context, deadline)?;
        if !snapshot.unavailable_sources.is_empty() {
            return Err("project_definition_unavailable");
        }
        let definition = snapshot.view(&host.projects()?.snapshot()?)?;
        let preview_id = uuid::Uuid::new_v4().to_string();
        self.pending.insert(
            preview_id.clone(),
            Pending {
                snapshot,
                created: Instant::now(),
            },
        );
        Ok(TrustPreview {
            preview_id,
            definition,
        })
    }
    pub fn approve_trust(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        id: &str,
        deadline: u64,
    ) -> Result<Registry> {
        let pending = self.pending.remove(id).ok_or("definition_preview_stale")?;
        if pending.created.elapsed() >= TTL || pending.snapshot.context != *context {
            return Err("definition_preview_stale");
        }
        let snapshot = pending.snapshot;
        let projects = host.projects()?;
        if projects.binding(context)? != *snapshot.files.lease().binding() {
            return Err("stale_context");
        }
        // This is a definitions digest only. Source/Git and LSP admission must
        // extend the native evidence before those engines are enabled.
        projects.review_definition_trust(
            snapshot.registry_revision,
            context,
            &snapshot.digest,
            || {
                snapshot.revalidate()?;
                crate::files_host::current_deadline(deadline)
            },
        )
    }
    pub fn revoke_trust(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        revision: u64,
    ) -> Result<Registry> {
        self.pending
            .retain(|_, pending| pending.snapshot.context != *context);
        host.projects()?.revoke_definition_trust(revision, context)
    }
    pub fn cancel(&mut self, id: &str) {
        self.pending.remove(id);
    }
    pub fn expire(&mut self) {
        self.pending
            .retain(|_, pending| pending.created.elapsed() < TTL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::project_probe::probe_fixture;
    use std::fs;
    fn snapshot(root: &std::path::Path, private: &std::path::Path) -> (Snapshot, Registry) {
        let lease = probe_fixture(root).unwrap();
        let mut registry = Registry::default();
        let context = registry
            .register(1, "project", lease.binding().clone())
            .unwrap();
        let snapshot = Snapshot::capture(
            &context,
            registry.revision,
            ProjectFiles::new(lease).unwrap(),
            MetadataRoot::open(private).unwrap(),
            u64::MAX,
        )
        .unwrap();
        (snapshot, registry)
    }
    #[test]
    fn execution_source_changes_revoke_the_definition_digest_without_executing_it() {
        let root = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".devbox")).unwrap();
        fs::write(root.path().join(".devbox/project.json"), br#"{"schemaVersion":1,"tasks":{"dev":{"kind":"package-script","source":"package.json","selector":"dev"}}}"#).unwrap();
        fs::write(
            root.path().join("package.json"),
            br#"{"scripts":{"dev":"never execute this source"}}"#,
        )
        .unwrap();
        let (snapshot, mut registry) = snapshot(root.path(), private.path());
        let original_context = snapshot.context.clone();
        registry
            .review_trust(registry.revision, &snapshot.context, &snapshot.digest)
            .unwrap();
        assert!(snapshot.view(&registry).unwrap().definitions_trusted);
        assert_eq!(registry.worktrees[0].context(), original_context);
        snapshot.local.validate(&original_context).unwrap();
        fs::write(
            root.path().join("package.json"),
            br#"{"scripts":{"dev":"changed command source"}}"#,
        )
        .unwrap();
        assert!(snapshot.revalidate().is_err());
        assert!(!root.path().join("never").exists());
    }
    #[test]
    fn future_manifest_and_overlay_changes_never_become_empty_trusted_definitions() {
        let root = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let (snapshot, _) = snapshot(root.path(), private.path());
        fs::write(private.path().join(OVERLAY), br#"{"schemaVersion":99}"#).unwrap();
        assert!(snapshot.revalidate().is_err());
        let lease = probe_fixture(root.path()).unwrap();
        assert!(Snapshot::capture(
            &snapshot.context,
            2,
            ProjectFiles::new(lease).unwrap(),
            MetadataRoot::open(private.path()).unwrap(),
            u64::MAX
        )
        .is_err());
        assert_eq!(
            fs::read(private.path().join(OVERLAY)).unwrap(),
            br#"{"schemaVersion":99}"#
        );
    }
}
