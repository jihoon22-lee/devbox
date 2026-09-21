//! Native index/HEAD witnesses, shared by Windows and the WSL Source helper.
use super::*;
use sha2::{Digest, Sha256};

pub(super) const STALE: &str = "commit_review_stale";
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub revision: String,
    pub staged_paths: Vec<String>,
}

fn index(context: &RepositoryContext, cancel: &AtomicBool) -> Result<String, String> {
    run_git_status_bounded_with_cancel(
        &["--no-optional-locks", "ls-files", "--stage", "-z"].map(str::to_owned),
        &context.worktree,
        cancel,
    )
}
fn head(context: &RepositoryContext, cancel: &AtomicBool) -> Result<String, String> {
    let status = run_git_status_bounded_with_cancel(
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=no",
            "-z",
        ]
        .map(str::to_owned),
        &context.worktree,
        cancel,
    )?;
    let headers: Vec<_> = status
        .split(['\0', '\n'])
        .filter(|line| line.starts_with("# branch.oid ") || line.starts_with("# branch.head "))
        .collect();
    if headers.len() != 2 {
        return Err(STALE.into());
    }
    Ok(headers.join("\n"))
}
pub(super) fn capture(context: &RepositoryContext, cancel: &AtomicBool) -> Result<Review, String> {
    let before_head = head(context, cancel)?;
    let before_index = index(context, cancel)?;
    let paths = run_git_status_bounded_with_cancel(
        &[
            "--no-optional-locks",
            "diff",
            "--cached",
            "--name-only",
            "--no-ext-diff",
            "--no-textconv",
            "-z",
            "--",
        ]
        .map(str::to_owned),
        &context.worktree,
        cancel,
    )?;
    if index(context, cancel)? != before_index || head(context, cancel)? != before_head {
        return Err(STALE.into());
    }
    revalidate_repository_context(context, GIT_MUTATION_ERROR)?;
    let mut hash = Sha256::new();
    for part in [format!("{context:?}"), before_head, before_index] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    Ok(Review {
        revision: hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        staged_paths: paths
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect(),
    })
}
pub(super) fn require(
    context: &RepositoryContext,
    revision: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(STALE.into());
    }
    let current = capture(context, cancel)?;
    if current.revision != revision || current.staged_paths.is_empty() {
        return Err(STALE.into());
    }
    Ok(())
}
