//! Terminal-owned, resumable preparation; importing definitions never starts PTYs.
use crate::private_metadata::MetadataRoot;
#[cfg(any(windows, test))]
use crate::terminal_export::Export;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use wsl_desktop_lib::component::{ProfileStore, WorkspaceProfile};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Notice {
    pub store: String,
    pub state: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Prepared {
    pub schema_version: u32,
    pub profiles: Vec<WorkspaceProfile>,
    pub preferences: BTreeMap<String, String>,
    pub notices: Vec<Notice>,
}
impl Prepared {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.profiles.len() > 100
            || self.preferences.len() > 6
            || self.notices.len() > 16
        {
            return Err("terminal_import_invalid");
        }
        ProfileStore {
            version: 2,
            profiles: self.profiles.clone(),
        }
        .validate()
        .map_err(|_| "terminal_import_invalid")?;
        if self.preferences.iter().any(|(key, value)| {
            !crate::terminal_export::KEYS.contains(&key.as_str())
                || key.ends_with(":last-layout")
                || value.len() > 1024 * 1024
        }) {
            return Err("terminal_import_invalid");
        }
        Ok(())
    }
}
#[cfg(any(windows, test))]
fn notice(notices: &mut Vec<Notice>, store: &str, state: &str) {
    notices.push(Notice {
        store: store.into(),
        state: state.into(),
    });
}
#[cfg(any(windows, test))]
fn valid_preference(key: &str, raw: &str) -> bool {
    match key {
        "wsl-desktop:cwd-pinned" | "wsl-desktop:copy-on-select" => matches!(raw, "0" | "1"),
        "wsl-desktop:cwd-value" => raw.len() <= 4096 && !raw.chars().any(char::is_control),
        "wsl-desktop:font-size" => raw
            .parse::<u16>()
            .is_ok_and(|size| (8..=32).contains(&size)),
        "wsl-desktop:recent-paths" => serde_json::from_str::<Vec<String>>(raw).is_ok_and(|paths| {
            paths.len() <= 12
                && paths
                    .iter()
                    .all(|path| path.len() <= 4096 && !path.chars().any(char::is_control))
        }),
        "wsl-desktop:settings" => serde_json::from_str::<Value>(raw).is_ok_and(|value| {
            value.is_object()
                && matches!(value.get("version").and_then(Value::as_u64), Some(1 | 2))
                && value.as_object().is_some_and(|object| {
                    object.keys().all(|key| {
                        matches!(
                            key.as_str(),
                            "version"
                                | "confirmSinglePaneClose"
                                | "openTerminalOnStart"
                                | "sidePanelOpen"
                                | "multiplexer"
                                | "fontId"
                                | "cursorStyle"
                                | "cursorBlink"
                                | "scrollbackLines"
                                | "theme"
                                | "quickSummonEnabled"
                                | "quickSummonShortcut"
                                | "keepInTray"
                        )
                    })
                })
                && [
                    "confirmSinglePaneClose",
                    "openTerminalOnStart",
                    "sidePanelOpen",
                    "cursorBlink",
                    "quickSummonEnabled",
                    "keepInTray",
                ]
                .iter()
                .all(|key| value.get(key).is_none_or(Value::is_boolean))
                && value
                    .get("scrollbackLines")
                    .is_none_or(|v| v.as_u64().is_some_and(|v| (1000..=100000).contains(&v)))
                && [
                    ("multiplexer", &["native", "tmux", "zellij"][..]),
                    (
                        "fontId",
                        &[
                            "cascadia-code",
                            "cascadia-mono",
                            "consolas",
                            "courier-new",
                            "system-mono",
                        ][..],
                    ),
                    ("cursorStyle", &["block", "underline", "bar"][..]),
                    ("theme", &["dark", "light", "highContrast"][..]),
                    (
                        "quickSummonShortcut",
                        &[
                            "Ctrl+Alt+Space",
                            "Ctrl+Shift+Space",
                            "Alt+Shift+Space",
                            "Ctrl+Alt+F12",
                        ][..],
                    ),
                ]
                .iter()
                .all(|(key, allowed)| {
                    value.get(key).is_none_or(|value| {
                        value.as_str().is_some_and(|value| allowed.contains(&value))
                    })
                })
        }),
        _ => false,
    }
}
#[cfg(any(windows, test))]
fn normalize(
    raw_profiles: Option<&[u8]>,
    exported: Option<Export>,
    browser_state: &str,
) -> Result<Prepared> {
    let mut prepared = Prepared {
        schema_version: 1,
        profiles: Vec::new(),
        preferences: BTreeMap::new(),
        notices: Vec::new(),
    };
    match raw_profiles {
        None => notice(&mut prepared.notices, "terminal-profiles.json", "missing"),
        Some(bytes) => match std::str::from_utf8(bytes)
            .ok()
            .and_then(|raw| ProfileStore::load(raw).ok())
        {
            Some(store) => {
                prepared.profiles = store.profiles;
                notice(&mut prepared.notices, "terminal-profiles.json", "ready");
            }
            None => notice(
                &mut prepared.notices,
                "terminal-profiles.json",
                "invalid-or-future-schema",
            ),
        },
    }
    if let Some(exported) = exported {
        exported.validate()?;
        for (key, value) in exported.values {
            let Some(raw) = value else {
                notice(&mut prepared.notices, &key, "missing");
                continue;
            };
            if key == "wsl-desktop:last-layout" {
                let profile = (|| {
                    let mut value: Value = serde_json::from_str(&raw).ok()?;
                    let version = value.get("version")?.as_u64()?;
                    if !matches!(version, 1 | 2) {
                        return None;
                    }
                    let object = value.as_object_mut()?;
                    object.remove("version");
                    object.insert("id".into(), json!("legacy-last-layout"));
                    object.insert("name".into(), json!("이전 마지막 레이아웃"));
                    ProfileStore::load(&json!({"version":version,"profiles":[value]}).to_string())
                        .ok()?
                        .profiles
                        .into_iter()
                        .next()
                })();
                if let Some(mut profile) = profile.filter(|_| prepared.profiles.len() < 100) {
                    // Keep the last layout distinct even if a named profile used this ID.
                    while prepared.profiles.iter().any(|p| p.id == profile.id) {
                        profile.id.push('_');
                    }
                    prepared.profiles.push(profile);
                    notice(&mut prepared.notices, &key, "explicit-restore-profile");
                } else {
                    notice(
                        &mut prepared.notices,
                        &key,
                        "invalid-future-schema-or-capacity",
                    );
                }
            } else if valid_preference(&key, &raw) {
                prepared.preferences.insert(key.clone(), raw);
                notice(&mut prepared.notices, &key, "ready");
            } else {
                notice(&mut prepared.notices, &key, "invalid-or-unsupported");
            }
        }
    } else {
        for key in crate::terminal_export::KEYS {
            notice(&mut prepared.notices, key, browser_state);
        }
    }
    prepared.validate()?;
    Ok(prepared)
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Job {
    schema_version: u32,
    id: String,
    state: String,
    issue: Option<String>,
}
fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|value| value.to_string() == id)
}
fn stages(root: &Path) -> Result<MetadataRoot> {
    MetadataRoot::open(root)?.child("terminal-imports")
}
fn write_job(stage: &MetadataRoot, job: &Job) -> Result<()> {
    stage.write(
        "job.json",
        &serde_json::to_vec(job).map_err(|_| "terminal_import_invalid")?,
    )
}
fn cleanup_copy(stage: &MetadataRoot) -> Result<()> {
    let copy = stage.path().join("webview-copy");
    match std::fs::symlink_metadata(&copy) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("terminal_import_copy_cleanup_pending"),
        Ok(_) => {}
    }
    if stage.read("worker-ticket.json")?.is_some() && stage.read("worker-retired")?.is_none() {
        return Err("terminal_export_retirement_unconfirmed");
    }
    let expected: (u64, u64) = serde_json::from_slice(
        &stage
            .read("browser-copy-identity.json")?
            .ok_or("terminal_import_copy_identity_missing")?,
    )
    .map_err(|_| "terminal_import_invalid")?;
    crate::platform::owned_copy::remove_owned_directory_matching(&copy, expected)
        .map_err(|_| "terminal_import_copy_cleanup_pending")
}
#[derive(Default)]
pub(crate) struct Imports {
    active: Mutex<HashMap<String, Arc<AtomicBool>>>,
}
impl Imports {
    pub(crate) fn start(self: &Arc<Self>, root: &Path, id: &str) -> Result<Value> {
        if !valid_id(id) {
            return Err("terminal_import_invalid");
        }
        let base = stages(root)?;
        let mut active = self.active.lock().map_err(|_| "terminal_import_busy")?;
        let stage = base.child(id)?;
        if stage.read("job.json")?.is_some() {
            return Ok(json!({"id":id}));
        }
        if !active.is_empty() {
            return Err("terminal_import_busy");
        }
        if std::fs::read_dir(base.path())
            .map_err(|_| "terminal_import_unavailable")?
            .take(65)
            .count()
            > 64
        {
            return Err("terminal_import_limit");
        }
        let job = Job {
            schema_version: 1,
            id: id.into(),
            state: "preparing".into(),
            issue: None,
        };
        write_job(&stage, &job)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        active.insert(id.into(), cancelled.clone());
        let owner = self.clone();
        let source = root
            .parent()
            .ok_or("terminal_import_source_unavailable")?
            .join("com.devbox.wsldesktop");
        tauri::async_runtime::spawn_blocking(move || {
            let mut job = job;
            match prepare(&source, &stage, &job.id, &cancelled) {
                Ok(prepared) => {
                    let bytes =
                        serde_json::to_vec(&prepared).map_err(|_| "terminal_import_invalid");
                    let saved = bytes.and_then(|bytes| stage.create_new("prepared.json", &bytes));
                    match saved {
                        Ok(()) => job.state = "ready".into(),
                        Err(issue) => {
                            job.state = "failed".into();
                            job.issue = Some(issue.into());
                        }
                    }
                }
                Err(issue) => {
                    job.state = if cancelled.load(Ordering::Acquire) {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    job.issue = Some(issue.into());
                }
            }
            if cleanup_copy(&stage).is_err() {
                job.issue = Some("terminal_import_copy_cleanup_pending".into());
            }
            let _ = write_job(&stage, &job);
            if let Ok(mut active) = owner.active.lock() {
                active.remove(&job.id);
            }
        });
        Ok(json!({"id":id}))
    }
    pub(crate) fn list(&self, root: &Path) -> Result<Value> {
        let base = stages(root)?;
        let active = self.active.lock().map_err(|_| "terminal_import_busy")?;
        let mut jobs = Vec::new();
        for entry in std::fs::read_dir(base.path())
            .map_err(|_| "terminal_import_unavailable")?
            .take(65)
        {
            let entry = entry.map_err(|_| "terminal_import_unavailable")?;
            let id = entry.file_name().to_string_lossy().to_string();
            if !valid_id(&id) {
                continue;
            }
            let stage = MetadataRoot::open(&entry.path())?;
            let Some(bytes) = stage.read("job.json")? else {
                continue;
            };
            let mut job: Job =
                serde_json::from_slice(&bytes).map_err(|_| "terminal_import_invalid")?;
            if job.schema_version != 1 || job.id != id {
                return Err("terminal_import_invalid");
            }
            if job.state == "preparing" && !active.contains_key(&id) {
                job.state = "interrupted".into();
            }
            jobs.push(job);
        }
        jobs.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(json!(jobs))
    }
    pub(crate) fn cleanup(&self, root: &Path, id: &str) -> Result<Value> {
        if !valid_id(id) {
            return Err("terminal_import_invalid");
        }
        let active = self.active.lock().map_err(|_| "terminal_import_busy")?;
        if active.contains_key(id) {
            return Err("terminal_import_busy");
        }
        let stage = MetadataRoot::open(&stages(root)?.path().join(id))?;
        cleanup_copy(&stage)?;
        let mut job: Job =
            serde_json::from_slice(&stage.read("job.json")?.ok_or("terminal_import_invalid")?)
                .map_err(|_| "terminal_import_invalid")?;
        if job.schema_version != 1 || job.id != id {
            return Err("terminal_import_invalid");
        }
        if job.issue.as_deref() == Some("terminal_import_copy_cleanup_pending") {
            job.issue = None;
            write_job(&stage, &job)?;
        }
        Ok(Value::Null)
    }
    pub(crate) fn cancel(&self, id: &str) -> Result<Value> {
        if !valid_id(id) {
            return Err("terminal_import_invalid");
        }
        if let Some(cancelled) = self
            .active
            .lock()
            .map_err(|_| "terminal_import_busy")?
            .get(id)
        {
            cancelled.store(true, Ordering::Release);
        }
        Ok(Value::Null)
    }
    pub(crate) fn stop(&self) -> Result<bool> {
        let active = self.active.lock().map_err(|_| "terminal_import_busy")?;
        for flag in active.values() {
            flag.store(true, Ordering::Release);
        }
        Ok(active.is_empty())
    }
}
pub(crate) fn prepared(root: &Path, id: &str) -> Result<(Prepared, String)> {
    if !valid_id(id) {
        return Err("terminal_import_invalid");
    }
    let stage = MetadataRoot::open(&stages(root)?.path().join(id))?;
    let job: Job =
        serde_json::from_slice(&stage.read("job.json")?.ok_or("terminal_import_not_ready")?)
            .map_err(|_| "terminal_import_invalid")?;
    if job.schema_version != 1 || job.id != id || job.state != "ready" {
        return Err("terminal_import_not_ready");
    }
    let bytes = stage
        .read("prepared.json")?
        .ok_or("terminal_import_not_ready")?;
    let prepared: Prepared =
        serde_json::from_slice(&bytes).map_err(|_| "terminal_import_invalid")?;
    prepared.validate()?;
    Ok((prepared, crate::definitions::digest(&bytes)))
}
#[cfg(windows)]
fn prepare(
    source: &Path,
    stage: &MetadataRoot,
    id: &str,
    cancelled: &AtomicBool,
) -> Result<Prepared> {
    use std::{io::Read, os::windows::fs::OpenOptionsExt};
    if !source
        .try_exists()
        .map_err(|_| "terminal_import_source_unavailable")?
    {
        return Err("terminal_legacy_missing");
    }
    let _source = MetadataRoot::open(source)?;
    let profile_path = source.join("terminal-profiles.json");
    if profile_path.exists() {
        devbox_filesystem::ensure_no_links(&profile_path)
            .map_err(|_| "terminal_import_source_unsafe")?;
    }
    let mut profile = match std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(0x0020_0000)
        .open(&profile_path)
    {
        Ok(file) => Some(file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("terminal_legacy_must_be_closed"),
    };
    let raw = if let Some(file) = &mut profile {
        let mut bytes = Vec::new();
        file.take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "terminal_import_source_unavailable")?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("terminal_import_limit");
        }
        stage.create_new("profiles-source.json", &bytes)?;
        Some(bytes)
    } else {
        None
    };
    if cancelled.load(Ordering::Acquire) {
        return Err("terminal_import_cancelled");
    }
    let (exported, state) = match crate::platform::browser_snapshot::snapshot_owned(
        source,
        stage.path(),
        cancelled,
        |copy| {
            let identity = devbox_filesystem::filesystem_identity(copy, true)
                .map_err(|_| "terminal_import_copy_identity_missing")?
                .components();
            stage
                .create_new(
                    "browser-copy-identity.json",
                    &serde_json::to_vec(&identity).map_err(|_| "terminal_import_invalid")?,
                )
                .map_err(str::to_owned)
        },
    ) {
        Ok((_copy, receipt)) => {
            stage.create_new(
                "browser-snapshot.json",
                &serde_json::to_vec(&receipt).map_err(|_| "terminal_import_invalid")?,
            )?;
            let nonce = uuid::Uuid::new_v4().simple().to_string();
            crate::terminal_export::ticket(stage, &nonce)?;
            let data = tauri::async_runtime::block_on(crate::terminal_export::export(
                stage, id, &nonce, cancelled,
            ))?;
            (Some(data), "ready")
        }
        Err(error) if error == "legacy_browser_store_missing_or_ambiguous" => {
            (None, "missing-or-ambiguous-browser-store")
        }
        Err(_) => return Err("terminal_legacy_must_be_closed"),
    };
    if cancelled.load(Ordering::Acquire) {
        return Err("terminal_import_cancelled");
    }
    normalize(raw.as_deref(), exported, state)
}
#[cfg(not(windows))]
fn prepare(_: &Path, _: &MetadataRoot, _: &str, _: &AtomicBool) -> Result<Prepared> {
    Err("terminal_export_windows_required")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_requires_recorded_copy_identity_and_retired_worker() {
        let directory = tempfile::tempdir().unwrap();
        let stage = MetadataRoot::open(directory.path()).unwrap();
        let copy = stage.child("webview-copy").unwrap();
        let identity = devbox_filesystem::filesystem_identity(copy.path(), true)
            .unwrap()
            .components();
        stage
            .write(
                "browser-copy-identity.json",
                &serde_json::to_vec(&identity).unwrap(),
            )
            .unwrap();
        drop(copy);
        stage.write("worker-ticket.json", b"ticket").unwrap();
        assert_eq!(
            cleanup_copy(&stage),
            Err("terminal_export_retirement_unconfirmed")
        );
        assert!(stage.path().join("webview-copy").is_dir());
        stage.write("worker-retired", b"retired").unwrap();
        stage
            .write(
                "browser-copy-identity.json",
                &serde_json::to_vec(&(identity.0, identity.1.wrapping_add(1))).unwrap(),
            )
            .unwrap();
        assert!(cleanup_copy(&stage).is_err());
        assert!(stage.path().join("webview-copy").is_dir());
        stage
            .write(
                "browser-copy-identity.json",
                &serde_json::to_vec(&identity).unwrap(),
            )
            .unwrap();
        cleanup_copy(&stage).unwrap();
        assert!(!stage.path().join("webview-copy").exists());
    }
    #[test]
    fn reports_each_missing_browser_key_without_claiming_it_was_imported() {
        let prepared = normalize(
            Some(br#"{"version":2,"profiles":[]}"#),
            None,
            "missing-or-ambiguous-browser-store",
        )
        .unwrap();
        assert_eq!(prepared.notices.len(), 8);
        assert!(prepared.preferences.is_empty());
        assert_eq!(
            prepared
                .notices
                .iter()
                .filter(|notice| notice.state == "missing-or-ambiguous-browser-store")
                .count(),
            7
        );
    }
    #[test]
    fn invalid_and_future_preferences_are_reported_without_default_replacement() {
        let mut values = crate::terminal_export::KEYS
            .iter()
            .map(|key| (key.to_string(), None))
            .collect::<BTreeMap<_, _>>();
        values.insert("wsl-desktop:font-size".into(), Some("999".into()));
        values.insert(
            "wsl-desktop:settings".into(),
            Some(r#"{"version":999,"theme":"dark"}"#.into()),
        );
        values.insert("wsl-desktop:copy-on-select".into(), Some("0".into()));
        let prepared = normalize(
            None,
            Some(Export {
                schema_version: 1,
                values,
            }),
            "ready",
        )
        .unwrap();
        assert_eq!(prepared.preferences.len(), 1);
        assert_eq!(prepared.preferences["wsl-desktop:copy-on-select"], "0");
        assert_eq!(
            prepared
                .notices
                .iter()
                .filter(|notice| notice.state == "invalid-or-unsupported")
                .count(),
            2
        );
    }
}
