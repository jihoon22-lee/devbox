//! Strict project settings and safe worktree file copying.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

mod copy;
pub use copy::copy_files;
use copy::relative;
pub(crate) use copy::{check_ancestors, open_regular};

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    pub copy: Vec<String>,
    pub setup: Vec<String>,
    pub teardown: Vec<String>,
    pub test: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LaunchTerminal {
    pub name: String,
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LaunchAgent {
    pub tool: String,
    #[serde(default)]
    pub worktree: bool,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[serde(default, deny_unknown_fields)]
pub struct LaunchConfig {
    pub services: Vec<String>,
    pub terminals: Vec<LaunchTerminal>,
    pub agents: Vec<LaunchAgent>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, TS)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    pub agent: AgentConfig,
    pub launch: LaunchConfig,
    #[serde(rename = "task", skip_serializing)]
    #[ts(skip)]
    pub raw_tasks: Vec<toml::Value>,
    #[serde(rename = "service", skip_serializing)]
    #[ts(skip)]
    pub raw_services: Vec<toml::Value>,
    #[serde(rename = "schedule", skip_serializing)]
    #[ts(skip)]
    pub raw_schedules: Vec<toml::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid project configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("[agent] must be a TOML table")]
    AgentNotTable,
    #[error("[launch] must be a TOML table")]
    LaunchNotTable,
    #[error("[launch].{section}[{index}] must be a TOML table")]
    LaunchEntryNotTable { section: &'static str, index: usize },
    #[error("unsafe copy path: {path}")]
    UnsafePath { path: PathBuf },
    #[error("copy destination already exists: {path}")]
    DestinationExists { path: PathBuf },
    #[error("project configuration I/O at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("project configuration exceeds {limit} bytes")]
    TooLarge { limit: usize },
}

fn io(path: &Path, source: std::io::Error) -> ConfigError {
    ConfigError::Io {
        path: path.into(),
        source,
    }
}

pub fn parse(text: &str) -> Result<ProjectConfig, ConfigError> {
    let document: toml::Table = toml::from_str(text)?;
    if document.get("agent").is_some_and(|agent| !agent.is_table()) {
        return Err(ConfigError::AgentNotTable);
    }
    if let Some(launch) = document.get("launch") {
        let launch = launch.as_table().ok_or(ConfigError::LaunchNotTable)?;
        for section in ["terminals", "agents"] {
            if let Some(entries) = launch.get(section).and_then(toml::Value::as_array) {
                for (index, entry) in entries.iter().enumerate() {
                    if !entry.is_table() {
                        return Err(ConfigError::LaunchEntryNotTable { section, index });
                    }
                }
            }
        }
    }
    let config: ProjectConfig = document.try_into()?;
    for path in &config.agent.copy {
        relative(Path::new(path))?;
    }
    Ok(config)
}

pub fn load(project: &Path) -> Result<ProjectConfig, ConfigError> {
    read_config(project).map(|(_, config)| config)
}

pub const CONFIG_MAX_BYTES: usize = 256 * 1024;

/// Digest callers and parsers consume this same bounded descriptor read.
pub(crate) fn read_config(project: &Path) -> Result<(Vec<u8>, ProjectConfig), ConfigError> {
    let path = project.join(".devbox/devbox.toml");
    check_ancestors(&path)?;
    let read = || -> std::io::Result<Vec<u8>> {
        use std::io::Read;
        let mut bytes = Vec::new();
        open_regular(&path)?
            .take((CONFIG_MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    match read() {
        Ok(bytes) => {
            if bytes.len() > CONFIG_MAX_BYTES {
                return Err(ConfigError::TooLarge {
                    limit: CONFIG_MAX_BYTES,
                });
            }
            let text = std::str::from_utf8(&bytes).map_err(|error| {
                io(
                    &path,
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                )
            })?;
            let config = parse(text)?;
            Ok((bytes, config))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok((Vec::new(), ProjectConfig::default()))
        }
        Err(error) => Err(io(&path, error)),
    }
}

#[cfg(test)]
mod tests;
