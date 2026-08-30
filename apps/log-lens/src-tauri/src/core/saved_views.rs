//! Strict app-local persistence for reusable Log Lens source configuration.
//!
//! Records, cursors, bookmarks, handoff bodies, and raw log bytes have no
//! field in this document. WSL file descriptors and ephemeral Webhook
//! captures are rejected because they would turn a one-time path/capture
//! handoff into durable data.

use super::{SavedView, SourceSpec};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const SAVED_VIEWS_FILE: &str = "saved-views.json";
pub const SAVED_VIEWS_SCHEMA_VERSION: u32 = 1;
pub const MAX_SAVED_VIEWS: usize = 20;
pub const MAX_SAVED_VIEWS_BYTES: u64 = 128 * 1024;
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub const SAVED_VIEWS_READ_ERROR: &str = "저장된 뷰 저장소를 읽을 수 없습니다";
pub const SAVED_VIEWS_WRITE_ERROR: &str = "저장된 뷰 저장소를 저장할 수 없습니다";
pub const SAVED_VIEWS_INPUT_ERROR: &str = "저장된 뷰 설정이 유효하지 않습니다";
pub const SAVED_VIEWS_CONFLICT_ERROR: &str =
    "저장된 뷰가 다른 작업에서 변경되었습니다. 다시 불러온 뒤 시도해 주세요";
pub const SAVED_VIEWS_LIMIT_ERROR: &str =
    "저장된 뷰가 최대 개수에 도달했습니다. 기존 뷰를 삭제한 뒤 다시 시도해 주세요";
pub const SAVED_VIEW_NOT_FOUND_ERROR: &str = "저장된 뷰를 찾을 수 없습니다";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedViewsDocument {
    pub schema_version: u32,
    pub revision: u64,
    pub views: Vec<SavedView>,
}

impl Default for SavedViewsDocument {
    fn default() -> Self {
        Self {
            schema_version: SAVED_VIEWS_SCHEMA_VERSION,
            revision: 0,
            views: Vec::new(),
        }
    }
}

impl SavedViewsDocument {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != SAVED_VIEWS_SCHEMA_VERSION
            || self.revision > MAX_SAFE_INTEGER
            || self.views.len() > MAX_SAVED_VIEWS
        {
            return Err(SAVED_VIEWS_INPUT_ERROR);
        }
        let mut names = HashSet::with_capacity(self.views.len());
        for view in &self.views {
            validate_saved_view(view)?;
            if !names.insert(view.name.as_str()) {
                return Err(SAVED_VIEWS_INPUT_ERROR);
            }
        }
        let bytes = serde_json::to_vec(self).map_err(|_| SAVED_VIEWS_INPUT_ERROR)?;
        if bytes.len() as u64 > MAX_SAVED_VIEWS_BYTES {
            return Err(SAVED_VIEWS_INPUT_ERROR);
        }
        Ok(())
    }
}

pub fn list_from_dir(directory: &Path) -> Result<SavedViewsDocument, &'static str> {
    let _guard = io_guard();
    load_unlocked(directory)
}

pub fn upsert_in_dir(
    directory: &Path,
    expected_revision: u64,
    view: SavedView,
) -> Result<SavedViewsDocument, &'static str> {
    let _guard = io_guard();
    validate_saved_view(&view)?;
    let mut document = load_unlocked(directory)?;
    if document.revision != expected_revision {
        return Err(SAVED_VIEWS_CONFLICT_ERROR);
    }
    if let Some(existing) = document
        .views
        .iter_mut()
        .find(|item| item.name == view.name)
    {
        *existing = view;
    } else {
        if document.views.len() >= MAX_SAVED_VIEWS {
            return Err(SAVED_VIEWS_LIMIT_ERROR);
        }
        document.views.push(view);
    }
    document.revision = next_revision(document.revision)?;
    save_unlocked(directory, &document)?;
    Ok(document)
}

pub fn delete_in_dir(
    directory: &Path,
    expected_revision: u64,
    name: &str,
) -> Result<SavedViewsDocument, &'static str> {
    let _guard = io_guard();
    validate_safe_text(name)?;
    let mut document = load_unlocked(directory)?;
    if document.revision != expected_revision {
        return Err(SAVED_VIEWS_CONFLICT_ERROR);
    }
    let before = document.views.len();
    document.views.retain(|view| view.name != name);
    if document.views.len() == before {
        return Err(SAVED_VIEW_NOT_FOUND_ERROR);
    }
    document.revision = next_revision(document.revision)?;
    save_unlocked(directory, &document)?;
    Ok(document)
}

fn saved_views_path(directory: &Path) -> PathBuf {
    directory.join(SAVED_VIEWS_FILE)
}

fn load_unlocked(directory: &Path) -> Result<SavedViewsDocument, &'static str> {
    let directory_identity = match devbox_filesystem::filesystem_identity(directory, true) {
        Ok(identity) => identity,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SavedViewsDocument::default())
        }
        Err(_) => return Err(SAVED_VIEWS_READ_ERROR),
    };
    devbox_filesystem::ensure_no_links(directory).map_err(|_| SAVED_VIEWS_READ_ERROR)?;
    let path = saved_views_path(directory);
    let (mut file, file_identity) = match devbox_filesystem::open_filesystem_object(&path, false) {
        Ok(result) => result,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SavedViewsDocument::default())
        }
        Err(_) => return Err(SAVED_VIEWS_READ_ERROR),
    };
    let metadata = file.metadata().map_err(|_| SAVED_VIEWS_READ_ERROR)?;
    if metadata.len() > MAX_SAVED_VIEWS_BYTES {
        return Err(SAVED_VIEWS_READ_ERROR);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_SAVED_VIEWS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SAVED_VIEWS_READ_ERROR)?;
    if bytes.len() as u64 > MAX_SAVED_VIEWS_BYTES
        || devbox_filesystem::filesystem_identity(directory, true).ok() != Some(directory_identity)
        || devbox_filesystem::filesystem_identity(&path, false).ok() != Some(file_identity)
    {
        return Err(SAVED_VIEWS_READ_ERROR);
    }
    let document: SavedViewsDocument =
        serde_json::from_slice(&bytes).map_err(|_| SAVED_VIEWS_READ_ERROR)?;
    document.validate().map_err(|_| SAVED_VIEWS_READ_ERROR)?;
    Ok(document)
}

fn save_unlocked(directory: &Path, document: &SavedViewsDocument) -> Result<(), &'static str> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document).map_err(|_| SAVED_VIEWS_WRITE_ERROR)?;
    if bytes.len() as u64 > MAX_SAVED_VIEWS_BYTES {
        return Err(SAVED_VIEWS_WRITE_ERROR);
    }
    fs::create_dir_all(directory).map_err(|_| SAVED_VIEWS_WRITE_ERROR)?;
    devbox_filesystem::ensure_no_links(directory).map_err(|_| SAVED_VIEWS_WRITE_ERROR)?;
    let directory_identity = devbox_filesystem::filesystem_identity(directory, true)
        .map_err(|_| SAVED_VIEWS_WRITE_ERROR)?;
    let path = saved_views_path(directory);
    match devbox_filesystem::filesystem_identity(&path, false) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(SAVED_VIEWS_WRITE_ERROR),
    }
    devbox_filesystem::atomic_write(&path, &bytes).map_err(|_| SAVED_VIEWS_WRITE_ERROR)?;
    if devbox_filesystem::filesystem_identity(directory, true).ok() != Some(directory_identity)
        || devbox_filesystem::filesystem_identity(&path, false).is_err()
    {
        return Err(SAVED_VIEWS_WRITE_ERROR);
    }
    Ok(())
}

fn validate_saved_view(view: &SavedView) -> Result<(), &'static str> {
    view.validate().map_err(|_| SAVED_VIEWS_INPUT_ERROR)?;
    validate_safe_text(&view.name)?;
    for source in &view.sources {
        match source {
            SourceSpec::WslFile { .. } | SourceSpec::WebhookCapture { .. } => {
                return Err(SAVED_VIEWS_INPUT_ERROR)
            }
            SourceSpec::LocalFile { path } => validate_safe_text(path)?,
            SourceSpec::Directory { path, pattern } => {
                validate_safe_text(path)?;
                validate_safe_text(pattern)?;
            }
            SourceSpec::WslJournal { distro, unit } => {
                validate_safe_text(distro)?;
                if let Some(unit) = unit {
                    validate_safe_text(unit)?;
                }
            }
            SourceSpec::Run { source_id } => validate_safe_text(source_id)?,
            SourceSpec::Container {
                engine: _,
                container_id,
            } => validate_safe_text(container_id)?,
        }
    }
    validate_safe_text(&view.filter.text)?;
    for value in [
        view.filter.source_id.as_deref(),
        view.filter.field.as_deref(),
        view.filter.field_value.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        validate_safe_text(value)?;
    }
    Ok(())
}

fn validate_safe_text(value: &str) -> Result<(), &'static str> {
    if devbox_applink::contains_sensitive_value(value) {
        Err(SAVED_VIEWS_INPUT_ERROR)
    } else {
        Ok(())
    }
}

fn next_revision(revision: u64) -> Result<u64, &'static str> {
    revision
        .checked_add(1)
        .filter(|value| *value <= MAX_SAFE_INTEGER)
        .ok_or(SAVED_VIEWS_WRITE_ERROR)
}

fn io_guard() -> std::sync::MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::FilterSpec;
    use tempfile::tempdir;

    fn view(name: &str) -> SavedView {
        SavedView {
            name: name.into(),
            sources: vec![SourceSpec::Run {
                source_id: "run-manager:run-1:stdout".into(),
            }],
            filter: FilterSpec {
                text: "error".into(),
                ..FilterSpec::default()
            },
        }
    }

    #[test]
    fn round_trip_uses_revision_cas_and_contains_only_configuration() {
        let root = tempdir().unwrap();
        let first = upsert_in_dir(root.path(), 0, view("오류")).unwrap();
        assert_eq!(first.revision, 1);
        assert_eq!(list_from_dir(root.path()).unwrap(), first);
        assert_eq!(
            upsert_in_dir(root.path(), 0, view("stale")),
            Err(SAVED_VIEWS_CONFLICT_ERROR)
        );
        let raw = fs::read_to_string(saved_views_path(root.path())).unwrap();
        assert!(!raw.contains("records"));
        assert!(!raw.contains("cursors"));
        assert!(!raw.contains("bodyPreview"));
        let empty = delete_in_dir(root.path(), 1, "오류").unwrap();
        assert_eq!(empty.revision, 2);
        assert!(empty.views.is_empty());
    }

    #[test]
    fn ephemeral_and_path_handoff_sources_never_persist() {
        let root = tempdir().unwrap();
        let wsl = SavedView {
            name: "WSL".into(),
            sources: vec![SourceSpec::WslFile {
                distro: "Ubuntu".into(),
                path: "/var/log/private.log".into(),
            }],
            filter: FilterSpec::default(),
        };
        assert_eq!(
            upsert_in_dir(root.path(), 0, wsl),
            Err(SAVED_VIEWS_INPUT_ERROR)
        );

        let capture = devbox_applink::webhook_log_payload("GET", "/hook", 1, &[], "ok").unwrap();
        let webhook = SavedView {
            name: "Webhook".into(),
            sources: vec![SourceSpec::WebhookCapture { capture }],
            filter: FilterSpec::default(),
        };
        assert_eq!(
            upsert_in_dir(root.path(), 0, webhook),
            Err(SAVED_VIEWS_INPUT_ERROR)
        );
        assert!(!saved_views_path(root.path()).exists());
    }

    #[test]
    fn credentials_and_capacity_fail_explicitly() {
        let root = tempdir().unwrap();
        let mut unsafe_view = view("unsafe");
        unsafe_view.filter.text = "password=raw-secret".into();
        assert_eq!(
            upsert_in_dir(root.path(), 0, unsafe_view),
            Err(SAVED_VIEWS_INPUT_ERROR)
        );

        let mut document = SavedViewsDocument::default();
        for index in 0..MAX_SAVED_VIEWS {
            document.views.push(view(&format!("view-{index}")));
        }
        save_unlocked(root.path(), &document).unwrap();
        assert_eq!(
            upsert_in_dir(root.path(), 0, view("overflow")),
            Err(SAVED_VIEWS_LIMIT_ERROR)
        );
    }

    #[test]
    fn corrupt_or_unknown_store_is_preserved() {
        let root = tempdir().unwrap();
        let path = saved_views_path(root.path());
        let corrupt = br#"{"schemaVersion":1,"revision":0,"views":[],"records":["secret"]}"#;
        fs::write(&path, corrupt).unwrap();
        assert_eq!(list_from_dir(root.path()), Err(SAVED_VIEWS_READ_ERROR));
        assert_eq!(
            upsert_in_dir(root.path(), 0, view("new")),
            Err(SAVED_VIEWS_READ_ERROR)
        );
        assert_eq!(fs::read(path).unwrap(), corrupt);
    }

    #[cfg(unix)]
    #[test]
    fn linked_store_is_rejected_without_replacing_its_target() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let outside = root.path().join("outside.json");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, saved_views_path(root.path())).unwrap();
        assert_eq!(list_from_dir(root.path()), Err(SAVED_VIEWS_READ_ERROR));
        assert!(upsert_in_dir(root.path(), 0, view("new")).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"outside");
    }
}
