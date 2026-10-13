use crate::{
    model::*,
    service::{AgentsService, Result},
    state::AgentState,
    store,
    tracker::Resources,
};
use devbox_protocol::{Done, Fail};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

fn safe_path(path: &Path) -> bool {
    path.is_absolute()
        && path
            .ancestors()
            .all(|p| !std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
        && std::fs::canonicalize(path).is_ok_and(|p| p == path)
}

impl AgentsService {
    pub(crate) async fn owned_worktree(
        &self,
        task: &AgentTask,
    ) -> Result<(devbox_projects::Project, PathBuf, String)> {
        let project = self
            .projects
            .get(&task.project_id)
            .await
            .map_err(|e| Fail::Internal(format!("agent ownership project: {e:?}")))?
            .ok_or_else(|| AgentsError::ProjectNotFound {
                id: task.project_id.clone(),
            })?;
        let (Some(path), Some(branch)) = (&task.worktree_path, &task.branch) else {
            return Err(AgentsError::UnsafeWorktree.into());
        };
        let slug = branch
            .strip_prefix("agent/")
            .ok_or(AgentsError::UnsafeWorktree)?;
        let expected = crate::plan::worktree_dir(Path::new(&project.path), slug)
            .map_err(|_| AgentsError::UnsafeWorktree)?;
        let path = PathBuf::from(path);
        if expected != path
            || task.workdir != path.to_string_lossy()
            || !safe_path(&path)
            || !safe_path(Path::new(&project.path))
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let id = task.id.clone();
        let binding = self
            .db
            .read(move |tx| store::binding(tx, &id))
            .await
            .map_err(|e| Fail::Internal(format!("agent ownership binding: {e}")))?;
        if !binding.is_some_and(|b| b.resources == Resources::Owned) {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let repo = Path::new(&project.path);
        let records = devbox_git::run::git(repo, &["worktree", "list", "--porcelain", "-z"])
            .await
            .map_err(git_failure)?;
        let registered = records.split("\0\0").skip(1).any(|record| {
            let fields: Vec<_> = record.split('\0').collect();
            fields.first().and_then(|f| f.strip_prefix("worktree ")) == path.to_str()
                && !fields
                    .iter()
                    .any(|f| f.starts_with("locked") || f.starts_with("prunable"))
        });
        let common = devbox_git::run::git(
            repo,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await
        .map_err(git_failure)?;
        let owned_common = devbox_git::run::git(
            &path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await
        .map_err(git_failure)?;
        if !registered
            || std::fs::canonicalize(common.trim_end_matches('\n'))
                .map_err(|_| AgentsError::UnsafeWorktree)?
                != std::fs::canonicalize(owned_common.trim_end_matches('\n'))
                    .map_err(|_| AgentsError::UnsafeWorktree)?
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        Ok((project, path, branch.clone()))
    }

    async fn owned_absent_branch(
        &self,
        task: &AgentTask,
    ) -> Result<(devbox_projects::Project, String)> {
        if task.state != AgentState::Closed {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let project = self
            .projects
            .get(&task.project_id)
            .await
            .map_err(|e| Fail::Internal(format!("agent ownership project: {e:?}")))?
            .ok_or_else(|| AgentsError::ProjectNotFound {
                id: task.project_id.clone(),
            })?;
        let (Some(path), Some(branch)) = (&task.worktree_path, &task.branch) else {
            return Err(AgentsError::UnsafeWorktree.into());
        };
        let slug = branch
            .strip_prefix("agent/")
            .ok_or(AgentsError::UnsafeWorktree)?;
        let expected = crate::plan::worktree_dir(Path::new(&project.path), slug)
            .map_err(|_| AgentsError::UnsafeWorktree)?;
        if expected != Path::new(path)
            || task.workdir != *path
            || !safe_path(Path::new(&project.path))
            || !expected.parent().is_some_and(safe_path)
            || !std::fs::symlink_metadata(&expected)
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let id = task.id.clone();
        let binding = self
            .db
            .read(move |tx| store::binding(tx, &id))
            .await
            .map_err(|e| Fail::Internal(format!("agent ownership binding: {e}")))?;
        if !binding.is_some_and(|b| b.resources == Resources::Owned) {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let tasks = self.cleanup_tasks(&task.project_id).await?;
        if tasks.iter().any(|t| {
            t.state != AgentState::Closed
                && (t.branch == task.branch || t.worktree_path == task.worktree_path)
        }) {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let repo = Path::new(&project.path);
        if task.closed_head_oid.as_deref()
            != Some(
                devbox_git::run::resolve(repo, branch)
                    .await
                    .map_err(git_failure)?
                    .as_str(),
            )
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        let records = devbox_git::run::git(repo, &["worktree", "list", "--porcelain", "-z"])
            .await
            .map_err(git_failure)?;
        // Stale and locked registrations also retain the ref; never prune metadata here.
        if records
            .split_terminator('\0')
            .any(|f| f == format!("worktree {path}") || f == format!("branch refs/heads/{branch}"))
            || !devbox_git::branches(repo)
                .await
                .map_err(git_failure)?
                .contains(branch)
            || devbox_git::mutate::unmerged_count(repo, branch, &task.base_branch)
                .await
                .map_err(git_failure)?
                != 0
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        Ok((project, branch.clone()))
    }

    async fn cleanup_tasks(&self, project_id: &str) -> Result<Vec<AgentTask>> {
        let project_id = project_id.to_owned();
        self.db
            .read(move |tx| {
                Ok(store::list(tx, i64::MIN)?
                    .into_iter()
                    .filter(|t| t.project_id == project_id)
                    .collect())
            })
            .await
            .map_err(|e| Fail::Internal(format!("agent cleanup tasks: {e}")))
    }

    pub async fn cleanup_candidates(&self, p: CleanupCandidates) -> Result<CleanupOptions> {
        let tasks = self.cleanup_tasks(&p.project_id).await?;
        let mut worktrees = Vec::new();
        let mut branches = Vec::new();
        for task in tasks.iter().filter(|t| t.state == AgentState::Closed) {
            // A matching path name is never ownership evidence. Keep unknown/partial resources.
            let Ok((project, path, branch)) = self.owned_worktree(task).await else {
                if let Ok((_, branch)) = self.owned_absent_branch(task).await {
                    branches.push(branch);
                }
                continue;
            };
            if tasks.iter().any(|t| {
                t.state != AgentState::Closed
                    && (t.worktree_path == task.worktree_path || t.branch == task.branch)
            }) {
                continue;
            }
            if task.closed_head_oid.as_deref()
                != Some(
                    devbox_git::run::resolve(&path, "HEAD")
                        .await
                        .map_err(git_failure)?
                        .as_str(),
                )
            {
                continue;
            }
            if devbox_git::current_branch(&path)
                .await
                .map_err(git_failure)?
                .as_deref()
                != Some(&branch)
                || cleanup_in_progress(&path).await?
                || !devbox_git::status(&path).await.map_err(git_failure)?.clean
                || devbox_git::mutate::unmerged_count(
                    Path::new(&project.path),
                    &branch,
                    &task.base_branch,
                )
                .await
                .map_err(git_failure)?
                    != 0
            {
                continue;
            }
            worktrees.push(path.to_string_lossy().into_owned());
            branches.push(branch);
        }
        worktrees.sort();
        worktrees.dedup();
        branches.sort();
        branches.dedup();
        Ok(CleanupOptions {
            worktrees,
            branches,
        })
    }

    pub async fn cleanup(&self, p: CleanupAgents) -> Result<Done> {
        let _guard = self.actions.lock().await;
        let candidates = self
            .cleanup_candidates(CleanupCandidates {
                project_id: p.project_id.clone(),
            })
            .await?;
        let selected_paths: HashSet<_> = p.worktrees.iter().collect();
        let selected_branches: HashSet<_> = p.branches.iter().collect();
        if p.worktrees
            .iter()
            .any(|v| !candidates.worktrees.contains(v))
            || p.branches.iter().any(|v| !candidates.branches.contains(v))
        {
            return Err(AgentsError::UnsafeWorktree.into());
        }
        // Validate the entire request before removing any resource; checked-out branches require their own selected worktree.
        let tasks = self.cleanup_tasks(&p.project_id).await?;
        let selected: Vec<_> = tasks
            .into_iter()
            .filter(|t| {
                t.worktree_path
                    .as_ref()
                    .is_some_and(|v| selected_paths.contains(v))
                    || t.branch
                        .as_ref()
                        .is_some_and(|v| selected_branches.contains(v))
            })
            .collect();
        for task in &selected {
            if task
                .branch
                .as_ref()
                .is_some_and(|v| selected_branches.contains(v))
                && !task
                    .worktree_path
                    .as_ref()
                    .is_some_and(|v| selected_paths.contains(v))
            {
                self.owned_absent_branch(task).await?;
            }
        }
        for task in selected {
            if !task
                .worktree_path
                .as_ref()
                .is_some_and(|v| selected_paths.contains(v))
            {
                let current = self.get(&task.id).await?;
                let (project, branch) = self.owned_absent_branch(&current).await?;
                devbox_git::mutate::delete_branch(Path::new(&project.path), &branch, false)
                    .await
                    .map_err(git_failure)?;
                self.record_resources(&task.id, Resources::Planned).await?;
                continue;
            }
            // Stop only the closed task's resources before removing its working directory.
            let (project, path, branch) = self.owned_worktree(&task).await?;
            self.quiesce(&task).await?;
            self.teardown_commands(&task, &project).await?;
            self.owned_worktree(&task).await?;
            if self.get(&task.id).await?.state != AgentState::Closed {
                return Err(AgentsError::StateChanged {
                    head: devbox_git::run::resolve(&path, "HEAD")
                        .await
                        .map_err(git_failure)?,
                    base: devbox_git::run::resolve(&path, &task.base_branch)
                        .await
                        .map_err(git_failure)?,
                    unmerged: devbox_git::mutate::unmerged_count(&path, &branch, &task.base_branch)
                        .await
                        .map_err(git_failure)?,
                }
                .into());
            }
            if devbox_git::mutate::unmerged_count(
                Path::new(&project.path),
                &branch,
                &task.base_branch,
            )
            .await
            .map_err(git_failure)?
                != 0
            {
                return Err(AgentsError::UnsafeWorktree.into());
            }
            if devbox_git::current_branch(&path)
                .await
                .map_err(git_failure)?
                .as_deref()
                != Some(&branch)
            {
                return Err(AgentsError::UnsafeWorktree.into());
            }
            if cleanup_in_progress(&path).await? {
                return Err(AgentsError::OperationInProgress.into());
            }
            if task.closed_head_oid.as_deref()
                != Some(
                    devbox_git::run::resolve(&path, "HEAD")
                        .await
                        .map_err(git_failure)?
                        .as_str(),
                )
            {
                return Err(AgentsError::UnsafeWorktree.into());
            }
            devbox_git::mutate::worktree_remove(Path::new(&project.path), &path, false)
                .await
                .map_err(git_failure)?;
            if selected_branches.contains(&branch) {
                devbox_git::mutate::delete_branch(Path::new(&project.path), &branch, false)
                    .await
                    .map_err(git_failure)?;
                self.record_resources(&task.id, Resources::Planned).await?;
            }
        }
        Ok(Done {
            rev: self.db.rev().to_string(),
        })
    }
}

async fn cleanup_in_progress(path: &Path) -> Result<bool> {
    for marker in [
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "sequencer",
    ] {
        let location = devbox_git::run::git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-path", marker],
        )
        .await
        .map_err(git_failure)?;
        if std::fs::symlink_metadata(location.trim_end_matches('\n')).is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn ownership_rejects_symlink_ancestors_and_noncanonical_paths() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("owned");
        std::fs::create_dir(&directory).unwrap();
        assert!(safe_path(&directory));
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&directory, &alias).unwrap();
        assert!(!safe_path(&alias));
        assert!(!safe_path(&alias.join(".")));
        assert!(!safe_path(Path::new("relative")));
    }
}
