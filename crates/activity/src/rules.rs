//! `config.toml [activity]`: privacy rules and the idle threshold. Core keeps the
//! table opaque; it is read here. Anything unreadable fails closed (nothing is
//! recorded) instead of stopping the daemon. Writes take the shared config lock
//! (also taken by `devbox_secrets` `save_name`) and keep comments and other sections.
use crate::model::idle::{DEFAULT_IDLE_THRESHOLD_MS, MAX_IDLE_THRESHOLD_MS, MIN_IDLE_THRESHOLD_MS};
use crate::model::privacy::{CompiledRules, InvalidRule, PrivacyRules};
use serde::Deserialize;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Empty file in the config folder; every `config.toml` writer holds it exclusively.
pub const LOCK_FILE: &str = ".config.toml.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivitySettings {
    pub rules: PrivacyRules,
    pub idle_threshold_ms: i64,
    /// `false`: the table did not parse, so nothing is recorded; `rules`, which the
    /// app is shown, then masks every title.
    pub readable: bool,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Raw {
    excluded_processes: Vec<String>,
    excluded_title_patterns: Vec<String>,
    redact_title_patterns: Vec<String>,
    mask_all_titles: bool,
    idle_threshold_ms: i64,
}
impl Default for Raw {
    fn default() -> Self {
        Self {
            excluded_processes: Vec::new(),
            excluded_title_patterns: Vec::new(),
            redact_title_patterns: Vec::new(),
            mask_all_titles: false,
            idle_threshold_ms: DEFAULT_IDLE_THRESHOLD_MS,
        }
    }
}

pub fn valid_idle(ms: i64) -> bool {
    (MIN_IDLE_THRESHOLD_MS..=MAX_IDLE_THRESHOLD_MS).contains(&ms)
}

impl ActivitySettings {
    /// Never fails: a table that does not parse becomes [`Self::unreadable`].
    pub fn from_table(table: &toml::Table) -> Self {
        Self::try_from_table(table).unwrap_or_else(|_| Self::unreadable())
    }

    /// The settings, or why the table does not parse (the daemon reports it).
    pub fn try_from_table(table: &toml::Table) -> Result<Self, toml::de::Error> {
        let raw = toml::Value::Table(table.clone()).try_into::<Raw>()?;
        Ok(Self {
            rules: PrivacyRules {
                excluded_processes: raw.excluded_processes,
                excluded_title_patterns: raw.excluded_title_patterns,
                redact_title_patterns: raw.redact_title_patterns,
                mask_all_titles: raw.mask_all_titles,
            },
            idle_threshold_ms: if valid_idle(raw.idle_threshold_ms) {
                raw.idle_threshold_ms
            } else {
                DEFAULT_IDLE_THRESHOLD_MS
            },
            readable: true,
        })
    }

    /// `[activity]` did not load: record nothing until it does or new rules are saved.
    pub fn unreadable() -> Self {
        Self {
            rules: PrivacyRules {
                mask_all_titles: true,
                ..PrivacyRules::default()
            },
            idle_threshold_ms: DEFAULT_IDLE_THRESHOLD_MS,
            readable: false,
        }
    }

    /// The rules the daemon applies; anything not compilable records nothing.
    pub fn compiled(&self) -> CompiledRules {
        if !self.readable {
            return CompiledRules::fail_closed();
        }
        CompiledRules::compile(&self.rules).unwrap_or_else(|_| CompiledRules::fail_closed())
    }

    /// Why rules that parsed do not compile; empty when they compile or did not parse.
    pub fn invalid_rules(&self) -> Vec<InvalidRule> {
        if !self.readable {
            return Vec::new();
        }
        CompiledRules::compile(&self.rules)
            .err()
            .unwrap_or_default()
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WriteError {
    #[error("config.toml is not a regular TOML file")]
    Unavailable,
    /// This file's `[activity]` does not parse.
    #[error("[activity] in {} does not parse", .0.display())]
    Invalid(PathBuf),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Whether `text`'s `[activity]` parses as the daemon reads each file's copy at start. A file
/// without one does; rules that parse but do not compile do too.
fn activity_parses(text: &str) -> bool {
    match text
        .parse::<toml::Table>()
        .map(|mut table| table.remove("activity"))
    {
        Ok(None) => true,
        Ok(Some(toml::Value::Table(activity))) => {
            ActivitySettings::try_from_table(&activity).is_ok()
        }
        _ => false,
    }
}

/// Opens (creating) the lock file without following a symlink and waits for the
/// exclusive lock. Released when the descriptor drops.
pub(crate) fn lock(dir: &Path) -> Result<rustix::fd::OwnedFd, WriteError> {
    use rustix::fs::{FlockOperation, Mode, OFlags};
    let flags = OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW;
    let fd =
        rustix::fs::open(dir.join(LOCK_FILE), flags, Mode::from_raw_mode(0o600)).map_err(|e| {
            match e {
                // A symlinked lock file is refused like a symlinked config.toml.
                rustix::io::Errno::LOOP => WriteError::Unavailable,
                e => WriteError::Io(e.into()),
            }
        })?;
    rustix::fs::flock(&fd, FlockOperation::LockExclusive).map_err(std::io::Error::from)?;
    Ok(fd)
}

fn string_array(values: &[String]) -> toml_edit::Value {
    toml_edit::Value::Array(values.iter().map(String::as_str).collect())
}

/// Sets one key, keeping the comments around an existing value.
fn set(section: &mut dyn toml_edit::TableLike, key: &str, value: toml_edit::Value) {
    match section.get_mut(key).and_then(toml_edit::Item::as_value_mut) {
        Some(old) => {
            let decor = old.decor().clone();
            *old = value;
            *old.decor_mut() = decor;
        }
        None => {
            section.insert(key, toml_edit::Item::Value(value));
        }
    }
}

/// Blocking: lock → read `config.toml` (missing is empty; symlink or not a regular
/// file is refused) → refuse while its or `config.local.toml`'s `[activity]` does not
/// parse → edit `[activity]` → same-folder temp file, fsync, rename.
pub(crate) fn write(dir: &Path, rules: &PrivacyRules, idle: i64) -> Result<(), WriteError> {
    std::fs::create_dir_all(dir)?;
    let _lock = lock(dir)?;
    let path = dir.join("config.toml");
    if std::fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file()) {
        return Err(WriteError::Unavailable);
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
            return Err(WriteError::Unavailable);
        }
        Err(e) => return Err(e.into()),
    };
    let mut document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| WriteError::Unavailable)?;
    // The editor was shown `unreadable()` lists then, not what the user wrote. Both files are
    // read now, under the lock: one broken by hand since the daemon started counts too.
    if !activity_parses(&text) {
        return Err(WriteError::Invalid(path));
    }
    let local = dir.join("config.local.toml");
    match std::fs::read_to_string(&local) {
        Ok(text) if activity_parses(&text) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        _ => return Err(WriteError::Invalid(local)),
    }
    let section = document
        .as_table_mut()
        .entry("activity")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_like_mut()
        .ok_or(WriteError::Unavailable)?;
    set(
        section,
        "excluded_processes",
        string_array(&rules.excluded_processes),
    );
    set(
        section,
        "excluded_title_patterns",
        string_array(&rules.excluded_title_patterns),
    );
    set(
        section,
        "redact_title_patterns",
        string_array(&rules.redact_title_patterns),
    );
    set(section, "mask_all_titles", rules.mask_all_titles.into());
    set(section, "idle_threshold_ms", idle.into());
    let temp = dir.join(format!(
        ".config.toml.activity-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(document.to_string().as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    Ok(result?)
}
