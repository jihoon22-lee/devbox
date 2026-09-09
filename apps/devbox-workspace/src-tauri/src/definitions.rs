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
use serde::{Deserialize, Serialize};
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
pub(crate) fn digest(bytes: &[u8]) -> String {
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
    edit_revision: String,
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
    defaults: Manifest,
    sources: BTreeMap<String, String>,
    unavailable_sources: Vec<String>,
    digest: String,
    files: ProjectFiles,
    private: MetadataRoot,
    overlay_bytes: Option<Vec<u8>>,
    project_bytes: Option<Vec<u8>>,
}
/// Native-only evidence retained by another execution owner. Source approval
/// can bind these inputs without granting task execution or changing Registry
/// definition approval. No source bytes are projected to the renderer here.
pub(crate) struct ExecutionDefinitions(Snapshot);
impl ExecutionDefinitions {
    pub(crate) fn digest(&self) -> &str {
        &self.0.digest
    }
    pub(crate) fn revalidate(&self) -> Result<()> {
        self.0.revalidate()
    }
}
impl Snapshot {
    fn capture(
        context: &ProjectContext,
        registry_revision: u64,
        mut files: ProjectFiles,
        private: MetadataRoot,
        defaults: Manifest,
        deadline: u64,
    ) -> Result<Self> {
        crate::files_host::current_deadline(deadline)?;
        let project_bytes = files.read(MANIFEST, true)?;
        let project = project_bytes
            .as_ref()
            .map(|bytes| Manifest::parse(bytes))
            .transpose()?
            .unwrap_or_default();
        let overlay_bytes = private.read(OVERLAY)?;
        let local = overlay_bytes
            .as_ref()
            .map(|bytes| LocalOverlay::parse(bytes, context))
            .transpose()?
            .unwrap_or_else(|| LocalOverlay::empty(context.clone()));
        let effective = manifest::effective(&defaults, &project, &local, context)?;
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
            defaults,
            sources,
            unavailable_sources,
            digest,
            files,
            private,
            overlay_bytes,
            project_bytes,
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
    fn edit_revision(&self) -> Result<String> {
        Ok(digest(
            &serde_json::to_vec(&(
                &self.context,
                self.registry_revision,
                &self.project_bytes,
                &self.overlay_bytes,
            ))
            .map_err(|_| "invalid_manifest")?,
        ))
    }
    fn view(&self, registry: &Registry) -> Result<DefinitionView> {
        Ok(DefinitionView {
            context: self.context.clone(),
            registry_revision: registry.revision,
            edit_revision: self.edit_revision()?,
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
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EditTarget {
    Project,
    Local,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditRequest {
    target: EditTarget,
    content: String,
    edit_revision: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditPreview {
    preview_id: String,
    target: EditTarget,
    before: serde_json::Value,
    after: serde_json::Value,
    effective_diff: manifest::DefinitionDiff,
}
struct PendingEdit {
    snapshot: Snapshot,
    target: crate::platform::definition_write::DefinitionTarget,
    bytes: Vec<u8>,
    created: Instant,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSaved {
    saved: bool,
    warning: Option<String>,
}
#[derive(Default)]
pub struct Definitions {
    data: Option<MetadataRoot>,
    views: Option<MetadataRoot>,
    pending: HashMap<String, Pending>,
    edits: HashMap<String, PendingEdit>,
}
impl Definitions {
    pub(crate) fn execution_evidence(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<ExecutionDefinitions> {
        let snapshot = self.snapshot(host, context, deadline)?;
        if !snapshot.unavailable_sources.is_empty() {
            return Err("project_definition_unavailable");
        }
        Ok(ExecutionDefinitions(snapshot))
    }
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
            Manifest {
                expected_ports: registry
                    .imported_profile_for(context)?
                    .map(|imported| imported.profile.expected_ports.clone()),
                ..Manifest::default()
            },
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
        if self.pending.len() + self.edits.len() >= MAX_PREVIEWS {
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
    pub fn preview_edit(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        request: EditRequest,
        deadline: u64,
    ) -> Result<EditPreview> {
        self.expire();
        if self.pending.len() + self.edits.len() >= MAX_PREVIEWS {
            return Err("project_preview_limit");
        }
        let snapshot = self.snapshot(host, context, deadline)?;
        if snapshot.edit_revision()? != request.edit_revision {
            return Err("project_definition_changed");
        }
        let (before, after, effective, bytes, path, expected) = match request.target {
            EditTarget::Project => {
                let next = Manifest::parse(request.content.as_bytes())?;
                let effective =
                    manifest::effective(&snapshot.defaults, &next, &snapshot.local, context)?;
                (
                    serde_json::to_value(&snapshot.project),
                    serde_json::to_value(&next),
                    effective,
                    next.encode()?,
                    std::path::Path::new(&snapshot.files.lease().binding().root).join(MANIFEST),
                    snapshot.project_bytes.as_deref(),
                )
            }
            EditTarget::Local => {
                let next = LocalOverlay::parse(request.content.as_bytes(), context)?;
                // Owner references may only be assigned by the corresponding
                // native owner. JSON editing cannot mint cross-owner authority.
                validate_local_edit(&snapshot.local, &next)?;
                let effective =
                    manifest::effective(&snapshot.defaults, &snapshot.project, &next, context)?;
                let bytes = serde_json::to_vec_pretty(&next).map_err(|_| "invalid_overlay")?;
                (
                    serde_json::to_value(&snapshot.local),
                    serde_json::to_value(&next),
                    effective,
                    bytes,
                    snapshot.private.path().join(OVERLAY),
                    snapshot.overlay_bytes.as_deref(),
                )
            }
        };
        if bytes.len() > 256 * 1024 {
            return Err("project_definition_limit");
        }
        let target = crate::platform::definition_write::DefinitionTarget::capture(&path, expected)?;
        snapshot.revalidate()?;
        crate::files_host::current_deadline(deadline)?;
        let preview_id = uuid::Uuid::new_v4().to_string();
        let preview = EditPreview {
            preview_id: preview_id.clone(),
            target: request.target,
            before: before.map_err(|_| "invalid_manifest")?,
            after: after.map_err(|_| "invalid_manifest")?,
            effective_diff: snapshot.effective.diff(&effective)?,
        };
        self.edits.insert(
            preview_id,
            PendingEdit {
                snapshot,
                target,
                bytes,
                created: Instant::now(),
            },
        );
        Ok(preview)
    }
    pub fn apply_edit(
        &mut self,
        host: &Host,
        context: &ProjectContext,
        id: &str,
        deadline: u64,
    ) -> Result<EditSaved> {
        let pending = self.edits.remove(id).ok_or("definition_preview_stale")?;
        if pending.created.elapsed() >= TTL || pending.snapshot.context != *context {
            return Err("definition_preview_stale");
        }
        let snapshot = pending.snapshot;
        let projects = host.projects()?;
        if projects.binding(context)? != *snapshot.files.lease().binding()
            || projects.snapshot()?.revision != snapshot.registry_revision
        {
            return Err("stale_context");
        }
        snapshot.revalidate()?;
        crate::files_host::current_deadline(deadline)?;
        // Revoke durably before file IO. If writing fails, the old definitions
        // remain available but require review; no partial write grants trust.
        projects.revoke_definition_trust(snapshot.registry_revision, context)?;
        self.pending
            .retain(|_, value| value.snapshot.context != *context);
        self.edits
            .retain(|_, value| value.snapshot.context != *context);
        let warning = pending.target.write(&pending.bytes, || {
            snapshot.revalidate()?;
            crate::files_host::current_deadline(deadline)
        })?;
        Ok(EditSaved {
            saved: true,
            warning,
        })
    }
    pub fn cancel(&mut self, id: &str) {
        self.pending.remove(id);
        self.edits.remove(id);
    }
    pub fn expire(&mut self) {
        self.edits
            .retain(|_, pending| pending.created.elapsed() < TTL);
        self.pending
            .retain(|_, pending| pending.created.elapsed() < TTL);
    }
}

fn validate_local_edit(before: &LocalOverlay, after: &LocalOverlay) -> Result<()> {
    if before.secrets != after.secrets || before.api_environment_id != after.api_environment_id {
        return Err("definition_owner_reference_required");
    }
    Ok(())
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
            Manifest::default(),
            u64::MAX,
        )
        .unwrap();
        (snapshot, registry)
    }
    #[test]
    fn imported_port_defaults_yield_to_project_and_explicit_empty_local_values() {
        let root = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let (original, _) = snapshot(root.path(), private.path());
        let capture = || {
            Snapshot::capture(
                &original.context,
                original.registry_revision,
                ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap(),
                MetadataRoot::open(private.path()).unwrap(),
                Manifest {
                    expected_ports: Some(vec![3000]),
                    ..Manifest::default()
                },
                u64::MAX,
            )
            .unwrap()
        };
        assert_eq!(capture().effective.expected_ports, Some(vec![3000]));
        fs::create_dir(root.path().join(".devbox")).unwrap();
        fs::write(
            root.path().join(MANIFEST),
            br#"{"schemaVersion":1,"expectedPorts":[8080]}"#,
        )
        .unwrap();
        assert_eq!(capture().effective.expected_ports, Some(vec![8080]));
        let mut local = LocalOverlay::empty(original.context.clone());
        local.expected_ports = Some(vec![]);
        fs::write(
            private.path().join(OVERLAY),
            serde_json::to_vec(&local).unwrap(),
        )
        .unwrap();
        assert_eq!(capture().effective.expected_ports, Some(vec![]));
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
            Manifest::default(),
            u64::MAX
        )
        .is_err());
        assert_eq!(
            fs::read(private.path().join(OVERLAY)).unwrap(),
            br#"{"schemaVersion":99}"#
        );
    }
    #[test]
    fn edit_revision_tracks_exact_bytes_and_owner_references_cannot_be_minted() {
        let root = tempfile::tempdir().unwrap();
        let private = tempfile::tempdir().unwrap();
        let (original, _) = snapshot(root.path(), private.path());
        let before = original.edit_revision().unwrap();
        let mut local = original.local.clone();
        local.api_environment_id = Some("unowned-environment".into());
        assert!(validate_local_edit(&original.local, &local).is_err());
        local.api_environment_id = None;
        local.expected_ports = Some(vec![8080]);
        assert!(validate_local_edit(&original.local, &local).is_ok());
        fs::write(
            private.path().join(OVERLAY),
            serde_json::to_vec(&local).unwrap(),
        )
        .unwrap();
        assert!(original.revalidate().is_err());
        let changed = Snapshot::capture(
            &original.context,
            original.registry_revision,
            ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap(),
            MetadataRoot::open(private.path()).unwrap(),
            Manifest::default(),
            u64::MAX,
        )
        .unwrap();
        assert_ne!(before, changed.edit_revision().unwrap());
    }
}
