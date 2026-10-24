use crate::{Commit, GitError, run::git};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BlameLine {
    pub oid: String,
    pub original_line: u32,
    pub line: u32,
    pub author: String,
    pub time_ms: i64,
    pub text: String,
}
pub async fn log(
    dir: &Path,
    path: Option<&str>,
    limit: u32,
    skip: u32,
) -> Result<Vec<Commit>, GitError> {
    if limit == 0 || limit > 500 || skip > 1_000_000 {
        return Err(GitError::InvalidInput);
    }
    if let Some(path) = path {
        crate::hunks::safe_path(dir, path).await?;
    }
    if crate::remote::state(dir).await?.head_oid.is_none() {
        return Ok(Vec::new());
    }
    let limit = format!("--max-count={limit}");
    let skip = format!("--skip={skip}");
    let mut args = vec![
        "log",
        "--no-show-signature",
        "--format=%H%x00%s%x00%an%x00%at%x00",
        &limit,
        &skip,
        "--",
    ];
    if let Some(path) = path {
        args.push(path);
    }
    let out = git(dir, &args).await?;
    let mut fields = out.split_terminator('\0');
    let mut result = Vec::new();
    while let Some(oid) = fields.next() {
        let oid = oid.trim_start_matches('\n');
        if oid.is_empty() {
            continue;
        }
        result.push(Commit {
            sha: oid.into(),
            subject: fields.next().ok_or(GitError::InvalidOutput)?.into(),
            author: fields.next().ok_or(GitError::InvalidOutput)?.into(),
            time_ms: fields
                .next()
                .ok_or(GitError::InvalidOutput)?
                .parse::<i64>()
                .map_err(|_| GitError::InvalidOutput)?
                .checked_mul(1000)
                .ok_or(GitError::InvalidOutput)?,
        });
    }
    Ok(result)
}
pub async fn blame(dir: &Path, path: &str) -> Result<Vec<BlameLine>, GitError> {
    crate::hunks::safe_path(dir, path).await?;
    let out = git(
        dir,
        &["blame", "--line-porcelain", "--no-textconv", "--", path],
    )
    .await?;
    let mut lines = out.lines();
    let mut result = Vec::new();
    while let Some(header) = lines.next() {
        let fields = header.split(' ').collect::<Vec<_>>();
        if fields.len() < 3 {
            return Err(GitError::InvalidOutput);
        }
        let mut line = BlameLine {
            oid: fields[0].into(),
            original_line: fields[1].parse().map_err(|_| GitError::InvalidOutput)?,
            line: fields[2].parse().map_err(|_| GitError::InvalidOutput)?,
            author: String::new(),
            time_ms: 0,
            text: String::new(),
        };
        let mut content = false;
        for field in lines.by_ref() {
            if let Some(text) = field.strip_prefix('\t') {
                line.text = text.into();
                content = true;
                break;
            }
            if let Some(author) = field.strip_prefix("author ") {
                line.author = author.into();
            }
            if let Some(time) = field.strip_prefix("author-time ") {
                line.time_ms = time
                    .parse::<i64>()
                    .map_err(|_| GitError::InvalidOutput)?
                    .checked_mul(1000)
                    .ok_or(GitError::InvalidOutput)?;
            }
        }
        if !content {
            return Err(GitError::InvalidOutput);
        }
        result.push(line);
    }
    Ok(result)
}
