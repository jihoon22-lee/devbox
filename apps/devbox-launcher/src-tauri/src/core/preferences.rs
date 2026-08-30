//! Privacy-safe Launcher favorites and recency ordering.
//!
//! The store contains result identifiers only. Labels, paths, queries,
//! handoff payloads, source details, and secrets are never persisted here.

use devbox_applink::contains_sensitive_value;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

pub const PREFERENCES_FILE: &str = "launcher-preferences.json";
pub const PREFERENCES_VERSION: u32 = 1;
pub const MAX_PREFERENCE_BYTES: u64 = 64 * 1024;
pub const MAX_FAVORITES: usize = 64;
pub const MAX_RECENTS: usize = 64;
pub const MAX_RESULT_ID_BYTES: usize = 256;

const PREFERENCES_ERROR: &str = "Launcher 즐겨찾기 설정을 읽을 수 없습니다";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub version: u32,
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub recents: Vec<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: PREFERENCES_VERSION,
            favorites: Vec::new(),
            recents: Vec::new(),
        }
    }
}

impl Preferences {
    pub fn load(path: &Path) -> Result<Self, String> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default())
            }
            Err(_) => return Err(PREFERENCES_ERROR.into()),
        };
        if metadata.file_type().is_symlink()
            || !metadata.file_type().is_file()
            || metadata.len() > MAX_PREFERENCE_BYTES
        {
            return Err(PREFERENCES_ERROR.into());
        }

        let (mut file, identity) = devbox_filesystem::open_filesystem_object(path, false)
            .map_err(|_| PREFERENCES_ERROR.to_string())?;
        let handle_metadata = file.metadata().map_err(|_| PREFERENCES_ERROR.to_string())?;
        if handle_metadata.len() > MAX_PREFERENCE_BYTES {
            return Err(PREFERENCES_ERROR.into());
        }
        let mut bytes = Vec::with_capacity(handle_metadata.len() as usize);
        file.by_ref()
            .take(MAX_PREFERENCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| PREFERENCES_ERROR.to_string())?;
        if bytes.len() as u64 > MAX_PREFERENCE_BYTES
            || devbox_filesystem::filesystem_identity(path, false)
                .map_err(|_| PREFERENCES_ERROR.to_string())?
                != identity
        {
            return Err(PREFERENCES_ERROR.into());
        }

        let preferences: Self =
            serde_json::from_slice(&bytes).map_err(|_| PREFERENCES_ERROR.to_string())?;
        preferences.validate()?;
        Ok(preferences)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(PREFERENCES_ERROR.into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(PREFERENCES_ERROR.into()),
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| PREFERENCES_ERROR.to_string())?;
        if bytes.len() as u64 > MAX_PREFERENCE_BYTES {
            return Err(PREFERENCES_ERROR.into());
        }
        devbox_filesystem::atomic_write(path, &bytes).map_err(|_| PREFERENCES_ERROR.to_string())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != PREFERENCES_VERSION
            || self.favorites.len() > MAX_FAVORITES
            || self.recents.len() > MAX_RECENTS
            || !unique_valid_ids(&self.favorites)
            || !unique_valid_ids(&self.recents)
        {
            return Err(PREFERENCES_ERROR.into());
        }
        Ok(())
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.favorites.iter().any(|candidate| candidate == id)
    }

    pub fn favorite_rank(&self, id: &str) -> Option<usize> {
        self.favorites.iter().position(|candidate| candidate == id)
    }

    pub fn recent_rank(&self, id: &str) -> Option<usize> {
        self.recents.iter().position(|candidate| candidate == id)
    }

    pub fn set_favorite(&mut self, id: &str, favorite: bool) -> Result<(), String> {
        validate_result_id(id)?;
        self.favorites.retain(|candidate| candidate != id);
        if favorite {
            self.favorites.insert(0, id.to_owned());
            self.favorites.truncate(MAX_FAVORITES);
        }
        self.validate()
    }

    pub fn record_recent(&mut self, id: &str) -> Result<(), String> {
        validate_result_id(id)?;
        self.recents.retain(|candidate| candidate != id);
        self.recents.insert(0, id.to_owned());
        self.recents.truncate(MAX_RECENTS);
        self.validate()
    }

    pub fn clear_recents(&mut self) {
        self.recents.clear();
    }
}

pub fn validate_result_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_RESULT_ID_BYTES
        || contains_sensitive_value(value)
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b':')
        })
    {
        return Err(PREFERENCES_ERROR.into());
    }
    Ok(())
}

fn unique_valid_ids(values: &[String]) -> bool {
    let mut seen = HashSet::with_capacity(values.len());
    values
        .iter()
        .all(|value| validate_result_id(value).is_ok() && seen.insert(value.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "launcher-preferences-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root.join(PREFERENCES_FILE)
    }

    #[test]
    fn round_trip_contains_only_ids_and_order() {
        let path = path("round-trip");
        let mut preferences = Preferences::default();
        preferences
            .set_favorite("snapshot/workbench/profile-1", true)
            .unwrap();
        preferences
            .record_recent("snapshot/repo-manager/repository-1")
            .unwrap();
        preferences.save(&path).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("C:\\"));
        assert!(!raw.contains("query"));
        assert_eq!(Preferences::load(&path).unwrap(), preferences);
    }

    #[test]
    fn favorite_and_recent_updates_are_bounded_and_deduplicated() {
        let mut preferences = Preferences::default();
        for index in 0..MAX_RECENTS + 5 {
            preferences
                .record_recent(&format!("catalog/app/app-{index}"))
                .unwrap();
        }
        preferences.record_recent("catalog/app/app-10").unwrap();
        assert_eq!(preferences.recents.len(), MAX_RECENTS);
        assert_eq!(preferences.recents[0], "catalog/app/app-10");
        assert_eq!(
            preferences
                .recents
                .iter()
                .filter(|id| id.as_str() == "catalog/app/app-10")
                .count(),
            1
        );

        preferences
            .set_favorite("catalog/app/app-10", true)
            .unwrap();
        preferences
            .set_favorite("catalog/app/app-10", false)
            .unwrap();
        assert!(!preferences.is_favorite("catalog/app/app-10"));
    }

    #[test]
    fn corrupt_unknown_duplicate_and_oversized_stores_are_not_replaced() {
        let path = path("corrupt");
        let corrupt =
            br#"{"version":1,"favorites":["same","same"],"recents":[],"path":"C:\\secret"}"#;
        std::fs::write(&path, corrupt).unwrap();
        assert!(Preferences::load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), corrupt);

        std::fs::write(
            &path,
            br#"{"version":1,"favorites":["sk-live-value"],"recents":[]}"#,
        )
        .unwrap();
        assert!(Preferences::load(&path).is_err());

        std::fs::write(&path, vec![b'x'; MAX_PREFERENCE_BYTES as usize + 1]).unwrap();
        assert!(Preferences::load(&path).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn linked_store_is_rejected_without_following_or_replacing_it() {
        use std::os::unix::fs::symlink;

        let path = path("link");
        let outside = path.with_file_name("outside.json");
        std::fs::write(&outside, b"outside").unwrap();
        symlink(&outside, &path).unwrap();

        assert!(Preferences::load(&path).is_err());
        assert!(Preferences::default().save(&path).is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
    }
}
