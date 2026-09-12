//! Product-owned preference record: settings and import receipt commit together.
use super::preferences::{self, PortManagerPreferences};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::Path,
    sync::{Mutex, OnceLock},
};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub revision: u64,
    pub preferences: PortManagerPreferences,
    pub imported_snapshot: Option<String>,
    pub previous: Option<PortManagerPreferences>,
}
fn guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn read(root: &Path) -> Result<Document> {
    devbox_filesystem::ensure_no_links(root).map_err(|_| "runtime_settings_unavailable")?;
    let path = root.join("product-preferences-v1.json");
    let (mut file, identity) = match devbox_filesystem::open_filesystem_object(&path, false) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Document {
                schema_version: 1,
                revision: 0,
                preferences: preferences::load_from_path(preferences::preferences_path(root))
                    .map_err(|_| "runtime_settings_unavailable")?,
                imported_snapshot: None,
                previous: None,
            })
        }
        Err(_) => return Err("runtime_settings_unavailable"),
    };
    let mut bytes = Vec::new();
    file.by_ref()
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "runtime_settings_unavailable")?;
    if bytes.len() > 256 * 1024
        || devbox_filesystem::filesystem_identity(&path, false).ok() != Some(identity)
    {
        return Err("runtime_settings_unavailable");
    }
    let document: Document =
        serde_json::from_slice(&bytes).map_err(|_| "runtime_settings_unavailable")?;
    if document.schema_version != 1 || document.revision > 9_007_199_254_740_991 {
        return Err("runtime_settings_unavailable");
    }
    document
        .preferences
        .validate()
        .map_err(|_| "runtime_settings_unavailable")?;
    if let Some(previous) = &document.previous {
        previous
            .validate()
            .map_err(|_| "runtime_settings_unavailable")?;
    }
    Ok(document)
}
fn write(root: &Path, document: &mut Document) -> Result<()> {
    document.revision = document
        .revision
        .checked_add(1)
        .filter(|value| *value <= 9_007_199_254_740_991)
        .ok_or("runtime_settings_unavailable")?;
    let bytes = serde_json::to_vec(document).map_err(|_| "runtime_settings_unavailable")?;
    if bytes.len() > 256 * 1024 {
        return Err("runtime_settings_unavailable");
    }
    devbox_filesystem::ensure_no_links(root).map_err(|_| "runtime_settings_unavailable")?;
    let path = root.join("product-preferences-v1.json");
    if fs::symlink_metadata(&path).is_ok() {
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "runtime_settings_unavailable")?;
    }
    devbox_filesystem::atomic_write(&path, &bytes).map_err(|_| "runtime_settings_unavailable")
}
pub fn load(root: &Path) -> Result<Document> {
    let _guard = guard();
    read(root)
}
pub fn save(root: &Path, preferences: PortManagerPreferences) -> Result<()> {
    preferences
        .validate()
        .map_err(|_| "runtime_settings_invalid")?;
    let _guard = guard();
    let mut document = read(root)?;
    document.preferences = preferences;
    write(root, &mut document)
}
pub fn import(
    root: &Path,
    expected_revision: u64,
    snapshot: &str,
    preferences: PortManagerPreferences,
    replace: bool,
) -> Result<bool> {
    preferences
        .validate()
        .map_err(|_| "runtime_settings_invalid")?;
    let _guard = guard();
    let mut document = read(root)?;
    if document.imported_snapshot.as_deref() == Some(snapshot) {
        return Ok(true);
    }
    if document.revision != expected_revision
        || (!replace
            && (document.revision != 0
                || document.preferences != PortManagerPreferences::default()))
    {
        return Err("runtime_settings_conflict");
    }
    document.previous = Some(document.preferences.clone());
    document.preferences = preferences;
    document.imported_snapshot = Some(snapshot.into());
    write(root, &mut document)?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_receipt_survives_normal_saves_and_cas_rejects_a_late_review() {
        let root = tempfile::tempdir().unwrap();
        let current = load(root.path()).unwrap();
        let preferences = PortManagerPreferences {
            refresh_interval_ms: 2000,
            ..Default::default()
        };
        assert!(!import(
            root.path(),
            current.revision,
            "synthetic-source",
            preferences.clone(),
            false
        )
        .unwrap());
        let edited = PortManagerPreferences {
            refresh_interval_ms: 8000,
            ..Default::default()
        };
        save(root.path(), edited.clone()).unwrap();
        assert!(import(
            root.path(),
            0,
            "synthetic-source",
            preferences.clone(),
            true
        )
        .unwrap());
        assert_eq!(load(root.path()).unwrap().preferences, edited);
        assert_eq!(
            import(root.path(), 1, "different-source", preferences, true),
            Err("runtime_settings_conflict")
        );
        assert_eq!(
            load(root.path()).unwrap().previous,
            Some(PortManagerPreferences::default())
        );
    }
    #[test]
    fn malformed_product_record_is_preserved_and_never_falls_back() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("product-preferences-v1.json");
        fs::write(&path, b"{\"schemaVersion\":999}").unwrap();
        assert!(save(root.path(), PortManagerPreferences::default()).is_err());
        assert_eq!(fs::read(path).unwrap(), b"{\"schemaVersion\":999}");
    }
}
