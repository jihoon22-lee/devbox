//! Registry metadata is owned by Workspace. Discovery proposes a binding; only
//! an explicit reviewed mutation changes IDs, aliases or trust. No operation
//! here executes a task, probes a distro, or edits a repository.
use std::collections::{BTreeMap, BTreeSet};

use product_contract::{ExecutionTarget, ProjectContext};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_ITEMS: usize = 512;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_REVISION: u64 = 9_007_199_254_740_991;
type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub id: String,
    pub name: String,
}

/// Native object evidence, never a path-derived ID. The platform adapter uses
/// volume/file-index (Windows) or selected distro binding + device/inode (WSL).
/// Stored evidence must be checked again before admitting a runtime operation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObjectStamp {
    pub scope: String,
    pub object: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub target: ExecutionTarget,
    /// Canonical Windows absolute path or WSL POSIX path, not a display alias.
    pub root: String,
    pub root_object: ObjectStamp,
    /// Common .git directory identity, shared by linked worktrees. None means
    /// an explicitly registered plain folder, not a failed Git observation.
    pub repository_object: Option<ObjectStamp>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Worktree {
    pub id: String,
    pub project_id: String,
    pub repo_id: Option<String>,
    pub revision: u64,
    pub binding: Binding,
    pub aliases: Vec<String>,
    /// Exact execution-definition digest reviewed for this binding revision.
    pub trusted_digest: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum LegacyOwner {
    Workbench,
    LifeLog,
    RepoManager,
    Terminal,
    Task,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LegacyReference {
    pub owner: LegacyOwner,
    pub old_id: String,
    pub worktree_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Registry {
    pub schema_version: u32,
    pub revision: u64,
    pub projects: Vec<Project>,
    pub worktrees: Vec<Worktree>,
    pub legacy_references: Vec<LegacyReference>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Discovery {
    Known { context: ProjectContext },
    AliasOrMove { context: ProjectContext },
    ReplacedRoot { context: ProjectContext },
    LinkedWorktree { project_id: String, repo_id: String },
    NewProject,
}

fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn bounded_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn stamp(value: &ObjectStamp) -> bool {
    [value.scope.as_str(), value.object.as_str()]
        .iter()
        .all(|s| {
            !s.is_empty()
                && s.len() <= 32
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
}
fn root_key(target: &ExecutionTarget, root: &str) -> Result<String> {
    let parsed = devbox_filesystem::parse_safe_project_path(root).ok_or("invalid_root")?;
    match target {
        ExecutionTarget::Windows
            if matches!(
                parsed.kind(),
                devbox_filesystem::ProjectPathKind::WindowsDrive
                    | devbox_filesystem::ProjectPathKind::WindowsUnc
            ) =>
        {
            if devbox_wsl::path::parse_wsl_unc_path(root)
                .map_err(|_| "invalid_root")?
                .is_some()
            {
                return Err("invalid_target");
            }
        }
        ExecutionTarget::Wsl { distro_id } if id(distro_id) && root.starts_with('/') => {}
        _ => return Err("invalid_target"),
    }
    Ok(parsed.identity().into())
}
impl Binding {
    pub fn validate(&self) -> Result<()> {
        root_key(&self.target, &self.root)?;
        if !stamp(&self.root_object) || self.repository_object.as_ref().is_some_and(|s| !stamp(s)) {
            return Err("invalid_object_evidence");
        }
        Ok(())
    }
    fn same_root(&self, other: &Self) -> bool {
        self.target == other.target && self.root_object == other.root_object
    }
    fn same_repo(&self, other: &Self) -> bool {
        self.target == other.target
            && self.repository_object.is_some()
            && self.repository_object == other.repository_object
    }
    fn same_path(&self, other: &Self) -> bool {
        self.target == other.target
            && root_key(&self.target, &self.root).ok() == root_key(&other.target, &other.root).ok()
    }
}
impl Worktree {
    pub fn context(&self) -> ProjectContext {
        ProjectContext {
            project_id: self.project_id.clone(),
            worktree_id: self.id.clone(),
            target: self.binding.target.clone(),
            revision: self.revision,
        }
    }
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: 1,
            projects: vec![],
            worktrees: vec![],
            legacy_references: vec![],
        }
    }
}
impl Registry {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BYTES {
            return Err("registry_limit");
        }
        let result: Self = serde_json::from_slice(bytes).map_err(|_| "invalid_registry")?;
        result.validate()?;
        Ok(result)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| "invalid_registry")?;
        if bytes.len() > MAX_BYTES {
            return Err("registry_limit");
        }
        Ok(bytes)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err("unsupported_registry_version");
        }
        if self.revision == 0 || self.revision > MAX_REVISION {
            return Err("invalid_revision");
        }
        if self.projects.len() > MAX_ITEMS
            || self.worktrees.len() > MAX_ITEMS
            || self.legacy_references.len() > MAX_ITEMS * 8
        {
            return Err("registry_limit");
        }
        let mut projects = BTreeSet::new();
        for project in &self.projects {
            if !id(&project.id)
                || !bounded_text(&project.name, 256)
                || !projects.insert(&project.id)
            {
                return Err("invalid_project");
            }
        }
        let mut worktrees = BTreeSet::new();
        let mut repo_owners = BTreeMap::new();
        for (index, tree) in self.worktrees.iter().enumerate() {
            tree.context().validate().map_err(|_| "invalid_context")?;
            tree.binding.validate()?;
            if !projects.contains(&tree.project_id)
                || !worktrees.insert(&tree.id)
                || tree.aliases.len() > 16
                || tree.revision > self.revision
                || tree.trusted_digest.as_ref().is_some_and(|d| !digest(d))
            {
                return Err("invalid_worktree");
            }
            if tree.repo_id.is_some() != tree.binding.repository_object.is_some()
                || tree.repo_id.as_ref().is_some_and(|r| !id(r))
            {
                return Err("invalid_repository");
            }
            if let Some(repo_id) = &tree.repo_id {
                if let Some(previous) = repo_owners.insert(repo_id, tree) {
                    if previous.project_id != tree.project_id
                        || !previous.binding.same_repo(&tree.binding)
                    {
                        return Err("repository_identity_conflict");
                    }
                }
            }
            let mut aliases = BTreeSet::new();
            for alias in &tree.aliases {
                if !aliases.insert(root_key(&tree.binding.target, alias)?) {
                    return Err("duplicate_alias");
                }
            }
            for other in &self.worktrees[..index] {
                if tree.binding.same_root(&other.binding) || tree.binding.same_path(&other.binding)
                {
                    return Err("duplicate_worktree");
                }
                if tree.binding.same_repo(&other.binding) && tree.repo_id != other.repo_id {
                    return Err("repository_identity_conflict");
                }
            }
        }
        let mut references = BTreeSet::new();
        for reference in &self.legacy_references {
            if !bounded_text(&reference.old_id, 512)
                || !worktrees.contains(&reference.worktree_id)
                || !references.insert((&reference.owner, &reference.old_id))
            {
                return Err("invalid_legacy_reference");
            }
        }
        Ok(())
    }
    pub fn discover(&self, observed: &Binding) -> Result<Discovery> {
        self.validate()?;
        observed.validate()?;
        if let Some(tree) = self
            .worktrees
            .iter()
            .find(|w| w.binding.same_root(observed))
        {
            if tree.binding.repository_object != observed.repository_object {
                return Ok(Discovery::ReplacedRoot {
                    context: tree.context(),
                });
            }
            return Ok(if tree.binding.same_path(observed) {
                Discovery::Known {
                    context: tree.context(),
                }
            } else {
                Discovery::AliasOrMove {
                    context: tree.context(),
                }
            });
        }
        if let Some(tree) = self
            .worktrees
            .iter()
            .find(|w| w.binding.same_path(observed))
        {
            return Ok(Discovery::ReplacedRoot {
                context: tree.context(),
            });
        }
        if let Some(tree) = self
            .worktrees
            .iter()
            .find(|w| w.binding.same_repo(observed))
        {
            return Ok(Discovery::LinkedWorktree {
                project_id: tree.project_id.clone(),
                repo_id: tree.repo_id.clone().ok_or("invalid_repository")?,
            });
        }
        Ok(Discovery::NewProject)
    }
    /// Admission is native-only: expected revision and the reviewed observation
    /// must be revalidated by the owner before persistence.
    pub fn register(
        &mut self,
        expected: u64,
        name: &str,
        observed: Binding,
    ) -> Result<ProjectContext> {
        self.check_revision(expected)?;
        if !bounded_text(name, 256) {
            return Err("invalid_project");
        }
        let proposal = self.discover(&observed)?;
        let (project_id, repo_id, project) = match proposal {
            Discovery::NewProject => {
                let project_id = Uuid::new_v4().to_string();
                (
                    project_id.clone(),
                    observed
                        .repository_object
                        .as_ref()
                        .map(|_| Uuid::new_v4().to_string()),
                    Some(Project {
                        id: project_id,
                        name: name.into(),
                    }),
                )
            }
            Discovery::LinkedWorktree {
                project_id,
                repo_id,
            } => (project_id, Some(repo_id), None),
            Discovery::Known { context } => return Ok(context),
            _ => return Err("binding_review_required"),
        };
        let mut next = self.clone();
        next.revision += 1;
        if let Some(project) = project {
            next.projects.push(project);
        }
        let tree = Worktree {
            id: Uuid::new_v4().to_string(),
            project_id,
            repo_id,
            revision: next.revision,
            binding: observed,
            aliases: vec![],
            trusted_digest: None,
        };
        let context = tree.context();
        next.worktrees.push(tree);
        next.validate()?;
        *self = next;
        Ok(context)
    }
    pub fn rebind(
        &mut self,
        expected: u64,
        context: &ProjectContext,
        observed: Binding,
    ) -> Result<ProjectContext> {
        self.check_revision(expected)?;
        observed.validate()?;
        let index = self.context_index(context)?;
        // Moving an existing project to a different repo is an explicit new
        // registration decision, not a silent reassignment of old references.
        if self.worktrees[index].binding.repository_object != observed.repository_object
            || self.worktrees[index].binding.target != observed.target
        {
            return Err("repository_reassignment_required");
        }
        let mut next = self.clone();
        next.revision += 1;
        let tree = &mut next.worktrees[index];
        if !tree.binding.same_path(&observed) {
            if tree.aliases.len() == 16 {
                return Err("alias_limit");
            }
            let previous = tree.binding.root.clone();
            if !tree.aliases.iter().any(|alias| {
                root_key(&tree.binding.target, alias).ok()
                    == root_key(&tree.binding.target, &previous).ok()
            }) {
                tree.aliases.push(previous);
            }
        }
        tree.binding = observed;
        tree.revision = next.revision;
        tree.trusted_digest = None;
        let context = tree.context();
        next.validate()?;
        *self = next;
        Ok(context)
    }
    pub fn rename(&mut self, expected: u64, project_id: &str, name: &str) -> Result<()> {
        self.check_revision(expected)?;
        if !bounded_text(name, 256) {
            return Err("invalid_project");
        }
        let project = self
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .ok_or("unknown_project")?;
        project.name = name.into();
        self.revision += 1;
        Ok(())
    }
    pub fn remove(&mut self, expected: u64, context: &ProjectContext) -> Result<()> {
        self.check_revision(expected)?;
        let index = self.context_index(context)?;
        // References are not cascaded away without an explicit reviewed unlink.
        if self
            .legacy_references
            .iter()
            .any(|r| r.worktree_id == context.worktree_id)
        {
            return Err("referenced_worktree");
        }
        self.worktrees.remove(index);
        if !self
            .worktrees
            .iter()
            .any(|w| w.project_id == context.project_id)
        {
            self.projects.retain(|p| p.id != context.project_id);
        }
        self.revision += 1;
        Ok(())
    }
    pub fn map_legacy(&mut self, expected: u64, reference: LegacyReference) -> Result<()> {
        self.check_revision(expected)?;
        if let Some(current) = self
            .legacy_references
            .iter()
            .find(|r| r.owner == reference.owner && r.old_id == reference.old_id)
        {
            return if current == &reference {
                Ok(())
            } else {
                Err("legacy_mapping_conflict")
            };
        }
        let mut next = self.clone();
        next.revision += 1;
        next.legacy_references.push(reference);
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn unlink_legacy(&mut self, expected: u64, reference: &LegacyReference) -> Result<()> {
        self.check_revision(expected)?;
        let index = self
            .legacy_references
            .iter()
            .position(|r| r == reference)
            .ok_or("legacy_mapping_conflict")?;
        self.legacy_references.remove(index);
        self.revision += 1;
        Ok(())
    }
    pub fn review_trust(
        &mut self,
        expected: u64,
        context: &ProjectContext,
        definition_digest: &str,
    ) -> Result<ProjectContext> {
        self.check_revision(expected)?;
        if !digest(definition_digest) {
            return Err("invalid_definition_digest");
        }
        let index = self.context_index(context)?;
        self.revision += 1;
        let tree = &mut self.worktrees[index];
        // Approval has its own digest and Registry CAS revision. It does not
        // rebind the worktree or invalidate its local overlay/editor context.
        tree.trusted_digest = Some(definition_digest.into());
        Ok(tree.context())
    }
    pub fn revoke_trust(&mut self, expected: u64, context: &ProjectContext) -> Result<()> {
        self.check_revision(expected)?;
        let index = self.context_index(context)?;
        self.worktrees[index].trusted_digest = None;
        self.revision += 1;
        Ok(())
    }
    pub fn trusted(&self, context: &ProjectContext, current_digest: &str) -> Result<bool> {
        let index = self.context_index(context)?;
        Ok(digest(current_digest)
            && self.worktrees[index].trusted_digest.as_deref() == Some(current_digest))
    }
    fn context_index(&self, context: &ProjectContext) -> Result<usize> {
        self.validate()?;
        self.worktrees
            .iter()
            .position(|w| w.context() == *context)
            .ok_or("stale_context")
    }
    fn check_revision(&self, expected: u64) -> Result<()> {
        self.validate()?;
        if self.revision != expected {
            return Err("stale_registry");
        }
        if self.revision == MAX_REVISION {
            return Err("revision_limit");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding(root: &str, object: &str, repo: Option<&str>) -> Binding {
        Binding {
            target: ExecutionTarget::Windows,
            root: root.into(),
            root_object: ObjectStamp {
                scope: "1".into(),
                object: object.into(),
            },
            repository_object: repo.map(|r| ObjectStamp {
                scope: "1".into(),
                object: r.into(),
            }),
        }
    }
    #[test]
    fn discovery_preserves_ids_across_aliases_and_separates_linked_worktrees() {
        let mut registry = Registry::default();
        let original = binding(r"C:\개발 폴더\main", "11", Some("ff"));
        let first = registry.register(1, "project", original.clone()).unwrap();
        assert_ne!(first.project_id, original.root);
        let before = registry.clone();
        let alias = binding("c:/개발 폴더/MAIN", "11", Some("ff"));
        assert_eq!(
            registry.discover(&alias).unwrap(),
            Discovery::Known {
                context: first.clone()
            }
        );
        assert_eq!(registry.register(2, "ignored", alias).unwrap(), first);
        assert_eq!(registry, before);
        let second = registry
            .register(
                2,
                "linked",
                binding(r"C:\개발 폴더\branch", "12", Some("ff")),
            )
            .unwrap();
        assert_eq!(first.project_id, second.project_id);
        assert_ne!(first.worktree_id, second.worktree_id);
        assert_eq!(registry.worktrees[0].repo_id, registry.worktrees[1].repo_id);
        assert_eq!(registry.projects.len(), 1);
        registry.validate().unwrap();
    }
    #[test]
    fn rebind_requires_review_invalidates_trust_and_preserves_legacy_mapping() {
        let mut registry = Registry::default();
        let original = binding(r"C:\repo\main", "11", Some("ff"));
        let first = registry.register(1, "project", original.clone()).unwrap();
        let first = registry.review_trust(2, &first, &"a".repeat(64)).unwrap();
        assert!(registry.trusted(&first, &"a".repeat(64)).unwrap());
        assert!(!registry.trusted(&first, &"b".repeat(64)).unwrap());
        let reference = LegacyReference {
            owner: LegacyOwner::Workbench,
            old_id: "legacy-1".into(),
            worktree_id: first.worktree_id.clone(),
        };
        registry.map_legacy(3, reference.clone()).unwrap();
        let moved = binding(r"D:\renamed\main", "11", Some("ff"));
        assert_eq!(
            registry.discover(&moved).unwrap(),
            Discovery::AliasOrMove {
                context: first.clone()
            }
        );
        let before = registry.clone();
        assert_eq!(
            registry.register(4, "duplicate", moved.clone()),
            Err("binding_review_required")
        );
        assert_eq!(registry, before);
        let next = registry.rebind(4, &first, moved).unwrap();
        assert_eq!(next.worktree_id, first.worktree_id);
        assert_eq!(registry.legacy_references, [reference]);
        assert_eq!(registry.worktrees[0].aliases, [original.root]);
        assert!(!registry.trusted(&next, &"a".repeat(64)).unwrap());
        assert_eq!(
            registry.trusted(&first, &"a".repeat(64)),
            Err("stale_context")
        );
        assert_eq!(registry.remove(5, &next), Err("referenced_worktree"));
    }
    #[test]
    fn reviewed_unlink_and_remove_touch_only_registry_metadata() {
        let mut registry = Registry::default();
        let context = registry
            .register(1, "project", binding(r"C:\repo", "11", None))
            .unwrap();
        let reference = LegacyReference {
            owner: LegacyOwner::Terminal,
            old_id: "terminal-a".into(),
            worktree_id: context.worktree_id.clone(),
        };
        registry.map_legacy(2, reference.clone()).unwrap();
        assert_eq!(registry.remove(3, &context), Err("referenced_worktree"));
        let mut incorrect = reference.clone();
        incorrect.old_id = "terminal-b".into();
        assert_eq!(
            registry.unlink_legacy(3, &incorrect),
            Err("legacy_mapping_conflict")
        );
        registry.unlink_legacy(3, &reference).unwrap();
        registry.remove(4, &context).unwrap();
        assert!(
            registry.projects.is_empty()
                && registry.worktrees.is_empty()
                && registry.legacy_references.is_empty()
        );
        assert_eq!(registry.revision, 5);
    }
    #[test]
    fn replaced_root_and_changed_git_metadata_never_become_known() {
        let mut registry = Registry::default();
        let first = registry
            .register(1, "project", binding(r"C:\repo\main", "11", Some("ff")))
            .unwrap();
        for observed in [
            binding(r"C:\repo\main", "12", Some("ff")),
            binding(r"C:\repo\main", "11", Some("ee")),
        ] {
            assert_eq!(
                registry.discover(&observed).unwrap(),
                Discovery::ReplacedRoot {
                    context: first.clone()
                }
            );
            assert_eq!(
                registry.register(2, "duplicate", observed),
                Err("binding_review_required")
            );
        }
        let before = registry.clone();
        assert_eq!(
            registry.rebind(2, &first, binding(r"D:\repo\main", "21", Some("ee"))),
            Err("repository_reassignment_required")
        );
        assert_eq!(registry, before);
    }
    #[test]
    fn wsl_target_and_case_are_distinct_from_windows_and_other_distros() {
        let mut registry = Registry::default();
        let mut first = binding("/home/user/Repo", "11", Some("ff"));
        first.target = ExecutionTarget::Wsl {
            distro_id: "distro-a".into(),
        };
        let a = registry.register(1, "A", first.clone()).unwrap();
        let mut other_distro = first.clone();
        other_distro.target = ExecutionTarget::Wsl {
            distro_id: "distro-b".into(),
        };
        let b = registry.register(2, "B", other_distro).unwrap();
        assert_ne!(a.project_id, b.project_id);
        let mut lower = first.clone();
        lower.root = "/home/user/repo".into();
        lower.root_object.object = "12".into();
        assert!(matches!(
            registry.discover(&lower).unwrap(),
            Discovery::LinkedWorktree { .. }
        ));
        assert!(
            binding(r"\\wsl.localhost\Ubuntu\home\user\Repo", "22", None)
                .validate()
                .is_err()
        );
        assert!(binding("/home/user/Repo", "22", None).validate().is_err());
        first.root = r"C:\repo".into();
        assert!(first.validate().is_err());
    }
    #[test]
    fn future_corrupt_duplicate_and_unknown_fields_fail_without_empty_fallback() {
        let mut registry = Registry::default();
        registry
            .register(1, "project", binding(r"C:\repo", "11", None))
            .unwrap();
        let bytes = registry.encode().unwrap();
        assert_eq!(Registry::parse(&bytes).unwrap(), registry);
        for change in ["future", "unknown", "duplicate", "bad-reference"] {
            let mut value = serde_json::to_value(&registry).unwrap();
            match change {
                "future" => value["schemaVersion"] = 2.into(),
                "unknown" => value["projects"][0]["command"] = "run".into(),
                "duplicate" => {
                    let tree = value["worktrees"][0].clone();
                    value["worktrees"].as_array_mut().unwrap().push(tree);
                }
                _ => value["worktrees"][0]["projectId"] = "missing".into(),
            }
            assert!(
                Registry::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
                "{change}"
            );
        }
        assert!(Registry::parse(b"{broken").is_err());
        assert!(Registry::parse(&vec![b' '; MAX_BYTES + 1]).is_err());
    }
    #[test]
    fn stale_mutations_and_conflicting_imports_are_atomic() {
        let mut registry = Registry::default();
        let first = registry
            .register(1, "project", binding(r"C:\repo", "11", None))
            .unwrap();
        let second = registry
            .register(2, "other", binding(r"C:\other", "12", None))
            .unwrap();
        let reference = LegacyReference {
            owner: LegacyOwner::LifeLog,
            old_id: "old-project".into(),
            worktree_id: first.worktree_id.clone(),
        };
        registry.map_legacy(3, reference.clone()).unwrap();
        registry.map_legacy(4, reference.clone()).unwrap();
        let before = registry.clone();
        assert_eq!(
            registry.rename(3, &first.project_id, "stale"),
            Err("stale_registry")
        );
        assert_eq!(
            registry.map_legacy(
                4,
                LegacyReference {
                    worktree_id: second.worktree_id,
                    ..reference
                }
            ),
            Err("legacy_mapping_conflict")
        );
        assert_eq!(registry, before);
    }
}
