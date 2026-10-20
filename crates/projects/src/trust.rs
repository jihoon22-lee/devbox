//! Persistent consent to exact configuration bytes; this is not a sandbox.
use crate::{Project, ProjectsError, ProjectsService, R, config::*, model::*, store};
use devbox_protocol::{Done, Internal};
use sha2::{Digest, Sha256};

pub const SCOPE_VERSION: u32 = 2;

pub(crate) struct Snapshot {
    pub(crate) project: Project,
    pub(crate) registered_path: String,
    pub(crate) digest: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) config: ProjectConfig,
}

/// Only projects can construct this whole-config capability. All fields remain immutable.
#[derive(Debug, Clone)]
pub struct AuthorizedConfig {
    project_id: String,
    canonical_path: String,
    config: ProjectConfig,
}
impl AuthorizedConfig {
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn canonical_path(&self) -> &str {
        &self.canonical_path
    }
    pub fn config(&self) -> &ProjectConfig {
        &self.config
    }
}

/// Restricted preparation data cannot authorize test, teardown or launch commands.
#[derive(Debug, Clone)]
pub struct SetupSnapshot {
    project_id: String,
    canonical_path: String,
    copy: Vec<String>,
    setup: Vec<String>,
}
impl SetupSnapshot {
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn canonical_path(&self) -> &str {
        &self.canonical_path
    }
    pub fn copy(&self) -> &[String] {
        &self.copy
    }
    pub fn setup(&self) -> &[String] {
        &self.setup
    }
}

#[derive(Debug, Clone)]
pub struct LaunchSnapshot {
    preparation: SetupSnapshot,
    launch: LaunchConfig,
    run_config: crate::run_config::RunConfig,
}
impl LaunchSnapshot {
    pub fn preparation(&self) -> &SetupSnapshot {
        &self.preparation
    }
    pub fn run_config(&self) -> &crate::run_config::RunConfig {
        &self.run_config
    }
    pub fn launch(&self) -> &LaunchConfig {
        &self.launch
    }
}

fn launch_exec(config: &ProjectConfig) -> bool {
    !config.launch.services.is_empty()
        || !config.launch.agents.is_empty()
        || config.launch.terminals.iter().any(|t| t.command.is_some())
}
pub fn required_scope_version(config: &ProjectConfig) -> Option<u32> {
    if !config.raw_tasks.is_empty()
        || !config.raw_services.is_empty()
        || !config.raw_schedules.is_empty()
    {
        Some(2)
    } else if has_scope_one_commands(config) {
        Some(1)
    } else {
        None
    }
}
fn has_scope_one_commands(config: &ProjectConfig) -> bool {
    !config.agent.setup.is_empty()
        || !config.agent.teardown.is_empty()
        || config.agent.test.is_some()
        || launch_exec(config)
}
fn operation_exec(config: &ProjectConfig, operation: TrustOperation) -> bool {
    match operation {
        TrustOperation::Run => !config.run_config().is_empty(),
        TrustOperation::Create => !config.agent.setup.is_empty(),
        TrustOperation::Test => config.agent.test.is_some(),
        TrustOperation::Launch => launch_exec(config),
        TrustOperation::Discard
        | TrustOperation::Merge
        | TrustOperation::Pr
        | TrustOperation::Cleanup => !config.agent.teardown.is_empty(),
    }
}

impl ProjectsService {
    pub(crate) async fn trust_snapshot(&self, id: &str) -> R<Snapshot> {
        let mut project = self
            .get(id)
            .await?
            .ok_or_else(|| ProjectsError::NotFound { id: id.into() })?;
        tokio::task::spawn_blocking(move || {
            let registered_path = project.path.clone();
            let path =
                std::fs::canonicalize(&project.path).map_err(|_| ProjectsError::PathInvalid {
                    path: project.path.clone(),
                })?;
            if !path.is_dir() {
                return Err(ProjectsError::NotDirectory { path: project.path }.into());
            }
            project.path = path
                .to_str()
                .ok_or_else(|| ProjectsError::PathInvalid {
                    path: registered_path.clone(),
                })?
                .into();
            let (bytes, config) =
                read_config(&path).map_err(|error| ProjectsError::ConfigurationInvalid {
                    message: error.to_string(),
                })?;
            Ok(Snapshot {
                project,
                registered_path,
                digest: format!("{:x}", Sha256::digest(&bytes)),
                bytes,
                config,
            })
        })
        .await
        .internal("read command trust snapshot")?
    }

    pub(crate) async fn snapshot_approved(&self, s: &Snapshot) -> R<bool> {
        let Some(scope) = required_scope_version(&s.config) else {
            return Ok(false);
        };
        let id = s.project.id.clone();
        let path = s.project.path.clone();
        let digest = s.digest.clone();
        self.db
            .read(move |tx| store::approved(tx, &id, &path, &digest, scope))
            .await
            .internal("read command trust approval")
    }

    async fn authorize_snapshot(&self, s: Snapshot) -> R<AuthorizedConfig> {
        if required_scope_version(&s.config).is_some() && !self.snapshot_approved(&s).await? {
            return Err(ProjectsError::TrustRequired { id: s.project.id }.into());
        }
        Ok(AuthorizedConfig {
            project_id: s.project.id,
            canonical_path: s.project.path,
            config: s.config,
        })
    }

    pub async fn authorized_config(&self, id: &str) -> R<AuthorizedConfig> {
        self.authorize_snapshot(self.trust_snapshot(id).await?)
            .await
    }

    pub async fn run_snapshot(&self, id: &str) -> R<AuthorizedConfig> {
        self.authorized_config(id).await
    }

    pub async fn run_config_status(&self, id: &str) -> R<crate::run_config::RunConfigState> {
        let s = self.trust_snapshot(id).await?;
        let trust = if required_scope_version(&s.config).is_none() {
            TrustPreviewStatus::NotRequired
        } else if self.snapshot_approved(&s).await? {
            TrustPreviewStatus::Approved
        } else {
            TrustPreviewStatus::ApprovalRequired
        };
        Ok(crate::run_config::RunConfigState {
            config: s.config.run_config(),
            trust,
            digest: s.digest,
        })
    }

    pub async fn setup_snapshot(&self, id: &str) -> R<SetupSnapshot> {
        let s = self.trust_snapshot(id).await?;
        // A command-free operation receives only restricted preparation fields,
        // never a reusable capability to unrelated unapproved definitions.
        if !s.config.agent.setup.is_empty() && !self.snapshot_approved(&s).await? {
            return Err(ProjectsError::TrustRequired { id: s.project.id }.into());
        }
        Ok(SetupSnapshot {
            project_id: s.project.id,
            canonical_path: s.project.path,
            copy: s.config.agent.copy,
            setup: s.config.agent.setup,
        })
    }

    pub async fn launch_snapshot(&self, id: &str) -> R<LaunchSnapshot> {
        let s = self.trust_snapshot(id).await?;
        if launch_exec(&s.config) && !self.snapshot_approved(&s).await? {
            return Err(ProjectsError::TrustRequired { id: s.project.id }.into());
        }
        // No launch agents means the preparation fields are never passed to create.
        let run_config = if s.config.launch.services.is_empty() {
            Default::default()
        } else {
            s.config.run_config()
        };
        let setup = if s.config.launch.agents.is_empty() {
            vec![]
        } else {
            s.config.agent.setup
        };
        Ok(LaunchSnapshot {
            run_config,
            preparation: SetupSnapshot {
                project_id: s.project.id,
                canonical_path: s.project.path,
                copy: s.config.agent.copy,
                setup,
            },
            launch: s.config.launch,
        })
    }

    pub async fn teardown_snapshot(&self, id: &str) -> R<Option<AuthorizedConfig>> {
        let s = self.trust_snapshot(id).await?;
        if s.config.agent.teardown.is_empty() {
            return Ok(None);
        }
        self.authorize_snapshot(s).await.map(Some)
    }

    pub async fn test_snapshot(&self, id: &str) -> R<Option<AuthorizedConfig>> {
        let s = self.trust_snapshot(id).await?;
        if s.config.agent.test.is_none() {
            return Ok(None);
        }
        self.authorize_snapshot(s).await.map(Some)
    }

    /// `command_branch` is UI guidance only. Mutations never consume this decision.
    pub async fn trust_preview(
        &self,
        p: TrustPreviewParams,
        command_branch: bool,
        mut execution_directories: Vec<String>,
    ) -> R<TrustPreview> {
        if !command_branch {
            let project = self
                .get(&p.id)
                .await?
                .ok_or_else(|| ProjectsError::NotFound { id: p.id.clone() })?;
            // This branch cannot execute repository commands. Do not parse unrelated
            // configuration or imply that an approval/config capability was issued.
            return Ok(TrustPreview {
                status: TrustPreviewStatus::NotRequired,
                project_id: project.id,
                project_name: project.name,
                config_path: format!("{}/.devbox/devbox.toml", project.path),
                canonical_path: project.path,
                digest: None,
                scope_version: SCOPE_VERSION,
                approved: false,
                requires_approval: false,
                config: None,
                run_definitions: vec![],
                execution_directories,
            });
        }
        let s = self.trust_snapshot(&p.id).await?;
        let approved = self.snapshot_approved(&s).await?;
        let requires_approval =
            command_branch && operation_exec(&s.config, p.operation) && !approved;
        if matches!(p.operation, TrustOperation::Launch)
            && s.config.launch.agents.iter().any(|a| a.worktree)
        {
            execution_directories.push(format!("작업 폴더를 사용하는 시작 에이전트: {}-agents/<새 작업 폴더> (생성할 때 이름 확정)",s.project.path));
        }
        Ok(TrustPreview {
            status: if !operation_exec(&s.config, p.operation) {
                TrustPreviewStatus::NotRequired
            } else if approved {
                TrustPreviewStatus::Approved
            } else {
                TrustPreviewStatus::ApprovalRequired
            },
            project_id: s.project.id,
            project_name: s.project.name,
            config_path: format!("{}/.devbox/devbox.toml", s.project.path),
            canonical_path: s.project.path,
            digest: Some(s.digest),
            scope_version: SCOPE_VERSION,
            approved,
            requires_approval,
            run_definitions: s.config.run_config().definitions(),
            config: Some(s.config),
            execution_directories,
        })
    }

    pub async fn trust_approve(&self, p: TrustApprove) -> R<Done> {
        let s = self.trust_snapshot(&p.id).await?;
        if s.project.path != p.expected_canonical_path
            || s.digest != p.expected_digest
            || p.expected_scope_version != SCOPE_VERSION
        {
            return Err(ProjectsError::TrustPreviewStale { id: p.id }.into());
        }
        let hub = self.hub.clone();
        let (_, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    if !store::get(tx, &p.id)?
                        .is_some_and(|project| project.path == s.registered_path)
                    {
                        return Ok(Err(ProjectsError::TrustPreviewStale { id: p.id }));
                    }
                    store::approve(
                        tx,
                        &p.id,
                        &s.project.path,
                        &s.digest,
                        SCOPE_VERSION,
                        crate::now_ms(),
                    )?;
                    Ok(Ok(()))
                },
                move |_, rev| crate::publish_changed(&hub, rev),
            )
            .await
            .internal("approve project commands")??;
        Ok(Done {
            rev: rev.to_string(),
        })
    }
}
