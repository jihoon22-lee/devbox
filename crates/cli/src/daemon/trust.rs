//! Operation-aware UI guidance. Mutation authorization remains in the services.
use super::state::Services;
use devbox_projects::{ProjectsError, TrustOperation, TrustPreview, TrustPreviewParams};
use devbox_protocol::Fail;
use std::{path::Path, sync::Arc};

pub async fn preview(
    s: Arc<Services>,
    p: TrustPreviewParams,
) -> Result<TrustPreview, Fail<ProjectsError>> {
    let project = s
        .projects
        .get(&p.id)
        .await?
        .ok_or_else(|| ProjectsError::NotFound { id: p.id.clone() })?;
    let (branch, dirs) = match p.operation {
        TrustOperation::Create => {
            let dir = if p.worktree.unwrap_or(true) {
                let path = Path::new(&project.path);
                let name = path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("project");
                format!(
                    "작업 폴더를 만들면 {}/{}-agents/<새 작업 폴더> (생성할 때 이름 확정)",
                    path.parent().unwrap_or(Path::new("/")).display(),
                    name
                )
            } else {
                project.path.clone()
            };
            (true, vec![dir])
        }
        TrustOperation::Launch | TrustOperation::Run => (true, vec![project.path.clone()]),
        TrustOperation::Cleanup => {
            let candidates = s
                .agents
                .cleanup_candidates(devbox_agents::CleanupCandidates {
                    project_id: p.id.clone(),
                })
                .await
                .map_err(|_| ProjectsError::TrustContextInvalid)?;
            if p.worktrees
                .iter()
                .any(|path| !candidates.worktrees.contains(path))
            {
                return Err(ProjectsError::TrustContextInvalid.into());
            }
            (!p.worktrees.is_empty(), p.worktrees.clone())
        }
        operation => {
            let id = p
                .agent_id
                .as_deref()
                .ok_or(ProjectsError::TrustContextInvalid)?;
            let task = s
                .agents
                .get(id)
                .await
                .map_err(|_| ProjectsError::TrustContextInvalid)?;
            if task.project_id != p.id {
                return Err(ProjectsError::TrustContextInvalid.into());
            }
            let branch = match operation {
                TrustOperation::Test => true,
                TrustOperation::Discard => task.worktree_path.is_some(),
                TrustOperation::Merge | TrustOperation::Pr => {
                    task.worktree_path.is_some() && s.agents.cleanup_after_close_enabled()
                }
                _ => false,
            };
            (branch, vec![task.workdir])
        }
    };
    s.projects.trust_preview(p, branch, dirs).await
}
