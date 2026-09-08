//! Source owns its execution approval separately from task/LSP permissions.
//! All methods run in bounded native workers after product/session admission.
use crate::{
    definitions::{self, Definitions, ExecutionDefinitions},
    host::Host,
    platform::{
        definition_write::DefinitionTarget,
        git_trust::{GitEnvironment, GitTrust},
        git_worktree::WorktreeTarget,
        storage_paths::ProtectedStorage,
    },
    private_metadata::MetadataRoot,
};
use product_contract::ProjectContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, &'static str>;
const APPROVAL: &str = "git-approval.json";
const TTL: Duration = Duration::from_secs(180);
const MAX_PREVIEWS: usize = 4;
#[derive(Clone, Copy)]
pub(crate) struct Budget {
    pub deadline_ms: u64,
    pub expires: Instant,
}
impl Budget {
    pub(crate) fn check(self) -> Result<()> {
        if Instant::now() >= self.expires {
            return Err("request_expired");
        }
        crate::files_host::current_deadline(self.deadline_ms)
    }
}
fn private(host: &Host, context: &ProjectContext) -> Result<MetadataRoot> {
    host.projects()?.binding(context)?;
    MetadataRoot::open(&host.component("overview")?)?
        .child("source")?
        .child(&format!("worktree-{}", context.worktree_id))
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Approval {
    schema_version: u32,
    context: ProjectContext,
    digest: String,
}
fn approval(bytes: Option<&[u8]>, context: &ProjectContext) -> Result<Option<Approval>> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    if bytes.len() > 8192 {
        return Err("source_trust_invalid");
    }
    let value: Option<Approval> =
        serde_json::from_slice(bytes).map_err(|_| "source_trust_invalid")?;
    if let Some(value) = &value {
        if value.schema_version != 1 {
            return Err("source_trust_future");
        }
        if value.context != *context
            || value.digest.len() != 64
            || !value.digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("source_trust_invalid");
        }
    }
    Ok(value)
}
struct Snapshot {
    context: ProjectContext,
    binding: crate::core::registry::Binding,
    git: GitTrust,
    definitions: ExecutionDefinitions,
    common: MetadataRoot,
    private: MetadataRoot,
    digest: String,
    approval_bytes: Option<Vec<u8>>,
}
impl Snapshot {
    fn capture(
        host: &Host,
        definitions: &mut Definitions,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<Self> {
        let projects = host.projects()?;
        let lease = projects.admit(context)?;
        let binding = lease.binding().clone();
        if lease.git_directories().is_none() {
            return Err("source_requires_repository");
        }
        let environment = GitEnvironment::native(std::path::Path::new(&binding.root), deadline)?;
        let git = GitTrust::capture(lease, environment, deadline)?;
        let definitions = definitions.execution_evidence(host, context, deadline)?;
        let common = MetadataRoot::open(&host.component("common")?)?;
        let private = private(host, context)?;
        let approval_bytes = private.read(APPROVAL)?;
        approval(approval_bytes.as_deref(), context)?;
        let digest = definitions::digest(
            &serde_json::to_vec(&(context, git.digest(), definitions.digest()))
                .map_err(|_| "source_trust_invalid")?,
        );
        let snapshot = Self {
            context: context.clone(),
            binding,
            git,
            definitions,
            common,
            private,
            digest,
            approval_bytes,
        };
        snapshot.revalidate(host, deadline)?;
        Ok(snapshot)
    }
    fn revalidate(&self, host: &Host, deadline: u64) -> Result<()> {
        self.revalidate_metadata(host, deadline)?;
        self.git.revalidate(deadline)?;
        crate::files_host::current_deadline(deadline)
    }
    fn revalidate_metadata(&self, host: &Host, deadline: u64) -> Result<()> {
        crate::files_host::current_deadline(deadline)?;
        if host.projects()?.binding(&self.context)? != self.binding
            || host.component("common")? != self.common.path()
        {
            return Err("source_context_changed");
        }
        self.common.revalidate()?;
        self.private.revalidate()?;
        self.definitions.revalidate()?;
        crate::files_host::current_deadline(deadline)
    }
    fn approved(&self) -> Result<bool> {
        Ok(
            approval(self.private.read(APPROVAL)?.as_deref(), &self.context)?
                .is_some_and(|value| value.digest == self.digest),
        )
    }
    fn view(&self) -> Result<Value> {
        Ok(
            json!({"approved":self.approved()?,"hasApproval":approval(self.approval_bytes.as_deref(), &self.context)?.is_some(),"review":self.git.review}),
        )
    }
    fn write_approval(&self, host: &Host, approved: bool, budget: Budget) -> Result<()> {
        let deadline = budget.deadline_ms;
        self.revalidate(host, deadline)?;
        budget.check()?;
        if self.private.read(APPROVAL)? != self.approval_bytes {
            return Err("source_trust_changed");
        }
        let value = approved.then(|| Approval {
            schema_version: 1,
            context: self.context.clone(),
            digest: self.digest.clone(),
        });
        let bytes = serde_json::to_vec(&value).map_err(|_| "source_trust_invalid")?;
        let target = DefinitionTarget::capture(
            &self.private.path().join(APPROVAL),
            self.approval_bytes.as_deref(),
        )?;
        target.write(&bytes, || {
            self.revalidate(host, deadline)?;
            budget.check()
        })?;
        Ok(())
    }
}
struct Pending {
    snapshot: Snapshot,
    created: Instant,
}
struct PendingWorktree {
    context: ProjectContext,
    digest: String,
    branch: String,
    target: Arc<WorktreeTarget>,
    created: Instant,
}
pub(crate) struct Invocation {
    pub context: ProjectContext,
    pub method: String,
    pub args: Value,
    pub budget: Budget,
    pub admitted: crate::core::source_operations::Request,
}
#[derive(Default)]
pub(crate) struct SourceHost {
    pending: HashMap<String, Pending>,
    worktrees: HashMap<String, PendingWorktree>,
    storage: Option<ProtectedStorage>,
    common: Option<PathBuf>,
}
impl SourceHost {
    pub(crate) fn expire(&mut self) {
        self.pending
            .retain(|_, pending| pending.created.elapsed() < TTL);
        self.worktrees
            .retain(|_, pending| pending.created.elapsed() < TTL);
    }
    pub(crate) fn initialize(&mut self, app: &tauri::AppHandle, host: &Host) -> Result<()> {
        let common = host.component("common")?;
        match &self.common {
            Some(previous) if previous == &common => Ok(()),
            Some(_) => Err("source_context_changed"),
            None => {
                let storage = ProtectedStorage::from_host(app, host)?;
                repo_manager_lib::component::initialize(app, &common)
                    .map_err(|_| "source_owner_unavailable")?;
                self.common = Some(common);
                self.storage = Some(storage);
                Ok(())
            }
        }
    }
    pub(crate) fn manage(
        &mut self,
        host: &Host,
        definitions: &mut Definitions,
        context: &ProjectContext,
        method: &str,
        args: Value,
        budget: Budget,
    ) -> Result<Value> {
        budget.check()?;
        let deadline = budget.deadline_ms;
        self.expire();
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Id {
            preview_id: String,
        }
        match method {
            "cancel_worktree" => {
                let id: Id = serde_json::from_value(args).map_err(|_| "invalid_request")?;
                if self
                    .worktrees
                    .get(&id.preview_id)
                    .is_some_and(|pending| pending.context == *context)
                {
                    self.worktrees.remove(&id.preview_id);
                }
                Ok(Value::Null)
            }
            "preview_worktree" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    branch: String,
                    target_dir: String,
                }
                let input: Input = serde_json::from_value(args).map_err(|_| "invalid_request")?;
                if !repo_manager_lib::component::source_branch_valid(&input.branch) {
                    return Err("worktree_branch_invalid");
                }
                if self.pending.len() + self.worktrees.len() >= MAX_PREVIEWS {
                    return Err("project_preview_limit");
                }
                let snapshot = Snapshot::capture(host, definitions, context, deadline)?;
                if !snapshot.approved()? {
                    return Err("source_review_required");
                }
                let (git, common) = snapshot
                    .git
                    .directories()
                    .ok_or("source_requires_repository")?;
                let storage = self.storage.as_ref().ok_or("source_owner_unavailable")?;
                if devbox_filesystem::parse_safe_project_path(&input.target_dir).is_none() {
                    return Err("worktree_target_invalid");
                }
                storage.ensure_user_path(std::path::Path::new(&input.target_dir))?;
                let target = Arc::new(WorktreeTarget::capture(
                    std::path::Path::new(&snapshot.binding.root),
                    &input.target_dir,
                    &[git.to_owned(), common.to_owned()],
                    deadline,
                )?);
                storage.ensure_user_path(target.path())?;
                snapshot.revalidate(host, deadline)?;
                budget.check()?;
                let preview_id = uuid::Uuid::new_v4().to_string();
                let view = json!({"previewId":preview_id,"branch":input.branch,"targetDir":target.path(),"root":snapshot.binding.root});
                self.worktrees.insert(
                    preview_id,
                    PendingWorktree {
                        context: context.clone(),
                        digest: snapshot.digest,
                        branch: input.branch,
                        target,
                        created: Instant::now(),
                    },
                );
                Ok(view)
            }
            "cancel_trust" => {
                let id: Id = serde_json::from_value(args).map_err(|_| "invalid_request")?;
                if self
                    .pending
                    .get(&id.preview_id)
                    .is_some_and(|pending| pending.snapshot.context == *context)
                {
                    self.pending.remove(&id.preview_id);
                }
                Ok(Value::Null)
            }
            "approve_trust" => {
                let id: Id = serde_json::from_value(args).map_err(|_| "invalid_request")?;
                let pending = self
                    .pending
                    .remove(&id.preview_id)
                    .ok_or("source_preview_stale")?;
                if pending.snapshot.context != *context || pending.created.elapsed() >= TTL {
                    return Err("source_preview_stale");
                }
                pending.snapshot.write_approval(host, true, budget)?;
                Ok(json!({"approved":true}))
            }
            "revoke_trust" => {
                if !args.as_object().is_some_and(|object| object.is_empty()) {
                    return Err("invalid_request");
                }
                let binding = host.projects()?.binding(context)?;
                let private = private(host, context)?;
                let before = private.read(APPROVAL)?;
                approval(before.as_deref(), context)?;
                if before.is_some() {
                    DefinitionTarget::capture(&private.path().join(APPROVAL), before.as_deref())?
                        .write(b"null", || {
                        budget.check()?;
                        if host.projects()?.binding(context)? != binding {
                            return Err("source_context_changed");
                        }
                        private.revalidate()?;
                        budget.check()
                    })?;
                }
                self.pending
                    .retain(|_, pending| pending.snapshot.context != *context);
                self.worktrees
                    .retain(|_, pending| pending.context != *context);
                Ok(json!({"approved":false}))
            }
            "trust_status" | "preview_trust" => {
                if !args.as_object().is_some_and(|object| object.is_empty()) {
                    return Err("invalid_request");
                }
                let snapshot = Snapshot::capture(host, definitions, context, deadline)?;
                budget.check()?;
                let view = snapshot.view()?;
                if method == "trust_status" {
                    return Ok(view);
                }
                if self.pending.len() + self.worktrees.len() >= MAX_PREVIEWS {
                    return Err("project_preview_limit");
                }
                let preview_id = uuid::Uuid::new_v4().to_string();
                self.pending.insert(
                    preview_id.clone(),
                    Pending {
                        snapshot,
                        created: Instant::now(),
                    },
                );
                Ok(json!({"previewId":preview_id,"status":view}))
            }
            _ => Err("invalid_request"),
        }
    }
    pub(crate) fn access<T: Send + Sync + 'static>(
        &mut self,
        host: Arc<Host>,
        definitions: &mut Definitions,
        invocation: Invocation,
        retained: T,
    ) -> Result<(repo_manager_lib::component::SourceAccess, Value)> {
        let Invocation {
            context,
            method,
            mut args,
            budget,
            admitted,
        } = invocation;
        budget.check()?;
        admitted.check()?;
        let deadline = budget.deadline_ms;
        let creation = if method == "create_worktree" {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Create {
                preview_id: String,
                operation_id: String,
            }
            let input: Create = serde_json::from_value(args).map_err(|_| "invalid_request")?;
            let pending = self
                .worktrees
                .remove(&input.preview_id)
                .ok_or("worktree_preview_stale")?;
            if pending.context != context || pending.created.elapsed() >= TTL {
                return Err("worktree_preview_stale");
            }
            args = json!({});
            Some((pending, input.operation_id))
        } else {
            None
        };
        let snapshot = Snapshot::capture(&host, definitions, &context, deadline)?;
        budget.check()?;
        admitted.check()?;
        if !snapshot.approved()? {
            return Err("source_review_required");
        }
        if let Some((pending, _)) = &creation {
            if pending.digest != snapshot.digest {
                return Err("worktree_preview_stale");
            }
            pending.target.revalidate(deadline)?;
            self.storage
                .as_ref()
                .ok_or("source_owner_unavailable")?
                .ensure_user_path(pending.target.path())?;
        }
        let target_boundary = creation.as_ref().map(|(pending, _)| pending.target.clone());
        let program = snapshot.git.environment.program.clone();
        let environment = snapshot.git.environment.environment.clone();
        let root = PathBuf::from(&snapshot.binding.root);
        let key = serde_json::to_string(&context).map_err(|_| "invalid_context")?;
        let policy = devbox_git::execution::ExecutionPolicy::new_cancellable(
            program,
            environment,
            budget.expires,
            admitted.flag(),
            move |target| {
                let _retained = &retained;
                budget.check().map_err(str::to_string)?;
                if let Some(target) = &target_boundary {
                    target.revalidate(deadline).map_err(str::to_string)?;
                }
                // Reject unapproved Git-returned roots before any IO, and
                // revalidate the selected Git evidence only once per boundary.
                let repository = snapshot
                    .git
                    .repository(target, deadline)
                    .map_err(str::to_string)?;
                snapshot
                    .revalidate_metadata(&host, deadline)
                    .map_err(str::to_string)?;
                if !snapshot.approved().map_err(str::to_string)? {
                    return Err("source_review_required".into());
                }
                Ok(repository)
            },
        )
        .map_err(|_| "source_context_changed")?;
        let mut access = repo_manager_lib::component::SourceAccess::for_project(root, key, policy);
        if let Some((pending, operation_id)) = creation {
            let target = pending.target;
            let path = target
                .path()
                .to_str()
                .ok_or("worktree_target_invalid")?
                .to_owned();
            let creation = repo_manager_lib::component::SourceCreation::new(
                pending.branch,
                path,
                operation_id,
                move || target.revalidate(deadline).map_err(str::to_string),
            )
            .map_err(|_| "worktree_target_invalid")?;
            access = access.with_creation(creation);
        }
        Ok((access, args))
    }
}

pub(crate) fn management(method: &str) -> bool {
    matches!(
        method,
        "trust_status"
            | "preview_trust"
            | "approve_trust"
            | "revoke_trust"
            | "cancel_trust"
            | "preview_worktree"
            | "cancel_worktree"
    )
}
pub(crate) fn issue(error: &str) -> &'static str {
    match error {
        "worktree_branch_invalid" => "worktree_branch_invalid",
        "worktree_preview_stale" => "worktree_preview_stale",
        "worktree_target_invalid" => "worktree_target_invalid",
        "worktree_target_changed" => "worktree_target_changed",
        "worktree_target_unavailable" => "worktree_target_unavailable",
        "file_owner_path" => "file_owner_path",
        "source_review_required" => "source_review_required",
        "source_cancelled" | "git_cancelled" => "source_cancelled",
        "source_requires_repository" => "source_requires_repository",
        "source_preview_stale" => "source_preview_stale",
        "source_trust_invalid" | "source_trust_future" => "source_trust_invalid",
        "git_sources_changed"
        | "project_definition_changed"
        | "source_trust_changed"
        | "source_context_changed"
        | "project_object_changed" => "source_context_changed",
        "git_source_limit" | "git_config_limit" => "git_source_limit",
        "git_config_invalid" => "git_config_invalid",
        "git_home_unavailable" => "git_environment_invalid",
        "git_source_unavailable" => "git_source_unavailable",
        "project_definition_unavailable" => "project_definition_unavailable",
        "project_definition_limit" => "project_definition_limit",
        "git_installation_unavailable" => "git_installation_unavailable",
        "git_source_transport_denied"
        | "native_path_transport_denied"
        | "git_source_path_invalid"
        | "git_source_path_unsupported" => "git_source_path_unsupported",
        "request_expired" | "git_timeout" => "request_expired",
        "busy" | "context_busy" => "busy",
        _ => "source_operation_unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monotonic_expiry_cannot_be_extended_by_a_future_wall_clock_deadline() {
        let budget = Budget {
            deadline_ms: u64::MAX,
            expires: Instant::now() - Duration::from_secs(1),
        };
        assert_eq!(budget.check().unwrap_err(), "request_expired");
    }
    #[test]
    fn invalid_future_and_foreign_context_approvals_never_become_permission() {
        let context = ProjectContext {
            project_id: "project".into(),
            worktree_id: "tree".into(),
            revision: 1,
            target: product_contract::ExecutionTarget::Windows,
        };
        assert!(approval(None, &context).unwrap().is_none());
        assert!(approval(Some(b"null"), &context).unwrap().is_none());
        assert!(approval(Some(b"broken"), &context).is_err());
        let mut value = json!({"schemaVersion":1,"context":context,"digest":"a".repeat(64)});
        assert!(
            approval(Some(&serde_json::to_vec(&value).unwrap()), &context)
                .unwrap()
                .is_some()
        );
        value["context"]["worktreeId"] = json!("foreign");
        assert!(approval(Some(&serde_json::to_vec(&value).unwrap()), &context).is_err());
        value["context"] = json!(context);
        value["schemaVersion"] = json!(99);
        assert!(matches!(
            approval(Some(&serde_json::to_vec(&value).unwrap()), &context),
            Err("source_trust_future")
        ));
    }
}
