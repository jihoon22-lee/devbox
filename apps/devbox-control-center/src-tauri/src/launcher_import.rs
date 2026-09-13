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
fn source(app: &tauri::AppHandle) -> Result<(Preferences, Option<LegacyShortcut>, String)> {
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
    let revision =
        digest(&serde_json::to_vec(&(&prefs, &shortcut)).map_err(|_| "launcher_import_invalid")?);
    Ok((
        preferences(prefs.as_deref())?,
        shortcut
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(|_| "launcher_import_invalid")?,
        revision,
    ))
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
                let (source, shortcut, revision) = source(app)?;
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
                    if source(app)?.2 != value.journal.source || exact != value.journal.exact {
                        return Err("launcher_import_source_changed");
                    }
                    value.journal.clone()
                };
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
    let (preferences, _, _) = source(app)?;
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
