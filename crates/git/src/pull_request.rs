//! Reconcile remote creation whose response may have been lost or cancelled.
use crate::{GitError, run};
use std::{path::Path, process::Stdio};
use tokio::process::Command;

pub(super) async fn create(
    worktree: &Path,
    gh: &Path,
    branch: &str,
    oid: &str,
    base: &str,
    title: &str,
    body: &Path,
) -> Result<String, GitError> {
    // A cancelled prior attempt may already have created the PR. Lookup errors
    // cannot establish absence, so never create when this preflight fails.
    if let Some(url) = existing(worktree, gh, branch, oid, base).await? {
        return Ok(url);
    }
    let body = body.to_str().ok_or(GitError::InvalidEncoding)?;
    let command = command(
        worktree,
        gh,
        &[
            "pr",
            "create",
            "--base",
            base,
            "--head",
            branch,
            "--title",
            title,
            "--body-file",
            body,
        ],
    );
    let failure = match run::execute(command, run::TIMEOUT).await {
        Ok((0, out, _)) if safe_url(out.trim()) => return Ok(out.trim().into()),
        Ok((0, _, _)) => GitError::GhInvalidOutput,
        Ok((_, _, err)) => GitError::GhFailed {
            stderr: err.trim().into(),
        },
        Err(error) => error,
    };
    // Timeout, nonzero exit, or invalid output does not prove remote creation
    // failed. Only an exact verified PR can turn that ambiguous failure into success.
    match existing(worktree, gh, branch, oid, base).await {
        Ok(Some(url)) => Ok(url),
        _ => Err(failure),
    }
}

async fn existing(
    worktree: &Path,
    gh: &Path,
    branch: &str,
    oid: &str,
    base: &str,
) -> Result<Option<String>, GitError> {
    let command = command(
        worktree,
        gh,
        &[
            "pr",
            "list",
            "--state",
            "open",
            "--base",
            base,
            "--head",
            branch,
            "--limit",
            "100",
            "--json",
            "url,baseRefName,headRefName,headRefOid,isCrossRepository,state",
            "--jq",
            ".[] | select(.isCrossRepository == false) | [.url,.baseRefName,.headRefName,.headRefOid,.isCrossRepository,.state] | @tsv",
        ],
    );
    let (code, out, err) = run::execute(command, run::TIMEOUT).await?;
    if code != 0 {
        return Err(GitError::GhFailed {
            stderr: err.trim().into(),
        });
    }
    let mut rows = out.lines().filter(|row| !row.is_empty());
    let Some(row) = rows.next() else {
        return Ok(None);
    };
    if rows.next().is_some() {
        return Err(GitError::GhInvalidOutput);
    }
    let fields: Vec<_> = row.split('\t').collect();
    if fields.len() != 6
        || !safe_url(fields[0])
        || fields[1] != base
        || fields[2] != branch
        || fields[4] != "false"
        || fields[5] != "OPEN"
    {
        return Err(GitError::GhInvalidOutput);
    }
    if fields[3] != oid {
        return Err(GitError::StateChanged);
    }
    Ok(Some(fields[0].into()))
}

fn safe_url(url: &str) -> bool {
    let Some(remainder) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = remainder.split('/').next().unwrap_or_default();
    !authority.is_empty()
        && !authority.contains('@')
        && !url.contains(['\\', '?', '#'])
        && !url
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
}

fn command(worktree: &Path, gh: &Path, args: &[&str]) -> Command {
    // Preserve configured authentication while removing the Git routing vars.
    let environment = run::command(worktree, &[]);
    let mut command = Command::new(gh);
    command.current_dir(worktree).args(args);
    for (key, value) in environment.as_std().get_envs() {
        match value {
            Some(value) => {
                command.env(key, value);
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}
