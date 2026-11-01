//! Keep the confirmed review independent of the eventual checkout and ref lifecycle.
use crate::{
    model::*,
    service::{AgentsService, Result, apply_event, changed, now_ms},
    state::{AgentEvent, AgentState, CloseHow},
    store,
};
use devbox_protocol::{Fail, Internal};
use serde::{Deserialize, Serialize};
use std::path::Path;
const SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ClosedReview {
    pub changes: AgentChanges,
    pub commits: Vec<devbox_git::Commit>,
}
/// How a task closes, with what that close made: a merge's commit on the base branch, or a PR's
/// address. Both are written in the same update as the close.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Closure {
    Merged { result: String },
    PullRequest { url: String },
    Discarded,
}
impl AgentsService {
    pub(crate) async fn capture_review(
        &self,
        task: &AgentTask,
        dir: &Path,
        head: &str,
        base: &str,
    ) -> Result<ClosedReview> {
        let files = devbox_git::diff::changed_files_at(dir, base, head)
            .await
            .map_err(git_failure)?;
        let commits = devbox_git::diff::log_range_at(dir, base, head)
            .await
            .map_err(git_failure)?;
        let (ahead, behind) = devbox_git::ahead_behind(dir, base, head)
            .await
            .map_err(git_failure)?;
        let snapshot = ClosedReview {
            changes: AgentChanges {
                base: task.base_branch.clone(),
                ahead,
                behind,
                head_oid: head.into(),
                base_oid: base.into(),
                files,
            },
            commits,
        };
        crate::limits::serialized(&snapshot, SNAPSHOT_BYTES)?;
        Ok(snapshot)
    }
    pub(crate) async fn closed_review(&self, id: &str) -> Result<ClosedReview> {
        let id = id.to_owned();
        self.db
            .read(move |tx| store::closed_review(tx, &id))
            .await
            .internal("read frozen agent review")?
            .ok_or_else(|| AgentsError::ReviewUnavailable.into())
    }
    pub(crate) async fn close_record(
        &self,
        id: &str,
        closure: Closure,
        cleanup_head: String,
        usage: AgentUsage,
        review: ClosedReview,
    ) -> Result<()> {
        let (how, url, merge) = match closure {
            Closure::Merged { result } => (CloseHow::Merged, None, Some(result)),
            Closure::PullRequest { url } => (CloseHow::PullRequest, Some(url), None),
            Closure::Discarded => (CloseHow::Discarded, None, None),
        };
        let id = id.to_owned();
        let hub = self.hub.clone();
        let approvals = self.approvals.clone();
        let count = u32::try_from(review.commits.len())
            .map_err(|_| Fail::Internal("closed commit count overflow".into()))?;
        self.db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = store::get(tx, &id)? else {
                        return Ok(Err(AgentsError::NotFound { id }));
                    };
                    let previous = task.state;
                    if previous == AgentState::Closed {
                        return Ok(Err(AgentsError::BadState {
                            state: "closed".into(),
                        }));
                    }
                    task.tokens = usage.tokens;
                    task.tokens_truncated = usage.truncated;
                    task.closed_how = Some(how);
                    task.closed_ms = Some(now_ms());
                    task.closed_commits = Some(count);
                    // Rebase can rewrite the source. Cleanup tracks its actual final head,
                    // while the immutable review retains the exact pair the user confirmed.
                    task.closed_head_oid = Some(cleanup_head);
                    // Cleanup offers a squash's source branch only while this commit is on the
                    // base branch, so it is part of the close, not a later write.
                    task.closed_merge_oid = merge;
                    task.pr_url = url;
                    apply_event(&mut task, &AgentEvent::Closed { how }, now_ms());
                    approvals.forget(&id);
                    store::put_closed_review(tx, &id, &review)?;
                    store::put(tx, &task)?;
                    Ok(Ok((task, previous)))
                },
                move |(task, previous), rev| changed(&hub, task, rev, task.state != *previous),
            )
            .await
            .internal("freeze closed agent record")?
            .map_err(Fail::Domain)?;
        Ok(())
    }
}
