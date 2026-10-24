use crate::{GitError, model::*};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct StashEntry {
    pub oid: String,
    pub reference: String,
    pub subject: String,
}
pub async fn list(dir: &Path) -> Result<Vec<StashEntry>, GitError> {
    let out = crate::run::git(dir, &["stash", "list", "--format=%H%x00%gd%x00%gs%x00"]).await?;
    let mut fields = out.split_terminator('\0');
    let mut entries = Vec::new();
    while let Some(oid) = fields.next() {
        let oid = oid.trim_start_matches('\n');
        if oid.is_empty() {
            continue;
        }
        entries.push(StashEntry {
            oid: oid.into(),
            reference: fields.next().ok_or(GitError::InvalidOutput)?.into(),
            subject: fields.next().ok_or(GitError::InvalidOutput)?.into(),
        });
    }
    Ok(entries)
}
pub async fn push(
    dir: &Path,
    message: &str,
    include_untracked: bool,
    expect: &SourceExpect,
) -> Result<(), SourceGitError> {
    if message.len() > 65536 || message.contains('\0') {
        return Err(SourceGitError::InvalidSourceInput);
    }
    crate::branches::guard(dir, expect).await?;
    let mut args = vec!["stash", "push", "-m", message];
    if include_untracked {
        args.push("--include-untracked");
    }
    args.push("--");
    crate::run::git(dir, &args)
        .await
        .map_err(crate::remote::failure)?;
    Ok(())
}
async fn entry(dir: &Path, oid: &str) -> Result<StashEntry, SourceGitError> {
    list(dir)
        .await
        .map_err(crate::remote::failure)?
        .into_iter()
        .find(|e| e.oid == oid)
        .ok_or(SourceGitError::StateChanged {
            current: crate::remote::state(dir)
                .await
                .map_err(crate::remote::failure)?,
        })
}
pub async fn apply(
    dir: &Path,
    oid: &str,
    pop: bool,
    expect: &SourceExpect,
) -> Result<(), SourceGitError> {
    let item = entry(dir, oid).await?;
    crate::branches::guard(dir, expect).await?;
    crate::run::git(dir, &["stash", "apply", "--", &item.oid])
        .await
        .map_err(crate::remote::failure)?;
    if pop {
        let now = crate::branches::expect(dir)
            .await
            .map_err(crate::remote::failure)?;
        drop(dir, oid, &now).await?;
    }
    Ok(())
}
pub async fn drop(dir: &Path, oid: &str, expect: &SourceExpect) -> Result<(), SourceGitError> {
    crate::branches::guard(dir, expect).await?;
    let item = entry(dir, oid).await?;
    crate::run::git(dir, &["stash", "drop", "--", &item.reference])
        .await
        .map_err(crate::remote::failure)?;
    Ok(())
}
