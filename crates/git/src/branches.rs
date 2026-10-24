use crate::{
    GitError,
    model::*,
    remote::failure,
    run::{git, output},
};
use std::path::Path;
pub async fn expect(dir: &Path) -> Result<SourceExpect, GitError> {
    let state = crate::remote::state(dir).await?;
    let index_hash =
        crate::hunks::digest(git(dir, &["ls-files", "--stage", "-z"]).await?.as_bytes());
    let mut work = git(
        dir,
        &["diff", "--no-ext-diff", "--no-textconv", "--binary", "--"],
    )
    .await?
    .into_bytes();
    let untracked = git(dir, &["ls-files", "--others", "--exclude-standard", "-z"]).await?;
    work.extend(untracked.as_bytes());
    for path in untracked.split_terminator('\0') {
        crate::hunks::safe_path(dir, path).await?;
        let meta = tokio::fs::symlink_metadata(dir.join(path))
            .await
            .map_err(|_| GitError::Internal)?;
        if meta.len() > crate::run::MAX_OUTPUT as u64 {
            return Err(GitError::LimitExceeded);
        }
        if meta.is_symlink() {
            work.extend(
                tokio::fs::read_link(dir.join(path))
                    .await
                    .map_err(|_| GitError::Internal)?
                    .as_os_str()
                    .as_encoded_bytes(),
            );
        } else {
            work.extend(
                crate::hunks::digest(
                    &tokio::fs::read(dir.join(path))
                        .await
                        .map_err(|_| GitError::Internal)?,
                )
                .as_bytes(),
            );
        }
    }
    let worktree_hash = crate::hunks::digest(&work);
    Ok(SourceExpect {
        state,
        index_hash,
        worktree_hash,
    })
}
pub async fn guard(dir: &Path, expected: &SourceExpect) -> Result<(), SourceGitError> {
    let current = expect(dir).await.map_err(failure)?;
    if &current != expected {
        return Err(SourceGitError::StateChanged {
            current: current.state,
        });
    }
    Ok(())
}
pub async fn commit(
    dir: &Path,
    message: &str,
    amend: bool,
    expected: &SourceExpect,
) -> Result<(), SourceGitError> {
    if message.trim().is_empty() || message.len() > 65536 || message.contains('\0') {
        return Err(SourceGitError::InvalidSourceInput);
    }
    guard(dir, expected).await?;
    let mut args = vec!["commit", "--file=-"];
    if amend {
        args.push("--amend");
    }
    let (code, _, _) = crate::run::execute_input(
        crate::run::command(dir, &args),
        crate::run::TIMEOUT,
        Some(message.as_bytes()),
    )
    .await
    .map_err(failure)?;
    if code == 0 {
        Ok(())
    } else {
        Err(SourceGitError::SourceFailed)
    }
}
async fn valid(dir: &Path, branch: &str) -> Result<(), SourceGitError> {
    if branch.is_empty()
        || branch.starts_with('-')
        || branch.contains('\0')
        || branch.contains("@{")
    {
        return Err(SourceGitError::InvalidSourceInput);
    }
    if output(dir, &["check-ref-format", &format!("refs/heads/{branch}")])
        .await
        .map_err(failure)?
        .0
        != 0
    {
        return Err(SourceGitError::InvalidSourceInput);
    }
    Ok(())
}
pub async fn checkout(
    dir: &Path,
    branch: &str,
    create: bool,
    expected: &SourceExpect,
) -> Result<(), SourceGitError> {
    valid(dir, branch).await?;
    guard(dir, expected).await?;
    let args = if create {
        vec!["switch", "--no-guess", "-c", branch]
    } else {
        vec!["switch", "--no-guess", "--", branch]
    };
    git(dir, &args).await.map_err(failure)?;
    Ok(())
}
pub async fn delete(
    dir: &Path,
    branch: &str,
    oid: &str,
    force: bool,
    expected: &SourceExpect,
) -> Result<(), SourceGitError> {
    valid(dir, branch).await?;
    guard(dir, expected).await?;
    if !force {
        let head = expected
            .state
            .head_oid
            .as_deref()
            .ok_or(SourceGitError::UnsafeSource)?;
        if output(dir, &["merge-base", "--is-ancestor", oid, head])
            .await
            .map_err(failure)?
            .0
            != 0
        {
            return Err(SourceGitError::UnsafeSource);
        }
    }
    match crate::mutate::delete_branch_at(dir, branch, oid).await {
        Ok(()) => Ok(()),
        Err(error) => Err(crate::service::failure(dir, error).await),
    }
}
