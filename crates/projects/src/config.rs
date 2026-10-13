//! Minimal agent settings; launch and task interpretation belong to later stages.
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    pub copy: Vec<String>,
    pub setup: Vec<String>,
    pub teardown: Vec<String>,
    pub test: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    pub agent: AgentConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid project configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("[agent] must be a TOML table")]
    AgentNotTable,
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
    let config: ProjectConfig = document.try_into()?;
    for path in &config.agent.copy {
        relative(Path::new(path))?;
    }
    Ok(config)
}

pub fn load(project: &Path) -> Result<ProjectConfig, ConfigError> {
    let path = project.join(".devbox/devbox.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ProjectConfig::default()),
        Err(error) => Err(io(&path, error)),
    }
}

// Inspect every existing ancestor, including the roots themselves. A missing
// source is optional, but a dangling symlink is never treated as missing.
fn check_ancestors(path: &Path) -> Result<(), ConfigError> {
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
    let mut files = Vec::new();
    for path in paths {
        relative(Path::new(path))?;
        collect(&project.join(path), &worktree.join(path), &mut files)?;
    }
    for (source, destination) in files {
        check_ancestors(&source)?;
        check_ancestors(&destination)?;
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
        }
        let mut input = std::fs::File::open(&source).map_err(|error| io(&source, error))?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&destination).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ConfigError::DestinationExists {
                    path: destination.clone(),
                }
            } else {
                io(&destination, error)
            }
        })?;
        std::io::copy(&mut input, &mut output).map_err(|error| io(&destination, error))?;
        let permissions = input
            .metadata()
            .map_err(|error| io(&source, error))?
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
            .map_err(|error| io(&destination, error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            copy_files(&source, &target, &[name.clone()]).expect("copy");
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
