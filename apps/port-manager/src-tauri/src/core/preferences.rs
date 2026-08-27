//! Strict, bounded Port Manager view preferences.
//!
//! Only endpoint/provenance identities are persisted here.  Display-only
//! command lines and executable paths deliberately have no representation in
//! this module, so a saved preference cannot accidentally become a secret or
//! process-control input.

use super::listeners::{ListenerIdentity, ListenerSource, MAX_NAME_BYTES};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub const PREFERENCES_SCHEMA_VERSION: u8 = 1;
pub const DEFAULT_REFRESH_INTERVAL_MS: u32 = 5_000;
pub const MIN_REFRESH_INTERVAL_MS: u32 = 1_000;
pub const MAX_REFRESH_INTERVAL_MS: u32 = 60_000;
pub const MAX_PREFERENCES_BYTES: usize = 64 * 1024;
pub const MAX_FAVORITES_PER_KIND: usize = 256;
pub const MAX_FAVORITE_FIELD_BYTES: usize = MAX_NAME_BYTES;
pub const PREFERENCES_FILE_NAME: &str = "port-manager-preferences-v1.json";

/// Port identity used for a favorite.  It intentionally contains no process
/// path, command line, or free-form secret-bearing metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PortFavorite {
    pub source: ListenerSource,
    pub proto: String,
    pub local_addr: String,
    pub port: u16,
}

/// Process identity used for a favorite.  The identity is the same validated
/// precondition used by the existing #285 kill boundary, but is never itself a
/// kill request when loaded from disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProcessFavorite {
    pub source: ListenerSource,
    pub identity: ListenerIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PortManagerPreferences {
    pub schema_version: u8,
    pub refresh_interval_ms: u32,
    pub pinned_only: bool,
    pub favorite_ports: Vec<PortFavorite>,
    pub favorite_processes: Vec<ProcessFavorite>,
}

impl Default for PortManagerPreferences {
    fn default() -> Self {
        Self {
            schema_version: PREFERENCES_SCHEMA_VERSION,
            refresh_interval_ms: DEFAULT_REFRESH_INTERVAL_MS,
            pinned_only: false,
            favorite_ports: Vec::new(),
            favorite_processes: Vec::new(),
        }
    }
}

impl PortManagerPreferences {
    pub fn validate(&self) -> Result<(), PreferencesError> {
        if self.schema_version != PREFERENCES_SCHEMA_VERSION
            || !(MIN_REFRESH_INTERVAL_MS..=MAX_REFRESH_INTERVAL_MS)
                .contains(&self.refresh_interval_ms)
            || self.favorite_ports.len() > MAX_FAVORITES_PER_KIND
            || self.favorite_processes.len() > MAX_FAVORITES_PER_KIND
        {
            return Err(PreferencesError::Invalid);
        }

        for favorite in &self.favorite_ports {
            validate_port_favorite(favorite)?;
        }
        for favorite in &self.favorite_processes {
            validate_process_favorite(favorite)?;
        }
        if has_duplicate(&self.favorite_ports) || has_duplicate(&self.favorite_processes) {
            return Err(PreferencesError::Invalid);
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, PreferencesError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| PreferencesError::Invalid)?;
        if bytes.is_empty() || bytes.len() > MAX_PREFERENCES_BYTES {
            return Err(PreferencesError::TooLarge);
        }
        Ok(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesError {
    Invalid,
    TooLarge,
    Io,
    Corrupt,
}

impl fmt::Display for PreferencesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Invalid => "invalid Port Manager preferences",
            Self::TooLarge => "Port Manager preferences exceed the size limit",
            Self::Io => "Port Manager preferences are unavailable",
            Self::Corrupt => "Port Manager preferences are corrupt",
        })
    }
}

impl std::error::Error for PreferencesError {}

pub fn preferences_path(app_local_data_dir: impl AsRef<Path>) -> PathBuf {
    app_local_data_dir.as_ref().join(PREFERENCES_FILE_NAME)
}

/// Load only a bounded, strict JSON document.  A missing file is the normal
/// first-launch case; corrupt or invalid state is surfaced so the caller can
/// retain in-memory defaults without touching the last stable view.
pub fn load_from_path(path: impl AsRef<Path>) -> Result<PortManagerPreferences, PreferencesError> {
    let path = path.as_ref();
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(PortManagerPreferences::default())
        }
        Err(_) => return Err(PreferencesError::Io),
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_PREFERENCES_BYTES as u64 {
        return Err(PreferencesError::TooLarge);
    }

    let file = File::open(path).map_err(|_| PreferencesError::Io)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_PREFERENCES_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| PreferencesError::Io)?;
    if bytes.is_empty() || bytes.len() > MAX_PREFERENCES_BYTES {
        return Err(PreferencesError::TooLarge);
    }
    let preferences = serde_json::from_slice::<PortManagerPreferences>(&bytes)
        .map_err(|_| PreferencesError::Corrupt)?;
    preferences.validate()?;
    Ok(preferences)
}

/// Write a complete preference document through the shared atomic writer.
/// The parent directory belongs to this command and is created explicitly;
/// no arbitrary frontend path is accepted.
pub fn save_to_path(
    path: impl AsRef<Path>,
    preferences: &PortManagerPreferences,
) -> Result<(), PreferencesError> {
    let path = path.as_ref();
    let bytes = preferences.encode()?;
    let parent = path.parent().ok_or(PreferencesError::Io)?;
    fs::create_dir_all(parent).map_err(|_| PreferencesError::Io)?;
    // Refuse an existing directory/reparse target before the atomic replace.
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() {
            return Err(PreferencesError::Io);
        }
    }
    devbox_filesystem::atomic_write(path, &bytes).map_err(|_| PreferencesError::Io)
}

fn validate_port_favorite(favorite: &PortFavorite) -> Result<(), PreferencesError> {
    if favorite.port == 0
        || favorite.proto.len() > 16
        || favorite.local_addr.is_empty()
        || favorite.local_addr.len() > MAX_FAVORITE_FIELD_BYTES
        || favorite.proto.chars().any(char::is_control)
        || favorite.local_addr.chars().any(char::is_control)
        || !matches!(
            favorite.proto.to_ascii_uppercase().as_str(),
            "TCP" | "TCP4" | "TCP6" | "UDP" | "UDP4" | "UDP6"
        )
        // Rows emitted by the native adapters use canonical uppercase
        // protocol names. Rejecting alternate casing keeps duplicate and
        // favorite matching deterministic across frontend/native boundaries.
        || favorite.proto != favorite.proto.to_ascii_uppercase()
        || !is_safe_local_endpoint(&favorite.local_addr, favorite.port)
    {
        return Err(PreferencesError::Invalid);
    }
    Ok(())
}

/// Favorites contain an endpoint, not an arbitrary user string. Keep the
/// persisted address to the small grammar emitted by netstat/ss/docker so a
/// path, URL query, or command fragment can never be written as a favorite.
fn is_safe_local_endpoint(value: &str, expected_port: u16) -> bool {
    if value.bytes().any(|byte| {
        !(byte.is_ascii_alphanumeric()
            || matches!(byte, b'.' | b':' | b'[' | b']' | b'%' | b'-' | b'*'))
    }) {
        return false;
    }
    let Some((_, port_text)) = value.rsplit_once(':') else {
        return false;
    };
    port_text.parse::<u16>().ok() == Some(expected_port)
}

fn validate_process_favorite(favorite: &ProcessFavorite) -> Result<(), PreferencesError> {
    favorite
        .identity
        .validate()
        .map_err(|_| PreferencesError::Invalid)?;
    let source_matches = matches!(
        (&favorite.source, &favorite.identity),
        (ListenerSource::Windows, ListenerIdentity::Windows { .. })
            | (ListenerSource::Wsl, ListenerIdentity::Wsl { .. })
            | (
                ListenerSource::Container,
                ListenerIdentity::Container { .. }
            )
    );
    if source_matches {
        Ok(())
    } else {
        Err(PreferencesError::Invalid)
    }
}

fn has_duplicate<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .any(|(index, item)| items[..index].iter().any(|previous| previous == item))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    fn test_dir() -> PathBuf {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "port-manager-preferences-test-{}-{id}",
            std::process::id()
        ))
    }

    fn port_favorite() -> PortFavorite {
        PortFavorite {
            source: ListenerSource::Windows,
            proto: "TCP".into(),
            local_addr: "127.0.0.1:3000".into(),
            port: 3000,
        }
    }

    fn process_favorite() -> ProcessFavorite {
        ProcessFavorite {
            source: ListenerSource::Windows,
            identity: ListenerIdentity::Windows {
                pid: 42,
                start_time: "100".into(),
            },
        }
    }

    #[test]
    fn default_is_bounded_and_round_trips_without_display_metadata() {
        let preferences = PortManagerPreferences {
            favorite_ports: vec![port_favorite()],
            favorite_processes: vec![process_favorite()],
            ..Default::default()
        };
        let bytes = preferences.encode().unwrap();
        let decoded: PortManagerPreferences = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, preferences);
        assert!(!String::from_utf8(bytes).unwrap().contains("path"));
        assert!(
            !String::from_utf8(serde_json::to_vec(&preferences).unwrap())
                .unwrap()
                .contains("command")
        );
    }

    #[test]
    fn rejects_out_of_range_interval_duplicate_and_unknown_fields() {
        let mut preferences = PortManagerPreferences {
            refresh_interval_ms: MIN_REFRESH_INTERVAL_MS - 1,
            ..Default::default()
        };
        assert_eq!(preferences.validate(), Err(PreferencesError::Invalid));

        let favorite = port_favorite();
        preferences.refresh_interval_ms = DEFAULT_REFRESH_INTERVAL_MS;
        preferences.favorite_ports = vec![favorite.clone(), favorite];
        assert_eq!(preferences.validate(), Err(PreferencesError::Invalid));

        let unknown = serde_json::json!({
            "schema_version": 1,
            "refresh_interval_ms": 5000,
            "pinned_only": false,
            "favorite_ports": [],
            "favorite_processes": [],
            "executable_path": "C:\\secret\\app.exe"
        });
        assert!(serde_json::from_value::<PortManagerPreferences>(unknown).is_err());

        let mut unsafe_endpoint = PortManagerPreferences {
            favorite_ports: vec![PortFavorite {
                local_addr: "C:\\secret\\app.exe:3000".into(),
                ..port_favorite()
            }],
            ..Default::default()
        };
        assert_eq!(unsafe_endpoint.validate(), Err(PreferencesError::Invalid));
        unsafe_endpoint.favorite_ports[0].local_addr = "127.0.0.1:3000?token=secret".into();
        assert_eq!(unsafe_endpoint.validate(), Err(PreferencesError::Invalid));

        unsafe_endpoint.favorite_ports[0] = PortFavorite {
            proto: "tcp".into(),
            ..port_favorite()
        };
        assert_eq!(unsafe_endpoint.validate(), Err(PreferencesError::Invalid));
    }

    #[test]
    fn rejects_mismatched_source_and_oversized_favorite_list() {
        let mut mismatched = PortManagerPreferences {
            favorite_processes: vec![ProcessFavorite {
                source: ListenerSource::Windows,
                identity: ListenerIdentity::Wsl {
                    distro: "Ubuntu".into(),
                    pid: 42,
                    start_tick: 9,
                },
            }],
            ..Default::default()
        };
        assert_eq!(mismatched.validate(), Err(PreferencesError::Invalid));

        mismatched.favorite_processes = (0..=MAX_FAVORITES_PER_KIND)
            .map(|pid| ProcessFavorite {
                source: ListenerSource::Windows,
                identity: ListenerIdentity::Windows {
                    pid: (pid as u32).saturating_add(1),
                    start_time: (pid as u64 + 1).to_string(),
                },
            })
            .collect();
        assert_eq!(mismatched.validate(), Err(PreferencesError::Invalid));
    }

    #[test]
    fn load_missing_is_default_and_save_is_atomic() {
        let root = test_dir();
        let path = preferences_path(&root);
        assert_eq!(
            load_from_path(&path).unwrap(),
            PortManagerPreferences::default()
        );

        let preferences = PortManagerPreferences {
            favorite_ports: vec![port_favorite()],
            ..Default::default()
        };
        save_to_path(&path, &preferences).unwrap();
        assert_eq!(load_from_path(&path).unwrap(), preferences);
        assert!(fs::read_dir(&root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_or_oversized_documents_are_not_loaded() {
        let root = test_dir();
        fs::create_dir_all(&root).unwrap();
        let path = preferences_path(&root);
        fs::write(&path, b"{not-json").unwrap();
        assert_eq!(load_from_path(&path), Err(PreferencesError::Corrupt));
        fs::write(&path, vec![b'x'; MAX_PREFERENCES_BYTES + 1]).unwrap();
        assert_eq!(load_from_path(&path), Err(PreferencesError::TooLarge));
        let _ = fs::remove_dir_all(root);
    }
}
