//! Populate implicit checkouts without executing configured content filters.
use crate::{GitError, run};
use std::{collections::BTreeSet, path::Path, time::Duration};
use tokio::process::Command;

enum Context<'a> {
    Worktree(&'a Path),
    #[cfg(target_os = "linux")]
    Pinned(&'a std::fs::File),
}

impl Context<'_> {
    fn command(&self, args: &[&str]) -> Command {
        match self {
            Self::Worktree(path) => run::command(path, args),
            #[cfg(target_os = "linux")]
            Self::Pinned(directory) => run::clone_command(directory, args),
        }
    }
    fn checkout_timeout(&self) -> Duration {
        match self {
            Self::Worktree(_) => run::TIMEOUT,
            #[cfg(target_os = "linux")]
            Self::Pinned(_) => run::CLONE_TIMEOUT,
        }
    }
}

/// Continue a no-checkout clone through its original pinned directory descriptor.
#[cfg(target_os = "linux")]
pub async fn pinned_checkout(directory: &std::fs::File) -> Result<(i32, String, String), GitError> {
    let context = Context::Pinned(directory);
    let (code, head, stderr) = run::execute(
        context.command(&[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            "HEAD^{commit}",
        ]),
        run::TIMEOUT,
    )
    .await?;
    // A successful clone of an unborn repository (or a missing remote HEAD)
    // has no selected commit to populate, matching Git clone's existing result.
    if code == 1 && head.is_empty() && stderr.is_empty() {
        return Ok((0, head, stderr));
    }
    if code != 0 {
        return Ok((code, head, stderr));
    }
    let head = head.trim_end_matches('\n');
    if !matches!(head.len(), 40 | 64) || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GitError::InvalidOutput);
    }
    checkout(&context, head).await
}

pub(crate) async fn worktree_checkout(directory: &Path, oid: &str) -> Result<(), GitError> {
    let (code, _, stderr) = checkout(&Context::Worktree(directory), oid).await?;
    if code == 0 {
        return Ok(());
    }
    Err(GitError::Failed {
        op: "populate implicit worktree".into(),
        stderr: stderr.trim().into(),
    })
}

async fn checkout(context: &Context<'_>, oid: &str) -> Result<(i32, String, String), GitError> {
    // Read effective names only after the new Git directory/branch exists. This
    // includes gitdir/onbranch/worktree/remote-conditional configuration and does
    // not expose helper command values or disable credentials and SSH settings.
    let (code, names, stderr) = run::execute(
        context.command(&[
            "config",
            "--includes",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\..*\\.(clean|smudge|process|required)$",
        ]),
        run::TIMEOUT,
    )
    .await?;
    let drivers = match code {
        0 => drivers(&names)?,
        1 if names.is_empty() && stderr.is_empty() => BTreeSet::new(),
        _ => return Ok((code, names, stderr)),
    };
    let mut command = context.command(&[
        "-c",
        "core.hooksPath=/dev/null",
        "reset",
        "--hard",
        "--no-recurse-submodules",
        oid,
    ]);
    if !drivers.is_empty() {
        let mut parameters = std::env::var_os("GIT_CONFIG_PARAMETERS").unwrap_or_default();
        for driver in drivers {
            for variable in ["clean", "smudge", "process"] {
                if !parameters.is_empty() {
                    parameters.push(" ");
                }
                // Git's split quoted key/value format also handles '=' and quotes
                // in a driver name; ordinary -c key=value would split '=' early.
                let key = format!("filter.{driver}.{variable}");
                parameters.push(format!("'{}'=''", key.replace('\'', "'\\''")));
            }
        }
        command.env("GIT_CONFIG_PARAMETERS", parameters);
    }
    // Keep required unchanged: selecting a required driver must refuse raw blobs
    // when its command cannot safely run. Unused required drivers do not block.
    run::execute(command, context.checkout_timeout()).await
}

fn drivers(names: &str) -> Result<BTreeSet<&str>, GitError> {
    let Some(names) = names.strip_suffix('\0') else {
        return Err(GitError::InvalidOutput);
    };
    let mut drivers = BTreeSet::new();
    for key in names.split('\0') {
        let (driver, variable) = key
            .strip_prefix("filter.")
            .and_then(|key| key.rsplit_once('.'))
            .ok_or(GitError::InvalidOutput)?;
        if !matches!(variable, "clean" | "smudge" | "process" | "required") {
            return Err(GitError::InvalidOutput);
        }
        drivers.insert(driver);
    }
    Ok(drivers)
}
