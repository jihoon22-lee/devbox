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
    let selected_worktree = matches!(p.operation, TrustOperation::Test)
        .then(|| p.agent_id.clone())
        .flatten();
    let source = selected_worktree
        .as_ref()
        .and_then(|_| dirs.first().cloned());
    let creating_worktree =
        matches!(p.operation, TrustOperation::Create) && p.worktree.unwrap_or(true);
    let mut preview = s.projects.trust_preview(p, branch, dirs).await?;
    if creating_worktree && preview.run_definitions.iter().any(|d|matches!(d,devbox_runtime::RunDef::Task(d) if d.target==devbox_runtime::RunTarget::Windows) || matches!(d,devbox_runtime::RunDef::Service(d) if d.target==devbox_runtime::RunTarget::Windows)) {
        preview.execution_directories.push("Windows 사본은 새 작업 폴더 ID로 생성할 때 확정됩니다. 기존 이름 기반 사본은 변경하지 않습니다.".into());
    } else {
        preview.execution_directories.extend(s.runs.windows_preview(
            &preview.project_id,
            source.as_deref().unwrap_or(&preview.canonical_path),
            selected_worktree.as_deref(),
            &preview.run_definitions,
        ));
    }
    Ok(preview)
}
