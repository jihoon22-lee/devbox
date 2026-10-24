//! `config.toml [activity]`: privacy rules and the idle threshold. Core keeps the
//! table opaque; it is read here. Anything unreadable fails closed (no titles)
//! instead of stopping the daemon. Writes take the shared config lock (also taken
//! by `devbox_secrets` `save_name`) and keep comments and other sections.
use crate::model::idle::{DEFAULT_IDLE_THRESHOLD_MS, MAX_IDLE_THRESHOLD_MS, MIN_IDLE_THRESHOLD_MS};
use crate::model::privacy::{CompiledRules, PrivacyRules};
use serde::Deserialize;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// Empty file in the config folder; every `config.toml` writer holds it exclusively.
pub const LOCK_FILE: &str = ".config.toml.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivitySettings {
    pub rules: PrivacyRules,
    pub idle_threshold_ms: i64,
    /// `false`: the table did not parse; `rules` then masks every title.
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
    /// Never fails: a table that does not parse becomes "mask all titles".
    pub fn from_table(table: &toml::Table) -> Self {
        match toml::Value::Table(table.clone()).try_into::<Raw>() {
            Ok(raw) => Self {
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
            },
            Err(_) => Self {
                rules: PrivacyRules {
                    mask_all_titles: true,
                    ..PrivacyRules::default()
                },
                idle_threshold_ms: DEFAULT_IDLE_THRESHOLD_MS,
                readable: false,
            },
        }
    }

    /// The rules the daemon applies; anything not compilable stores no titles.
    pub fn compiled(&self) -> CompiledRules {
        if !self.readable {
            return CompiledRules::fail_closed();
        }
        CompiledRules::compile(&self.rules).unwrap_or_else(|_| CompiledRules::fail_closed())
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum WriteError {
    #[error("config.toml is not a regular TOML file")]
    Unavailable,
    #[error("{0}")]
    Io(#[from] std::io::Error),
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
/// file is refused) → edit `[activity]` → same-folder temp file, fsync, rename.
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
