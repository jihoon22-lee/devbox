//! `[notes]` in `config.toml`. Core keeps the table opaque; its keys are checked here.
use crate::templates::safe_relative_path;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesSettings {
    /// Absolute vault folder (`~` expanded).
    pub vault: PathBuf,
    pub daily_folder: String,
    pub inbox: String,
    pub templates_folder: String,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Raw {
    vault: String,
    daily_folder: String,
    inbox: String,
    templates_folder: String,
}
impl Default for Raw {
    fn default() -> Self {
        Self {
            vault: "~/notes".into(),
            daily_folder: "daily".into(),
            inbox: "inbox.md".into(),
            templates_folder: "templates".into(),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("[notes] {0}")]
    Invalid(String),
}

impl NotesSettings {
    pub fn from_table(table: &toml::Table, home: &Path) -> Result<Self, SettingsError> {
        let raw: Raw = toml::Value::Table(table.clone())
            .try_into()
            .map_err(|e: toml::de::Error| SettingsError::Invalid(e.message().to_owned()))?;
        let vault = devbox_core::expand_home(&raw.vault, home);
        if !vault.is_absolute() || vault.components().any(|c| c.as_os_str() == "..") {
            return Err(SettingsError::Invalid(
                "vault must be an absolute path or start with ~/".into(),
            ));
        }
        for (name, value) in [
            ("daily_folder", &raw.daily_folder),
            ("templates_folder", &raw.templates_folder),
            ("inbox", &raw.inbox),
        ] {
            if !safe_relative_path(value) || crate::vault::hidden(value) {
                return Err(SettingsError::Invalid(format!(
                    "{name} must be a relative path inside the vault"
                )));
            }
        }
        if !raw.inbox.to_ascii_lowercase().ends_with(".md") {
            return Err(SettingsError::Invalid("inbox must be a .md note".into()));
        }
        Ok(Self {
            vault,
            daily_folder: raw.daily_folder,
            inbox: raw.inbox,
            templates_folder: raw.templates_folder,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> toml::Table {
        text.parse().expect("fixture toml")
    }

    #[test]
    fn defaults_expand_home_and_unknown_keys_fail() {
        let home = Path::new("/home/u");
        let s = NotesSettings::from_table(&toml::Table::new(), home).expect("defaults");
        assert_eq!(s.vault, home.join("notes"));
        assert_eq!(
            (s.daily_folder.as_str(), s.inbox.as_str()),
            ("daily", "inbox.md")
        );
        let s = NotesSettings::from_table(&table("vault='~/n2'"), home).expect("vault");
        assert_eq!(s.vault, home.join("n2"));
        for bad in [
            "valt='~/x'",
            "vault='relative'",
            "vault=1",
            "daily_folder='../x'",
            "templates_folder='/abs'",
            "inbox='x.txt'",
            "inbox='.trash/x.md'",
        ] {
            assert!(
                NotesSettings::from_table(&table(bad), home).is_err(),
                "{bad}"
            );
        }
    }
}
