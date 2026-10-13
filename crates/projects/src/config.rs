//! Strict project settings and safe worktree file copying.
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use ts_rs::TS;

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
}

fn io(path: &Path, source: std::io::Error) -> ConfigError {
    ConfigError::Io {
        path: path.into(),
        source,
    }
}

fn relative(path: &Path) -> Result<(), ConfigError> {
    let text = path.to_string_lossy();
    if text.is_empty()
        || text.contains(['\\', ':'])
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(ConfigError::UnsafePath { path: path.into() });
    }
    Ok(())
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
    let path = project.join(".devbox/devbox.toml");
    check_ancestors(&path)?;
    let read = || -> std::io::Result<String> {
        use std::io::Read;
        let mut text = String::new();
        open_regular(&path)?.read_to_string(&mut text)?;
        Ok(text)
    };
    match read() {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ProjectConfig::default()),
        Err(error) => Err(io(&path, error)),
    }
}

/// Open beneath verified directory descriptors so concurrent symlink replacement
/// cannot redirect configuration or prompt reads outside the project.
#[cfg(unix)]
pub(crate) fn open_regular(path: &Path) -> std::io::Result<std::fs::File> {
    use nix::fcntl::{OFlag, open, openat};
    use nix::sys::stat::Mode;
    let convert = |error: nix::errno::Errno| std::io::Error::from_raw_os_error(error as i32);
    let directory_flags =
        OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_DIRECTORY;
    let mut directory = open(
        if path.is_absolute() {
            Path::new("/")
        } else {
            Path::new(".")
        },
        directory_flags,
        Mode::empty(),
    )
    .map_err(convert)?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsafe path component",
            ));
        };
        if components.peek().is_none() {
            let descriptor = openat(
                &directory,
                Path::new(name),
                OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK,
                Mode::empty(),
            )
            .map_err(convert)?;
            let file = std::fs::File::from(descriptor);
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "expected a regular file",
                ));
            }
            return Ok(file);
        }
        directory =
            openat(&directory, Path::new(name), directory_flags, Mode::empty()).map_err(convert)?;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "missing file name",
    ))
}

#[cfg(not(unix))]
pub(crate) fn open_regular(path: &Path) -> std::io::Result<std::fs::File> {
    check_ancestors(path).map_err(std::io::Error::other)?;
    let file = std::fs::File::open(path)?;
    check_ancestors(path).map_err(std::io::Error::other)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "expected a regular file",
        ));
    }
    Ok(file)
}

// Inspect every existing ancestor, including the roots themselves. A missing
// source is optional, but a dangling symlink is never treated as missing.
pub(crate) fn check_ancestors(path: &Path) -> Result<(), ConfigError> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ConfigError::UnsafePath {
                    path: ancestor.into(),
                });
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io(ancestor, error)),
        }
    }
    Ok(())
}

fn collect(
    source: &Path,
    destination: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), ConfigError> {
    check_ancestors(source)?;
    check_ancestors(destination)?;
    let metadata = match std::fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io(source, error)),
    };
    if metadata.is_dir() {
        for entry in std::fs::read_dir(source).map_err(|error| io(source, error))? {
            let entry = entry.map_err(|error| io(source, error))?;
            collect(&entry.path(), &destination.join(entry.file_name()), files)?;
        }
    } else if metadata.is_file() {
        if std::fs::symlink_metadata(destination).is_ok() {
            return Err(ConfigError::DestinationExists {
                path: destination.into(),
            });
        }
        files.push((source.into(), destination.into()));
    } else {
        return Err(ConfigError::UnsafePath {
            path: source.into(),
        });
    }
    Ok(())
}

/// Copy optional project files to a newly owned worktree, never replacing files.
pub fn copy_files(project: &Path, worktree: &Path, paths: &[String]) -> Result<(), ConfigError> {
    check_ancestors(project)?;
    check_ancestors(worktree)?;
    let target =
        crate::copy_target::CopyTarget::open(worktree).map_err(|error| io(worktree, error))?;
    let mut files = Vec::new();
    for path in paths {
        relative(Path::new(path))?;
        collect(&project.join(path), &worktree.join(path), &mut files)?;
    }
    for (source, destination) in files {
        check_ancestors(&source)?;
        check_ancestors(&destination)?;
        copy_collected_file(&source, &destination, &target)?;
    }
    Ok(())
}

fn copy_collected_file(
    source: &Path,
    destination: &Path,
    target: &crate::copy_target::CopyTarget,
) -> Result<(), ConfigError> {
    let mut input = open_regular(source).map_err(|error| io(source, error))?;
    let mut output = target.create(destination).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            ConfigError::DestinationExists {
                path: destination.to_owned(),
            }
        } else {
            io(destination, error)
        }
    })?;
    std::io::copy(&mut input, &mut output).map_err(|error| io(destination, error))?;
    let permissions = input
        .metadata()
        .map_err(|error| io(source, error))?
        .permissions();
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = permissions;
        permissions.set_mode(permissions.mode() & 0o777);
        permissions
    };
    output
        .set_permissions(permissions)
        .map_err(|error| io(destination, error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn collected_copy_rejects_source_replacement_after_ancestor_validation() {
        for replace_ancestor in [false, true] {
            let root = tempfile::tempdir().expect("owned fixture");
            let source = root.path().join("source");
            let external = root.path().join("external");
            let destination = root.path().join("copied");
            std::fs::create_dir(&source).expect("owned fixture");
            std::fs::create_dir(&external).expect("owned fixture");
            std::fs::write(source.join("file"), "project file").expect("owned fixture");
            std::fs::write(external.join("file"), "outside file").expect("owned fixture");
            check_ancestors(&source.join("file")).expect("initial validation");
            if replace_ancestor {
                std::fs::rename(&source, root.path().join("old-source")).expect("owned fixture");
                std::os::unix::fs::symlink(&external, &source).expect("owned fixture");
            } else {
                std::fs::remove_file(source.join("file")).expect("owned fixture");
                std::os::unix::fs::symlink(external.join("file"), source.join("file"))
                    .expect("owned fixture");
            }
            assert!(
                copy_collected_file(
                    &source.join("file"),
                    &destination,
                    &crate::copy_target::CopyTarget::open(root.path()).expect("pin destination")
                )
                .is_err()
            );
            assert!(!destination.exists(), "outside source was copied");
        }
    }

    #[cfg(unix)]
    #[test]
    fn collected_copy_rejects_destination_parent_replacement() {
        let root = tempfile::tempdir().expect("owned fixture");
        let source = root.path().join("source");
        let target = root.path().join("target");
        let external = root.path().join("external");
        std::fs::write(&source, "project file").expect("owned fixture");
        std::fs::create_dir_all(target.join("dir")).expect("owned fixture");
        std::fs::create_dir(&external).expect("owned fixture");
        let destination = target.join("dir/file");
        check_ancestors(&destination).expect("initial validation");
        let pinned = crate::copy_target::CopyTarget::open(&target).expect("pin destination");
        std::fs::rename(target.join("dir"), target.join("old-dir")).expect("owned fixture");
        std::os::unix::fs::symlink(&external, target.join("dir")).expect("owned fixture");
        let result = copy_collected_file(&source, &destination, &pinned);
        assert!(
            !external.join("file").exists(),
            "copy escaped owned worktree"
        );
        assert!(result.is_err());
    }

    #[test]
    fn launch_sections_and_entries_require_named_toml_tables() {
        assert!(matches!(
            parse("launch=[[],[[\"shell\",\"echo\"]],[]]"),
            Err(ConfigError::LaunchNotTable)
        ));
        for (text, section) in [
            ("[launch]\nterminals=[[\"shell\",\"echo\"]]", "terminals"),
            ("[launch]\nagents=[[\"fake\",false,\"prompt\"]]", "agents"),
            (
                "[launch]\nterminals=[{name='valid'}, ['shell','echo']]",
                "terminals",
            ),
        ] {
            assert!(
                matches!(parse(text), Err(ConfigError::LaunchEntryNotTable { section: actual, .. }) if actual == section),
                "{text}"
            );
        }
        let config = parse("[[launch.terminals]]\nname='shell'\ncommand='echo'\n[[launch.agents]]\ntool='fake'\nworktree=false\nprompt='prompt'").expect("named table entries");
        assert_eq!(config.launch.terminals[0].name, "shell");
        assert_eq!(config.launch.agents[0].tool, "fake");
    }

    #[test]
    fn parses_launch_and_preserves_future_sections_without_exposing_raw_values() {
        let config = parse("[[task]]\nid='test'\ncommand='pnpm test'\n[[service]]\nid='web'\n[[schedule]]\nid='daily'\n[launch]\nservices=['web']\nterminals=[{name='shell'}, {name='logs',command='tail -f x.log'}]\nagents=[{tool='claude',worktree=false,prompt='hello'}]").expect("parse fixture");
        assert_eq!(
            config.launch.terminals[1].command.as_deref(),
            Some("tail -f x.log")
        );
        assert!(!config.launch.agents[0].worktree);
        assert_eq!(config.raw_tasks.len(), 1);
        assert_eq!(config.raw_services.len(), 1);
        assert_eq!(config.raw_schedules.len(), 1);
        let wire = serde_json::to_string(&config).expect("serialize config");
        assert!(!wire.contains("pnpm test"));
        assert!(!wire.contains("raw_tasks"));
        assert!(parse("[launch]\nterminals=[{name='shell',commnad='x'}]").is_err());
        assert!(parse("[launch]\nagents=[{tool='claude',unknown=true}]").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn config_load_rejects_symlinked_config_and_ancestors() {
        let root = tempfile::tempdir().expect("fixture");
        std::fs::create_dir(root.path().join(".devbox")).expect("fixture");
        std::fs::write(root.path().join("outside.toml"), "[agent]\nsetup=[]").expect("fixture");
        std::os::unix::fs::symlink("../outside.toml", root.path().join(".devbox/devbox.toml"))
            .expect("fixture");
        assert!(load(root.path()).is_err());
        std::fs::remove_file(root.path().join(".devbox/devbox.toml")).expect("fixture");
        std::fs::remove_dir(root.path().join(".devbox")).expect("fixture");
        std::os::unix::fs::symlink(root.path(), root.path().join(".devbox")).expect("fixture");
        assert!(load(root.path()).is_err());
    }

    #[test]
    fn strict_agent_config_and_missing_default() {
        let config =
            parse("[agent]\ncopy=['.env']\nsetup=['install']\nteardown=['stop']\ntest='test'")
                .expect("parse fixture");
        assert_eq!(config.agent.copy, [".env"]);
        assert_eq!(config.agent.test.as_deref(), Some("test"));
        assert!(parse("[agent]\nsetpu=[]").is_err());
        assert!(parse("[agnet]\nsetup=[]").is_err());
        assert!(parse("agent=[[],[],[],\"x\"]").is_err());
        for path in [
            "../secret",
            "/etc/passwd",
            "",
            ".",
            "a/../b",
            "C:\\secret",
            "a\\b",
        ] {
            assert!(
                parse(&format!("[agent]\ncopy=[{path:?}]")).is_err(),
                "{path}"
            );
        }
        let root = tempfile::tempdir().expect("fixture");
        assert!(
            load(root.path())
                .expect("missing config")
                .agent
                .copy
                .is_empty()
        );
        std::fs::create_dir(root.path().join(".devbox")).expect("fixture");
        std::fs::write(root.path().join(".devbox/devbox.toml"), "[agent").expect("fixture");
        assert!(load(root.path()).is_err());
    }

    #[test]
    fn copies_nested_files_and_skips_missing_without_overwriting() {
        let root = tempfile::tempdir().expect("fixture");
        let source = root.path().join("source");
        let target = root.path().join("target");
        std::fs::create_dir_all(source.join("nested")).expect("fixture");
        std::fs::create_dir(&target).expect("fixture");
        std::fs::write(source.join("nested/.env"), "secret").expect("fixture");
        copy_files(&source, &target, &["nested/.env".into(), "missing".into()]).expect("copy");
        assert_eq!(
            std::fs::read_to_string(target.join("nested/.env")).expect("read"),
            "secret"
        );
        assert!(copy_files(&source, &target, &["nested/.env".into()]).is_err());
        assert!(copy_files(&source, &target, &["../escape".into()]).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_file_permissions_without_privileged_bits() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().expect("fixture");
        let source = root.path().join("source");
        let target = root.path().join("target");
        std::fs::create_dir(&source).expect("fixture");
        std::fs::create_dir(&target).expect("fixture");
        for mode in [0o600, 0o700, 0o6700] {
            let name = format!("file-{mode}");
            std::fs::write(source.join(&name), "secret").expect("fixture");
            std::fs::set_permissions(source.join(&name), std::fs::Permissions::from_mode(mode))
                .expect("fixture");
            copy_files(&source, &target, std::slice::from_ref(&name)).expect("copy");
            assert_eq!(
                std::fs::metadata(target.join(name))
                    .expect("fixture")
                    .permissions()
                    .mode()
                    & 0o7777,
                mode & 0o777
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_files_and_ancestors_on_both_sides() {
        let root = tempfile::tempdir().expect("fixture");
        let source = root.path().join("source");
        let target = root.path().join("target");
        std::fs::create_dir_all(source.join("dir")).expect("fixture");
        std::fs::create_dir(&target).expect("fixture");
        std::fs::write(source.join("dir/file"), "x").expect("fixture");
        std::os::unix::fs::symlink("dir", source.join("alias")).expect("fixture");
        std::os::unix::fs::symlink("dir/file", source.join("link")).expect("fixture");
        for path in ["alias/file", "link"] {
            assert!(copy_files(&source, &target, &[path.into()]).is_err());
        }
        std::os::unix::fs::symlink(&source, target.join("dir")).expect("fixture");
        assert!(copy_files(&source, &target, &["dir/file".into()]).is_err());
        std::fs::write(source.join("plain"), "source").expect("fixture");
        std::os::unix::fs::symlink(source.join("plain"), target.join("plain")).expect("fixture");
        assert!(copy_files(&source, &target, &["plain".into()]).is_err());
        std::os::unix::fs::symlink(&source, root.path().join("root-alias")).expect("fixture");
        assert!(
            copy_files(
                &root.path().join("root-alias"),
                &target,
                &["dir/file".into()]
            )
            .is_err()
        );
    }
}
