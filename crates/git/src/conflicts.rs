//! Target-aware bounded conflict queries. Resolution mutates exactly one path.
use crate::{
    conflict_model::*,
    model::{SourceGitError as E, SourceTarget},
    remote::failure,
    run::git,
};
#[cfg(target_os = "linux")]
use crate::{hunks::digest, run};
use std::{collections::BTreeMap, path::Path};
type R<T> = Result<T, E>;
#[cfg(target_os = "linux")]
const TEXT_LIMIT: usize = 128 * 1024;
#[derive(Default)]
struct Stages {
    oids: [Option<String>; 3],
    modes: [Option<String>; 3],
}
async fn stages(dir: &Path, path: Option<&str>) -> R<BTreeMap<String, Stages>> {
    let mut args = vec!["ls-files", "--unmerged", "-z"];
    if let Some(path) = path {
        args.extend(["--", path]);
    }
    let text = git(dir, &args).await.map_err(failure)?;
    let mut entries: BTreeMap<String, Stages> = BTreeMap::new();
    for row in text.split_terminator('\0') {
        let (header, name) = row.split_once('\t').ok_or(E::SourceFailed)?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3
            || !matches!(fields[1].len(), 40 | 64)
            || !fields[1].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(E::SourceFailed);
        }
        let index: usize = fields[2].parse().map_err(|_| E::SourceFailed)?;
        if !(1..=3).contains(&index) || path.is_some_and(|p| p != name) {
            return Err(E::SourceFailed);
        }
        let entry = entries.entry(name.into()).or_default();
        entry.oids[index - 1] = Some(fields[1].into());
        entry.modes[index - 1] = Some(fields[0].into());
    }
    Ok(entries)
}
pub async fn list(dir: &Path) -> R<Vec<ConflictEntry>> {
    let entries = stages(dir, None).await?;
    if entries.len() > 2048 || entries.keys().map(String::len).sum::<usize>() > 128 * 1024 {
        return Err(E::SourceLimitExceeded);
    }
    Ok(entries
        .into_iter()
        .map(|(path, s)| {
            let kind = match s.oids.each_ref().map(Option::is_some) {
                [true, true, true] => ConflictKind::ModifyModify,
                [false, true, true] => ConflictKind::AddAdd,
                [true, true, false] | [true, false, true] => ConflictKind::DeleteModify,
                _ => ConflictKind::Other,
            };
            ConflictEntry { path, kind }
        })
        .collect())
}
#[cfg(target_os = "linux")]
async fn text(dir: &Path, oid: &Option<String>) -> R<(Option<String>, bool)> {
    let Some(oid) = oid else {
        return Ok((None, true));
    };
    let size = git(dir, &["cat-file", "-s", oid]).await.map_err(failure)?;
    if size.trim().parse::<usize>().map_err(|_| E::SourceFailed)? > TEXT_LIMIT {
        return Ok((None, false));
    }
    match git(dir, &["cat-file", "blob", oid]).await {
        Ok(s) if !s.contains('\0') => Ok((Some(s), true)),
        Ok(_) | Err(crate::GitError::InvalidEncoding) => Ok((None, false)),
        Err(e) => Err(failure(e)),
    }
}
#[cfg(target_os = "linux")]
async fn label(dir: &Path, reference: &str) -> R<String> {
    let (code, out, _) = run::output(dir, &["show", "-s", "--format=%h %s", reference, "--"])
        .await
        .map_err(failure)?;
    if code != 0 {
        return Ok(reference.into());
    }
    Ok(out.trim_end().chars().take(160).collect())
}
#[cfg(target_os = "linux")]
async fn labels(dir: &Path) -> R<ConflictLabels> {
    let (code, _, _) = run::output(dir, &["rev-parse", "--verify", "REBASE_HEAD"])
        .await
        .map_err(failure)?;
    let rebase = code == 0;
    let branch = git(dir, &["branch", "--show-current"])
        .await
        .map_err(failure)?;
    let head = label(dir, "HEAD").await?;
    let ours = if rebase {
        format!("기준(onto): {head}")
    } else {
        format!("{}: {head}", branch.trim())
    };
    let theirs = label(dir, if rebase { "REBASE_HEAD" } else { "MERGE_HEAD" }).await?;
    Ok(ConflictLabels {
        base: "공통 기준 (stage 1)".into(),
        ours,
        theirs: if rebase {
            format!("적용 중: {theirs}")
        } else {
            theirs
        },
        rebase,
    })
}
#[cfg(target_os = "linux")]
pub async fn preview(dir: &Path, target: &SourceTarget, path: &str) -> R<ConflictPreview> {
    let pinned = crate::conflict_disk::Pinned::open(dir, path)?;
    let entry = stages(dir, Some(path))
        .await?
        .remove(path)
        .ok_or_else(|| stale(target, path))?;
    let disk = pinned.read()?;
    let (base, a) = text(dir, &entry.oids[0]).await?;
    let (ours, b) = text(dir, &entry.oids[1]).await?;
    let (theirs, c) = text(dir, &entry.oids[2]).await?;
    let merged = disk
        .bytes
        .as_ref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .filter(|s| !s.contains('\0'))
        .map(str::to_owned);
    let supported = a
        && b
        && c
        && (!disk.exists || merged.is_some())
        && entry
            .modes
            .iter()
            .flatten()
            .all(|m| m == "100644" || m == "100755");
    let identity = identity(dir, path, target, &entry).await?;
    Ok(ConflictPreview {
        base,
        ours,
        theirs,
        merged,
        labels: labels(dir).await?,
        token: ConflictToken {
            target: target.clone(),
            path: path.into(),
            worktree_hash: disk.hash,
            stage_oids: entry.oids,
            identity,
        },
        text_supported: supported,
    })
}
#[cfg(target_os = "linux")]
async fn identity(dir: &Path, path: &str, target: &SourceTarget, s: &Stages) -> R<String> {
    let canonical = std::fs::canonicalize(dir).map_err(|_| E::InvalidTarget)?;
    // Include stage modes and operation identity as well as the public OIDs.
    let mut bytes = format!(
        "{canonical:?}\0{target:?}\0{path}\0{:?}\0{:?}",
        s.oids, s.modes
    )
    .into_bytes();
    for reference in ["HEAD", "MERGE_HEAD", "REBASE_HEAD"] {
        let (_, oid, _) = run::output(dir, &["rev-parse", "--verify", reference])
            .await
            .map_err(failure)?;
        bytes.extend(oid.as_bytes());
        bytes.push(0);
    }
    Ok(digest(&bytes))
}
#[cfg(target_os = "linux")]
fn stale(target: &SourceTarget, path: &str) -> E {
    E::ConflictStale {
        target: target.clone(),
        path: path.into(),
    }
}
#[cfg(target_os = "linux")]
fn partial(p: &ConflictApply) -> E {
    E::ConflictPartial {
        target: p.target.clone(),
        path: p.path.clone(),
    }
}
/// Explicit local test seams; RPC always supplies the default, never caller callbacks.
#[derive(Default)]
pub struct Probe<'a> {
    pub before_replace: Option<&'a (dyn Fn() + Send + Sync)>,
    pub after_replace: Option<&'a (dyn Fn() + Send + Sync)>,
    pub fail_index: bool,
}
pub async fn apply(dir: &Path, p: &ConflictApply) -> R<()> {
    apply_with_probe(dir, p, Probe::default()).await
}
#[cfg(target_os = "linux")]
pub async fn apply_with_probe(dir: &Path, p: &ConflictApply, probe: Probe<'_>) -> R<()> {
    use crate::conflict_disk::{IndexLock, Pinned, Replacement};
    if p.resolution.as_deref().unwrap_or_default().len() > TEXT_LIMIT
        || p.resolution.as_deref().unwrap_or_default().contains('\0')
    {
        return Err(E::InvalidSourceInput);
    }
    if p.token.target != p.target || p.token.path != p.path {
        return Err(stale(&p.target, &p.path));
    }
    let pinned = Pinned::open(dir, &p.path)?;
    let index = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )
    .await
    .map_err(failure)?;
    let mut lock = IndexLock::acquire(index.trim_end_matches('\n').into())?;
    let current = preview(dir, &p.target, &p.path).await?;
    if current.token != p.token {
        return Err(stale(&p.target, &p.path));
    }
    if !current.text_supported {
        return Err(E::InvalidSourceInput);
    }
    let mut disk = pinned.read()?;
    if disk.hash != p.token.worktree_hash || !pinned.valid() {
        return Err(stale(&p.target, &p.path));
    }
    if !disk.exists && p.resolution.is_some() {
        let entry = stages(dir, Some(&p.path))
            .await?
            .remove(&p.path)
            .ok_or_else(|| stale(&p.target, &p.path))?;
        if identity(dir, &p.path, &p.target, &entry).await? != p.token.identity {
            return Err(stale(&p.target, &p.path));
        }
        // No worktree mode exists to preserve. Require agreement among every
        // present stage (including base); never silently select a side's mode.
        // Explicit deletion needs no mode and bypasses this recreation policy.
        let mut modes = entry.modes.iter().flatten();
        let mode = modes.next().ok_or(E::InvalidSourceInput)?;
        if modes.any(|other| other != mode) {
            return Err(E::InvalidSourceInput);
        }
        disk.mode = match mode.as_str() {
            "100755" => 0o100755,
            "100644" => 0o100644,
            _ => return Err(E::InvalidSourceInput),
        };
    }
    let temp = tempfile::tempdir().map_err(|_| E::SourceFailed)?;
    let temp_index = temp.path().join("index");
    std::fs::write(&temp_index, lock.original()).map_err(|_| E::SourceFailed)?;
    if let Some(resolution) = &p.resolution {
        let (code, oid, _) = run::execute_input(
            run::command(dir, &["hash-object", "-w", "--stdin"]),
            run::TIMEOUT,
            Some(resolution.as_bytes()),
        )
        .await
        .map_err(failure)?;
        if code != 0 {
            return Err(E::SourceFailed);
        }
        let oid = oid.trim();
        if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(E::SourceFailed);
        }
        let mode = if disk.mode & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        };
        // No checkout, filters, or hooks; only stage 0 of the selected literal path changes.
        run::with_index(
            dir,
            &temp_index,
            temp.path(),
            &["update-index", "--add", "--cacheinfo", mode, oid, &p.path],
        )
        .await
        .map_err(failure)?;
    } else {
        run::with_index(
            dir,
            &temp_index,
            temp.path(),
            &["update-index", "--force-remove", "--", &p.path],
        )
        .await
        .map_err(failure)?;
    }
    let index_bytes = std::fs::read(&temp_index).map_err(|_| E::SourceFailed)?;
    let mut replacement =
        Replacement::prepare(&pinned, disk, p.resolution.as_deref().map(str::as_bytes))?;
    // Final preflight after all awaited Git work. From here through commit/undo there is no await.
    if !lock.unchanged() || pinned.read()?.hash != p.token.worktree_hash || !pinned.valid() {
        return Err(stale(&p.target, &p.path));
    }
    if let Some(hook) = probe.before_replace {
        hook();
    }
    if replacement.publish().is_err() {
        return Err(stale(&p.target, &p.path));
    }
    if let Some(hook) = probe.after_replace {
        hook();
    }
    if !replacement.verified() || !lock.unchanged() {
        return Err(if replacement.rollback() {
            stale(&p.target, &p.path)
        } else {
            partial(p)
        });
    }
    if probe.fail_index || lock.commit(&index_bytes).is_err() {
        replacement.rollback();
        return Err(partial(p));
    }
    replacement.finish();
    Ok(())
}
#[cfg(not(target_os = "linux"))]
pub async fn preview(_: &Path, _: &SourceTarget, _: &str) -> R<ConflictPreview> {
    Err(E::UnsafeSource)
}
#[cfg(not(target_os = "linux"))]
pub async fn apply_with_probe(_: &Path, _: &ConflictApply, _: Probe<'_>) -> R<()> {
    Err(E::UnsafeSource)
}
