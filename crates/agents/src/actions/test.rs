use super::open;
use crate::{
    model::{AgentTest, AgentsError, project_trust_failure},
    service::{AgentsService, Result},
};
use std::{
    future::Future,
    path::{Path, PathBuf},
};

/// Prepared under the action lock; the runner must finish starting before it is released.
pub struct AgentTestSpec {
    pub project_id: String,
    pub cwd: PathBuf,
    pub command: String,
    pub snapshot: devbox_projects::trust::AuthorizedConfig,
}

impl AgentsService {
    pub async fn test_with_runner<F, Fut>(&self, id: &str, runner: F) -> Result<AgentTest>
    where
        F: FnOnce(AgentTestSpec) -> Fut,
        Fut: Future<Output = Result<AgentTest>>,
    {
        let _guard = self.actions.lock().await;
        let task = self.get(id).await?;
        open(&task)?;
        let project = self
            .projects
            .get(&task.project_id)
            .await
            .map_err(|e| devbox_protocol::Fail::Internal(format!("test project: {e:?}")))?
            .ok_or(AgentsError::ProjectNotFound {
                id: task.project_id.clone(),
            })?;
        let cwd = if task.worktree_path.is_some() {
            self.owned_worktree(&task).await?.1
        } else {
            Path::new(&project.path).to_owned()
        };
        let snapshot = self
            .projects
            .test_snapshot(&task.project_id)
            .await
            .map_err(project_trust_failure)?
            .ok_or(AgentsError::InvalidInput)?;
        if snapshot.canonical_path() != project.path {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let command = snapshot
            .config()
            .agent
            .test
            .clone()
            .ok_or(AgentsError::InvalidInput)?;
        runner(AgentTestSpec {
            project_id: task.project_id,
            cwd,
            command,
            snapshot,
        })
        .await
    }
}
