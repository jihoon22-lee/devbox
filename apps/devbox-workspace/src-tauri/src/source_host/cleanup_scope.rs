//! Explicit sibling admission for cleanup only. Registry enumeration is metadata-only.
use super::*;
use crate::{
    core::registry::{Binding, Registry, Worktree},
    files_host::FilesHost,
};
use std::{collections::HashSet, sync::Mutex};

const FILE: &str = "git-cleanup-scope.json";
const MAX_MEMBERS: usize = 8;
const MAX_BYTES: usize = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Approval {
    schema_version: u32,
    context: ProjectContext,
    root_digest: String,
    members: Vec<ProjectContext>,
    digest: String,
}
fn decode(bytes: Option<&[u8]>, context: &ProjectContext) -> Result<Option<Approval>> {
    let Some(bytes) = bytes else { return Ok(None) };
    if bytes.len() > 16384 {
        return Err("source_cleanup_scope_invalid");
    }
    let value: Option<Approval> =
        serde_json::from_slice(bytes).map_err(|_| "source_cleanup_scope_invalid")?;
    if let Some(value) = &value {
        let digest_valid =
            |digest: &str| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit());
        let mut seen = HashSet::new();
        if value.schema_version != 1
            || value.context != *context
            || !digest_valid(&value.root_digest)
            || !digest_valid(&value.digest)
            || value.members.is_empty()
            || value.members.len() > MAX_MEMBERS
            || value.members.iter().any(|member| {
                member.validate().is_err() || member == context || !seen.insert(&member.worktree_id)
            })
        {
            return Err("source_cleanup_scope_invalid");
        }
    }
    Ok(value)
}
fn available<'a>(registry: &'a Registry, context: &ProjectContext) -> Result<Vec<&'a Worktree>> {
    let root = registry
        .worktrees
        .iter()
        .find(|tree| tree.context() == *context)
        .ok_or("stale_context")?;
    Ok(registry
        .worktrees
        .iter()
        .filter(|tree| {
            tree.id != root.id
                && root.repo_id.is_some()
                && tree.repo_id == root.repo_id
                && tree.project_id == root.project_id
                && tree.binding.target == root.binding.target
                && tree.binding.repository_object == root.binding.repository_object
        })
        .collect())
}
fn selected(
    registry: &Registry,
    context: &ProjectContext,
    ids: &[String],
) -> Result<Vec<ProjectContext>> {
    if ids.is_empty() || ids.len() > MAX_MEMBERS {
        return Err("source_cleanup_scope_invalid");
    }
    let choices = available(registry, context)?;
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            return Err("source_cleanup_scope_invalid");
        }
        result.push(
            choices
                .iter()
                .find(|tree| tree.id == *id)
                .ok_or("source_cleanup_scope_invalid")?
                .context(),
        );
    }
    result.sort_by(|a, b| a.worktree_id.cmp(&b.worktree_id));
    Ok(result)
}
struct Member {
    context: ProjectContext,
    binding: Binding,
    git: GitTrust,
    definitions: ExecutionDefinitions,
}
impl Member {
    fn capture(
        host: &Host,
        definitions: &mut Definitions,
        context: &ProjectContext,
        root: &Snapshot,
        budget: Budget,
    ) -> Result<Self> {
        budget.check()?;
        let lease = host.projects()?.admit(context)?;
        let binding = lease.binding().clone();
        if binding.repository_object != root.binding.repository_object
            || binding.target != root.binding.target
        {
            return Err("source_cleanup_scope_changed");
        }
        let environment =
            GitEnvironment::native(std::path::Path::new(&binding.root), budget.deadline_ms)?;
        if environment.program != root.git.environment.program
            || environment.environment != root.git.environment.environment
        {
            return Err("source_cleanup_scope_changed");
        }
        let git = GitTrust::capture(lease, environment, budget.deadline_ms)?;
        let definitions = definitions.execution_evidence(host, context, budget.deadline_ms)?;
        let member = Self {
            context: context.clone(),
            binding,
            git,
            definitions,
        };
        member.revalidate(host, budget)?;
        Ok(member)
    }
    fn revalidate_metadata(&self, host: &Host, budget: Budget) -> Result<()> {
        budget.check()?;
        if host.projects()?.binding(&self.context)? != self.binding {
            return Err("source_cleanup_scope_changed");
        }
        self.definitions.revalidate()?;
        budget.check()
    }
    fn revalidate(&self, host: &Host, budget: Budget) -> Result<()> {
        self.revalidate_metadata(host, budget)?;
        self.git.revalidate(budget.deadline_ms)?;
        budget.check()
    }
}
struct Evidence {
    members: Vec<Member>,
    digest: String,
}
impl Evidence {
    fn capture(
        host: &Host,
        definitions: &mut Definitions,
        root: &Snapshot,
        contexts: &[ProjectContext],
        budget: Budget,
    ) -> Result<Self> {
        // Validate the whole requested set from native metadata before probing any sibling.
        let registry = host.projects()?.snapshot()?;
        let chosen = selected(
            &registry,
            &root.context,
            &contexts
                .iter()
                .map(|c| c.worktree_id.clone())
                .collect::<Vec<_>>(),
        )?;
        if chosen != contexts {
            return Err("source_cleanup_scope_changed");
        }
        let mut total = root.git.bytes();
        let mut members = Vec::new();
        for context in contexts {
            let member = Member::capture(host, definitions, context, root, budget)?;
            total = total
                .checked_add(member.git.bytes())
                .ok_or("git_source_limit")?;
            if total > MAX_BYTES {
                return Err("git_source_limit");
            }
            members.push(member);
        }
        let digest = definitions::digest(
            &serde_json::to_vec(&(
                &root.digest,
                members
                    .iter()
                    .map(|member| {
                        (
                            &member.context,
                            member.git.digest(),
                            member.definitions.digest(),
                        )
                    })
                    .collect::<Vec<_>>(),
            ))
            .map_err(|_| "source_cleanup_scope_invalid")?,
        );
        Ok(Self { members, digest })
    }
    fn revalidate(&self, host: &Host, budget: Budget) -> Result<()> {
        for member in &self.members {
            member.revalidate(host, budget)?;
        }
        Ok(())
    }
}
struct Pending {
    root: Snapshot,
    evidence: Evidence,
    before: Option<Vec<u8>>,
    created: Instant,
}
#[derive(Default)]
pub(super) struct Owner {
    pending: HashMap<String, Pending>,
}
impl Owner {
    pub(super) fn expire(&mut self) {
        self.pending.retain(|_, p| p.created.elapsed() < TTL);
    }
    pub(super) fn len(&self) -> usize {
        self.pending.len()
    }
    pub(super) fn manage(
        &mut self,
        host: &Host,
        definitions: &mut Definitions,
        context: &ProjectContext,
        method: &str,
        args: Value,
        budget: Budget,
    ) -> Result<Value> {
        budget.check()?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Id {
            preview_id: String,
        }
        match method {
            "cancel_cleanup_scope" => {
                let input: Id = serde_json::from_value(args).map_err(|_| "invalid_request")?;
                if self
                    .pending
                    .get(&input.preview_id)
                    .is_some_and(|p| p.root.context == *context)
                {
                    self.pending.remove(&input.preview_id);
                }
                Ok(Value::Null)
            }
            "cleanup_scope_status" | "revoke_cleanup_scope" => {
                if !args.as_object().is_some_and(|o| o.is_empty()) {
                    return Err("invalid_request");
                }
                let private = private(host, context)?;
                let before = private.read(FILE)?;
                let approval = decode(before.as_deref(), context)?;
                if method == "cleanup_scope_status" {
                    let registry = host.projects()?.snapshot()?;
                    let choices = available(&registry, context)?
                        .into_iter()
                        .map(|tree| json!({"id":tree.id,"root":tree.binding.root}))
                        .collect::<Vec<_>>();
                    return Ok(
                        json!({"available":choices,"hasApproval":approval.is_some(),"selectedIds":approval.map(|a| a.members.into_iter().map(|c| c.worktree_id).collect::<Vec<_>>()).unwrap_or_default()}),
                    );
                }
                let binding = host.projects()?.binding(context)?;
                if before.is_some() {
                    DefinitionTarget::capture(&private.path().join(FILE), before.as_deref())?
                        .write(b"null", || {
                            budget.check()?;
                            if host.projects()?.binding(context)? != binding {
                                return Err("source_context_changed");
                            }
                            private.revalidate()
                        })?;
                }
                self.pending.retain(|_, p| p.root.context != *context);
                Ok(json!({"hasApproval":false}))
            }
            "preview_cleanup_scope" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    worktree_ids: Vec<String>,
                }
                let input: Input = serde_json::from_value(args).map_err(|_| "invalid_request")?;
                let contexts =
                    selected(&host.projects()?.snapshot()?, context, &input.worktree_ids)?;
                let root = Snapshot::capture(host, definitions, context, budget.deadline_ms)?;
                if !root.approved()? {
                    return Err("source_review_required");
                }
                let before = root.private.read(FILE)?;
                decode(before.as_deref(), context)?;
                let evidence = Evidence::capture(host, definitions, &root, &contexts, budget)?;
                root.revalidate(host, budget.deadline_ms)?;
                budget.check()?;
                let id = uuid::Uuid::new_v4().to_string();
                let view = json!({"previewId":id,"members":evidence.members.iter().map(|m| json!({"id":m.context.worktree_id,"root":m.binding.root,"review":m.git.review})).collect::<Vec<_>>()});
                self.pending.insert(
                    id,
                    Pending {
                        root,
                        evidence,
                        before,
                        created: Instant::now(),
                    },
                );
                Ok(view)
            }
            "approve_cleanup_scope" => {
                let input: Id = serde_json::from_value(args).map_err(|_| "invalid_request")?;
                let pending = self
                    .pending
                    .remove(&input.preview_id)
                    .ok_or("source_preview_stale")?;
                if pending.root.context != *context || pending.created.elapsed() >= TTL {
                    return Err("source_preview_stale");
                }
                let validate = || {
                    budget.check()?;
                    pending.root.revalidate(host, budget.deadline_ms)?;
                    if !pending.root.approved()? {
                        return Err("source_review_required");
                    }
                    pending.evidence.revalidate(host, budget)
                };
                validate()?;
                if pending.root.private.read(FILE)? != pending.before {
                    return Err("source_cleanup_scope_changed");
                }
                let value = Approval {
                    schema_version: 1,
                    context: context.clone(),
                    root_digest: pending.root.digest.clone(),
                    members: pending
                        .evidence
                        .members
                        .iter()
                        .map(|m| m.context.clone())
                        .collect(),
                    digest: pending.evidence.digest.clone(),
                };
                let bytes =
                    serde_json::to_vec(&Some(value)).map_err(|_| "source_cleanup_scope_invalid")?;
                DefinitionTarget::capture(
                    &pending.root.private.path().join(FILE),
                    pending.before.as_deref(),
                )?
                .write(&bytes, validate)?;
                Ok(json!({"hasApproval":true}))
            }
            _ => Err("invalid_request"),
        }
    }
}
pub(super) fn management(method: &str) -> bool {
    matches!(
        method,
        "cleanup_scope_status"
            | "preview_cleanup_scope"
            | "approve_cleanup_scope"
            | "cancel_cleanup_scope"
            | "revoke_cleanup_scope"
    )
}
pub(super) struct Execution {
    evidence: Evidence,
    private: MetadataRoot,
    bytes: Vec<u8>,
    files: Arc<Mutex<FilesHost>>,
}
impl Execution {
    pub(super) fn capture(
        host: &Host,
        definitions: &mut Definitions,
        root: &Snapshot,
        files: Arc<Mutex<FilesHost>>,
        budget: Budget,
    ) -> Result<Option<Self>> {
        let private = private(host, &root.context)?;
        let bytes = private.read(FILE)?;
        let Some(approval) = decode(bytes.as_deref(), &root.context)? else {
            return Ok(None);
        };
        if approval.root_digest != root.digest {
            return Err("source_cleanup_scope_changed");
        }
        let evidence = Evidence::capture(host, definitions, root, &approval.members, budget)?;
        if evidence.digest != approval.digest {
            return Err("source_cleanup_scope_changed");
        }
        Ok(Some(Self {
            evidence,
            private,
            bytes: bytes.ok_or("source_cleanup_scope_invalid")?,
            files,
        }))
    }
    pub(super) fn matches(&self, target: &devbox_git::GitTarget) -> bool {
        self.evidence
            .members
            .iter()
            .any(|member| member.git.matches(target))
    }
    pub(super) fn repository(
        &self,
        host: &Host,
        target: &devbox_git::GitTarget,
        budget: Budget,
    ) -> Result<devbox_git::execution::NativeRepository> {
        let member = self
            .evidence
            .members
            .iter()
            .find(|member| member.git.matches(target))
            .ok_or("source_context_changed")?;
        // Native document grants cover picker documents as well as project tabs.
        if self
            .files
            .try_lock()
            .map_err(|_| "busy")?
            .has_documents_under(member.git.native_root_identity())
        {
            return Err("source_cleanup_open_files");
        }
        if self.private.read(FILE)?.as_deref() != Some(self.bytes.as_slice()) {
            return Err("source_cleanup_scope_changed");
        }
        member.revalidate_metadata(host, budget)?;
        member.git.repository(target, budget.deadline_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::registry::ObjectStamp;
    fn binding(root: &str, object: &str, repo: &str) -> Binding {
        Binding {
            target: product_contract::ExecutionTarget::Windows,
            root: root.into(),
            root_object: ObjectStamp {
                scope: "1".into(),
                object: object.into(),
            },
            repository_object: Some(ObjectStamp {
                scope: "1".into(),
                object: repo.into(),
            }),
        }
    }
    #[test]
    fn selection_uses_only_registered_siblings_and_never_admits_paths_or_foreign_contexts() {
        let mut registry = Registry::default();
        let root = registry
            .register(1, "source", binding(r"C:\missing-source", "11", "ff"))
            .unwrap();
        let sibling = registry
            .register(
                registry.revision,
                "ignored",
                binding(r"C:\missing-sibling", "12", "ff"),
            )
            .unwrap();
        let foreign = registry
            .register(
                registry.revision,
                "other",
                binding(r"C:\missing-foreign", "13", "ee"),
            )
            .unwrap();
        assert_eq!(
            selected(&registry, &root, std::slice::from_ref(&sibling.worktree_id))
                .unwrap()
                .as_slice(),
            std::slice::from_ref(&sibling)
        );
        for ids in [
            vec![],
            vec![root.worktree_id.clone()],
            vec![foreign.worktree_id],
            vec![r"C:\missing-sibling".into()],
            vec![sibling.worktree_id.clone(); 2],
            vec!["unknown".into(); 9],
        ] {
            assert!(selected(&registry, &root, &ids).is_err());
        }
        let mut stale = root.clone();
        stale.revision += 1;
        assert!(available(&registry, &stale).is_err());
        registry
            .worktrees
            .iter_mut()
            .find(|tree| tree.id == sibling.worktree_id)
            .unwrap()
            .binding
            .repository_object = None;
        assert!(selected(&registry, &root, &[sibling.worktree_id]).is_err());
    }
    #[test]
    fn corrupt_future_foreign_and_duplicate_scope_metadata_cannot_become_permission() {
        let context = ProjectContext {
            project_id: "project".into(),
            worktree_id: "root".into(),
            revision: 1,
            target: product_contract::ExecutionTarget::Windows,
        };
        let mut sibling = context.clone();
        sibling.worktree_id = "sibling".into();
        let original = json!({"schemaVersion":1,"context":context,"rootDigest":"a".repeat(64),"members":[sibling],"digest":"b".repeat(64)});
        assert!(decode(None, &context).unwrap().is_none());
        assert!(decode(Some(b"null"), &context).unwrap().is_none());
        assert!(decode(Some(b"broken"), &context).is_err());
        assert!(
            decode(Some(&serde_json::to_vec(&original).unwrap()), &context)
                .unwrap()
                .is_some()
        );
        for (field, value) in [
            ("schemaVersion", json!(99)),
            ("digest", json!("bad")),
            ("members", json!([sibling.clone(), sibling])),
            ("members", json!([context.clone()])),
            ("members", json!([])),
        ] {
            let mut invalid = original.clone();
            invalid[field] = value;
            assert!(decode(Some(&serde_json::to_vec(&invalid).unwrap()), &context).is_err());
        }
        let mut other = context.clone();
        other.revision += 1;
        assert!(decode(Some(&serde_json::to_vec(&original).unwrap()), &other).is_err());
    }
}
