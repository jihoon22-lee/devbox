use crate::{
    GitError,
    run::{git, number, output, resolve},
};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StatusSummary {
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: Vec<String>,
    pub clean: bool,
}
pub async fn current_branch(dir: &Path) -> Result<Option<String>, GitError> {
    git(dir, &["rev-parse", "--git-dir"]).await?;
    let (code, out, err) = output(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await?;
    match code {
        0 => Ok(Some(out.trim_end_matches('\n').into())),
        1 => Ok(None),
        _ => Err(GitError::Failed {
            op: "symbolic-ref".into(),
            stderr: err.trim().into(),
        }),
    }
}
pub async fn status(dir: &Path) -> Result<StatusSummary, GitError> {
    parse_status(
        &git(
            dir,
            &[
                "status",
                "--porcelain=v2",
                "--branch",
                "--untracked-files=all",
                "-z",
            ],
        )
        .await?,
    )
}
fn parse_status(out: &str) -> Result<StatusSummary, GitError> {
    let mut s = StatusSummary::default();
    let mut records = out.split_terminator('\0');
    while let Some(entry) = records.next() {
        if let Some(head) = entry.strip_prefix("# branch.head ") {
            s.branch = (head != "(detached)").then(|| head.into());
        } else if let Some(ab) = entry.strip_prefix("# branch.ab ") {
            let (a, b) = ab.split_once(' ').ok_or(GitError::InvalidOutput)?;
            s.ahead = number(a.strip_prefix('+').ok_or(GitError::InvalidOutput)?)?;
            s.behind = number(b.strip_prefix('-').ok_or(GitError::InvalidOutput)?)?;
        } else if entry.starts_with("1 ") || entry.starts_with("2 ") {
            let fields: Vec<_> = entry
                .splitn(if entry.starts_with("2 ") { 10 } else { 9 }, ' ')
                .collect();
            if fields.len() != if entry.starts_with("2 ") { 10 } else { 9 } || fields[1].len() != 2
            {
                return Err(GitError::InvalidOutput);
            }
            s.staged += u32::from(fields[1].as_bytes()[0] != b'.');
            s.unstaged += u32::from(fields[1].as_bytes()[1] != b'.');
            if entry.starts_with("2 ") {
                records.next().ok_or(GitError::InvalidOutput)?;
            }
        } else if entry.starts_with("u ") {
            let fields: Vec<_> = entry.splitn(11, ' ').collect();
            if fields.len() != 11 {
                return Err(GitError::InvalidOutput);
            }
            s.conflicted.push(fields[10].into());
        } else if entry.starts_with("? ") {
            s.untracked += 1;
        } else if !entry.starts_with("# ") && !entry.starts_with("! ") {
            return Err(GitError::InvalidOutput);
        }
    }
    s.clean = s.staged == 0 && s.unstaged == 0 && s.untracked == 0 && s.conflicted.is_empty();
    Ok(s)
}
pub async fn ahead_behind(dir: &Path, base: &str, head: &str) -> Result<(u32, u32), GitError> {
    let base = resolve(dir, base).await?;
    let head = resolve(dir, head).await?;
    let text = git(
        dir,
        &[
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base}...{head}"),
        ],
    )
    .await?;
    let fields: Vec<_> = text.split_whitespace().collect();
    if fields.len() != 2 {
        return Err(GitError::InvalidOutput);
    }
    Ok((number(fields[1])?, number(fields[0])?))
}
pub async fn branches(dir: &Path) -> Result<Vec<String>, GitError> {
    Ok(git(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads/"],
    )
    .await?
    .lines()
    .map(String::from)
    .collect())
}
