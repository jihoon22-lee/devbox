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
async fn rebase_markers_absent(worktree: &Path) -> Result<bool> {
    for marker in ["rebase-merge", "rebase-apply"] {
        let path = devbox_git::run::git(
            worktree,
            &["rev-parse", "--path-format=absolute", "--git-path", marker],
        )
        .await
        .map_err(git_failure)?;
        match std::fs::symlink_metadata(Path::new(path.trim_end_matches('\n'))) {
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => {
                return Err(Fail::Internal(format!(
                    "inspect owned rebase markers: {error}"
                )));
            }
        }
    }
    Ok(true)
}
impl AgentsService {
    async fn quiesce_confirmed_review(
        &self,
        task: &AgentTask,
        repo: &Path,
        wt: &Path,
        branch: &str,
        expect: &ReviewExpect,
    ) -> Result<()> {
        // Closing always stops the owned tool, independently of Git cleanup.
        // Stop failures happen before irreversible merge/push/PR side effects.
        self.quiesce(task).await?;
        self.owned_worktree(task).await?;
        self.check_expect(task, repo, wt, branch, expect).await?;
        if !devbox_git::status(wt).await.map_err(git_failure)?.clean {
            return Err(AgentsError::WorktreeDirty {
                files: self.dirty_paths(wt).await?,
            }
            .into());
        }
        Ok(())
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
        let base = devbox_git::run::resolve(repo, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let expected = ReviewExpect {
            head_oid: head,
            base_oid: Some(base.clone()),
            unmerged_commits: None,
            discard_fingerprint: None,
        };
        self.check_expect(&task, repo, &wt, &branch, &expected)
            .await?;
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
        self.quiesce(&task).await?;
        self.update(id, |task| {
            let exited = !task.tool_exited;
            task.tool_exited = true;
            let changed = apply_event(task, &AgentEvent::UserReview, now_ms());
            if exited && !changed {
                task.activity_seq = task.activity_seq.wrapping_add(1);
                task.updated_ms = now_ms();
            }
            exited || changed
        })
        .await?;
        self.owned_worktree(&task).await?;
        self.check_expect(&task, repo, &wt, &branch, &expected)
            .await?;
        if !devbox_git::status(&wt).await.map_err(git_failure)?.clean {
            return Err(AgentsError::WorktreeDirty {
                files: self.dirty_paths(&wt).await?,
            }
            .into());
        }
        if !rebase_markers_absent(&wt).await? {
            return Err(AgentsError::OperationInProgress.into());
        }
        self.mark_rebase(id, true).await?;
        let result = devbox_git::mutate::rebase_onto(&wt, &base).await;
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                // A failed/timed-out command can leave our rebase active. Unknown
                // inspection retains recovery ownership; abort still checks head-name.
                match rebase_markers_absent(&wt).await {
                    Ok(true) => self.mark_rebase(id, false).await?,
                    Ok(false) => (),
                    Err(inspection) => tracing::warn!(
                        ?inspection,
                        agent = id,
                        "retained owned rebase after failed marker inspection"
                    ),
                }
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
        if !self.has_owned_rebase(&task, &wt, &branch).await? {
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
        let message = p
            .message
            .unwrap_or_else(|| format!("{} (agent)", task.title));
        if matches!(p.strategy, MergeKind::Squash) {
            crate::limits::squash_message(&message)?;
        }
        let (project, wt, branch) = self.owned_worktree(&task).await?;
        let cleanup_snapshot = if self.cleanup_after_close {
            self.authorize_teardown(&task, &project).await?
        } else {
            None
        };
        let repo = Path::new(&project.path);
        self.check_expect(&task, repo, &wt, &branch, &p.expect)
            .await?;
        let base = devbox_git::run::resolve(repo, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let review = self
            .capture_review(&task, repo, &p.expect.head_oid, &base)
            .await?;
        let strategy = match p.strategy {
            MergeKind::Merge => devbox_git::mutate::MergeStrategy::Merge,
            MergeKind::Squash => devbox_git::mutate::MergeStrategy::Squash { message },
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
        self.quiesce_confirmed_review(&task, repo, &wt, &branch, &p.expect)
            .await?;
        let merged = devbox_git::mutate::merge_into_at(
            repo,
            &task.base_branch,
            &p.expect.head_oid,
            strategy,
            p.expect.base_oid.as_deref(),
            Some(&branch),
        )
        .await;
        let outcome = match merged {
            Ok(outcome) => outcome,
            Err(error) => return Err(self.mutation_failure(&task, repo, error).await),
        };
        let usage = self.closure_usage(&task.id).await;
        if let Err(error) = self
            .close_record(
                &task.id,
                CloseHow::Merged,
                None,
                outcome.source_sha.clone(),
                usage,
                review,
            )
            .await
        {
            tracing::error!(?error, agent = %task.id, merged_base = %outcome.sha, source_head = %outcome.source_sha,
                "Git merge applied; task closure persistence pending; refresh review before retry");
            return Err(error);
        }
        if self.cleanup_after_close {
            self.cleanup_closed_after_quiesce(&task.id, &cleanup_snapshot)
                .await;
        }
        self.get(&task.id).await
    }
    pub async fn pr(&self, p: PrAgent) -> Result<AgentTask> {
        let _guard = self.actions.lock().await;
        let task = self.get(&p.id).await?;
        open(&task)?;
        crate::limits::pr_title(p.title.as_deref().unwrap_or(&task.title))?;
        let (project, wt, branch) = self.owned_worktree(&task).await?;
        let cleanup_snapshot = if self.cleanup_after_close {
            self.authorize_teardown(&task, &project).await?
        } else {
            None
        };
        let repo = Path::new(&project.path);
        self.check_expect(&task, repo, &wt, &branch, &p.expect)
            .await?;
        if !devbox_git::status(&wt).await.map_err(git_failure)?.clean {
            return Err(AgentsError::WorktreeDirty {
                files: self.dirty_paths(&wt).await?,
            }
            .into());
        }
        let base = p
            .expect
            .base_oid
            .as_deref()
            .ok_or(AgentsError::InvalidInput)?;
        let review = self
            .capture_review(&task, repo, &p.expect.head_oid, base)
            .await?;
        let commits = &review.commits;
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
        self.quiesce_confirmed_review(&task, repo, &wt, &branch, &p.expect)
            .await?;
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
            Some(url),
            p.expect.head_oid.clone(),
            usage,
            review,
        )
        .await?;
        if self.cleanup_after_close {
            self.cleanup_closed_after_quiesce(&task.id, &cleanup_snapshot)
                .await;
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
            let current_head = devbox_git::run::resolve(&repo, "HEAD")
                .await
                .map_err(git_failure)?;
            let current_base = devbox_git::run::resolve(&repo, &task.base_branch)
                .await
                .map_err(git_failure)?;
            let current_commits =
                devbox_git::mutate::unmerged_count(&repo, &current_head, &current_base)
                    .await
                    .map_err(git_failure)?;
            if current_head != head || current_base != base || current_commits != commits {
                return Err(AgentsError::StateChanged {
                    head: current_head,
                    base: current_base,
                    unmerged: current_commits,
                }
                .into());
            }
            let review = self.capture_review(&task, &repo, &head, &base).await?;
            let usage = self.closure_usage(&task.id).await;
            self.close_record(
                &task.id,
                CloseHow::Discarded,
                None,
                p.expect.head_oid.clone(),
                usage,
                review,
            )
            .await?;
            return self.get(&task.id).await;
        }
        let (project, wt, branch) = self.owned_worktree(&task).await?;
        let repo = Path::new(&project.path);
        self.check_expect(&task, repo, &wt, &branch, &p.expect)
            .await?;
        let base = devbox_git::run::resolve(repo, &task.base_branch)
            .await
            .map_err(git_failure)?;
        let review = self
            .capture_review(&task, repo, &p.expect.head_oid, &base)
            .await?;
        self.check_discard_fingerprint(&wt, &p.expect.head_oid, &base, &p.expect)
            .await?;
        let snapshot = self.authorize_teardown(&task, &project).await?;
        self.quiesce(&task).await?;
        let usage = self.closure_usage(&task.id).await;
        self.teardown_commands(&task, &snapshot).await?;
        // Hooks and teardown may have committed. Destructive removal still uses the confirmed state.
        self.owned_worktree(&task).await?;
        self.check_expect(&task, repo, &wt, &branch, &p.expect)
            .await?;
        let base = devbox_git::run::resolve(repo, &task.base_branch)
            .await
            .map_err(git_failure)?;
        self.check_discard_fingerprint(&wt, &p.expect.head_oid, &base, &p.expect)
            .await?;
        // Freeze the confirmed outcome before cleanup can remove its Git resources.
        self.close_record(
            &task.id,
            CloseHow::Discarded,
            None,
            p.expect.head_oid.clone(),
            usage,
            review,
        )
        .await?;
        let cleanup: Result<()> = async {
            devbox_git::mutate::worktree_remove(repo, &wt, true)
                .await
                .map_err(git_failure)?;
            devbox_git::mutate::delete_branch_at(repo, &branch, &p.expect.head_oid)
                .await
                .map_err(git_failure)?;
            self.record_resources(&task.id, crate::tracker::Resources::Planned)
                .await?;
            Ok(())
        }
        .await;
        if let Err(error) = cleanup {
            // Closed records retain their ownership and report partial cleanup.
            self.record_cleanup_error(&task.id, &error).await;
        }
        self.get(&task.id).await
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
