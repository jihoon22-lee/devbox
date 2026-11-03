//! Lenient loading for the daemon: a file or section that does not load turns off only
//! what it configures and is reported as a [`StartupProblem`] instead of stopping startup.
use super::*;
use devbox_protocol::system::{SectionEffect, StartupProblem};
use std::collections::BTreeSet;

/// Features that stay off because their settings did not load. They never fall back to
/// a default vault, search scope or Windows copy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Disabled {
    pub notes: bool,
    pub search: bool,
    pub windows: bool,
}

/// The configuration the daemon runs with and everything that was ignored to get it.
#[derive(Debug)]
pub struct ConfigLoad {
    pub config: Config,
    pub disabled: Disabled,
    pub problems: Vec<StartupProblem>,
    /// Each readable file's sections that passed, for checks the domains run.
    files: Vec<(PathBuf, toml::Table)>,
    /// Sections on their failure policy (all of them after a whole-file failure).
    failed: BTreeSet<String>,
    /// `config.toml`, named by a problem that no file's copy explains.
    shared: PathBuf,
}

/// Top-level sections. Core checks the first seven; the domains check the rest.
const SECTIONS: [&str; 10] = [
    "projects", "pc", "windows", "terminal", "agents", "mcp", "secrets", "notes", "search",
    "activity",
];
/// Checked entry by entry in `devbox_agents::profile::configured_lenient`.
const PROFILES: &str = "agents.profiles";

/// Reads `config.toml` then `config.local.toml`. A whole-file failure (unreadable or not
/// TOML) puts every section on its failure policy. Otherwise each file's sections are
/// checked separately, so a problem names its own file, and a section that fails in either
/// file gets its failure policy: notes, search and Windows copies off, `[mcp]` write off,
/// `[agents]` cleanup after close off, other sections default. Unknown sections and
/// `[windows]` in `config.toml` are ignored.
pub fn load_lenient(paths: &Paths) -> ConfigLoad {
    let mut load = assemble(paths);
    // The daemon reads an unset key as on, and cleanup deletes a closed task's work folder and
    // branch. So a failed `[agents]`, which a whole-file failure also is, never leaves it unset,
    // whichever way `assemble` returned. `agents.profiles` fails on its own and leaves the key
    // as configured.
    if load.failed.contains("agents") {
        load.config.agents.cleanup_after_close = Some(false);
    }
    load
}

fn assemble(paths: &Paths) -> ConfigLoad {
    let shared = paths.config_dir.join("config.toml");
    let local = paths.config_dir.join("config.local.toml");
    let mut load = ConfigLoad {
        config: Config::default(),
        disabled: Disabled::default(),
        problems: Vec::new(),
        files: Vec::new(),
        failed: BTreeSet::new(),
        shared: shared.clone(),
    };
    for path in [shared, local.clone()] {
        match read(&path) {
            Ok(Some(table)) => load.files.push((path, table)),
            Ok(None) => {}
            Err(message) => load.problems.push(StartupProblem::ConfigFile {
                file: path.display().to_string(),
                message,
            }),
        }
    }
    if !load.problems.is_empty() {
        load.fail_all();
        return load;
    }
    let mut files = std::mem::take(&mut load.files);
    for (path, table) in &mut files {
        load.check_file(path, table, *path == local);
    }
    let mut merged = toml::Table::new();
    for (_, table) in &files {
        merge(&mut merged, table.clone());
    }
    // A failed section never falls back to the other file's copy.
    for section in &load.failed {
        match section.as_str() {
            PROFILES => {
                if let Some(agents) = merged.get_mut("agents").and_then(toml::Value::as_table_mut) {
                    agents.remove("profiles");
                }
            }
            "agents" => {
                if let Some(agents) = merged.get_mut("agents").and_then(toml::Value::as_table_mut) {
                    agents.retain(|key, _| key == "profiles");
                }
            }
            section => {
                merged.remove(section);
            }
        }
    }
    match toml::Value::Table(merged).try_into::<Config>() {
        Ok(config) => load.config = config,
        // Each section was valid in its own file, so this cannot happen; never fail open.
        Err(error) => {
            load.problems.push(StartupProblem::ConfigFile {
                file: local.display().to_string(),
                message: error.message().to_owned(),
            });
            load.fail_all();
            return load;
        }
    }
    if !load.failed.contains(PROFILES) {
        load.config.agents.profiles_file = files
            .iter()
            .rev()
            .find(|(_, table)| {
                table
                    .get("agents")
                    .and_then(|a| a.get("profiles"))
                    .is_some()
            })
            .map(|(path, _)| path.clone());
    }
    load.files = files;
    load
}

impl ConfigLoad {
    /// Reads a section its domain owns (`notes`, `search`, `activity`). `read` checks each
    /// file's copy first, so a problem names the file it is in, then reads the merged copy.
    /// `None` means the section is on its failure policy: notes and search are disabled,
    /// the caller handles the rest. A section that already failed is not reported again.
    pub fn section<T, E: std::fmt::Display>(
        &mut self,
        name: &str,
        read: impl Fn(&toml::Table) -> Result<T, E>,
    ) -> Option<T> {
        if self.failed.contains(name) {
            return None;
        }
        let (mut merged, mut errors, mut last) = (toml::Table::new(), Vec::new(), None);
        for (path, table) in &self.files {
            let Some(copy) = table.get(name).and_then(toml::Value::as_table) else {
                continue;
            };
            if let Err(error) = read(copy) {
                errors.push((path.clone(), error.to_string()));
            }
            merge(&mut merged, copy.clone());
            last = Some(path.clone());
        }
        if !errors.is_empty() {
            for (path, message) in errors {
                self.report(&path, name, SectionEffect::Disabled, message);
            }
            self.fail(name);
            return None;
        }
        match read(&merged) {
            Ok(value) => Some(value),
            Err(error) => {
                let file = last.unwrap_or_else(|| self.shared.clone());
                self.report(&file, name, SectionEffect::Disabled, error.to_string());
                self.fail(name);
                None
            }
        }
    }

    /// The file whose `section.key` the merged copy holds: the last that sets it, since lists
    /// replace, or `config.toml`.
    pub fn source(&self, section: &str, key: &str) -> PathBuf {
        let sets = |table: &toml::Table| table.get(section).and_then(|s| s.get(key)).is_some();
        let found = self.files.iter().rev().find(|(_, table)| sets(table));
        found.map_or_else(|| self.shared.clone(), |(path, _)| path.clone())
    }

    fn report(&mut self, file: &Path, section: &str, effect: SectionEffect, message: String) {
        self.problems.push(StartupProblem::ConfigSection {
            file: file.display().to_string(),
            section: section.to_owned(),
            message,
            effect,
        });
    }

    fn fail(&mut self, section: &str) {
        match section {
            "notes" => self.disabled.notes = true,
            "search" => self.disabled.search = true,
            "windows" => self.disabled.windows = true,
            _ => {}
        }
        self.failed.insert(section.to_owned());
    }

    fn fail_all(&mut self) {
        self.config = Config::default();
        self.files.clear();
        for section in SECTIONS.into_iter().chain([PROFILES]) {
            self.fail(section);
        }
    }

    /// Drops what this file may not set or did not get right, reporting each section once.
    fn check_file(&mut self, path: &Path, table: &mut toml::Table, local: bool) {
        let keys: Vec<String> = table.keys().cloned().collect();
        for key in keys {
            // These two only drop this file's copy: nothing else is turned off.
            if !SECTIONS.contains(&key.as_str()) {
                table.remove(&key);
                self.report(path, &key, SectionEffect::Ignored, "unknown section".into());
            } else if key == "windows" && !local {
                table.remove(&key);
                self.report(
                    path,
                    &key,
                    SectionEffect::Ignored,
                    "windows is only allowed in config.local.toml".into(),
                );
            } else if let Err(message) = check(&key, &table[&key]) {
                match table.get_mut("agents").and_then(toml::Value::as_table_mut) {
                    // Profiles are checked entry by entry; other agents keys must not drop them.
                    Some(agents) if key == "agents" => agents.retain(|k, _| k == "profiles"),
                    _ => {
                        table.remove(&key);
                    }
                }
                self.report(path, &key, SectionEffect::Disabled, message);
                self.fail(&key);
            }
        }
        let profiles = table
            .get_mut("agents")
            .and_then(toml::Value::as_table_mut)
            .filter(|agents| agents.get("profiles").is_some_and(|p| !p.is_array()));
        if let Some(agents) = profiles {
            agents.remove("profiles");
            self.report(
                path,
                PROFILES,
                SectionEffect::Disabled,
                "must be an array of tables".into(),
            );
            self.fail(PROFILES);
        }
    }
}

/// `Ok(None)`: the file does not exist.
fn read(path: &Path) -> Result<Option<toml::Table>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    text.parse::<toml::Table>().map(Some).map_err(|error| {
        let message = error.message().trim().replace('\n', "; ");
        match error.span().and_then(|span| text.get(..span.start)) {
            Some(before) => format!(
                "line {}, column {}: {message}",
                before.matches('\n').count() + 1,
                before
                    .rsplit('\n')
                    .next()
                    .map_or(0, |line| line.chars().count())
                    + 1
            ),
            None => message,
        }
    })
}

/// Core's own sections; the domain-owned ones only need to be tables here.
fn check(section: &str, value: &toml::Value) -> Result<(), String> {
    let table = value.as_table().ok_or("must be a table")?;
    match section {
        "projects" => typed::<ProjectsConfig>(table),
        "pc" => typed::<PcConfig>(table),
        "windows" => typed::<WindowsConfig>(table),
        // Serde structs also accept positional sequences; profiles must be tables.
        "terminal"
            if table
                .get("profiles")
                .and_then(toml::Value::as_array)
                .is_some_and(|profiles| profiles.iter().any(|value| !value.is_table())) =>
        {
            Err("terminal profiles must be tables".into())
        }
        "terminal" => typed::<TerminalConfig>(table),
        "agents" => {
            let mut rest = table.clone();
            rest.remove("profiles");
            typed::<AgentsConfig>(&rest)
        }
        "mcp" => typed::<McpConfig>(table),
        "secrets" => typed::<SecretsConfig>(table),
        _ => Ok(()),
    }
}

fn typed<T: serde::de::DeserializeOwned>(table: &toml::Table) -> Result<(), String> {
    toml::Value::Table(table.clone())
        .try_into::<T>()
        .map(drop)
        .map_err(|error| error.message().to_owned())
}

#[cfg(test)]
#[path = "lenient_tests.rs"]
mod tests;
