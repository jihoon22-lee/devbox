//! Serialized user actions. Hooks still update the latest database row independently.
use crate::{
    model::*,
    service::{AgentsService, Result, apply_event, now_ms},
    state::{AgentEvent, AgentState, CloseHow},
};
use devbox_protocol::{Fail, Internal};
use std::path::Path;

pub(crate) fn open(task: &AgentTask) -> Result<()> {
    if matches!(task.state, AgentState::Closed | AgentState::Preparing) {
        return Err(AgentsError::BadState {
            state: format!("{:?}", task.state).to_lowercase(),
        }
        .into());
    }
    Ok(())
}
impl AgentsService {
    pub async fn send(&self, p: SendToAgent) -> Result<AgentTask> {
        let _guard = self.actions.lock().await;
        let task = self.get(&p.id).await?;
        open(&task)?;
        if task.tool_exited
            || !matches!(
                self.tool_phase(&task).await?,
                Some(crate::tool_lifecycle::Phase::Tool)
            )
        {
            return Err(AgentsError::BadState {
                state: "tool-ended".into(),
            }
            .into());
        }
        let session = task
            .terminal_id
            .as_deref()
            .ok_or(AgentsError::SessionMissing)?;
        if !self
            .terminal
            .tmux()
            .list()
            .await
            .internal("list agent sessions")?
            .iter()
            .any(|s| s.name == session)
        {
            return Err(AgentsError::SessionMissing.into());
        }
        let instruction = p.text.clone();
        // Record the submitted instruction before the tool can synchronously emit completion hooks.
        self.update(&p.id, move |t| {
            if t.state == AgentState::Closed {
                return false;
            }
            t.follow_ups.push(FollowUp {
                at_ms: now_ms(),
                text: p.text,
            });
            apply_event(t, &AgentEvent::UserInstructed, now_ms());
            t.updated_ms = now_ms();
            true
        })
        .await?;
        self.terminal
            .tmux()
            .paste(session, &instruction, true)
            .await
            .internal("send agent instruction")?;
        self.get(&p.id).await
    }
    pub async fn review(&self, id: &str) -> Result<AgentTask> {
        let _guard = self.actions.lock().await;
        open(&self.get(id).await?)?;
        self.update(id, |t| apply_event(t, &AgentEvent::UserReview, now_ms()))
            .await?;
        self.get(id).await
    }
    pub(crate) async fn check_expect(
        &self,
        task: &AgentTask,
        repo: &Path,
        wt: &Path,
        branch: &str,
        e: &ReviewExpect,
    ) -> Result<()> {
        let head = devbox_git::run::resolve(repo, branch)
            .await
            .map_err(git_failure)?;
        let base = devbox_git::run::resolve(repo, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let unmerged = devbox_git::mutate::unmerged_count(repo, &head, &base)
            .await
            .map_err(git_failure)?;
        let actual = devbox_git::run::resolve(wt, "HEAD")
            .await
            .map_err(git_failure)?;
        let identity = devbox_git::current_branch(wt).await.map_err(git_failure)?;
        if head != e.head_oid
            || actual != head
            || identity.as_deref() != Some(branch)
            || e.base_oid.as_ref().is_some_and(|v| v != &base)
            || e.unmerged_commits.is_some_and(|v| v != unmerged)
        {
            return Err(AgentsError::StateChanged {
                head,
                base,
                unmerged,
            }
            .into());
        }
        Ok(())
    }
    async fn mark_rebase(&self, id: &str, owned: bool) -> Result<()> {
        let id = id.to_owned();
        self.db
            .write(move |tx| {
                let mut binding =
                    crate::store::binding(tx, &id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                binding.owned_rebase = owned;
                crate::store::put_binding(tx, &id, &binding)
            })
            .await
            .internal("record owned rebase")?;
        Ok(())
    }
    pub async fn rebase(&self, id: &str) -> Result<AgentRebase> {
        let _guard = self.actions.lock().await;
        let task = self.get(id).await?;
        open(&task)?;
        let (project, wt, branch) = self.owned_worktree(&task).await?;
        let repo = Path::new(&project.path);
        let head = devbox_git::run::resolve(repo, &branch)
            .await
            .map_err(git_failure)?;
        self.check_expect(
            &task,
            repo,
            &wt,
            &branch,
            &ReviewExpect {
                head_oid: head,
                base_oid: None,
                unmerged_commits: None,
            },
        )
        .await?;
        let base = devbox_git::run::resolve(repo, &task.base_branch)
            .await
            .map_err(git_failure)?;
        for marker in [
            "MERGE_HEAD",
            "rebase-merge",
            "rebase-apply",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "sequencer",
        ] {
            let marker_path = devbox_git::run::git(
                &wt,
                &["rev-parse", "--path-format=absolute", "--git-path", marker],
            )
            .await
            .map_err(git_failure)?;
            if Path::new(marker_path.trim()).exists() {
                return Err(AgentsError::OperationInProgress.into());
            }
        }
        self.mark_rebase(id, true).await?;
        let result = devbox_git::mutate::rebase_onto(&wt, &base).await;
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                self.mark_rebase(id, false).await?;
                return Err(git_failure(error));
            }
        };
        match outcome {
            devbox_git::mutate::RebaseOutcome::Done => {
                self.mark_rebase(id, false).await?;
                Ok(AgentRebase {
                    result: RebaseResult::Done,
                    files: vec![],
                })
            }
            devbox_git::mutate::RebaseOutcome::Conflict { files } => Ok(AgentRebase {
                result: RebaseResult::Conflict,
                files,
            }),
        }
    }
    pub async fn rebase_abort(&self, id: &str) -> Result<AgentTask> {
        let _guard = self.actions.lock().await;
        let task = self.get(id).await?;
        open(&task)?;
        let (_, wt, branch) = self.owned_worktree(&task).await?;
        let key = id.to_owned();
        let binding = self
            .db
            .read(move |tx| crate::store::binding(tx, &key))
            .await
            .internal("read owned rebase")?;
        if !binding.is_some_and(|b| b.owned_rebase) {
            return Err(AgentsError::OperationInProgress.into());
        }
        // Only abort a rebase whose recorded branch is the task's branch.
        let mut owned = false;
        for marker in ["rebase-merge", "rebase-apply"] {
            let path = devbox_git::run::git(
                &wt,
                &["rev-parse", "--path-format=absolute", "--git-path", marker],
            )
            .await
            .map_err(git_failure)?;
            if std::fs::read_to_string(Path::new(path.trim()).join("head-name"))
                .is_ok_and(|v| v.trim() == format!("refs/heads/{branch}"))
            {
                owned = true;
            }
        }
        if !owned {
            return Err(AgentsError::OperationInProgress.into());
        }
        devbox_git::mutate::rebase_abort(&wt)
            .await
            .map_err(git_failure)?;
        self.mark_rebase(id, false).await?;
        self.get(id).await
    }
    async fn mutation_failure(
        &self,
        task: &AgentTask,
        repo: &Path,
        error: devbox_git::GitError,
    ) -> Fail<AgentsError> {
        if !matches!(error, devbox_git::GitError::StateChanged) {
            return git_failure(error);
        }
        let Some(branch) = &task.branch else {
            return git_failure(error);
        };
        let head = devbox_git::run::resolve(repo, branch).await;
        let base = devbox_git::run::resolve(repo, &task.base_branch).await;
        if let (Ok(head), Ok(base)) = (head, base)
            && let Ok(unmerged) = devbox_git::mutate::unmerged_count(repo, &head, &base).await
        {
            return AgentsError::StateChanged {
                head,
                base,
                unmerged,
            }
            .into();
        }
        git_failure(error)
    }
    pub async fn merge(&self, p: MergeAgent) -> Result<AgentTask> {
        let _guard = self.actions.lock().await;
        let task = self.get(&p.id).await?;
        open(&task)?;
        let (project, wt, branch) = self.owned_worktree(&task).await?;
        let repo = Path::new(&project.path);
        self.check_expect(&task, repo, &wt, &branch, &p.expect)
            .await?;
        let commits =
            devbox_git::mutate::unmerged_count(repo, &p.expect.head_oid, &task.base_branch)
                .await
                .map_err(git_failure)?;
        let strategy = match p.strategy {
            MergeKind::Merge => devbox_git::mutate::MergeStrategy::Merge,
            MergeKind::Squash => devbox_git::mutate::MergeStrategy::Squash {
                message: p
                    .message
                    .unwrap_or_else(|| format!("{} (agent)", task.title)),
            },
            MergeKind::Rebase => devbox_git::mutate::MergeStrategy::Rebase {
                worktree: wt.clone(),
            },
        };
        if !devbox_git::status(&wt).await.map_err(git_failure)?.clean {
            return Err(AgentsError::WorktreeDirty {
                files: self.dirty_paths(&wt).await?,
            }
            .into());
        }
        let merged = devbox_git::mutate::merge_into_at(
            repo,
            &task.base_branch,
            &p.expect.head_oid,
            strategy,
            p.expect.base_oid.as_deref(),
            Some(&branch),
        )
        .await;
        let closed_head = match merged {
            Ok(outcome) => outcome.source_sha,
            Err(error) => return Err(self.mutation_failure(&task, repo, error).await),
        };
        let usage = self.closure_usage(&task.id).await;
        self.close_record(
            &task.id,
            CloseHow::Merged,
            commits,
            None,
            closed_head,
            usage,
        )
        .await?;
        if self.cleanup_after_close {
            self.cleanup_closed(&task.id).await;
        }
        self.get(&task.id).await
    }
    pub async fn pr(&self, p: PrAgent) -> Result<AgentTask> {
        let _guard = self.actions.lock().await;
        let task = self.get(&p.id).await?;
        open(&task)?;
        let (project, wt, branch) = self.owned_worktree(&task).await?;
        let repo = Path::new(&project.path);
        self.check_expect(&task, repo, &wt, &branch, &p.expect)
            .await?;
        let commits = devbox_git::log_range(&wt, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let count = u32::try_from(commits.len())
            .map_err(|_| Fail::Internal("commit count overflow".into()))?;
        let body = p.body.unwrap_or_else(|| {
            format!(
                "{}\n\n{}",
                task.prompt,
                commits
                    .iter()
                    .map(|c| format!("- {} {}", c.sha, c.subject))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
        let result = devbox_git::mutate::push_and_pr_at(
            &wt,
            &branch,
            &p.expect.head_oid,
            &task.base_branch,
            p.title.as_deref().unwrap_or(&task.title),
            &body,
        )
        .await;
        let url = match result {
            Ok(url) => url,
            Err(error) => return Err(self.mutation_failure(&task, repo, error).await),
        };
        let usage = self.closure_usage(&task.id).await;
        self.close_record(
            &task.id,
            CloseHow::PullRequest,
            count,
            Some(url),
            p.expect.head_oid.clone(),
            usage,
        )
        .await?;
        if self.cleanup_after_close {
            self.cleanup_closed(&task.id).await;
        }
        self.get(&task.id).await
    }
    pub async fn discard(&self, p: DiscardAgent) -> Result<AgentTask> {
        let _guard = self.actions.lock().await;
        let task = self.get(&p.id).await?;
        open(&task)?;
        if task.worktree_path.is_none() && task.branch.is_none() {
            let project = self
                .projects
                .get(&task.project_id)
                .await
                .map_err(|e| Fail::Internal(format!("inline discard project: {e:?}")))?
                .ok_or(AgentsError::ProjectNotFound {
                    id: task.project_id.clone(),
                })?;
            let repo = self.query_directory(&task).await?;
            if task.workdir != project.path || repo != Path::new(&project.path) {
                return Err(AgentsError::UnsafeWorktree.into());
            }
            let head = devbox_git::run::resolve(&repo, "HEAD")
                .await
                .map_err(git_failure)?;
            let base = devbox_git::run::resolve(&repo, &task.base_branch)
                .await
                .map_err(git_failure)?;
            let commits = devbox_git::mutate::unmerged_count(&repo, &head, &base)
                .await
                .map_err(git_failure)?;
            if head != p.expect.head_oid
                || p.expect.unmerged_commits != Some(commits)
                || p.expect
                    .base_oid
                    .as_ref()
                    .is_some_and(|expected| expected != &base)
            {
                return Err(AgentsError::StateChanged {
                    head,
                    base,
                    unmerged: commits,
                }
                .into());
            }
            self.quiesce(&task).await?;
            let usage = self.closure_usage(&task.id).await;
            self.close_record(
                &task.id,
                CloseHow::Discarded,
                commits,
                None,
                p.expect.head_oid.clone(),
                usage,
            )
            .await?;
            return self.get(&task.id).await;
        }
        let (project, wt, branch) = self.owned_worktree(&task).await?;
        let repo = Path::new(&project.path);
        self.check_expect(&task, repo, &wt, &branch, &p.expect)
            .await?;
        let commits =
            devbox_git::mutate::unmerged_count(repo, &p.expect.head_oid, &task.base_branch)
                .await
                .map_err(git_failure)?;
        self.quiesce(&task).await?;
        let usage = self.closure_usage(&task.id).await;
        self.teardown_commands(&task, &project).await?;
        // Hooks and teardown may have committed. Destructive removal still uses the confirmed state.
        self.owned_worktree(&task).await?;
        self.check_expect(&task, repo, &wt, &branch, &p.expect)
            .await?;
        devbox_git::mutate::worktree_remove(repo, &wt, true)
            .await
            .map_err(git_failure)?;
        if let Err(error) = devbox_git::mutate::delete_branch(repo, &branch, true).await {
            // Removal completed; retain an accurate closed record and a reviewable branch cleanup failure.
            self.close_record(
                &task.id,
                CloseHow::Discarded,
                commits,
                None,
                p.expect.head_oid.clone(),
                usage,
            )
            .await?;
            self.record_cleanup_error(&task.id, &git_failure(error))
                .await;
            return self.get(&task.id).await;
        }
        self.record_resources(&task.id, crate::tracker::Resources::Planned)
            .await?;
        self.close_record(
            &task.id,
            CloseHow::Discarded,
            commits,
            None,
            p.expect.head_oid.clone(),
            usage,
        )
        .await?;
        self.get(&task.id).await
    }
    async fn close_record(
        &self,
        id: &str,
        how: CloseHow,
        count: u32,
        url: Option<String>,
        head: String,
        usage: AgentUsage,
    ) -> Result<()> {
        self.update(id, move |t| {
            if t.state == AgentState::Closed {
                return false;
            }
            t.tokens = usage.tokens;
            t.tokens_truncated = usage.truncated;
            t.closed_how = Some(how);
            t.closed_ms = Some(now_ms());
            t.closed_commits = Some(count);
            t.closed_head_oid = Some(head);
            t.pr_url = url;
            apply_event(t, &AgentEvent::Closed { how }, now_ms());
            true
        })
        .await?;
        Ok(())
    }
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
        let command = devbox_projects::config::load(Path::new(&project.path))
            .internal("test config")?
            .agent
            .test
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
