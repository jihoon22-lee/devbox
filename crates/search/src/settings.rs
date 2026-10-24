//! `[search]` in `config.toml`. Core keeps the table opaque; its keys are checked here.
use serde::Deserialize;

/// Exclude globs at most, and bytes per glob.
pub const MAX_EXCLUDES: usize = 64;
pub const MAX_GLOB_BYTES: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchSettings {
    /// gitignore-style globs left out of code and file-name search (`**/dist/**`).
    pub exclude: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Raw {
    exclude: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("[search] {0}")]
    Invalid(String),
}

impl SearchSettings {
    pub fn from_table(table: &toml::Table) -> Result<Self, SettingsError> {
        let raw: Raw = toml::Value::Table(table.clone())
            .try_into()
            .map_err(|e: toml::de::Error| SettingsError::Invalid(e.message().to_owned()))?;
        if raw.exclude.len() > MAX_EXCLUDES {
            return Err(SettingsError::Invalid(format!(
                "exclude has more than {MAX_EXCLUDES} globs"
            )));
        }
        let mut check = ignore::overrides::OverrideBuilder::new("/");
        for glob in &raw.exclude {
            if glob.is_empty() || glob.len() > MAX_GLOB_BYTES || glob.contains('\0') {
                return Err(SettingsError::Invalid(format!(
                    "exclude globs must be 1..={MAX_GLOB_BYTES} bytes without NUL"
                )));
            }
            check
                .add(&format!("!{glob}"))
                .map_err(|_| SettingsError::Invalid("exclude has an invalid glob".into()))?;
        }
        Ok(Self {
            exclude: raw.exclude,
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
    fn excludes_are_checked_globs_and_unknown_keys_fail() {
        let s = SearchSettings::from_table(&toml::Table::new()).expect("defaults");
        assert!(s.exclude.is_empty());
        let s = SearchSettings::from_table(&table("exclude = ['**/dist/**']")).expect("globs");
        assert_eq!(s.exclude, ["**/dist/**"]);
        let many = format!("exclude = [{}]", vec!["'a'"; MAX_EXCLUDES + 1].join(","));
        let long = format!("exclude = ['{}']", "a".repeat(MAX_GLOB_BYTES + 1));
        for bad in [
            "exclud = []".to_string(),
            "exclude = 'x'".into(),
            "exclude = ['a/{b']".into(),
            "exclude = ['']".into(),
            "exclude = [\"a\\u0000b\"]".into(),
            many,
            long,
        ] {
            assert!(SearchSettings::from_table(&table(&bad)).is_err(), "{bad}");
        }
    }
}
