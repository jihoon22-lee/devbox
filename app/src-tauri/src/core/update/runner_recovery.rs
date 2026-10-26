//! Read only owned records; unknown execution is never acknowledged into a fresh install.
use super::{
    ProcessIdentity, Result, RunnerError,
    files::{Manifest, Outcome, RECORD_LIMIT, Record, SpawnRecord},
};
use crate::core::{channel::AppChannel, update::storage};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOutcome {
    None,
    Success,
    InstallFailed,
    RelaunchFailed,
    UnknownBusy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recovery {
    pub operation: Option<u64>,
    pub outcome: RecoveryOutcome,
}
struct Entry {
    path: PathBuf,
    manifest: Manifest,
    spawn: SpawnRecord,
    record: Record,
}
#[derive(Deserialize)]
struct Owner {
    schema: Option<u8>,
    channel: Option<String>,
}
enum Ownership {
    Ours,
    Other,
    Absent,
}
fn ownership(path: &Path, channel: AppChannel) -> Result<Ownership> {
    if !path.try_exists().map_err(|_| RunnerError::Unknown)? {
        return Ok(Ownership::Absent);
    }
    // An existing owner that cannot be read is unknown, not evidence of an unrelated directory.
    let owner: Owner = read(path)?;
    Ok(
        if owner.schema == Some(1) && owner.channel.as_deref() == Some(channel.config().instance) {
            Ownership::Ours
        } else {
            Ownership::Other
        },
    )
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = storage::bounded(path, RECORD_LIMIT).map_err(|_| RunnerError::Unknown)?;
    serde_json::from_slice(&bytes).map_err(|_| RunnerError::Unknown)
}
fn entries(root: &Path, channel: AppChannel) -> Result<Vec<Entry>> {
    if !root.try_exists().map_err(|_| RunnerError::Storage)? {
        return Ok(Vec::new());
    }
    if !matches!(
        ownership(&root.join("owner.json"), channel)?,
        Ownership::Ours
    ) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    // Only runner stages count toward the bound: leaked `download-*` or other root entries
    // must never turn into a permanent startup block.
    let mut stages = 0;
    for entry in fs::read_dir(root).map_err(|_| RunnerError::Storage)? {
        let entry = entry.map_err(|_| RunnerError::Storage)?;
        if !entry.file_name().to_string_lossy().starts_with("install-") {
            continue;
        }
        stages += 1;
        if stages > 64 {
            return Err(RunnerError::Unknown);
        }
        match ownership(&entry.path().join("owner.json"), channel)? {
            Ownership::Other => continue,
            Ownership::Absent => {
                // A partial/crashed handoff with process records is not an unrelated folder.
                if entry
                    .path()
                    .join("spawn.json")
                    .try_exists()
                    .map_err(|_| RunnerError::Unknown)?
                    || entry
                        .path()
                        .join("outcome.json")
                        .try_exists()
                        .map_err(|_| RunnerError::Unknown)?
                {
                    return Err(RunnerError::Unknown);
                }
                continue;
            }
            Ownership::Ours => {}
        }
        // bounded() rejects symlink/reparse ancestors before canonicalizing the owned directory.
        let manifest: Manifest = read(&entry.path().join("owner.json"))?;
        let path = fs::canonicalize(entry.path()).map_err(|_| RunnerError::Identity)?;
        manifest.validate(channel, &path.join("runner.exe"))?;
        let spawn: SpawnRecord = read(&path.join("spawn.json"))?;
        let record: Record = read(&path.join("outcome.json"))?;
        if spawn.nonce != manifest.nonce
            || spawn.runner.pid == 0
            || spawn.runner.started == 0
            || record.schema != 1
            || record.nonce != manifest.nonce
            || record.binding != manifest.binding
            || record.channel != manifest.channel
            || record.runner != spawn.runner
        {
            return Err(RunnerError::Unknown);
        }
        result.push(Entry {
            path,
            manifest,
            spawn,
            record,
        });
    }
    Ok(result)
}
fn terminal(outcome: Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Installed | Outcome::Failed | Outcome::Cancelled
    )
}
fn classify(
    entry: &Entry,
    live: &mut impl FnMut(ProcessIdentity) -> Result<bool>,
) -> Result<Recovery> {
    let runner_live = live(entry.spawn.runner)?;
    let installer_live = entry
        .record
        .installer
        .map(&mut *live)
        .transpose()?
        .unwrap_or(false);
    if installer_live
        || !terminal(entry.record.outcome)
        || entry.record.error == Some(RunnerError::Unknown)
    {
        return Ok(Recovery {
            operation: None,
            outcome: RecoveryOutcome::UnknownBusy,
        });
    }
    let outcome = match entry.record.outcome {
        Outcome::Installed => RecoveryOutcome::Success,
        Outcome::Failed if entry.record.error == Some(RunnerError::Relaunch) => {
            RecoveryOutcome::RelaunchFailed
        }
        _ => RecoveryOutcome::InstallFailed,
    };
    // A terminal runner may still be delivering relaunch. Retain its files until a later dead-process proof.
    Ok(Recovery {
        operation: (!runner_live).then_some(entry.manifest.binding.operation),
        outcome,
    })
}
pub fn inspect(
    root: &Path,
    channel: AppChannel,
    mut live: impl FnMut(ProcessIdentity) -> Result<bool>,
) -> Result<Recovery> {
    let mut recovery = Recovery {
        operation: None,
        outcome: RecoveryOutcome::None,
    };
    for entry in entries(root, channel)? {
        let next = classify(&entry, &mut live)?;
        if next.outcome == RecoveryOutcome::UnknownBusy {
            return Ok(next);
        }
        if recovery.outcome == RecoveryOutcome::None || next.outcome != RecoveryOutcome::Success {
            recovery = next;
        }
    }
    Ok(recovery)
}
pub fn early_busy(
    root: &Path,
    channel: AppChannel,
    mut live: impl FnMut(ProcessIdentity) -> Result<bool>,
) -> Result<bool> {
    for entry in entries(root, channel)? {
        let ambiguous = matches!(
            entry.record.outcome,
            Outcome::Committed | Outcome::Running | Outcome::Unknown
        ) || entry.record.error == Some(RunnerError::Unknown);
        // Spawn may have succeeded before its PID/start was durably recorded. Runner exit alone
        // cannot prove installer absence in this window, even when no installer ID is available.
        if ambiguous && entry.record.installer.is_none() {
            return Ok(true);
        }
        if entry
            .record
            .installer
            .map(&mut live)
            .transpose()?
            .unwrap_or(false)
        {
            return Ok(true);
        }
        if ambiguous && live(entry.spawn.runner)? {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn startup_blocked(result: Result<bool>) -> bool {
    result.unwrap_or(true)
}
pub fn acknowledge(
    root: &Path,
    channel: AppChannel,
    operation: u64,
    mut live: impl FnMut(ProcessIdentity) -> Result<bool>,
) -> Result<()> {
    let selected: Vec<_> = entries(root, channel)?
        .into_iter()
        .filter(|e| e.manifest.binding.operation == operation)
        .collect();
    if selected.is_empty() {
        return Err(RunnerError::InvalidBinding);
    }
    for entry in &selected {
        if !terminal(entry.record.outcome)
            || live(entry.spawn.runner)?
            || entry.record.error == Some(RunnerError::Unknown)
            || entry
                .record
                .installer
                .map(&mut live)
                .transpose()?
                .unwrap_or(false)
        {
            return Err(RunnerError::Unknown);
        }
        let names = [
            "installer.exe",
            "runner.exe",
            "outcome.json",
            "spawn.json",
            "owner.json",
        ];
        for item in fs::read_dir(&entry.path).map_err(|_| RunnerError::Storage)? {
            let item = item.map_err(|_| RunnerError::Storage)?;
            if !names.iter().any(|name| item.file_name() == *name)
                || !item
                    .file_type()
                    .map_err(|_| RunnerError::Storage)?
                    .is_file()
            {
                return Err(RunnerError::Storage);
            }
            // Reject links/reparse files even for recognized names; never follow an unexpected entry.
            if !storage::read_locked(&item.path())
                .map_err(|_| RunnerError::Storage)?
                .metadata()
                .map_err(|_| RunnerError::Storage)?
                .is_file()
            {
                return Err(RunnerError::Storage);
            }
        }
        let token = CancellationToken::new();
        super::files::verify(
            &super::files::locked(&entry.path.join("installer.exe"))?,
            &entry.manifest.installer_proof,
            &token,
        )?;
        super::files::verify(
            &super::files::locked(&entry.path.join("runner.exe"))?,
            &entry.manifest.runner_proof,
            &token,
        )?;
        // Installer/runner first. Keep owner and process evidence if a frozen file cannot be removed.
        for name in names {
            fs::remove_file(entry.path.join(name)).map_err(|_| RunnerError::Storage)?;
        }
        fs::remove_dir(&entry.path).map_err(|_| RunnerError::Storage)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "runner_recovery_tests.rs"]
mod tests;
