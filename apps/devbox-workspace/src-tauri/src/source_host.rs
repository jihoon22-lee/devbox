//! Source owns its execution approval separately from task/LSP permissions.
//! All methods run in bounded native workers after product/session admission.
mod cleanup_scope;
#[cfg(all(test, windows))]
mod wsl_execution_tests;
use crate::{
    definitions::{self, Definitions, ExecutionDefinitions},
    host::Host,
    platform::{
        definition_write::DefinitionTarget, git_worktree::WorktreeTarget, source_git::SourceGit,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    evidence: Option<ApprovalEvidence>,
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApprovalEvidence {
    files: String,
    environment: String,
    definitions: String,
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
            || value.evidence.as_ref().is_some_and(|evidence| {
                [
                    &evidence.files,
                    &evidence.environment,
                    &evidence.definitions,
                ]
                .iter()
                .any(|digest| {
                    digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            })
        {
            return Err("source_trust_invalid");
        }
    }
    Ok(value)
}
pub(crate) enum PreparedSource {
    Native(Box<(repo_manager_lib::component::SourceAccess, Value)>),
    #[cfg(windows)]
    Wsl(Box<WslExecution>),
}
pub(crate) enum ReadySource {
    Native(Box<(repo_manager_lib::component::SourceAccess, Value)>),
    #[cfg(windows)]
    Complete(Value),
}
impl PreparedSource {
    /// Called after Source/definition mutexes have been released, while the
    /// original bounded worker still owns all request/context/filesystem slots.
    pub(crate) fn finish_on_worker(self) -> Result<ReadySource> {
        match self {
            Self::Native(access) => Ok(ReadySource::Native(access)),
            #[cfg(windows)]
            Self::Wsl(execution) => execution.run().map(ReadySource::Complete),
        }
    }
}
#[cfg(windows)]
pub(crate) struct WslExecution {
    snapshot: Snapshot,
    cleanup: Option<cleanup_scope::Execution>,
    host: Arc<Host>,
    method: String,
    args: Value,
    budget: Budget,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    _retained: Box<dyn std::any::Any + Send + Sync>,
}
#[cfg(windows)]
impl WslExecution {
    fn run(mut self: Box<Self>) -> Result<Value> {
        self.budget.check()?;
        let args = std::mem::take(&mut self.args);
        self.snapshot.git.execute_source(
            &self.method,
            args,
            self.cleanup
                .as_ref()
                .map(|scope| scope.native_members())
                .unwrap_or_default(),
            self.budget.expires,
            self.cancelled.clone(),
            &|root| {
                if self.cancelled.load(std::sync::atomic::Ordering::Acquire) {
                    return Err("source_cancelled");
                }
                self.budget.check()?;
                // Git files are checked in Linux. Calling that same pipe here would
                // deadlock; this callback checks Windows metadata and the separate
                // definition owner before granting one native admission ticket.
                self.snapshot
                    .revalidate_metadata(&self.host, self.budget.deadline_ms)?;
                if !self.snapshot.approved()? {
                    return Err("source_review_required");
                }
                if let Some(cleanup) = &self.cleanup {
                    cleanup.revalidate_approval()?;
                }
                if root != self.snapshot.binding.root {
                    self.cleanup
                        .as_ref()
                        .ok_or("source_context_changed")?
                        .authorize_wsl(&self.host, root, self.budget)?;
                }
                self.budget.check()
            },
        )
    }
}
#[cfg(windows)]
impl Drop for WslExecution {
    fn drop(&mut self) {
        // This object never leaves the blocking worker. Retain its request and
        // filesystem/context permits through an unconfirmed Linux retirement.
        while self.snapshot.git.shutdown().is_err() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}
struct Snapshot {
    context: ProjectContext,
    binding: crate::core::registry::Binding,
    git: SourceGit,
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
        let (binding, git) = SourceGit::capture(host, context, deadline)?;
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
        let approval = approval(self.approval_bytes.as_deref(), &self.context)?;
        let mut changed = Vec::new();
        if let Some(evidence) = approval
            .as_ref()
            .and_then(|approval| approval.evidence.as_ref())
        {
            let (files, environment) = self.git.evidence_digests();
            if evidence.files != files {
                changed.push("execution_files");
            }
            if evidence.environment != environment {
                changed.push("execution_environment");
            }
            if evidence.definitions != self.definitions.digest() {
                changed.push("project_definitions");
            }
        }
        Ok(
            json!({"approved":self.approved()?,"hasApproval":approval.is_some(),"review":self.git.review(),"changedEvidence":changed}),
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
            evidence: Some({
                let (files, environment) = self.git.evidence_digests();
                ApprovalEvidence {
                    files,
                    environment,
                    definitions: self.definitions.digest().to_owned(),
                }
            }),
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
#[cfg(windows)]
struct PendingWslWorktree {
    snapshot: Snapshot,
    native_preview: String,
    created: Instant,
}
pub(crate) struct Invocation {
    pub files: Arc<std::sync::Mutex<crate::files_host::FilesHost>>,
    pub context: ProjectContext,
    pub method: String,
    pub args: Value,
    pub budget: Budget,
    pub admitted: crate::core::source_operations::Request,
}
#[derive(Default)]
pub(crate) struct SourceHost {
    pending: HashMap<String, Pending>,
    cleanup: cleanup_scope::Owner,
    worktrees: HashMap<String, PendingWorktree>,
    #[cfg(windows)]
    wsl_worktrees: HashMap<String, PendingWslWorktree>,
    storage: Option<ProtectedStorage>,
    common: Option<PathBuf>,
}
impl SourceHost {
    fn preview_count(&self) -> usize {
        let count = self.pending.len() + self.worktrees.len() + self.cleanup.len();
        #[cfg(windows)]
        {
            count + self.wsl_worktrees.len()
        }
        #[cfg(not(windows))]
        {
            count
        }
    }
    pub(crate) fn expire(&mut self) {
        self.cleanup.expire();
        self.pending
            .retain(|_, pending| pending.created.elapsed() < TTL);
        self.worktrees
            .retain(|_, pending| pending.created.elapsed() < TTL);
        #[cfg(windows)]
        self.wsl_worktrees
            .retain(|_, pending| pending.created.elapsed() < TTL);
    }
    pub(crate) fn initialize(&mut self, app: &tauri::AppHandle, host: &Host) -> Result<()> {
        let common = host.component("common")?;
        match &self.common {
            Some(previous) if previous == &common => Ok(()),
            Some(_) => Err("source_context_changed"),
            None => {
                let storage = crate::platform::storage_paths::from_host(app, host)?;
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
        if cleanup_scope::management(method) {
            if method == "preview_cleanup_scope" && self.preview_count() >= MAX_PREVIEWS {
                return Err("project_preview_limit");
            }
            return self
                .cleanup
                .manage(host, definitions, context, method, args, budget);
        }
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
                #[cfg(windows)]
                if self
                    .wsl_worktrees
                    .get(&id.preview_id)
                    .is_some_and(|pending| pending.snapshot.context == *context)
                {
                    self.wsl_worktrees.remove(&id.preview_id);
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
                if self.preview_count() >= MAX_PREVIEWS {
                    return Err("project_preview_limit");
                }
                let snapshot = Snapshot::capture(host, definitions, context, deadline)?;
                if !snapshot.approved()? {
                    return Err("source_review_required");
                }
                #[cfg(windows)]
                if snapshot.git.is_wsl() {
                    let preview = snapshot.git.preview_worktree(
                        &input.branch,
                        &input.target_dir,
                        deadline,
                    )?;
                    snapshot.revalidate(host, deadline)?;
                    budget.check()?;
                    let preview_id = uuid::Uuid::new_v4().to_string();
                    let view = json!({"previewId":preview_id,"branch":preview.branch,"targetDir":preview.target_dir,"root":snapshot.binding.root});
                    self.wsl_worktrees.insert(
                        preview_id,
                        PendingWslWorktree {
                            snapshot,
                            native_preview: preview.preview_id,
                            created: Instant::now(),
                        },
                    );
                    return Ok(view);
                }
                let (git, common) = snapshot
                    .git
                    .native()?
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
                #[cfg(windows)]
                self.wsl_worktrees
                    .retain(|_, pending| pending.snapshot.context != *context);
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
                if self.preview_count() >= MAX_PREVIEWS {
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
    ) -> Result<PreparedSource> {
        let Invocation {
            files,
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
            #[cfg(windows)]
            if let Some(pending) = self.wsl_worktrees.remove(&input.preview_id) {
                if pending.snapshot.context != context || pending.created.elapsed() >= TTL {
                    return Err("worktree_preview_stale");
                }
                pending.snapshot.revalidate(&host, deadline)?;
                if !pending.snapshot.approved()? {
                    return Err("source_review_required");
                }
                budget.check()?;
                admitted.check()?;
                return Ok(PreparedSource::Wsl(Box::new(WslExecution {
                    snapshot: pending.snapshot,
                    cleanup: None,
                    host,
                    method,
                    args: json!({"previewId":pending.native_preview,"operationId":input.operation_id}),
                    budget,
                    cancelled: admitted.flag(),
                    _retained: Box::new(retained),
                })));
            }
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
        let cleanup = if matches!(method.as_str(), "repo_cleanup_preview" | "repo_cleanup") {
            cleanup_scope::Execution::capture(&host, definitions, &snapshot, files, budget)?
        } else {
            None
        };
        #[cfg(windows)]
        if snapshot.git.is_wsl() {
            if creation.is_some() || !workspace_wsl::control::source_method(&method) {
                return Err("wsl_source_method_unavailable");
            }
            return Ok(PreparedSource::Wsl(Box::new(WslExecution {
                snapshot,
                cleanup,
                host,
                method,
                args,
                budget,
                cancelled: admitted.flag(),
                _retained: Box::new(retained),
            })));
        }
        let target_boundary = creation.as_ref().map(|(pending, _)| pending.target.clone());
        let program = snapshot.git.native()?.environment.program.clone();
        let environment = snapshot.git.native()?.environment.environment.clone();
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
                // Unknown Git-returned roots are rejected before any filesystem IO.
                let selected = snapshot.git.matches(target);
                if !selected && !cleanup.as_ref().is_some_and(|scope| scope.matches(target)) {
                    return Err("source_context_changed".into());
                }
                snapshot
                    .revalidate_metadata(&host, deadline)
                    .map_err(str::to_string)?;
                if !snapshot.approved().map_err(str::to_string)? {
                    return Err("source_review_required".into());
                }
                if selected {
                    snapshot
                        .git
                        .repository(target, deadline)
                        .map_err(str::to_string)
                } else {
                    snapshot.git.revalidate(deadline).map_err(str::to_string)?;
                    cleanup
                        .as_ref()
                        .ok_or("source_context_changed")?
                        .repository(&host, target, budget)
                        .map_err(str::to_string)
                }
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
        Ok(PreparedSource::Native(Box::new((access, args))))
    }
}

pub(crate) fn management(method: &str) -> bool {
    cleanup_scope::management(method)
        || matches!(
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
        "source_cleanup_scope_invalid" => "source_cleanup_scope_invalid",
        "source_cleanup_scope_changed" => "source_cleanup_scope_changed",
        "source_cleanup_open_files" => "source_cleanup_open_files",
        "worktree_branch_invalid" => "worktree_branch_invalid",
        "worktree_preview_stale" => "worktree_preview_stale",
        "worktree_target_invalid" => "worktree_target_invalid",
        "worktree_target_changed" => "worktree_target_changed",
        "worktree_target_unavailable" => "worktree_target_unavailable",
        "file_owner_path" => "file_owner_path",
        "wsl_source_method_unavailable" => "wsl_source_method_unavailable",
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
    #[cfg(windows)]
    #[test]
    #[ignore = "requires the current GitHub run-owned WSL distro and packaged helper"]
    fn owned_wsl_source_review_preserves_native_approval_and_changed_evidence() {
        use crate::project_owner::RegistrationAction;
        use std::{fs, path::Path};
        assert_eq!(std::env::var("GITHUB_ACTIONS").unwrap(), "true");
        assert_eq!(
            std::env::var("RUNNER_ENVIRONMENT").unwrap(),
            "github-hosted"
        );
        let run = std::env::var("GITHUB_RUN_ID").unwrap();
        let name = std::env::var("DEVBOX_KNOWLEDGE_WSL_DISTRO").unwrap();
        assert!(name.starts_with(&format!("DevboxKnowledgeFixture-{run}-")));
        let distro = crate::platform::wsl_distro::list()
            .unwrap()
            .into_iter()
            .find(|d| d.name == name)
            .unwrap();
        let nonce = uuid::Uuid::new_v4();
        let root = format!("/home/devbox-fixture/source 한글 {nonce}");
        let unc = format!(r"\\wsl.localhost\{name}\home\devbox-fixture\source 한글 {nonce}");
        let path = Path::new(&unc);
        fs::create_dir(path).unwrap();
        fs::create_dir_all(path.join(".git/objects")).unwrap();
        fs::create_dir(path.join(".git/hooks")).unwrap();
        fs::write(path.join(".git/HEAD"), b"ref: refs/heads/main\n").unwrap();
        fs::write(
            path.join(".git/config"),
            b"[core]\nrepositoryformatversion=0\n",
        )
        .unwrap();
        fs::write(
            path.join(".git/hooks/pre-commit"),
            b"#!/bin/sh\ntouch must-not-exist\n",
        )
        .unwrap();
        let original = fs::read(path.join(".git/config")).unwrap();
        let store = tempfile::tempdir().unwrap();
        let host =
            Host::open_with_resources(store.path(), Path::new(env!("CARGO_MANIFEST_DIR")).into())
                .unwrap();
        host.start_empty().unwrap();
        let projects = host.projects().unwrap();
        let preview = projects
            .preview_wsl(host.helper_directory().unwrap(), &distro.id, &root, false)
            .unwrap();
        let (_, context) = projects
            .apply(
                &preview.preview_id,
                "Source fixture",
                RegistrationAction::Register,
            )
            .unwrap();
        let mut source = SourceHost::default();
        let mut definitions = Definitions::default();
        let mut call = |method, args| {
            source.manage(
                &host,
                &mut definitions,
                &context,
                method,
                args,
                Budget {
                    deadline_ms: u64::MAX,
                    expires: Instant::now() + Duration::from_secs(30),
                },
            )
        };
        let status = call("trust_status", json!({})).unwrap();
        assert_eq!(status["approved"], false);
        assert_eq!(status["review"]["executable"], "/usr/bin/git");
        assert!(!path.join("must-not-exist").exists());
        let preview = call("preview_trust", json!({})).unwrap();
        let stale_id = preview["previewId"].clone();
        fs::write(
            path.join(".git/hooks/pre-commit"),
            b"#!/bin/sh\ntouch changed-must-not-exist\n",
        )
        .unwrap();
        assert!(call("approve_trust", json!({"previewId":stale_id})).is_err());
        let preview = call("preview_trust", json!({})).unwrap();
        call("approve_trust", json!({"previewId":preview["previewId"]})).unwrap();
        // A new helper connection retains the effective environment digest.
        assert_eq!(call("trust_status", json!({})).unwrap()["approved"], true);
        fs::write(
            path.join(".git/hooks/pre-commit"),
            b"changed reviewed file\n",
        )
        .unwrap();
        let changed = call("trust_status", json!({})).unwrap();
        assert_eq!(changed["approved"], false);
        assert!(changed["changedEvidence"]
            .as_array()
            .unwrap()
            .contains(&json!("execution_files")));
        let preview = call("preview_trust", json!({})).unwrap();
        call("approve_trust", json!({"previewId":preview["previewId"]})).unwrap();
        call("revoke_trust", json!({})).unwrap();
        assert_eq!(call("trust_status", json!({})).unwrap()["approved"], false);
        assert_eq!(fs::read(path.join(".git/config")).unwrap(), original);
        assert!(!path.join("must-not-exist").exists());
        assert!(!path.join("changed-must-not-exist").exists());
        assert!(!path.join(".devbox").exists());
        drop(source);
        drop(definitions);
        drop(host);
        fs::remove_dir_all(path).unwrap();
    }
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
