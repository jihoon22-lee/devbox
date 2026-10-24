//! Ownership candidates come only from the agents domain's persisted ownership checks.
use crate::{GitError, run::git};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceWorktree {
    pub path: String,
    pub head_oid: Option<String>,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub main: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct CleanupCandidate {
    pub path: Option<String>,
    pub branch: String,
    pub oid: String,
}
pub async fn worktrees(dir: &Path) -> Result<Vec<SourceWorktree>, GitError> {
    let out = git(dir, &["worktree", "list", "--porcelain", "-z"]).await?;
    let mut items = Vec::new();
    for record in out.split("\0\0").filter(|s| !s.is_empty()) {
        let mut fields = record.split('\0');
        let path = fields
            .next()
            .and_then(|s| s.strip_prefix("worktree "))
            .ok_or(GitError::InvalidOutput)?;
        let mut item = SourceWorktree {
            path: path.into(),
            head_oid: None,
            branch: None,
            locked: false,
            prunable: false,
            main: items.is_empty(),
        };
        for field in fields {
            if let Some(oid) = field.strip_prefix("HEAD ") {
                item.head_oid = Some(oid.into());
            } else if let Some(branch) = field.strip_prefix("branch refs/heads/") {
                item.branch = Some(branch.into());
            } else if field.starts_with("locked") {
                item.locked = true;
            } else if field.starts_with("prunable") {
                item.prunable = true;
            }
        }
        items.push(item);
    }
    Ok(items)
}
pub async fn candidates(
    dir: &Path,
    owned_paths: &[String],
    owned_branches: &[String],
) -> Result<Vec<CleanupCandidate>, GitError> {
    let worktrees = worktrees(dir).await?;
    let mut items = Vec::new();
    for branch in owned_branches {
        // Prefix alone is insufficient; each input was already validated by authoritative agents service.
        if !branch.starts_with("agent/") {
            continue;
        }
        let reference = format!("refs/heads/{branch}");
        let oid = crate::run::resolve(dir, &reference).await?;
        let tree = worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(branch));
        let path = if let Some(tree) = tree {
            if tree.main
                || tree.locked
                || tree.prunable
                || !owned_paths.contains(&tree.path)
                || !crate::status(Path::new(&tree.path)).await?.clean
            {
                continue;
            }
            Some(tree.path.clone())
        } else {
            None
        };
        if crate::run::output(dir, &["merge-base", "--is-ancestor", &oid, "HEAD"])
            .await?
            .0
            != 0
        {
            continue;
        }
        items.push(CleanupCandidate {
            path,
            branch: branch.clone(),
            oid,
        });
    }
    Ok(items)
}
