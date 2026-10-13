use crate::{
    GitError,
    run::{MAX_OUTPUT, git, merge_base, number, output, resolve},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Unmerged,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub added: u32,
    pub removed: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub time_ms: i64,
}

pub async fn changed_files(dir: &Path, base: &str) -> Result<Vec<FileChange>, GitError> {
    changed_files_between(dir, base, None).await
}
/// Compare immutable trees, without reading the current index or checkout.
pub async fn changed_files_at(
    dir: &Path,
    base: &str,
    head: &str,
) -> Result<Vec<FileChange>, GitError> {
    changed_files_between(dir, base, Some(head)).await
}
async fn comparison_base(dir: &Path, base: &str, head: &str) -> Result<String, GitError> {
    let base = resolve(dir, base).await?;
    let head = resolve(dir, head).await?;
    Ok(git(dir, &["merge-base", &base, &head]).await?.trim().into())
}
async fn changed_files_between(
    dir: &Path,
    base: &str,
    head: Option<&str>,
) -> Result<Vec<FileChange>, GitError> {
    let root = crate::run::repository_root(dir).await?;
    let dir = root.as_path();
    let base = match head {
        Some(head) => comparison_base(dir, base, head).await?,
        None => merge_base(dir, base).await?,
    };
    let mut map = changed_names(dir, &base, head).await?;
    let mut stats_args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--numstat",
        "-z",
        "-M",
        &base,
    ];
    if let Some(head) = head {
        stats_args.push(head);
    }
    stats_args.push("--");
    let stats = git(dir, &stats_args).await?;
    let mut entries = stats.split_terminator('\0');
    while let Some(line) = entries.next() {
        let mut fields = line.splitn(3, '\t');
        let added = count(fields.next().ok_or(GitError::InvalidOutput)?)?;
        let removed = count(fields.next().ok_or(GitError::InvalidOutput)?)?;
        let mut path = fields.next().ok_or(GitError::InvalidOutput)?;
        if path.is_empty() {
            entries.next().ok_or(GitError::InvalidOutput)?;
            path = entries.next().ok_or(GitError::InvalidOutput)?;
        }
        let file = map.get_mut(path).ok_or(GitError::InvalidOutput)?;
        // An unmerged path may also have a second worktree numstat record.
        file.added = added;
        file.removed = removed;
    }
    if head.is_some() {
        return Ok(map.into_values().collect());
    }
    for path in crate::status(dir).await?.conflicted {
        map.entry(path.clone())
            .and_modify(|f| f.status = ChangeStatus::Unmerged)
            .or_insert(FileChange {
                path,
                old_path: None,
                status: ChangeStatus::Unmerged,
                added: 0,
                removed: 0,
            });
    }
    for path in untracked(dir).await? {
        let existed = baseline_contains(dir, &base, &path).await?;
        let stats = if existed {
            recreated_diff(dir, &base, &path, "--numstat").await?
        } else {
            new_file_diff(dir, &path, true).await?
        };
        if existed && stats.is_empty() {
            map.remove(&path);
            continue;
        }
        let mut fields = stats.splitn(3, '\t');
        let (added, removed) = if stats.is_empty() {
            (0, 0)
        } else {
            (
                count(fields.next().ok_or(GitError::InvalidOutput)?)?,
                count(fields.next().ok_or(GitError::InvalidOutput)?)?,
            )
        };
        map.insert(
            path.clone(),
            FileChange {
                path,
                old_path: None,
                status: if existed {
                    ChangeStatus::Modified
                } else {
                    ChangeStatus::Added
                },
                added,
                removed,
            },
        );
    }
    Ok(map.into_values().collect())
}
async fn changed_names(
    dir: &Path,
    base: &str,
    head: Option<&str>,
) -> Result<BTreeMap<String, FileChange>, GitError> {
    let mut names_args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--name-status",
        "-z",
        "-M",
        base,
    ];
    if let Some(head) = head {
        names_args.push(head);
    }
    names_args.push("--");
    let names = git(dir, &names_args).await?;
    let mut map = BTreeMap::new();
    let mut entries = names.split_terminator('\0');
    while let Some(code) = entries.next() {
        let (status, old_path) = match code.as_bytes().first() {
            Some(b'A') => (ChangeStatus::Added, None),
            Some(b'D') => (ChangeStatus::Deleted, None),
            Some(b'R') => (
                ChangeStatus::Renamed,
                Some(entries.next().ok_or(GitError::InvalidOutput)?.into()),
            ),
            Some(b'U') => (ChangeStatus::Unmerged, None),
            Some(b'M' | b'T') => (ChangeStatus::Modified, None),
            _ => return Err(GitError::InvalidOutput),
        };
        let path = entries.next().ok_or(GitError::InvalidOutput)?.to_owned();
        map.insert(
            path.clone(),
            FileChange {
                path,
                old_path,
                status,
                added: 0,
                removed: 0,
            },
        );
    }
    Ok(map)
}
async fn patch_paths(
    dir: &Path,
    base: &str,
    head: Option<&str>,
    path: &str,
) -> Result<Vec<String>, GitError> {
    let changes = changed_names(dir, base, head).await?;
    let Some(old) = changes
        .get(path)
        .and_then(|change| change.old_path.as_deref())
    else {
        return Ok(vec![path.to_owned()]);
    };
    validate_path(old)?;
    let mut paths = vec![format!(":(literal){path}"), format!(":(literal){old}")];
    let old_prefix = format!("{old}/");
    let new_prefix = format!("{path}/");
    // Either endpoint can be reused as a directory. Exclude other recorded files
    // under it, never the selected endpoints (which can themselves be nested).
    // Include rename sources too: a moved-away descendant is otherwise a deletion
    // candidate when its destination lies outside this pair of pathspecs.
    let others: BTreeSet<_> = changes
        .values()
        .flat_map(|change| std::iter::once(change.path.as_str()).chain(change.old_path.as_deref()))
        .collect();
    for other in others {
        if other != old
            && other != path
            && (other.starts_with(&old_prefix) || other.starts_with(&new_prefix))
        {
            validate_path(other)?;
            paths.push(format!(":(literal,exclude){other}"));
        }
    }
    Ok(paths)
}
async fn patch_between(
    dir: &Path,
    base: &str,
    head: Option<&str>,
    path: &str,
) -> Result<String, GitError> {
    let paths = patch_paths(dir, base, head, path).await?;
    let mut args = Vec::new();
    if paths.len() > 1 {
        // Only generated literal pathspecs may opt into exclusion syntax.
        args.push("--no-literal-pathspecs");
    }
    args.extend([
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "-M",
        base,
    ]);
    if let Some(head) = head {
        args.push(head);
    }
    args.push("--");
    args.extend(paths.iter().map(String::as_str));
    git(dir, &args).await
}
fn count(s: &str) -> Result<u32, GitError> {
    if s == "-" { Ok(0) } else { number(s) }
}
async fn untracked(dir: &Path) -> Result<Vec<String>, GitError> {
    Ok(
        git(dir, &["ls-files", "--others", "--exclude-standard", "-z"])
            .await?
            .split_terminator('\0')
            .map(String::from)
            .collect(),
    )
}
fn validate_path(path: &str) -> Result<(), GitError> {
    if path.is_empty()
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(GitError::InvalidInput);
    }
    Ok(())
}
async fn inspect_untracked(dir: &Path, path: &str) -> Result<(), GitError> {
    validate_path(path)?;
    let root = tokio::fs::canonicalize(dir)
        .await
        .map_err(|error| crate::run::internal(error, "inspect untracked path"))?;
    let full = dir.join(path);
    let parent = tokio::fs::canonicalize(full.parent().ok_or(GitError::InvalidInput)?)
        .await
        .map_err(|error| crate::run::internal(error, "inspect untracked path"))?;
    if !parent.starts_with(&root) {
        return Err(GitError::InvalidInput);
    }
    let meta = tokio::fs::symlink_metadata(&full)
        .await
        .map_err(|error| crate::run::internal(error, "inspect untracked path"))?;
    if !meta.is_file() && !meta.is_symlink() {
        return Err(GitError::InvalidInput);
    }
    if meta.len() > MAX_OUTPUT as u64 {
        return Err(GitError::LimitExceeded);
    }
    Ok(())
}
async fn baseline_contains(dir: &Path, base: &str, path: &str) -> Result<bool, GitError> {
    Ok(!git(dir, &["ls-tree", "-z", base, "--", path])
        .await?
        .is_empty())
}
async fn recreated_diff(
    dir: &Path,
    base: &str,
    path: &str,
    format: &str,
) -> Result<String, GitError> {
    inspect_untracked(dir, path).await?;
    // Seed only an owned temporary index with the baseline. Git can then compare
    // the recreated file to its original blob without treating it as a new file.
    let temp =
        tempfile::tempdir().map_err(|error| crate::run::internal(error, "create query index"))?;
    let index = temp.path().join("index");
    let hooks = temp.path().join("hooks");
    tokio::fs::create_dir(&hooks)
        .await
        .map_err(|error| crate::run::internal(error, "create empty query hooks directory"))?;
    crate::run::with_index(dir, &index, &hooks, &["read-tree", base]).await?;
    crate::run::with_index(
        dir,
        &index,
        &hooks,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            format,
            base,
            "--",
            path,
        ],
    )
    .await
}
async fn new_file_diff(dir: &Path, path: &str, numstat: bool) -> Result<String, GitError> {
    inspect_untracked(dir, path).await?;
    let format = if numstat { "--numstat" } else { "--patch" };
    // Git no-index reads symlink contents as the link text, without following its target.
    let (code, out, err) = output(
        dir,
        &[
            "diff",
            "--no-index",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            format,
            "--",
            "/dev/null",
            path,
        ],
    )
    .await?;
    if code == 0 || code == 1 {
        Ok(out)
    } else {
        Err(GitError::Failed {
            op: "diff".into(),
            stderr: err.trim().into(),
        })
    }
}
pub async fn file_patch(dir: &Path, base: &str, path: &str) -> Result<String, GitError> {
    validate_path(path)?;
    let root = crate::run::repository_root(dir).await?;
    let dir = root.as_path();
    let base = merge_base(dir, base).await?;
    if untracked(dir).await?.iter().any(|p| p == path) {
        return if baseline_contains(dir, &base, path).await? {
            recreated_diff(dir, &base, path, "--patch").await
        } else {
            new_file_diff(dir, path, false).await
        };
    }
    patch_between(dir, &base, None, path).await
}
pub async fn log_range(dir: &Path, base: &str) -> Result<Vec<Commit>, GitError> {
    let head = resolve(dir, "HEAD").await?;
    log_range_at(dir, base, &head).await
}
pub async fn log_range_at(dir: &Path, base: &str, head: &str) -> Result<Vec<Commit>, GitError> {
    let base = resolve(dir, base).await?;
    let head = resolve(dir, head).await?;
    let text = git(
        dir,
        &[
            "log",
            "--format=%H%x00%s%x00%an%x00%at%x00",
            &format!("{base}..{head}"),
            "--",
        ],
    )
    .await?;
    let mut fields = text.split_terminator('\0');
    let mut result = Vec::new();
    while let Some(sha) = fields.next() {
        let sha = sha.strip_prefix('\n').unwrap_or(sha);
        if sha == "\n" || sha.is_empty() {
            continue;
        }
        let subject = fields.next().ok_or(GitError::InvalidOutput)?.into();
        let author = fields.next().ok_or(GitError::InvalidOutput)?.into();
        let time = fields
            .next()
            .ok_or(GitError::InvalidOutput)?
            .parse::<i64>()
            .map_err(|_| GitError::InvalidOutput)?;
        result.push(Commit {
            sha: sha.into(),
            subject,
            author,
            time_ms: time.checked_mul(1000).ok_or(GitError::InvalidOutput)?,
        });
    }
    Ok(result)
}

pub async fn file_patch_at(
    dir: &Path,
    base: &str,
    head: &str,
    path: &str,
) -> Result<String, GitError> {
    validate_path(path)?;
    let root = crate::run::repository_root(dir).await?;
    let base = comparison_base(&root, base, head).await?;
    let head = resolve(&root, head).await?;
    patch_between(&root, &base, Some(&head), path).await
}
