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
    trust: GitTrust<Lease>,
}
impl Review {
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
        Ok(Self { context, trust })
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
