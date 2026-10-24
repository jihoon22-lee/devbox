//! Daemon-created patches; callers can select an ID, never submit patch text.
use crate::{
    GitError,
    run::{git, output},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FileExpect {
    pub fingerprint: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub id: String,
    pub patch: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DiffSnapshot {
    pub path: String,
    pub old_path: Option<String>,
    pub staged: bool,
    pub patch: String,
    pub hunks: Vec<Hunk>,
    pub file_only: bool,
    pub expect: FileExpect,
}
#[derive(Debug, Clone, Copy)]
pub enum FileAction {
    Stage,
    Unstage,
    Discard,
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) async fn safe_path(dir: &Path, path: &str) -> Result<(), GitError> {
    if path.is_empty()
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(GitError::InvalidInput);
    }
    let root = tokio::fs::canonicalize(dir)
        .await
        .map_err(|_| GitError::Internal)?;
    let mut current = root;
    let parts = Path::new(path).components().collect::<Vec<_>>();
    for (i, part) in parts.iter().enumerate() {
        current.push(part.as_os_str());
        match tokio::fs::symlink_metadata(&current).await {
            Ok(meta) if i + 1 < parts.len() && (!meta.is_dir() || meta.is_symlink()) => {
                return Err(GitError::InvalidInput);
            }
            Ok(meta) if i + 1 == parts.len() && !meta.is_file() && !meta.is_symlink() => {
                return Err(GitError::InvalidInput);
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(GitError::Internal),
        }
    }
    Ok(())
}
async fn fingerprint(dir: &Path, path: &str) -> Result<FileExpect, GitError> {
    safe_path(dir, path).await?;
    let index = git(dir, &["ls-files", "--stage", "-z", "--", path]).await?;
    if index.split_terminator('\0').any(|s| {
        s.starts_with("160000 ") || !s.split_once('\t').is_some_and(|(h, _)| h.ends_with(" 0"))
    }) {
        return Err(GitError::InvalidInput);
    }
    let mut bytes = path.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(index.into_bytes());
    let (_, head, _) = output(dir, &["rev-parse", "--verify", "HEAD"]).await?;
    bytes.extend(head.as_bytes());
    match tokio::fs::symlink_metadata(dir.join(path)).await {
        Ok(meta) => {
            bytes.extend(
                format!("{:?}:{}", meta.file_type(), meta.permissions().readonly()).as_bytes(),
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                bytes.extend(meta.permissions().mode().to_le_bytes());
            }
            if meta.len() > crate::run::MAX_OUTPUT as u64 {
                return Err(GitError::LimitExceeded);
            }
            if meta.is_symlink() {
                let link = tokio::fs::read_link(dir.join(path))
                    .await
                    .map_err(|_| GitError::Internal)?;
                bytes.extend(link.as_os_str().as_encoded_bytes());
            } else {
                bytes.extend(
                    tokio::fs::read(dir.join(path))
                        .await
                        .map_err(|_| GitError::Internal)?,
                );
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => bytes.extend(b"missing"),
        Err(_) => return Err(GitError::Internal),
    }
    Ok(FileExpect {
        fingerprint: digest(&bytes),
    })
}
pub async fn snapshot(dir: &Path, path: &str, staged: bool) -> Result<DiffSnapshot, GitError> {
    let old_path = renamed_from(dir, path).await?;
    let expect = combined_fingerprint(dir, path, old_path.as_deref()).await?;
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-renames",
    ];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    let mut patch = git(dir, &args).await?;
    if !staged
        && patch.is_empty()
        && git(dir, &["ls-files", "--error-unmatch", "--", path])
            .await
            .is_err()
        && dir.join(path).exists()
    {
        let (code, text, _) = output(
            dir,
            &[
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--no-textconv",
                "--",
                "/dev/null",
                path,
            ],
        )
        .await?;
        if code > 1 {
            return Err(GitError::InvalidOutput);
        }
        patch = text;
    }
    let file_only = old_path.is_some()
        || patch.contains("Binary files ")
        || patch.contains("GIT binary patch")
        || patch.contains("new file mode ")
        || patch.contains("deleted file mode ")
        || patch.contains("old mode ")
        || patch.contains("new mode ")
        || !patch.contains("\n@@ ");
    let mut hunks = Vec::new();
    if !file_only {
        let starts = patch
            .match_indices("\n@@ ")
            .map(|(i, _)| i + 1)
            .collect::<Vec<_>>();
        for (i, start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(patch.len());
            let body = &patch[*start..end];
            hunks.push(Hunk {
                id: digest(format!("{}:{i}:{}", expect.fingerprint, patch).as_bytes()),
                patch: body.into(),
            });
        }
    }
    if combined_fingerprint(dir, path, old_path.as_deref()).await? != expect {
        return Err(GitError::StateChanged);
    }
    Ok(DiffSnapshot {
        path: path.into(),
        old_path,
        staged,
        patch,
        hunks,
        file_only,
        expect,
    })
}
pub async fn change(
    dir: &Path,
    path: &str,
    hunk: Option<&str>,
    expect: &FileExpect,
    action: FileAction,
) -> Result<(), GitError> {
    let staged = matches!(action, FileAction::Unstage);
    let snapshot = snapshot(dir, path, staged).await?;
    if &snapshot.expect != expect {
        return Err(GitError::StateChanged);
    }
    if let Some(id) = hunk {
        let selected = snapshot
            .hunks
            .iter()
            .find(|h| h.id == id)
            .ok_or(GitError::StateChanged)?;
        let header = snapshot
            .patch
            .split_once("\n@@ ")
            .ok_or(GitError::InvalidInput)?
            .0;
        let patch = format!("{header}\n{}", selected.patch);
        let mut args = vec!["apply", "--whitespace=nowarn"];
        if !matches!(action, FileAction::Discard) {
            args.push("--cached");
        }
        if !matches!(action, FileAction::Stage) {
            args.push("--reverse");
        }
        args.push("-");
        let (code, _, _) = crate::run::execute_input(
            crate::run::command(dir, &args),
            crate::run::TIMEOUT,
            Some(patch.as_bytes()),
        )
        .await?;
        if code != 0 {
            return Err(GitError::StateChanged);
        }
    } else {
        match action {
            FileAction::Stage => {
                git(dir, &["add", "--", path]).await?;
            }
            FileAction::Unstage => {
                if output(dir, &["rev-parse", "--verify", "HEAD"]).await?.0 == 0 {
                    let mut args = vec!["restore", "--staged", "--", path];
                    if let Some(old) = snapshot.old_path.as_deref() {
                        args.push(old);
                    }
                    git(dir, &args).await?;
                } else {
                    git(dir, &["rm", "--cached", "--", path]).await?;
                }
            }
            FileAction::Discard => {
                let tracked = git(dir, &["ls-files", "-z", "--", path]).await?;
                if tracked.is_empty() {
                    tokio::fs::remove_file(dir.join(path))
                        .await
                        .map_err(|_| GitError::Internal)?;
                } else {
                    git(dir, &["restore", "--worktree", "--", path]).await?;
                }
            }
        }
    }
    Ok(())
}

async fn renamed_from(dir: &Path, path: &str) -> Result<Option<String>, GitError> {
    let out = git(
        dir,
        &["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    )
    .await?;
    let mut records = out.split_terminator('\0');
    while let Some(record) = records.next() {
        if record.starts_with("2 ") {
            let old = records.next().ok_or(GitError::InvalidOutput)?;
            if record.splitn(10, ' ').nth(9) == Some(path) {
                safe_path(dir, old).await?;
                return Ok(Some(old.into()));
            }
        }
    }
    Ok(None)
}
async fn combined_fingerprint(
    dir: &Path,
    path: &str,
    old: Option<&str>,
) -> Result<FileExpect, GitError> {
    let mut fingerprint = fingerprint(dir, path).await?.fingerprint;
    if let Some(old) = old {
        fingerprint.push_str(&self::fingerprint(dir, old).await?.fingerprint);
    }
    Ok(FileExpect {
        fingerprint: digest(fingerprint.as_bytes()),
    })
}
