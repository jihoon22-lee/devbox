use super::open;
use crate::{
    model::{AgentTest, AgentsError, project_trust_failure},
    service::{AgentsService, Result},
};
use std::path::Path;

impl AgentsService {
    pub async fn test(&self, id: &str) -> Result<AgentTest> {
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
        let session = self
            .terminal
            .create_internal(
                devbox_terminal::SessionKind::Shell,
                Some(task.project_id),
                cwd,
                format!("{} 테스트", task.title),
                command,
                &[],
            )
            .await
            .map_err(|e| devbox_protocol::Fail::Internal(format!("agent test terminal: {e:?}")))?;
        Ok(AgentTest {
            terminal_id: session.id,
        })
    }
}
