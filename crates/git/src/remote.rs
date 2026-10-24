//! Remote commands use the confirmed branch and immutable source OID.
use crate::{
    GitError,
    model::*,
    run::{git, output},
};
use std::{path::Path, time::Duration};
async fn optional(dir: &Path, args: &[&str]) -> Result<Option<String>, GitError> {
    let (code, out, _) = output(dir, args).await?;
    match code {
        0 => Ok(Some(out.trim_end_matches('\n').into())),
        1 | 128 => Ok(None),
        _ => Err(GitError::InvalidOutput),
    }
}
pub async fn state(dir: &Path) -> Result<RemoteExpect, GitError> {
    let branch = optional(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await?;
    let head_oid = optional(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).await?;
    let mut upstream = None;
    let mut upstream_oid = None;
    if let Some(branch) = &branch {
        let remote = optional(
            dir,
            &["config", "--get", &format!("branch.{branch}.remote")],
        )
        .await?;
        let destination =
            optional(dir, &["config", "--get", &format!("branch.{branch}.merge")]).await?;
        if let (Some(remote), Some(destination_ref)) = (remote, destination) {
            upstream = Some(Upstream {
                remote,
                destination_ref,
            });
            upstream_oid =
                optional(dir, &["rev-parse", "--verify", "--quiet", "@{upstream}"]).await?;
        }
    }
    Ok(RemoteExpect {
        branch,
        head_oid,
        upstream,
        upstream_oid,
    })
}
pub fn failure(error: GitError) -> SourceGitError {
    match error {
        GitError::InvalidInput => SourceGitError::InvalidSourceInput,
        GitError::LimitExceeded => SourceGitError::SourceLimitExceeded,
        GitError::TimedOut => SourceGitError::SourceTimedOut,
        GitError::UnsafeWorktree => SourceGitError::UnsafeSource,
        _ => SourceGitError::SourceFailed,
    }
}
pub async fn preflight(dir: &Path, expect: &RemoteExpect) -> Result<(), SourceGitError> {
    let current = state(dir).await.map_err(failure)?;
    if &current != expect {
        return Err(SourceGitError::StateChanged { current });
    }
    if let Some(upstream) = &expect.upstream {
        if !upstream.destination_ref.starts_with("refs/heads/")
            || output(dir, &["check-ref-format", &upstream.destination_ref])
                .await
                .map_err(failure)?
                .0
                != 0
        {
            return Err(SourceGitError::InvalidSourceInput);
        }
    }
    Ok(())
}
fn destination(expect: &RemoteExpect) -> Result<(&str, &Upstream), SourceGitError> {
    let head = expect
        .head_oid
        .as_deref()
        .ok_or(SourceGitError::UnsafeSource)?;
    let branch = expect
        .branch
        .as_deref()
        .ok_or(SourceGitError::UnsafeSource)?;
    let up = expect
        .upstream
        .as_ref()
        .ok_or(SourceGitError::UnsafeSource)?;
    if !matches!(head.len(), 40 | 64)
        || !head.bytes().all(|b| b.is_ascii_hexdigit())
        || branch.is_empty()
        || up.remote.is_empty()
        || up.remote.starts_with('-')
        || up.remote.contains(['\0', '\n'])
        || !up.destination_ref.starts_with("refs/heads/")
        || up.destination_ref.contains(['\0', '\n', ':'])
    {
        return Err(SourceGitError::InvalidSourceInput);
    }
    Ok((head, up))
}
async fn command(dir: &Path, args: &[&str]) -> Result<tokio::process::Command, SourceGitError> {
    // Credential helpers remain configured. Neither stderr nor stdout crosses the RPC boundary.
    let mut command = crate::run::command(dir, args);
    #[cfg(target_os = "linux")]
    {
        let ssh = if let Some(value) = std::env::var_os("GIT_SSH_COMMAND") {
            value
                .into_string()
                .map_err(|_| SourceGitError::InvalidSourceInput)?
        } else if let Some(value) = std::env::var_os("GIT_SSH") {
            format!(
                "'{}'",
                value
                    .to_str()
                    .ok_or(SourceGitError::InvalidSourceInput)?
                    .replace('\'', "'\\''")
            )
        } else {
            optional(dir, &["config", "--get", "core.sshCommand"])
                .await
                .map_err(failure)?
                .unwrap_or_else(|| "ssh".into())
        };
        command
            .env(
                "GIT_SSH_COMMAND",
                crate::run::ssh_with_batch(&ssh).map_err(failure)?,
            )
            .env_remove("GIT_SSH");
    }
    command
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .env("GCM_INTERACTIVE", "Never")
        .env("GCM_GUI_PROMPT", "false");
    Ok(command)
}
async fn execute(dir: &Path, args: &[&str]) -> Result<(), SourceGitError> {
    let (code, _, _) = crate::run::execute(command(dir, args).await?, Duration::from_secs(600))
        .await
        .map_err(failure)?;
    if code == 0 {
        Ok(())
    } else {
        Err(SourceGitError::SourceFailed)
    }
}
pub async fn push(dir: &Path, expect: &RemoteExpect, force: bool) -> Result<(), SourceGitError> {
    let (head, up) = destination(expect)?;
    let spec = format!("{head}:{}", up.destination_ref);
    let lease = format!(
        "--force-with-lease={}:{}",
        up.destination_ref,
        expect
            .upstream_oid
            .as_deref()
            .ok_or(SourceGitError::UnsafeSource)?
    );
    let mut args = vec![
        "push",
        "--porcelain",
        "--no-follow-tags",
        "--recurse-submodules=no",
    ];
    if force {
        args.push(&lease);
    }
    args.extend(["--", &up.remote, &spec]);
    preflight(dir, expect).await?;
    execute(dir, &args).await
}
pub async fn fetch(dir: &Path) -> Result<(), SourceGitError> {
    execute(dir, &["fetch", "--all", "--no-recurse-submodules"]).await
}
pub async fn pull(dir: &Path, expect: &RemoteExpect, rebase: bool) -> Result<(), SourceGitError> {
    let (_, up) = destination(expect)?;
    preflight(dir, expect).await?;
    execute(
        dir,
        &[
            "fetch",
            "--no-recurse-submodules",
            "--",
            &up.remote,
            &up.destination_ref,
        ],
    )
    .await?;
    // Fetch can take time. Refuse applying its result to a different checkout.
    let fetched = crate::run::resolve(dir, "FETCH_HEAD")
        .await
        .map_err(failure)?;
    let current = state(dir).await.map_err(failure)?;
    if current.branch != expect.branch
        || current.head_oid != expect.head_oid
        || current.upstream != expect.upstream
        || (current.upstream_oid != expect.upstream_oid
            && current.upstream_oid.as_deref() != Some(&fetched))
    {
        return Err(SourceGitError::StateChanged { current });
    }
    let args = if rebase {
        vec!["rebase", "--", &fetched]
    } else {
        vec!["merge", "--no-edit", "--", &fetched]
    };
    git(dir, &args).await.map_err(failure)?;
    Ok(())
}

// Kept private: resolved URLs can contain credentials and must never enter events/logs.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct PushIdentity(String);
pub(crate) async fn push_identity(
    dir: &Path,
    expect: &RemoteExpect,
) -> Result<PushIdentity, SourceGitError> {
    let (_, up) = destination(expect)?;
    // Reapplying URL rewrite rules to an already expanded URL can change its identity.
    // Conservatively leave cancellation unconfirmed for these configurations.
    let (rewrite, _, _) = output(
        dir,
        &[
            "config",
            "--name-only",
            "--get-regexp",
            r"^url\..*\.(insteadof|pushinsteadof)$",
        ],
    )
    .await
    .map_err(failure)?;
    if rewrite != 1 {
        return Err(SourceGitError::UnsafeSource);
    }
    let urls = git(dir, &["remote", "get-url", "--push", "--all", &up.remote])
        .await
        .map_err(failure)?;
    let urls = urls.lines().collect::<Vec<_>>();
    if urls.len() != 1 || urls[0].is_empty() {
        return Err(SourceGitError::UnsafeSource);
    }
    Ok(PushIdentity(urls[0].into()))
}
/// Reconcile only the unchanged, exact single push destination captured before execution.
pub(crate) async fn remote_oid(
    dir: &Path,
    expect: &RemoteExpect,
    identity: &PushIdentity,
) -> Result<Option<String>, SourceGitError> {
    let (_, up) = destination(expect)?;
    if &push_identity(dir, expect).await? != identity {
        return Err(SourceGitError::UnsafeSource);
    }
    let cmd = command(
        dir,
        &[
            "ls-remote",
            "--refs",
            "--",
            &identity.0,
            &up.destination_ref,
        ],
    )
    .await?;
    let (code, out, _) = crate::run::execute(cmd, Duration::from_secs(15))
        .await
        .map_err(failure)?;
    if code != 0 {
        return Err(SourceGitError::SourceFailed);
    }
    for line in out.lines() {
        let (oid, reference) = line.split_once('\t').ok_or(SourceGitError::SourceFailed)?;
        if reference == up.destination_ref {
            if matches!(oid.len(), 40 | 64) && oid.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Ok(Some(oid.into()));
            }
            return Err(SourceGitError::SourceFailed);
        }
    }
    Ok(None)
}
