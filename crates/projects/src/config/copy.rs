//! Descriptor-based project reads and worktree copies.
use super::{ConfigError, io};
use std::path::{Component, Path, PathBuf};

pub(super) fn relative(path: &Path) -> Result<(), ConfigError> {
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

pub(super) fn copy_collected_file(
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
