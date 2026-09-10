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
}
impl Review {
    pub(crate) fn execute<T: Send + Sync + 'static>(
        &self,
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
        } = execution;
        // Creation/cleanup need their own native destination/sibling capabilities;
        // cancellation is delivered by the current pipe owner's EOF, not a method
        // that could target another request's domain operation ID.
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
                // Unknown targets fail before filesystem IO or Windows authorization.
                trust.repository(target, deadline).map_err(str::to_owned)?;
                authorize(target.cwd()).map_err(str::to_owned)?;
                trust.repository(target, deadline).map_err(str::to_owned)
            },
        )
        .map_err(|_| "source_context_changed")?
        .with_linux_supervisor()
        .map_err(|_| "source_context_changed")?;
        let access = repo_manager_lib::component::SourceAccess::for_project(root, key, policy);
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
        })
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
    #[serde(rename = "source_validate")]
    Validate {
        context: ProjectContext,
        digest: String,
    },
}
impl Method {
    pub(crate) fn context(&self) -> &ProjectContext {
        match self {
            Self::Capture { context } | Self::Validate { context, .. } => context,
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
}
