use crate::{GitError, model::*, run::git};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub path: String,
    pub old_path: Option<String>,
    pub index_status: String,
    pub worktree_status: String,
    pub conflicted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub expect: SourceExpect,
    pub summary: crate::StatusSummary,
    pub files: Vec<SourceFile>,
    pub warnings: Vec<crate::safety::GitConfigWarning>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceBranch {
    pub name: String,
    pub oid: String,
    pub current: bool,
    pub remote: bool,
    pub upstream: Option<String>,
}
pub async fn status(dir: &Path) -> Result<SourceStatus, GitError> {
    let expect = crate::branches::expect(dir).await?;
    let out = git(
        dir,
        &["status", "--porcelain=v2", "--untracked-files=all", "-z"],
    )
    .await?;
    let mut records = out.split_terminator('\0');
    let mut files = Vec::new();
    while let Some(record) = records.next() {
        let count = match record.as_bytes().first() {
            Some(b'1') => 9,
            Some(b'2') => 10,
            Some(b'u') => 11,
            Some(b'?') => {
                files.push(SourceFile {
                    path: record[2..].into(),
                    old_path: None,
                    index_status: "?".into(),
                    worktree_status: "?".into(),
                    conflicted: false,
                });
                continue;
            }
            _ => return Err(GitError::InvalidOutput),
        };
        let fields = record.splitn(count, ' ').collect::<Vec<_>>();
        if fields.len() != count || fields[1].len() != 2 {
            return Err(GitError::InvalidOutput);
        }
        let old_path = if count == 10 {
            Some(records.next().ok_or(GitError::InvalidOutput)?.into())
        } else {
            None
        };
        files.push(SourceFile {
            path: fields[count - 1].into(),
            old_path,
            index_status: fields[1][0..1].into(),
            worktree_status: fields[1][1..2].into(),
            conflicted: count == 11,
        });
    }
    let summary = crate::status(dir).await?;
    let warnings = crate::safety::warnings(dir).await?;
    if crate::branches::expect(dir).await? != expect {
        return Err(GitError::StateChanged);
    }
    Ok(SourceStatus {
        expect,
        summary,
        files,
        warnings,
    })
}
pub async fn branches(dir: &Path) -> Result<Vec<SourceBranch>, GitError> {
    let out = git(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(HEAD)%00%(upstream)%00",
            "refs/heads/",
            "refs/remotes/",
        ],
    )
    .await?;
    let mut fields = out.split_terminator('\0');
    let mut result = Vec::new();
    while let Some(name) = fields.next() {
        let name = name.trim_start_matches('\n');
        if name.is_empty() {
            continue;
        }
        let oid = fields.next().ok_or(GitError::InvalidOutput)?;
        let current = fields.next().ok_or(GitError::InvalidOutput)? == "*";
        let upstream = fields.next().ok_or(GitError::InvalidOutput)?;
        let remote = name.starts_with("refs/remotes/");
        result.push(SourceBranch {
            name: name
                .strip_prefix(if remote {
                    "refs/remotes/"
                } else {
                    "refs/heads/"
                })
                .ok_or(GitError::InvalidOutput)?
                .into(),
            oid: oid.into(),
            current,
            remote,
            upstream: (!upstream.is_empty()).then(|| upstream.into()),
        });
    }
    Ok(result)
}
