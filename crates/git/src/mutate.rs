//! Git mutations. Callers own confirmation and task-resource authorization.
//! External Git processes are not serialized; state checks do not provide atomicity.
use crate::{
    GitError, current_branch,
    run::{git, number, resolve},
    status,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum MergeStrategy {
    Merge,
    Squash { message: String },
    Rebase { worktree: PathBuf },
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub sha: String,
    pub source_sha: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseOutcome {
    Done,
    Conflict { files: Vec<String> },
}
fn text(path: &Path) -> Result<&str, GitError> {
    path.to_str().ok_or(GitError::InvalidEncoding)
}
async fn branch_valid(repo: &Path, branch: &str) -> Result<(), GitError> {
    if branch.starts_with('-') || branch.is_empty() {
        return Err(GitError::InvalidInput);
    }
    let checked = git(repo, &["check-ref-format", "--branch", branch]).await?;
    if checked.strip_suffix('\n') != Some(branch) {
        return Err(GitError::InvalidInput);
    }
    Ok(())
}
async fn common(dir: &Path) -> Result<PathBuf, GitError> {
    let p = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?;
    std::fs::canonicalize(p.trim_end_matches('\n'))
        .map_err(|e| crate::run::internal(e, "canonicalize Git directory"))
}
async fn registered(repo: &Path, dir: &Path) -> Result<(), GitError> {
    let canonical =
        std::fs::canonicalize(dir).map_err(|e| crate::run::internal(e, "canonicalize worktree"))?;
    // Reject aliases, subdirectories, and the primary checkout even with force.
    if dir != canonical || common(repo).await? != common(dir).await? {
        return Err(GitError::UnsafeWorktree);
    }
    let records = git(repo, &["worktree", "list", "--porcelain", "-z"]).await?;
    let mut first = true;
    for record in records.split("\0\0") {
        let fields: Vec<_> = record.split('\0').collect();
        if fields.first().and_then(|s| s.strip_prefix("worktree ")) == Some(text(dir)?) {
            if first || fields.iter().any(|s| s.starts_with("locked")) {
                return Err(GitError::UnsafeWorktree);
            }
            return Ok(());
        }
        if fields.iter().any(|s| s.starts_with("worktree ")) {
            first = false;
        }
    }
    Err(GitError::UnsafeWorktree)
}
async fn progress(dir: &Path) -> Result<(), GitError> {
    for marker in [
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "sequencer",
    ] {
        let p = git(
            dir,
            &["rev-parse", "--path-format=absolute", "--git-path", marker],
        )
        .await?;
        if Path::new(p.trim_end_matches('\n')).exists() {
            return Err(GitError::OperationInProgress);
        }
    }
    Ok(())
}
async fn dirty_files(dir: &Path) -> Result<Vec<String>, GitError> {
    let out = git(
        dir,
        &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
    )
    .await?;
    let mut records = out.split_terminator('\0');
    let mut files = Vec::new();
    while let Some(r) = records.next() {
        let name = r.get(3..).ok_or(GitError::InvalidOutput)?;
        files.push(name.into());
        if r.starts_with('R')
            || r.starts_with('C')
            || r.as_bytes()
                .get(1)
                .is_some_and(|b| *b == b'R' || *b == b'C')
        {
            records.next().ok_or(GitError::InvalidOutput)?;
        }
    }
    Ok(files)
}
async fn clean(dir: &Path, base: bool) -> Result<(), GitError> {
    let files = dirty_files(dir).await?;
    if files.is_empty() {
        Ok(())
    } else if base {
        Err(GitError::BaseDirty { files })
    } else {
        Err(GitError::WorktreeDirty { files })
    }
}
async fn baseline(dir: &Path, branch: &str) -> Result<String, GitError> {
    if current_branch(dir).await?.as_deref() != Some(branch) {
        return Err(GitError::BaseNotCheckedOut {
            branch: branch.into(),
        });
    }
    progress(dir).await?;
    clean(dir, true).await?;
    resolve(dir, "HEAD").await
}
pub async fn worktree_add(
    repo: &Path,
    dir: &Path,
    branch: &str,
    base: &str,
) -> Result<(), GitError> {
    branch_valid(repo, branch).await?;
    if crate::branches(repo).await?.iter().any(|b| b == branch) {
        return Err(GitError::BranchExists {
            branch: branch.into(),
        });
    }
    if std::fs::symlink_metadata(dir).is_ok() {
        return Err(GitError::UnsafeWorktree);
    }
    if !dir.is_absolute() {
        return Err(GitError::UnsafeWorktree);
    }
    let ancestor = dir
        .ancestors()
        .skip(1)
        .find(|p| p.exists())
        .ok_or(GitError::UnsafeWorktree)?;
    if std::fs::canonicalize(ancestor)
        .map_err(|e| crate::run::internal(e, "canonicalize worktree parent"))?
        != ancestor
    {
        return Err(GitError::UnsafeWorktree);
    }
    let oid = resolve(repo, base).await?;
    if let Err(cause) = git(
        repo,
        &["worktree", "add", "-b", branch, "--", text(dir)?, &oid],
    )
    .await
    {
        let branch_exists = crate::branches(repo)
            .await
            .ok()
            .map(|branches| branches.iter().any(|b| b == branch));
        let expected = format!("worktree {}", text(dir)?);
        let worktree_registered = git(repo, &["worktree", "list", "--porcelain", "-z"])
            .await
            .ok()
            .map(|records| {
                records
                    .split_terminator('\0')
                    .any(|field| field == expected)
            });
        let path_exists = std::fs::symlink_metadata(dir).is_ok();
        if !path_exists && branch_exists == Some(false) && worktree_registered == Some(false) {
            return Err(cause);
        }
        // Hooks may write files or move refs. Report observed residual resources instead
        // of deleting them. None means inspection failed; callers must reconcile state.
        return Err(GitError::WorktreeAddPartial {
            branch: branch.into(),
            path: text(dir)?.into(),
            path_exists,
            branch_exists,
            worktree_registered,
            cause: Box::new(cause),
        });
    }
    Ok(())
}
pub async fn worktree_remove(repo: &Path, dir: &Path, force: bool) -> Result<(), GitError> {
    registered(repo, dir).await?;
    if !force {
        clean(dir, false).await?;
    }
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.extend(["--", text(dir)?]);
    git(repo, &args).await?;
    Ok(())
}
pub async fn delete_branch(repo: &Path, branch: &str, force: bool) -> Result<(), GitError> {
    branch_valid(repo, branch).await?;
    git(
        repo,
        &["branch", if force { "-D" } else { "-d" }, "--", branch],
    )
    .await?;
    Ok(())
}
pub async fn unmerged_count(repo: &Path, branch: &str, base: &str) -> Result<u32, GitError> {
    let a = resolve(repo, branch).await?;
    let b = resolve(repo, base).await?;
    number(
        git(repo, &["rev-list", "--count", &format!("{b}..{a}")])
            .await?
            .trim(),
    )
}
async fn failed_merge(
    dir: &Path,
    branch: &str,
    original: &str,
    failure: GitError,
) -> Result<MergeOutcome, GitError> {
    if current_branch(dir).await?.as_deref() != Some(branch)
        || resolve(dir, "HEAD").await? != original
    {
        return Err(GitError::StateChanged);
    }
    let files = status(dir).await?.conflicted;
    // Only called after our clean-baseline mutation. A failed reset is itself an error.
    git(dir, &["reset", "--merge", original]).await?;
    if files.is_empty() {
        Err(failure)
    } else {
        Err(GitError::Conflict { files })
    }
}
pub async fn merge_into(
    dir: &Path,
    base_branch: &str,
    source: &str,
    strategy: MergeStrategy,
) -> Result<MergeOutcome, GitError> {
    merge_into_at(dir, base_branch, source, strategy, None, None).await
}
/// Bind the base checkout to the commit shown by the confirmation dialog.
pub async fn merge_into_at(
    dir: &Path,
    base_branch: &str,
    source: &str,
    strategy: MergeStrategy,
    expected_base: Option<&str>,
    expected_branch: Option<&str>,
) -> Result<MergeOutcome, GitError> {
    let original = baseline(dir, base_branch).await?;
    if expected_base.is_some_and(|oid| oid != original) {
        return Err(GitError::StateChanged);
    }
    let source_oid = resolve(dir, source).await?;
    let mut merged_source = source_oid.clone();
    if let Some(branch) = expected_branch
        && resolve(dir, branch).await? != source_oid
    {
        return Err(GitError::StateChanged);
    }
    if let MergeStrategy::Rebase { worktree } = strategy {
        registered(dir, &worktree).await?;
        let branch = current_branch(&worktree)
            .await?
            .ok_or(GitError::UnsafeWorktree)?;
        if expected_branch.is_some_and(|expected| expected != branch)
            || (crate::branches(dir).await?.iter().any(|b| b == source) && branch != source)
        {
            return Err(GitError::StateChanged);
        }
        if resolve(&worktree, "HEAD").await? != source_oid
            || resolve(dir, &branch).await? != source_oid
        {
            return Err(GitError::StateChanged);
        }
        progress(&worktree).await?;
        clean(&worktree, false).await?;
        match rebase_onto(&worktree, &original).await? {
            RebaseOutcome::Conflict { files } => {
                rebase_abort(&worktree).await?;
                return Err(GitError::Conflict { files });
            }
            RebaseOutcome::Done => {}
        }
        let rebased = resolve(&worktree, "HEAD").await?;
        merged_source = rebased.clone();
        if current_branch(&worktree).await?.as_deref() != Some(&branch)
            || baseline(dir, base_branch).await? != original
        {
            return Err(GitError::StateChanged);
        }
        git(dir, &["merge", "--ff-only", &rebased]).await?;
    } else {
        if baseline(dir, base_branch).await? != original {
            return Err(GitError::StateChanged);
        }
        let result = match &strategy {
            MergeStrategy::Merge => git(dir, &["merge", "--no-ff", "--no-edit", &source_oid]).await,
            MergeStrategy::Squash { .. } => git(dir, &["merge", "--squash", &source_oid]).await,
            _ => unreachable!(),
        };
        if let Err(error) = result {
            return failed_merge(dir, base_branch, &original, error).await;
        }
        if let MergeStrategy::Squash { message } = strategy
            && let Err(error) = git(dir, &["commit", "-m", &message]).await
        {
            return failed_merge(dir, base_branch, &original, error).await;
        }
    }
    Ok(MergeOutcome {
        sha: resolve(dir, "HEAD").await?,
        source_sha: merged_source,
    })
}
pub async fn rebase_onto(worktree: &Path, base: &str) -> Result<RebaseOutcome, GitError> {
    progress(worktree).await?;
    clean(worktree, false).await?;
    let base = resolve(worktree, base).await?;
    match git(worktree, &["rebase", &base]).await {
        Ok(_) => Ok(RebaseOutcome::Done),
        Err(error) => {
            let files = status(worktree).await?.conflicted;
            if files.is_empty() {
                Err(error)
            } else {
                Ok(RebaseOutcome::Conflict { files })
            }
        }
    }
}
pub async fn rebase_abort(worktree: &Path) -> Result<(), GitError> {
    git(worktree, &["rebase", "--abort"]).await?;
    Ok(())
}
pub async fn merged_agent_branches(repo: &Path, base: &str) -> Result<Vec<String>, GitError> {
    let base = resolve(repo, base).await?;
    Ok(git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            &format!("--merged={base}"),
            "refs/heads/agent/",
        ],
    )
    .await?
    .lines()
    .map(String::from)
    .collect())
}
pub async fn push_and_pr(
    worktree: &Path,
    branch: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<String, GitError> {
    let oid = resolve(worktree, branch).await?;
    push_and_pr_at(worktree, branch, &oid, base, title, body).await
}
/// Push the confirmed source commit, even if the branch subsequently moves.
pub async fn push_and_pr_at(
    worktree: &Path,
    branch: &str,
    oid: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<String, GitError> {
    use std::io::Write;
    branch_valid(worktree, branch).await?;
    branch_valid(worktree, base).await?;
    let oid = resolve(worktree, oid).await?;
    if current_branch(worktree).await?.as_deref() != Some(branch)
        || resolve(worktree, "HEAD").await? != oid
        || resolve(worktree, branch).await? != oid
    {
        return Err(GitError::StateChanged);
    }
    let gh = which::which("gh").map_err(|_| GitError::GhMissing)?;
    let mut file =
        tempfile::NamedTempFile::new().map_err(|e| crate::run::internal(e, "create PR body"))?;
    file.write_all(body.as_bytes())
        .map_err(|e| crate::run::internal(e, "write PR body"))?;
    git(
        worktree,
        &["push", "origin", &format!("{oid}:refs/heads/{branch}")],
    )
    .await
    .map_err(|e| match e {
        GitError::Failed { stderr, .. } => GitError::PushFailed { stderr },
        other => other,
    })?;
    // Reuse the Git runner's sanitized repository environment and bounded execution.
    let command = crate::run::command(worktree, &[]);
    // Build a gh command while preserving authentication and removing Git routing vars.
    let mut gh_command = tokio::process::Command::new(gh);
    gh_command.current_dir(worktree).args([
        "pr",
        "create",
        "--base",
        base,
        "--head",
        branch,
        "--title",
        title,
        "--body-file",
        text(file.path())?,
    ]);
    for (key, value) in command.as_std().get_envs() {
        match value {
            Some(value) => {
                gh_command.env(key, value);
            }
            None => {
                gh_command.env_remove(key);
            }
        }
    }
    gh_command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let (code, out, err) = crate::run::execute(gh_command, crate::run::TIMEOUT).await?;
    if code != 0 {
        return Err(GitError::GhFailed {
            stderr: err.trim().into(),
        });
    }
    let url = out.trim();
    if !(url.starts_with("https://") && !url.contains(char::is_whitespace)) {
        return Err(GitError::GhInvalidOutput);
    }
    Ok(url.into())
}
