//! Resolve runtime's agent selector without exposing agent storage to another domain.
use crate::{
    actions::open,
    model::AgentsError,
    service::{AgentsService, Result},
};
impl AgentsService {
    pub async fn run_worktree(&self, project_id: &str, id: &str) -> Result<(String, u16)> {
        let _guard = self.actions.lock().await;
        let task = self.get(id).await?;
        if task.project_id != project_id {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        open(&task)?;
        let (_, directory, branch) = self.owned_worktree(&task).await?;
        if devbox_git::current_branch(&directory)
            .await
            .map_err(crate::model::git_failure)?
            .as_deref()
            != Some(&branch)
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        Ok((directory.to_string_lossy().into_owned(), task.port))
    }
}
