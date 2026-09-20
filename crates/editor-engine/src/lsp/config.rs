//! Persistence for the user-selected LSP configuration.
//!
//! LSP configuration is disposable metadata.  It is deliberately kept out of
//! the editor session file and contains no process handles, buffers, or binary
//! data.  The path helpers in this module accept an app-local-data directory
//! instead of a Tauri handle so loading and saving remain directly testable.
//! Writes use the same sibling-temp/atomic-replace helper as `session.json`.

use super::catalog::{LspConfig, SchemaError};
use serde::Serialize;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const LSP_CONFIG_DIRECTORY: &str = "lsp";
pub const LSP_CONFIG_FILE_NAME: &str = "config.json";

/// Returns the persisted config location below `app_local_data_dir()`.
///
/// Keeping this function pure avoids making the schema depend on a Tauri
/// `AppHandle`; the command layer can obtain the app-local directory and pass
/// it here when it is eventually wired to UI commands.
pub fn lsp_config_path(app_local_data_dir: impl AsRef<Path>) -> PathBuf {
    app_local_data_dir
        .as_ref()
        .join(LSP_CONFIG_DIRECTORY)
        .join(LSP_CONFIG_FILE_NAME)
}

/// Alias with a more explicit name for callers that already have a path from
/// `app_local_data_dir()`.
pub fn config_path_from_app_local_data_dir(app_local_data_dir: impl AsRef<Path>) -> PathBuf {
    lsp_config_path(app_local_data_dir)
}

/// The result of reading a config file.  A malformed or unsupported file is
/// not an I/O failure: it starts an empty in-memory config, preserves the
/// original file, and marks persistence as disallowed until the caller makes
/// an explicit recovery decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoadedLspConfig {
    pub config: LspConfig,
    pub persist_allowed: bool,
    pub error: Option<String>,
}

impl LoadedLspConfig {
    fn missing() -> Self {
        Self {
            config: LspConfig::empty(),
            persist_allowed: true,
            error: None,
        }
    }

    fn valid(config: LspConfig) -> Self {
        Self {
            config,
            persist_allowed: true,
            error: None,
        }
    }

    fn invalid(error: SchemaError) -> Self {
        Self {
            config: LspConfig::empty(),
            persist_allowed: false,
            error: Some(error.to_string()),
        }
    }
}

/// Errors reading the file itself.  JSON corruption and schema mismatches are
/// intentionally represented by `LoadedLspConfig::error` instead, so callers
/// can keep the editor usable while still showing an actionable status.
#[derive(Debug)]
pub enum LspConfigLoadError {
    Read { path: PathBuf, source: io::Error },
}

impl fmt::Display for LspConfigLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "could not read LSP config {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LspConfigLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
        }
    }
}

/// Errors validating or atomically writing a config file.
#[derive(Debug)]
pub enum LspConfigSaveError {
    Validation(super::catalog::ValidationError),
    Serialize(serde_json::Error),
    Write { path: PathBuf, source: io::Error },
}

impl fmt::Display for LspConfigSaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => error.fmt(f),
            Self::Serialize(error) => write!(f, "could not serialize LSP config: {error}"),
            Self::Write { path, source } => {
                write!(f, "could not save LSP config {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for LspConfigSaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            Self::Serialize(error) => Some(error),
            Self::Write { source, .. } => Some(source),
        }
    }
}

/// Read a config from a path with the required disposable-state policy.
pub fn load_from_path(path: &Path) -> Result<LoadedLspConfig, LspConfigLoadError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LoadedLspConfig::missing())
        }
        Err(source) => {
            return Err(LspConfigLoadError::Read {
                path: path.to_path_buf(),
                source,
            })
        }
    };

    Ok(match LspConfig::from_json(&contents) {
        Ok(config) => LoadedLspConfig::valid(config),
        Err(error) => LoadedLspConfig::invalid(error),
    })
}

/// Verbose alias matching the existing session persistence API.
pub fn load_from_path_with_status(path: &Path) -> Result<LoadedLspConfig, LspConfigLoadError> {
    load_from_path(path)
}

/// Load from an app-local-data directory without requiring a Tauri handle.
pub fn load_from_app_local_data_dir(
    app_local_data_dir: impl AsRef<Path>,
) -> Result<LoadedLspConfig, LspConfigLoadError> {
    load_from_path(&lsp_config_path(app_local_data_dir))
}

/// Validate, serialize, and atomically replace a config file.
///
/// `atomic_write` creates the `lsp/` directory and writes a uniquely named
/// sibling temporary file before replacing the destination.  In particular,
/// this function never truncates the existing config before the replacement
/// commit point.
pub fn save_to_path(path: &Path, config: &LspConfig) -> Result<(), LspConfigSaveError> {
    config.validate().map_err(LspConfigSaveError::Validation)?;
    let json = config.to_json().map_err(LspConfigSaveError::Serialize)?;
    crate::commands::session::atomic_write(path, json.as_bytes()).map_err(|source| {
        LspConfigSaveError::Write {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// Save below an app-local-data directory without requiring a Tauri handle.
pub fn save_to_app_local_data_dir(
    app_local_data_dir: impl AsRef<Path>,
    config: &LspConfig,
) -> Result<(), LspConfigSaveError> {
    save_to_path(&lsp_config_path(app_local_data_dir), config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::{RuntimeKind, RuntimeSpec, ServerRef};
    use std::collections::BTreeMap;

    fn config() -> LspConfig {
        let mut config = LspConfig::empty();
        config.enabled = true;
        config.workspace_root = "/work/project".into();
        config.server_by_language = BTreeMap::from([(
            "rust".into(),
            ServerRef::custom("rust-analyzer", vec!["--stdio".into()]),
        )]);
        config.custom_servers.push(crate::lsp::CustomServer {
            language_ids: vec!["python".into()],
            executable: "pyright-langserver".into(),
            args: vec!["--stdio".into()],
            runtime: RuntimeSpec {
                kind: RuntimeKind::Node,
                executable: "node".into(),
                min_version: Some(">=14".into()),
            },
            source: "user-provided/unchecked".into(),
            license: "unknown".into(),
            version: "unknown".into(),
        });
        config
    }

    #[test]
    fn app_local_path_is_pure_and_nested_under_lsp() {
        assert_eq!(
            lsp_config_path("/tmp/code-pad"),
            PathBuf::from("/tmp/code-pad/lsp/config.json")
        );
    }

    #[test]
    fn missing_config_is_empty_and_persistable() {
        let directory = tempfile::tempdir().unwrap();
        let loaded = load_from_app_local_data_dir(directory.path()).unwrap();
        assert_eq!(loaded.config, LspConfig::empty());
        assert!(loaded.persist_allowed);
        assert!(loaded.error.is_none());
        assert!(!lsp_config_path(directory.path()).exists());
    }

    #[test]
    fn corrupt_and_unknown_version_configs_are_not_rewritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = lsp_config_path(directory.path());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{ definitely not json").unwrap();
        let before = fs::read(&path).unwrap();
        let loaded = load_from_path_with_status(&path).unwrap();
        assert_eq!(loaded.config, LspConfig::empty());
        assert!(!loaded.persist_allowed);
        assert!(loaded
            .error
            .as_deref()
            .is_some_and(|error| !error.is_empty()));
        assert_eq!(fs::read(&path).unwrap(), before);

        let mut unknown = config();
        unknown.version += 1;
        fs::write(&path, unknown.to_json().unwrap()).unwrap();
        let before = fs::read(&path).unwrap();
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.config, LspConfig::empty());
        assert!(!loaded.persist_allowed);
        assert!(loaded
            .error
            .as_deref()
            .unwrap()
            .contains("unsupported schema"));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn save_roundtrips_and_uses_atomic_sibling_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = lsp_config_path(directory.path());
        let first = config();
        save_to_path(&path, &first).unwrap();
        let loaded = load_from_path(&path).unwrap();
        assert_eq!(loaded.config, first);
        assert!(loaded.persist_allowed);

        let mut second = first.clone();
        second.enabled = false;
        save_to_path(&path, &second).unwrap();
        assert_eq!(load_from_path(&path).unwrap().config, second);
        let entries: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from("config.json")]);
    }

    #[test]
    fn invalid_config_is_rejected_before_any_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = lsp_config_path(directory.path());
        let good = config();
        save_to_path(&path, &good).unwrap();
        let before = fs::read(&path).unwrap();

        let mut invalid = good;
        invalid.workspace_root = "relative/workspace".into();
        assert!(matches!(
            save_to_path(&path, &invalid),
            Err(LspConfigSaveError::Validation(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
