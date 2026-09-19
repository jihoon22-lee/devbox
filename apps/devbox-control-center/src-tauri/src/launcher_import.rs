//! Reviewed JSON preference migration into Control Center's namespace. Source
//! files are read only; a bounded journal preserves an interrupted destination apply.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]
use crate::core::launcher_import::{self as plan, LegacyShortcut, Plan};
use product_contract::launcher_preferences::{Preferences, PREFERENCES_FILE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::Manager;
type Result<T> = std::result::Result<T, &'static str>;
const JOURNAL: &str = "launcher-import-v1.json";
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    schema: u32,
    id: String,
    source: String,
    before: Option<String>,
    after: Preferences,
    plan: Plan,
    committed: bool,
    exact: BTreeMap<String, String>,
}
struct Pending {
    journal: Journal,
    source_bytes: Vec<u8>,
    created: Instant,
}
#[derive(Default)]
pub(crate) struct Owner(Mutex<Option<Pending>>);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    id: String,
    plan: Plan,
    committed: bool,
}
fn view(journal: &Journal) -> View {
    View {
        id: journal.id.clone(),
        plan: journal.plan.clone(),
        committed: journal.committed,
    }
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    if !path
        .try_exists()
        .map_err(|_| "launcher_import_unavailable")?
    {
        return Ok(None);
    }
    devbox_filesystem::ensure_no_links(path).map_err(|_| "launcher_import_unsafe")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
        .map_err(|_| "launcher_import_unavailable")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "launcher_import_unavailable")?;
    if bytes.len() > limit {
        return Err("launcher_import_limit");
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "launcher_import_unavailable")?;
    let mut again = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut again)
        .map_err(|_| "launcher_import_unavailable")?;
    if bytes != again || devbox_filesystem::filesystem_identity(path, false).ok() != Some(identity)
    {
        return Err("launcher_import_source_changed");
    }
    Ok(Some(bytes))
}
fn preferences(bytes: Option<&[u8]>) -> Result<Preferences> {
    let value: Preferences = match bytes {
        Some(bytes) => serde_json::from_slice(bytes).map_err(|_| "launcher_import_invalid")?,
        None => Preferences::default(),
    };
    value.validate().map_err(|_| "launcher_import_invalid")?;
    Ok(value)
}
struct CapturedSource {
    preferences: Preferences,
    shortcut: Option<LegacyShortcut>,
    revision: String,
    bytes: Vec<u8>,
}
fn source(app: &tauri::AppHandle) -> Result<CapturedSource> {
    let root = app
        .path()
        .local_data_dir()
        .map_err(|_| "launcher_import_unavailable")?
        .join("com.devbox.devboxlauncher");
    #[cfg(windows)]
    let _pins = crate::suite::platform::component_scope::pin_directories(&root)?;
    let prefs = read_bounded(&root.join(PREFERENCES_FILE), 65536)?;
    let shortcut = read_bounded(&root.join("shortcut.json"), 4096)?;
    if prefs.is_none() && shortcut.is_none() {
        return Err("launcher_import_source_missing");
    }
    if read_bounded(&root.join(PREFERENCES_FILE), 65536)? != prefs
        || read_bounded(&root.join("shortcut.json"), 4096)? != shortcut
    {
        return Err("launcher_import_source_changed");
    }
    // Preserve exact original bytes and absence separately for the two files.
    // This stable pair observation is not a transaction across legacy writers.
    let bytes = serde_json::to_vec(&(&prefs, &shortcut)).map_err(|_| "launcher_import_invalid")?;
    Ok(CapturedSource {
        preferences: preferences(prefs.as_deref())?,
        shortcut: shortcut
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(|_| "launcher_import_invalid")?,
        revision: digest(&bytes),
        bytes,
    })
}
fn retain_record(root: &Path, kind: &str, id: &str, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if !matches!(
        kind,
        "launcher-source-backups-v1" | "launcher-import-history-v1"
    ) || !(product_contract::commands::revision(id)
        || uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id))
        || bytes.len() > 512 * 1024
    {
        return Err("launcher_import_backup_invalid");
    }
    devbox_filesystem::ensure_no_links(root).map_err(|_| "launcher_import_unsafe")?;
    let directory = root.join(kind);
    match std::fs::create_dir(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("launcher_import_backup_unavailable"),
    }
    devbox_filesystem::ensure_no_links(&directory).map_err(|_| "launcher_import_unsafe")?;
    let path = directory.join(format!("{id}.json"));
    if let Some(existing) = read_bounded(&path, 512 * 1024)? {
        return if existing == bytes {
            Ok(())
        } else {
            Err("launcher_import_backup_changed")
        };
    }
    if std::fs::read_dir(&directory)
        .map_err(|_| "launcher_import_backup_unavailable")?
        .take(64)
        .count()
        >= 32
    {
        return Err("launcher_import_backup_retention_review_required");
    }
    let pending = directory.join(format!("{}.pending", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| "launcher_import_backup_unavailable")?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "launcher_import_backup_unavailable")?;
    drop(file);
    std::fs::hard_link(&pending, &path).map_err(|_| "launcher_import_backup_unavailable")?;
    let _ = std::fs::remove_file(pending);
    Ok(())
}
fn source_retained(root: &Path, revision: &str) -> Result<bool> {
    if !product_contract::commands::revision(revision) {
        return Err("launcher_import_backup_invalid");
    }
    let Some(bytes) = read_bounded(
        &root
            .join("launcher-source-backups-v1")
            .join(format!("{revision}.json")),
        512 * 1024,
    )?
    else {
        return Ok(false);
    };
    if digest(&bytes) != revision {
        return Err("launcher_import_backup_changed");
    }
    Ok(true)
}
fn retain_source(root: &Path, revision: &str, bytes: &[u8]) -> Result<()> {
    if digest(bytes) != revision {
        return Err("launcher_import_source_changed");
    }
    retain_record(root, "launcher-source-backups-v1", revision, bytes)
}
fn read_journal(root: &Path) -> Result<Option<Journal>> {
    let Some(bytes) = read_bounded(&root.join(JOURNAL), 256 * 1024)? else {
        return Ok(None);
    };
    let value: Journal =
        serde_json::from_slice(&bytes).map_err(|_| "launcher_import_journal_invalid")?;
    if value.schema != 1
        || uuid::Uuid::parse_str(&value.id).is_err()
        || !product_contract::commands::revision(&value.source)
        || value
            .before
            .as_ref()
            .is_some_and(|value| !product_contract::commands::revision(value))
        || value.after != value.plan.preferences
    {
        return Err("launcher_import_journal_invalid");
    }
    value
        .after
        .validate()
        .map_err(|_| "launcher_import_journal_invalid")?;
    Ok(Some(value))
}
fn save_journal(root: &Path, journal: &Journal) -> Result<()> {
    if let Some(prior) = read_journal(root)? {
        if prior.id != journal.id {
            if !prior.committed {
                return Err("launcher_import_resume_required");
            }
            retain_record(
                root,
                "launcher-import-history-v1",
                &prior.id,
                &serde_json::to_vec(&prior).map_err(|_| "launcher_import_invalid")?,
            )?;
        }
    }
    let bytes = serde_json::to_vec(journal).map_err(|_| "launcher_import_invalid")?;
    if bytes.len() > 256 * 1024 {
        return Err("launcher_import_limit");
    }
    devbox_filesystem::atomic_write(root.join(JOURNAL), &bytes)
        .map_err(|_| "launcher_import_unavailable")
}
pub(crate) fn dispatch(
    app: &tauri::AppHandle,
    method: &str,
    id: Option<&str>,
    exact: BTreeMap<String, String>,
    deadline: u64,
) -> Result<serde_json::Value> {
    #[cfg(not(windows))]
    {
        let _ = (app, method, id, exact, deadline);
        Err("launcher_import_windows_required")
    }
    #[cfg(windows)]
    {
        let alive = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .is_ok_and(|time| time.as_millis() < u128::from(deadline))
        };
        if !alive() {
            return Err("launcher_import_expired");
        }
        let owner = app.state::<Owner>();
        let mut pending = owner.0.lock().map_err(|_| "launcher_import_busy")?;
        let preferences_owner = app.state::<crate::commands::PreferenceOwner>();
        let _preferences = preferences_owner
            .0
            .lock()
            .map_err(|_| "launcher_import_busy")?;
        let path = crate::commands::preferences_path(app)?;
        let root = path.parent().ok_or("launcher_import_unavailable")?;
        let _pins = crate::suite::platform::component_scope::pin_directories(root)?;
        let prior = read_journal(root)?;
        let result = match method {
            "status" => {
                return serde_json::to_value(prior.as_ref().map(view))
                    .map_err(|_| "launcher_import_invalid")
            }
            "preview" => {
                if prior.as_ref().is_some_and(|journal| !journal.committed) {
                    return Err("launcher_import_resume_required");
                }
                let CapturedSource {
                    preferences: source,
                    shortcut,
                    revision,
                    bytes: source_bytes,
                } = source(app)?;
                let current = read_bounded(&path, 65536)?;
                let proposed = plan::prepare(
                    &source,
                    &preferences(current.as_deref())?,
                    shortcut,
                    root.join("suite-shortcuts.json")
                        .try_exists()
                        .map_err(|_| "launcher_import_unavailable")?,
                    &exact,
                )?;
                let journal = Journal {
                    schema: 1,
                    id: uuid::Uuid::new_v4().to_string(),
                    source: revision,
                    before: current.as_deref().map(digest),
                    after: proposed.preferences.clone(),
                    plan: proposed,
                    committed: false,
                    exact,
                };
                let result = view(&journal);
                *pending = Some(Pending {
                    journal,
                    source_bytes,
                    created: Instant::now(),
                });
                result
            }
            "apply" | "resume" => {
                let mut journal = if method == "resume" {
                    prior
                        .filter(|journal| Some(journal.id.as_str()) == id)
                        .ok_or("launcher_import_review_stale")?
                } else {
                    let value = pending
                        .as_ref()
                        .filter(|pending| {
                            Some(pending.journal.id.as_str()) == id
                                && pending.created.elapsed() < Duration::from_secs(180)
                        })
                        .ok_or("launcher_import_review_stale")?;
                    if source(app)?.revision != value.journal.source || exact != value.journal.exact
                    {
                        return Err("launcher_import_source_changed");
                    }
                    retain_source(root, &value.journal.source, &value.source_bytes)?;
                    value.journal.clone()
                };
                if method == "resume" && !source_retained(root, &journal.source)? {
                    let captured = source(app)?;
                    if captured.revision != journal.source {
                        return Err("launcher_import_backup_missing");
                    }
                    retain_source(root, &journal.source, &captured.bytes)?;
                }
                if journal.committed {
                    return serde_json::to_value(view(&journal))
                        .map_err(|_| "launcher_import_invalid");
                }
                let current = read_bounded(&path, 65536)?;
                let next = serde_json::to_vec_pretty(&journal.after)
                    .map_err(|_| "launcher_import_invalid")?;
                let write_preferences =
                    resume_state(journal.before.as_deref(), current.as_deref(), &next)?;
                if !alive() {
                    return Err("launcher_import_expired");
                }
                if method == "apply" {
                    save_journal(root, &journal)?;
                }
                if write_preferences {
                    devbox_filesystem::atomic_write(&path, &next)
                        .map_err(|_| "launcher_import_unavailable")?;
                }
                if let Some(config) = &journal.plan.proposed_shortcut {
                    app.state::<crate::shortcuts::Owner>()
                        .import_disabled(app, config)
                        .map_err(|_| "launcher_import_shortcut_changed")?;
                }
                journal.committed = true;
                save_journal(root, &journal)?;
                *pending = None;
                view(&journal)
            }
            _ => return Err("launcher_import_method_invalid"),
        };
        serde_json::to_value(result).map_err(|_| "launcher_import_invalid")
    }
}

pub(crate) fn mapping_ids(app: &tauri::AppHandle) -> Result<Vec<String>> {
    let preferences = source(app)?.preferences;
    Ok(preferences
        .favorites
        .into_iter()
        .chain(preferences.recents)
        .filter(|id| id.starts_with("snapshot/"))
        .collect())
}

fn resume_state(before: Option<&str>, current: Option<&[u8]>, after: &[u8]) -> Result<bool> {
    if current == Some(after) {
        return Ok(false);
    }
    if current.map(digest).as_deref() == before {
        return Ok(true);
    }
    Err("launcher_import_destination_changed")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_original_json_is_idempotent_and_never_overwritten_after_tampering() {
        let root = std::env::temp_dir().join(format!("launcher-source-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let bytes =
            serde_json::to_vec(&(Some(b"original settings".to_vec()), Option::<Vec<u8>>::None))
                .unwrap();
        let revision = digest(&bytes);
        retain_source(&root, &revision, &bytes).unwrap();
        retain_source(&root, &revision, &bytes).unwrap();
        assert!(source_retained(&root, &revision).unwrap());
        let path = root
            .join("launcher-source-backups-v1")
            .join(format!("{revision}.json"));
        std::fs::write(&path, b"changed").unwrap();
        assert!(source_retained(&root, &revision).is_err());
        assert!(retain_source(&root, &revision, &bytes).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"changed");
        assert!(source_retained(&root, "../foreign").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn interrupted_apply_recognizes_exact_preimage_postimage_and_rejects_user_edits() {
        let before = digest(b"old preferences");
        assert_eq!(
            resume_state(
                Some(&before),
                Some(b"old preferences"),
                b"planned preferences"
            ),
            Ok(true)
        );
        assert_eq!(
            resume_state(
                Some(&before),
                Some(b"planned preferences"),
                b"planned preferences"
            ),
            Ok(false)
        );
        assert_eq!(
            resume_state(
                Some(&before),
                Some(b"user changed preferences"),
                b"planned preferences"
            ),
            Err("launcher_import_destination_changed")
        );
        assert_eq!(resume_state(None, None, b"planned preferences"), Ok(true));
        assert!(resume_state(Some(&before), None, b"planned preferences").is_err());
    }
}

/// Read current owned preferences and import journal without applying a plan or
/// registering shortcuts. Pending imports remain visibly unready for activation.
pub(crate) fn suite_status(app: &tauri::AppHandle) -> Result<serde_json::Value> {
    let owner = app
        .try_state::<Owner>()
        .ok_or("launcher_import_unavailable")?;
    let pending = owner.0.try_lock().map_err(|_| "launcher_import_busy")?;
    let preferences_owner = app
        .try_state::<crate::commands::PreferenceOwner>()
        .ok_or("preferences_unavailable")?;
    let _preferences = preferences_owner
        .0
        .try_lock()
        .map_err(|_| "preferences_busy")?;
    let root = app
        .path()
        .app_local_data_dir()
        .map_err(|_| "preferences_unavailable")?;
    devbox_filesystem::ensure_no_links(&root).map_err(|_| "preferences_unavailable")?;
    let prefs = preferences(read_bounded(&root.join(PREFERENCES_FILE), 65536)?.as_deref())?;
    let journal = read_journal(&root)?;
    let shortcuts = read_bounded(&root.join("suite-shortcuts.json"), 4096)?;
    if let Some(bytes) = &shortcuts {
        let config: product_contract::shortcuts::Config =
            serde_json::from_slice(bytes).map_err(|_| "shortcut_invalid")?;
        config.validate().map_err(|_| "shortcut_invalid")?;
    }
    let review = pending.is_some() || journal.as_ref().is_some_and(|value| !value.committed);
    let native =
        serde_json::to_vec(&(prefs, journal, shortcuts)).map_err(|_| "migration_unavailable")?;
    serde_json::to_value(product_contract::migration_status::Summary::new(
        "control-center",
        env!("CARGO_PKG_VERSION"),
        false,
        true,
        review,
        &native,
    )?)
    .map_err(|_| "migration_unavailable")
}
