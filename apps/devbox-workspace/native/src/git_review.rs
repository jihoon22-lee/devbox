//! Native Linux Git evidence retained behind an admitted root/context. Review
//! observes bytes only; it does not grant execution or own Windows approvals.
use crate::{
    files::RootLease,
    git_environment::SourceEnvironment,
    git_trust::{GitLease, GitTrust},
};
use devbox_filesystem::{project::ProjectObservation, FilesystemIdentity};
use product_contract::{ExecutionTarget, ProjectContext};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
type Result<T> = std::result::Result<T, &'static str>;

struct Lease {
    observation: ProjectObservation,
    target: ExecutionTarget,
}
impl RootLease for Lease {
    fn root(&self) -> &Path {
        self.observation.root()
    }
    fn target(&self) -> &ExecutionTarget {
        &self.target
    }
    fn native_root_identity(&self) -> FilesystemIdentity {
        self.observation.root_identity()
    }
    fn revalidate(&self) -> Result<()> {
        self.observation.revalidate()
    }
}
impl GitLease for Lease {
    fn git_directories(&self) -> Option<(&Path, &Path)> {
        self.observation.git_directories()
    }
}
pub(crate) struct Review {
    context: ProjectContext,
    trust: std::sync::Arc<GitTrust<Lease>>,
    creation: Option<crate::git_creation::Creation>,
    repository_identity: Option<FilesystemIdentity>,
}
impl Review {
    pub(crate) fn execute<T: Send + Sync + 'static>(
        &mut self,
        execution: Execution<'_, T>,
    ) -> Result<serde_json::Value> {
        let Execution {
            expected,
            method,
            args,
            budget_ms,
            cancelled,
            authorize,
            retained,
            members,
            source,
        } = execution;
        // Cancellation uses this pipe owner; renderer operation IDs never
        // acquire authority over another request.
        if !crate::control::source_method(method) {
            return Err("wsl_source_method_unavailable");
        }
        let expires =
            std::time::Instant::now() + std::time::Duration::from_millis(u64::from(budget_ms));
        let deadline = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| "request_expired")?
            .as_millis()
            .saturating_add(u128::from(budget_ms))
            .min(u128::from(u64::MAX)) as u64;
        self.revalidate(expected, deadline)?;
        let (target_boundary, creation, args) = if method == "create_worktree" {
            let plan = self.creation.take().ok_or("worktree_preview_stale")?;
            let (target, creation) = plan.consume(args, deadline)?;
            (Some(target), Some(creation), json!({}))
        } else {
            (None, None, args)
        };
        let cleanup = matches!(method, "repo_cleanup_preview" | "repo_cleanup");
        if members.len() > 8 || (!cleanup && !members.is_empty()) {
            return Err("source_cleanup_scope_invalid");
        }
        let mut seen_roots = std::collections::HashSet::new();
        let mut seen_contexts = std::collections::HashSet::new();
        // Validate every descriptor before observing any sibling filesystem.
        for member in &members {
            member
                .context
                .validate()
                .map_err(|_| "source_cleanup_scope_invalid")?;
            crate::engine::admit(Path::new(&member.root))?;
            if member.context.project_id != self.context.project_id
                || member.context.target != self.context.target
                || member.context.worktree_id == self.context.worktree_id
                || member.root == self.trust.root().to_string_lossy()
                || !seen_roots.insert(&member.root)
                || !seen_contexts.insert(&member.context.worktree_id)
                || member.digest.len() != 64
                || !member.digest.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("source_cleanup_scope_invalid");
            }
        }
        let mut siblings = Vec::new();
        let mut bytes = self.trust.bytes();
        for member in &members {
            let observation =
                ProjectObservation::capture(Path::new(&member.root), crate::linux_files::admit)?;
            if self.repository_identity.is_none()
                || observation.repository_identity() != self.repository_identity
                || observation.root().to_str() != Some(member.root.as_str())
            {
                return Err("source_cleanup_scope_changed");
            }
            let review = Self::capture(
                &observation,
                member.context.clone(),
                source,
                deadline,
                &|| Ok(()),
            )?;
            if review.trust.environment.program != self.trust.environment.program
                || review.trust.environment.environment != self.trust.environment.environment
            {
                return Err("source_cleanup_scope_changed");
            }
            review.revalidate(&member.digest, deadline)?;
            bytes = bytes
                .checked_add(review.trust.bytes())
                .ok_or("git_source_limit")?;
            if bytes > 64 * 1024 * 1024 {
                return Err("git_source_limit");
            }
            siblings.push(review.trust);
        }
        let program = self.trust.environment.program.clone();
        let environment = self.trust.environment.environment.clone();
        let root = self.trust.root().to_path_buf();
        let trust = self.trust.clone();
        let key = serde_json::to_string(&self.context).map_err(|_| "wsl_context_invalid")?;
        let policy = devbox_git::execution::ExecutionPolicy::new_cancellable(
            program,
            environment,
            expires,
            cancelled,
            move |target| {
                let _retained = &retained;
                if let Some(target) = &target_boundary {
                    target.revalidate(deadline).map_err(str::to_owned)?;
                }
                // Unknown targets fail before filesystem IO or Windows authorization.
                let selected = if trust.matches(target) {
                    &trust
                } else {
                    siblings
                        .iter()
                        .find(|member| member.matches(target))
                        .ok_or("source_context_changed")?
                };
                trust.revalidate(deadline).map_err(str::to_owned)?;
                selected
                    .repository(target, deadline)
                    .map_err(str::to_owned)?;
                authorize(target.cwd()).map_err(str::to_owned)?;
                if let Some(target) = &target_boundary {
                    target.revalidate(deadline).map_err(str::to_owned)?;
                }
                trust.revalidate(deadline).map_err(str::to_owned)?;
                selected.repository(target, deadline).map_err(str::to_owned)
            },
        )
        .map_err(|_| "source_context_changed")?
        .with_linux_supervisor()
        .map_err(|_| "source_context_changed")?;
        let mut access = repo_manager_lib::component::SourceAccess::for_project(root, key, policy);
        if let Some(creation) = creation {
            access = access.with_creation(creation);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "source_operation_unavailable")?;
        let result = runtime.block_on(repo_manager_lib::component::dispatch_source_native(
            access, method, args,
        ));
        // Native headless workers belong to this runtime. The binary additionally
        // retains its process permit until every policy clone and owner has retired.
        drop(runtime);
        result.map_err(|error| match error.as_str() {
            "git_cancelled" | "wsl_request_cancelled" => "source_cancelled",
            "git_timeout" | "wsl_timeout" | "request_expired" => "request_expired",
            "source_review_required" => "source_review_required",
            "source_context_changed" | "git_sources_changed" | "project_object_changed" => {
                "source_context_changed"
            }
            "git_output_too_large" => "git_source_limit",
            "worktree_preview_stale" => "worktree_preview_stale",
            "worktree_target_changed" => "worktree_target_changed",
            _ => "source_operation_unavailable",
        })
    }

    pub(crate) fn context(&self) -> &ProjectContext {
        &self.context
    }
    pub(crate) fn capture(
        original: &ProjectObservation,
        context: ProjectContext,
        source: &SourceEnvironment,
        deadline: u64,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        guard()?;
        original.revalidate()?;
        let observation = ProjectObservation::capture(original.root(), crate::linux_files::admit)?;
        if observation.root_identity() != original.root_identity()
            || observation.repository_identity() != original.repository_identity()
        {
            return Err("project_object_changed");
        }
        let environment = source.observe(observation.root(), deadline)?;
        let trust = GitTrust::capture(
            Lease {
                observation,
                target: context.target.clone(),
            },
            environment,
            deadline,
        )?;
        original.revalidate()?;
        guard()?;
        Ok(Self {
            context,
            trust: std::sync::Arc::new(trust),
            creation: None,
            repository_identity: original.repository_identity(),
        })
    }
    pub(crate) fn preview_worktree(
        &mut self,
        expected: &str,
        branch: String,
        path: &str,
        deadline: u64,
    ) -> Result<Value> {
        self.revalidate(expected, deadline)?;
        if self.creation.is_some() {
            return Err("project_preview_limit");
        }
        let (git, common) = self
            .trust
            .directories()
            .ok_or("source_requires_repository")?;
        let creation = crate::git_creation::Creation::capture(
            self.trust.root(),
            &[git.to_owned(), common.to_owned()],
            branch,
            path,
            deadline,
        )?;
        self.revalidate(expected, deadline)?;
        let view = creation.view();
        self.creation = Some(creation);
        Ok(view)
    }
    pub(crate) fn view(&self) -> Value {
        let (files, environment) = self.trust.evidence_digests();
        json!({"digest": self.trust.digest(), "files": files, "environment": environment, "review": self.trust.review, "bytes": self.trust.bytes()})
    }
    pub(crate) fn revalidate(&self, expected: &str, deadline: u64) -> Result<()> {
        if expected != self.trust.digest() {
            return Err("git_sources_changed");
        }
        self.trust.revalidate(deadline)
    }
}
#[derive(Deserialize)]
#[serde(
    tag = "method",
    content = "args",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Method {
    #[serde(rename = "source_capture")]
    Capture { context: ProjectContext },
    #[serde(rename = "source_worktree_preview")]
    Worktree {
        context: ProjectContext,
        digest: String,
        branch: String,
        target_dir: String,
    },
    #[serde(rename = "source_validate")]
    Validate {
        context: ProjectContext,
        digest: String,
    },
}
impl Method {
    pub(crate) fn context(&self) -> &ProjectContext {
        match self {
            Self::Capture { context }
            | Self::Validate { context, .. }
            | Self::Worktree { context, .. } => context,
        }
    }
}

pub(crate) struct Execution<'a, T> {
    pub expected: &'a str,
    pub method: &'a str,
    pub args: serde_json::Value,
    pub budget_ms: u32,
    pub cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub authorize: crate::engine::SourceAuthorization,
    pub retained: T,
    pub members: Vec<CleanupMember>,
    pub source: &'a SourceEnvironment,
}

/// Only the Windows Source owner constructs this list from its stored explicit
/// cleanup approval. It is never forwarded from renderer command arguments.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CleanupMember {
    context: ProjectContext,
    root: String,
    digest: String,
}
