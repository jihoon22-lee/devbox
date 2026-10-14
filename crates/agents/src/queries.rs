use crate::state::AgentState;
use crate::{
    model::*,
    service::{AgentsService, Result},
};
use devbox_protocol::{Fail, Internal};
use std::path::{Path, PathBuf};

const PATCH_LIMIT: usize = 1024 * 1024;

fn bounded_patch(mut patch: String) -> AgentPatch {
    let truncated = patch.len() > PATCH_LIMIT;
    if truncated {
        let mut end = PATCH_LIMIT;
        while !patch.is_char_boundary(end) {
            end -= 1;
        }
        patch.truncate(end);
    }
    AgentPatch { patch, truncated }
}

impl AgentsService {
    pub(crate) async fn has_owned_rebase(
        &self,
        task: &AgentTask,
        worktree: &Path,
        branch: &str,
    ) -> Result<bool> {
        let key = task.id.clone();
        let binding = self
            .db
            .read(move |tx| crate::store::binding(tx, &key))
            .await
            .internal("read owned rebase")?;
        if !binding.is_some_and(|b| b.owned_rebase) {
            return Ok(false);
        }
        // Reuse abort's ownership rule: the recorded branch must be this task's.
        for marker in ["rebase-merge", "rebase-apply"] {
            let path = devbox_git::run::git(
                worktree,
                &["rev-parse", "--path-format=absolute", "--git-path", marker],
            )
            .await
            .map_err(git_failure)?;
            if std::fs::read_to_string(Path::new(path.trim()).join("head-name"))
                .is_ok_and(|v| v.trim() == format!("refs/heads/{branch}"))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub async fn rebase_status(&self, id: &str) -> Result<AgentRebaseStatus> {
        let task = self.get(id).await?;
        if task.state == AgentState::Closed || task.worktree_path.is_none() {
            return Ok(AgentRebaseStatus {
                owned: false,
                files: vec![],
            });
        }
        let (_, worktree, branch) = self.owned_worktree(&task).await?;
        let owned = self.has_owned_rebase(&task, &worktree, &branch).await?;
        let files = devbox_git::status(&worktree)
            .await
            .map_err(git_failure)?
            .conflicted;
        Ok(AgentRebaseStatus { owned, files })
    }
    pub(crate) async fn query_directory(&self, task: &AgentTask) -> Result<PathBuf> {
        let project = self
            .projects
            .get(&task.project_id)
            .await
            .map_err(|e| Fail::Internal(format!("agent query project: {e:?}")))?
            .ok_or_else(|| AgentsError::ProjectNotFound {
                id: task.project_id.clone(),
            })?;
        let directory = match &task.worktree_path {
            Some(path) => match std::fs::symlink_metadata(path) {
                Ok(_) => self.owned_worktree(task).await?.1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(AgentsError::UnsafeWorktree.into());
                }
                Err(e) => {
                    return Err(Fail::Internal(format!(
                        "inspect agent query directory: {e}"
                    )));
                }
            },
            None => PathBuf::from(&project.path),
        };
        for ancestor in directory.ancestors() {
            if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(AgentsError::UnsafeWorktree.into());
            }
        }
        Ok(directory)
    }

    pub async fn changes(&self, id: &str) -> Result<AgentChanges> {
        let task = self.get(id).await?;
        if task.state == AgentState::Closed {
            return Ok(self.closed_review(id).await?.changes);
        }
        let dir = self.query_directory(&task).await?;
        let head_oid = devbox_git::run::resolve(&dir, "HEAD")
            .await
            .map_err(git_failure)?;
        let base_oid = devbox_git::run::resolve(&dir, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let files = devbox_git::changed_files(&dir, &base_oid)
            .await
            .map_err(git_failure)?;
        let (ahead, behind) = devbox_git::ahead_behind(&dir, &base_oid, &head_oid)
            .await
            .map_err(git_failure)?;
        query_snapshot(&dir, &task.base_branch, &head_oid, &base_oid).await?;
        Ok(AgentChanges {
            base: task.base_branch,
            ahead,
            behind,
            head_oid,
            base_oid,
            files,
        })
    }

    pub async fn patch(&self, p: GetAgentPatch) -> Result<AgentPatch> {
        let task = self.get(&p.id).await?;
        if task.state == AgentState::Closed {
            let review = self.closed_review(&p.id).await?;
            let project = self
                .projects
                .get(&task.project_id)
                .await
                .map_err(|e| Fail::Internal(format!("closed review project: {e:?}")))?
                .ok_or(AgentsError::ReviewUnavailable)?;
            let dir = Path::new(&project.path);
            for oid in [&review.changes.base_oid, &review.changes.head_oid] {
                if let Err(error) = devbox_git::run::resolve(dir, oid).await {
                    tracing::warn!(?error, agent = %task.id, "frozen review object unavailable");
                    return Err(AgentsError::ReviewUnavailable.into());
                }
            }
            return Ok(bounded_patch(
                devbox_git::diff::file_patch_at(
                    dir,
                    &review.changes.base_oid,
                    &review.changes.head_oid,
                    &p.path,
                )
                .await
                .map_err(git_failure)?,
            ));
        }
        let dir = self.query_directory(&task).await?;
        let base = devbox_git::run::resolve(&dir, &task.base_branch)
            .await
            .map_err(git_failure)?;
        Ok(bounded_patch(
            devbox_git::file_patch(&dir, &base, &p.path)
                .await
                .map_err(git_failure)?,
        ))
    }

    pub async fn commits(&self, id: &str) -> Result<AgentCommits> {
        let task = self.get(id).await?;
        if task.state == AgentState::Closed {
            return Ok(AgentCommits {
                items: self.closed_review(id).await?.commits,
            });
        }
        let dir = self.query_directory(&task).await?;
        let head = devbox_git::run::resolve(&dir, "HEAD")
            .await
            .map_err(git_failure)?;
        let base = devbox_git::run::resolve(&dir, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let items = devbox_git::log_range(&dir, &base)
            .await
            .map_err(git_failure)?;
        query_snapshot(&dir, &task.base_branch, &head, &base).await?;
        Ok(AgentCommits { items })
    }

    pub async fn discard_check(&self, id: &str) -> Result<DiscardCheck> {
        let task = self.get(id).await?;
        let dir = self.query_directory(&task).await?;
        let head_oid = devbox_git::run::resolve(&dir, "HEAD")
            .await
            .map_err(git_failure)?;
        let base = devbox_git::run::resolve(&dir, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let unmerged_commits = devbox_git::mutate::unmerged_count(&dir, &head_oid, &base)
            .await
            .map_err(git_failure)?;
        let out = devbox_git::run::git(
            &dir,
            &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
        )
        .await
        .map_err(git_failure)?;
        let dirty_files = dirty_count(&out)?;
        let fingerprint = if task.worktree_path.is_some() {
            Some(self.discard_fingerprint(&dir, &head_oid, &base).await?)
        } else {
            None
        };
        query_snapshot(&dir, &task.base_branch, &head_oid, &base).await?;
        Ok(DiscardCheck {
            head_oid,
            unmerged_commits,
            dirty_files,
            fingerprint,
        })
    }
}

async fn query_snapshot(dir: &Path, branch: &str, head: &str, base: &str) -> Result<()> {
    let current_head = devbox_git::run::resolve(dir, "HEAD")
        .await
        .map_err(git_failure)?;
    let current_base = devbox_git::run::resolve(dir, branch)
        .await
        .map_err(git_failure)?;
    if current_head != head || current_base != base {
        let unmerged = devbox_git::mutate::unmerged_count(dir, &current_head, &current_base)
            .await
            .map_err(git_failure)?;
        return Err(AgentsError::StateChanged {
            head: current_head,
            base: current_base,
            unmerged,
        }
        .into());
    }
    Ok(())
}

impl AgentsService {
    pub(crate) async fn dirty_paths(&self, dir: &Path) -> Result<Vec<String>> {
        let out = devbox_git::run::git(
            dir,
            &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
        )
        .await
        .map_err(git_failure)?;
        parse_dirty_paths(&out)
    }
}

fn dirty_count(out: &str) -> Result<u32> {
    u32::try_from(parse_dirty_paths(out)?.len())
        .map_err(|_| Fail::Internal("Git dirty count overflow".into()))
}

fn parse_dirty_paths(out: &str) -> Result<Vec<String>> {
    let mut entries = out.split_terminator('\0');
    let mut paths = Vec::new();
    while let Some(entry) = entries.next() {
        let path = entry
            .get(3..)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Fail::Internal("invalid Git status record".into()))?;
        paths.push(path.to_owned());
        if entry.as_bytes()[..2]
            .iter()
            .any(|b| matches!(b, b'R' | b'C'))
        {
            entries
                .next()
                .ok_or_else(|| Fail::Internal("invalid Git rename record".into()))?;
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_count_deduplicates_index_and_worktree_and_rename_source() {
        assert_eq!(
            dirty_count("MM file\0R  new\0old\0?? untracked\0").expect("owned fixture"),
            3
        );
        assert!(dirty_count("R  new\0").is_err());
    }
    #[test]
    fn patch_limit_preserves_utf8() {
        let result = bounded_patch(format!("{}가", "x".repeat(PATCH_LIMIT - 1)));
        assert!(result.truncated);
        assert_eq!(result.patch.len(), PATCH_LIMIT - 1);
        assert!(!bounded_patch("small".into()).truncated);
    }
}
