//! Source Git targets resolve through registered projects and persisted agent ownership.
use super::state::Services;
use devbox_git::{
    hunks::FileAction,
    model::*,
    rpc::*,
    service::{GitService, RemoteAction},
};
use devbox_protocol::{Done, Fail, Router};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
type R<T> = Result<T, Fail<SourceGitError>>;
static REV: AtomicU64 = AtomicU64::new(0);
pub(super) fn service(hub: &Arc<devbox_core::Hub>) -> Arc<GitService> {
    let changed = hub.clone();
    let operations = hub.clone();
    Arc::new(GitService::new(
        Arc::new(move |event| {
            changed.publish::<GitChangedTopic>(REV.fetch_add(1, Ordering::SeqCst) + 1, &event)
        }),
        Arc::new(move |event| {
            operations
                .publish::<GitOperationChanged>(REV.fetch_add(1, Ordering::SeqCst) + 1, &event)
        }),
    ))
}
async fn root(s: &Services, target: &SourceTarget) -> R<PathBuf> {
    let project = s
        .projects
        .get(&target.project_id)
        .await
        .map_err(|_| SourceGitError::InvalidTarget)?
        .ok_or(SourceGitError::InvalidTarget)?;
    let path = if let Some(id) = &target.worktree_id {
        let task = s
            .agents
            .get(id)
            .await
            .map_err(|_| SourceGitError::InvalidTarget)?;
        if task.project_id != target.project_id {
            return Err(SourceGitError::InvalidTarget.into());
        }
        s.agents
            .owned_worktree(&task)
            .await
            .map_err(|_| SourceGitError::UnsafeSource)?
            .1
    } else {
        PathBuf::from(project.path)
    };
    let canonical = tokio::fs::canonicalize(&path)
        .await
        .map_err(|_| SourceGitError::InvalidTarget)?;
    let repo = devbox_git::run::git(&canonical, &["rev-parse", "--show-toplevel"])
        .await
        .map_err(devbox_git::remote::failure)?;
    if tokio::fs::canonicalize(repo.trim_end_matches('\n'))
        .await
        .map_err(|_| SourceGitError::InvalidTarget)?
        != canonical
    {
        return Err(SourceGitError::InvalidTarget.into());
    }
    s.git.observe(&canonical, target).await?;
    Ok(canonical)
}
async fn done(s: &Services, target: &SourceTarget, result: Result<(), SourceGitError>) -> R<Done> {
    result?;
    s.git.changed(target);
    Ok(Done {
        rev: (REV.fetch_add(1, Ordering::SeqCst) + 1).to_string(),
    })
}
async fn map<T>(dir: &std::path::Path, result: Result<T, devbox_git::GitError>) -> R<T> {
    match result {
        Ok(v) => Ok(v),
        Err(e) => Err(devbox_git::service::failure(dir, e).await.into()),
    }
}
async fn file(s: &Services, p: GitFileChange, action: FileAction) -> R<Done> {
    let dir = root(s, &p.target).await?;
    let _guard = s.git.lock(&dir).await?;
    let result =
        devbox_git::hunks::change(&dir, &p.path, p.hunk.as_deref(), &p.expect, action).await;
    map(&dir, result).await?;
    done(s, &p.target, Ok(())).await
}
async fn candidates(
    s: &Services,
    target: &SourceTarget,
    dir: &std::path::Path,
) -> R<Vec<devbox_git::cleanup::CleanupCandidate>> {
    if target.worktree_id.is_some() {
        return Err(SourceGitError::InvalidTarget.into());
    }
    let owned = s
        .agents
        .cleanup_candidates(devbox_agents::CleanupCandidates {
            project_id: target.project_id.clone(),
        })
        .await
        .map_err(|_| SourceGitError::UnsafeSource)?;
    map(
        dir,
        devbox_git::cleanup::candidates(dir, &owned.worktrees, &owned.branches).await,
    )
    .await
}
pub(super) fn register(router: Router<Services>) -> Router<Services> {
    router
        .register::<GitStatus, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            map(&dir, devbox_git::source_queries::status(&dir).await).await
        })
        .register::<GitDiff, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            map(
                &dir,
                devbox_git::hunks::snapshot(&dir, &p.path, p.staged).await,
            )
            .await
        })
        .register::<GitBranches, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            map(&dir, devbox_git::source_queries::branches(&dir).await).await
        })
        .register::<GitLog, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            map(
                &dir,
                devbox_git::blame::log(&dir, p.path.as_deref(), p.limit, p.skip).await,
            )
            .await
        })
        .register::<GitBlame, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            map(&dir, devbox_git::blame::blame(&dir, &p.path).await).await
        })
        .register::<GitWorktrees, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            map(&dir, devbox_git::cleanup::worktrees(&dir).await).await
        })
        .register::<GitStashList, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            map(&dir, devbox_git::stash::list(&dir).await).await
        })
        .register::<GitCleanupCandidates, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            candidates(&s, &p.target, &dir).await
        })
        .register::<GitStage, _, _>(|s, _, p| async move { file(&s, p, FileAction::Stage).await })
        .register::<GitUnstage, _, _>(
            |s, _, p| async move { file(&s, p, FileAction::Unstage).await },
        )
        .register::<GitDiscard, _, _>(
            |s, _, p| async move { file(&s, p, FileAction::Discard).await },
        )
        .register::<GitCommit, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            let _guard = s.git.lock(&dir).await?;
            done(
                &s,
                &p.target,
                devbox_git::branches::commit(&dir, &p.message, p.amend, &p.expect).await,
            )
            .await
        })
        .register::<GitCheckout, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            let _guard = s.git.lock(&dir).await?;
            done(
                &s,
                &p.target,
                devbox_git::branches::checkout(
                    &dir,
                    &p.branch,
                    p.create.unwrap_or(false),
                    &p.expect,
                )
                .await,
            )
            .await
        })
        .register::<GitBranchDelete, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            let _guard = s.git.lock(&dir).await?;
            done(
                &s,
                &p.target,
                devbox_git::branches::delete(&dir, &p.branch, &p.oid, p.force, &p.expect).await,
            )
            .await
        })
        .register::<GitStash, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            let _guard = s.git.lock(&dir).await?;
            let result = match p.payload {
                GitStashAction::Push {
                    message,
                    include_untracked,
                } => devbox_git::stash::push(&dir, &message, include_untracked, &p.expect).await,
                GitStashAction::Apply { oid } => {
                    devbox_git::stash::apply(&dir, &oid, false, &p.expect).await
                }
                GitStashAction::Pop { oid } => {
                    devbox_git::stash::apply(&dir, &oid, true, &p.expect).await
                }
            };
            done(&s, &p.target, result).await
        })
        .register::<GitStashDrop, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            let _guard = s.git.lock(&dir).await?;
            done(
                &s,
                &p.target,
                devbox_git::stash::drop(&dir, &p.oid, &p.expect).await,
            )
            .await
        })
        .register::<GitFetch, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            Ok(s.git.begin(dir, p.target, RemoteAction::Fetch).await?)
        })
        .register::<GitPull, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            Ok(s.git
                .begin(
                    dir,
                    p.target,
                    RemoteAction::Pull {
                        expect: p.expect,
                        rebase: p.rebase,
                    },
                )
                .await?)
        })
        .register::<GitPush, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            Ok(s.git
                .begin(
                    dir,
                    p.target,
                    RemoteAction::Push {
                        expect: p.expect,
                        force: p.force_with_lease,
                    },
                )
                .await?)
        })
        .register::<GitOperationStatus, _, _>(|s, _, p| async move {
            root(&s, &p.target).await?;
            Ok(s.git.operation(&p).await?)
        })
        .register::<GitOperationCancel, _, _>(|s, _, p| async move {
            root(&s, &p.target).await?;
            Ok(s.git.cancel(&p).await?)
        })
        .register::<GitCleanup, _, _>(|s, _, p| async move {
            let dir = root(&s, &p.target).await?;
            let _guard = s.git.lock(&dir).await?;
            let current = candidates(&s, &p.target, &dir).await?;
            if p.candidates.is_empty()
                || p.candidates.iter().any(|wanted| {
                    !current.iter().any(|now| {
                        now.path == wanted.path
                            && now.branch == wanted.branch
                            && now.oid == wanted.oid
                    })
                })
            {
                return Err(SourceGitError::StateChanged {
                    current: devbox_git::remote::state(&dir)
                        .await
                        .map_err(devbox_git::remote::failure)?,
                }
                .into());
            }
            s.agents
                .cleanup(devbox_agents::CleanupAgents {
                    project_id: p.target.project_id.clone(),
                    worktrees: p.candidates.iter().filter_map(|c| c.path.clone()).collect(),
                    branches: p.candidates.iter().map(|c| c.branch.clone()).collect(),
                })
                .await
                .map_err(|_| SourceGitError::UnsafeSource)?;
            done(&s, &p.target, Ok(())).await
        })
}
