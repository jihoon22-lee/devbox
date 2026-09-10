//! Git evidence stays with the selected native target. Windows owns approval
//! persistence and combines this evidence with the project-definition digest.
use super::git_trust::{native_environment, GitTrust};
use crate::{core::registry::Binding, host::Host};
use product_contract::ProjectContext;
use workspace_wsl::git_trust::GitReview;
type Result<T> = std::result::Result<T, &'static str>;

pub(crate) enum SourceGit {
    Native(Box<GitTrust>),
    #[cfg(windows)]
    Wsl {
        context: ProjectContext,
        lease: Box<super::wsl_project::WslProjectLease>,
        report: Box<Report>,
    },
}
#[cfg(windows)]
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    digest: String,
    files: String,
    environment: String,
    review: GitReview,
    bytes: usize,
}
#[cfg(windows)]
impl Report {
    fn validate(&self) -> Result<()> {
        let hash = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        };
        let text = |value: &str| {
            !value.is_empty() && value.len() <= 32768 && !value.chars().any(char::is_control)
        };
        if ![&self.digest, &self.files, &self.environment]
            .into_iter()
            .all(|value| hash(value))
            || self.bytes > 48 * 1024 * 1024
            || !text(&self.review.executable)
            || !self.review.executable.starts_with('/')
            || self.review.sources.len() > 1024
            || self.review.sources.iter().any(|source| {
                !text(&source.path)
                    || !source.path.starts_with('/')
                    || !matches!(
                        source.kind.as_str(),
                        "config" | "executable" | "hook" | "hooksDirectory"
                    )
                    || source.digest.as_ref().is_some_and(|value| !hash(value))
            })
            || self.review.environment_keys.len() > 2048
            || self.review.environment_keys.iter().any(|key| !text(key))
            || self.review.execution_keys.len() > 256
            || self.review.execution_keys.iter().any(|key| !text(key))
        {
            return Err("wsl_protocol_invalid");
        }
        Ok(())
    }
}
impl SourceGit {
    pub(crate) fn capture(
        host: &Host,
        context: &ProjectContext,
        deadline: u64,
    ) -> Result<(Binding, Self)> {
        crate::files_host::current_deadline(deadline)?;
        let projects = host.projects()?;
        #[cfg(windows)]
        if matches!(
            context.target,
            product_contract::ExecutionTarget::Wsl { .. }
        ) {
            let lease = projects.admit_wsl(host.helper_directory()?, context)?;
            let report: Report = serde_json::from_value(lease.file_request_until(
                context,
                "source_capture",
                serde_json::json!({}),
                deadline,
            )?)
            .map_err(|_| "wsl_protocol_invalid")?;
            report.validate()?;
            let binding = lease.binding().clone();
            return Ok((
                binding,
                Self::Wsl {
                    context: context.clone(),
                    lease: Box::new(lease),
                    report: Box::new(report),
                },
            ));
        }
        let lease = projects.admit(context)?;
        let binding = lease.binding().clone();
        if lease.git_directories().is_none() {
            return Err("source_requires_repository");
        }
        let environment = native_environment(
            std::path::Path::new(&binding.root),
            host.source_environment(),
            deadline,
        )?;
        Ok((
            binding,
            Self::Native(Box::new(GitTrust::capture(lease, environment, deadline)?)),
        ))
    }
    #[cfg(windows)]
    pub(crate) fn is_wsl(&self) -> bool {
        matches!(self, Self::Wsl { .. })
    }
    #[cfg(windows)]
    pub(crate) fn execute_source(
        &self,
        method: &str,
        args: serde_json::Value,
        expires: std::time::Instant,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
        authorize: &dyn Fn(&str) -> Result<()>,
    ) -> Result<serde_json::Value> {
        let Self::Wsl {
            context,
            lease,
            report,
        } = self
        else {
            return Err("wsl_context_invalid");
        };
        lease.execute_source(
            context,
            serde_json::json!({"digest":report.digest,"method":method,"args":args}),
            expires,
            cancelled,
            authorize,
        )
    }
    #[cfg(windows)]
    pub(crate) fn shutdown(&self) -> Result<()> {
        match self {
            Self::Native(_) => Ok(()),
            Self::Wsl { lease, .. } => lease.shutdown(),
        }
    }
    /// Native Windows Git must never receive a WSL root through UNC or argv.
    pub(crate) fn native(&self) -> Result<&GitTrust> {
        match self {
            Self::Native(trust) => Ok(trust),
            #[cfg(windows)]
            Self::Wsl { .. } => Err("wsl_source_required"),
        }
    }
    pub(crate) fn digest(&self) -> &str {
        match self {
            Self::Native(trust) => trust.digest(),
            #[cfg(windows)]
            Self::Wsl { report, .. } => &report.digest,
        }
    }
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Native(trust) => trust.bytes(),
            #[cfg(windows)]
            Self::Wsl { report, .. } => report.bytes,
        }
    }
    pub(crate) fn review(&self) -> &GitReview {
        match self {
            Self::Native(trust) => &trust.review,
            #[cfg(windows)]
            Self::Wsl { report, .. } => &report.review,
        }
    }
    pub(crate) fn evidence_digests(&self) -> (String, String) {
        match self {
            Self::Native(trust) => trust.evidence_digests(),
            #[cfg(windows)]
            Self::Wsl { report, .. } => (report.files.clone(), report.environment.clone()),
        }
    }
    pub(crate) fn revalidate(&self, deadline: u64) -> Result<()> {
        crate::files_host::current_deadline(deadline)?;
        match self {
            Self::Native(trust) => trust.revalidate(deadline),
            #[cfg(windows)]
            Self::Wsl {
                context,
                lease,
                report,
            } => lease
                .file_request_until(
                    context,
                    "source_validate",
                    serde_json::json!({"digest": report.digest}),
                    deadline,
                )
                .map(|_| ()),
        }
    }
    pub(crate) fn matches(&self, target: &devbox_git::GitTarget) -> bool {
        self.native().is_ok_and(|trust| trust.matches(target))
    }
    pub(crate) fn repository(
        &self,
        target: &devbox_git::GitTarget,
        deadline: u64,
    ) -> Result<devbox_git::execution::NativeRepository> {
        self.native()?.repository(target, deadline)
    }
}
