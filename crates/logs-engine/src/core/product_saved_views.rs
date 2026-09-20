//! Product saved views and import receipt form a single atomic record.
use super::{
    saved_views::{self, SavedViewsDocument, MAX_SAFE_INTEGER, MAX_SAVED_VIEWS},
    SavedView,
};
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
    pub views: SavedViewsDocument,
    pub imported_snapshot: Option<String>,
    pub previous: Option<SavedViewsDocument>,
}
fn guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn read(root: &Path) -> Result<Document> {
    devbox_filesystem::ensure_no_links(root).map_err(|_| "runtime_settings_unavailable")?;
    let path = root.join("product-saved-views-v1.json");
    let (mut file, identity) = match devbox_filesystem::open_filesystem_object(&path, false) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Document {
                schema_version: 1,
                views: saved_views::list_from_dir(root)?,
                imported_snapshot: None,
                previous: None,
            })
        }
        Err(_) => return Err("runtime_settings_unavailable"),
    };
    let mut bytes = Vec::new();
    file.by_ref()
        .take(384 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "runtime_settings_unavailable")?;
    if bytes.len() > 384 * 1024
        || devbox_filesystem::filesystem_identity(&path, false).ok() != Some(identity)
    {
        return Err("runtime_settings_unavailable");
    }
    let document: Document =
        serde_json::from_slice(&bytes).map_err(|_| "runtime_settings_unavailable")?;
    if document.schema_version != 1 {
        return Err("runtime_settings_unavailable");
    }
    document.views.validate()?;
    if let Some(previous) = &document.previous {
        previous.validate()?;
    }
    Ok(document)
}
fn write(root: &Path, document: &mut Document) -> Result<()> {
    document.views.revision = document
        .views
        .revision
        .checked_add(1)
        .filter(|value| *value <= MAX_SAFE_INTEGER)
        .ok_or("runtime_settings_unavailable")?;
    document.views.validate()?;
    let bytes = serde_json::to_vec(document).map_err(|_| "runtime_settings_unavailable")?;
    if bytes.len() > 384 * 1024 {
        return Err("runtime_settings_unavailable");
    }
    devbox_filesystem::ensure_no_links(root).map_err(|_| "runtime_settings_unavailable")?;
    let path = root.join("product-saved-views-v1.json");
    if fs::symlink_metadata(&path).is_ok() {
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "runtime_settings_unavailable")?;
    }
    devbox_filesystem::atomic_write(&path, &bytes).map_err(|_| "runtime_settings_unavailable")
}
pub fn load(root: &Path) -> Result<Document> {
    let _guard = guard();
    read(root)
}
pub fn save(root: &Path, expected: u64, view: SavedView) -> Result<SavedViewsDocument> {
    let _guard = guard();
    let mut document = read(root)?;
    if document.views.revision != expected {
        return Err(saved_views::SAVED_VIEWS_CONFLICT_ERROR);
    }
    if let Some(existing) = document
        .views
        .views
        .iter_mut()
        .find(|item| item.name == view.name)
    {
        *existing = view;
    } else {
        if document.views.views.len() >= MAX_SAVED_VIEWS {
            return Err(saved_views::SAVED_VIEWS_LIMIT_ERROR);
        }
        document.views.views.push(view);
    }
    write(root, &mut document)?;
    Ok(document.views)
}
pub fn delete(root: &Path, expected: u64, name: &str) -> Result<SavedViewsDocument> {
    let _guard = guard();
    let mut document = read(root)?;
    if document.views.revision != expected {
        return Err(saved_views::SAVED_VIEWS_CONFLICT_ERROR);
    }
    let count = document.views.views.len();
    document.views.views.retain(|view| view.name != name);
    if count == document.views.views.len() {
        return Err(saved_views::SAVED_VIEW_NOT_FOUND_ERROR);
    }
    write(root, &mut document)?;
    Ok(document.views)
}
pub fn import(
    root: &Path,
    expected: u64,
    snapshot: &str,
    mut views: SavedViewsDocument,
    replace: bool,
) -> Result<bool> {
    views.validate()?;
    let _guard = guard();
    let mut document = read(root)?;
    if document.imported_snapshot.as_deref() == Some(snapshot) {
        return Ok(true);
    }
    if document.views.revision != expected
        || (!replace && (document.views.revision != 0 || !document.views.views.is_empty()))
    {
        return Err("runtime_settings_conflict");
    }
    document.previous = Some(document.views.clone());
    views.revision = document.views.revision;
    document.views = views;
    document.imported_snapshot = Some(snapshot.into());
    write(root, &mut document)?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{FilterSpec, SourceSpec};
    fn view(name: &str) -> SavedView {
        SavedView {
            name: name.into(),
            sources: vec![SourceSpec::LocalFile {
                path: "C:/synthetic/app.log".into(),
            }],
            filter: FilterSpec::default(),
        }
    }
    #[test]
    fn receipt_and_previous_document_survive_edits_without_reimporting_old_views() {
        let root = tempfile::tempdir().unwrap();
        let source = SavedViewsDocument {
            views: vec![view("imported")],
            ..Default::default()
        };
        assert!(!import(root.path(), 0, "source", source.clone(), false).unwrap());
        save(root.path(), 1, view("edited")).unwrap();
        assert!(import(root.path(), 0, "source", source, true).unwrap());
        let current = load(root.path()).unwrap();
        assert_eq!(current.views.views.len(), 2);
        assert_eq!(current.views.revision, 2);
        assert!(current.previous.unwrap().views.is_empty());
        assert_eq!(
            delete(root.path(), 1, "imported"),
            Err(saved_views::SAVED_VIEWS_CONFLICT_ERROR)
        );
    }
    #[test]
    fn ephemeral_sources_and_capacity_overflow_never_replace_current_bytes() {
        let root = tempfile::tempdir().unwrap();
        save(root.path(), 0, view("existing")).unwrap();
        let path = root.path().join("product-saved-views-v1.json");
        let before = fs::read(&path).unwrap();
        let mut source = SavedViewsDocument {
            views: vec![view("private")],
            ..Default::default()
        };
        source.views[0].sources = vec![SourceSpec::WslFile {
            distro: "synthetic".into(),
            path: "/tmp/private.log".into(),
        }];
        assert!(import(root.path(), 1, "source", source, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let source = SavedViewsDocument {
            views: (0..21)
                .map(|index| view(&format!("view-{index}")))
                .collect(),
            ..Default::default()
        };
        assert!(import(root.path(), 1, "source", source, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
