//! Pin the owned destination and create descendants without following symlinks.
use std::{fs::File, io, path::Path};

pub(crate) struct CopyTarget {
    path: std::path::PathBuf,
    #[cfg(unix)]
    directory: File,
}

#[cfg(unix)]
const DIRECTORY_FLAGS: nix::fcntl::OFlag = nix::fcntl::OFlag::O_RDONLY
    .union(nix::fcntl::OFlag::O_CLOEXEC)
    .union(nix::fcntl::OFlag::O_NOFOLLOW)
    .union(nix::fcntl::OFlag::O_DIRECTORY);

#[cfg(unix)]
fn os_error(error: nix::errno::Errno) -> io::Error {
    io::Error::from_raw_os_error(error as i32)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "unsafe copy destination")
}

impl CopyTarget {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use nix::{
                fcntl::{open, openat},
                sys::stat::Mode,
            };
            use std::path::Component;
            let mut directory = open(
                Path::new(if path.is_absolute() { "/" } else { "." }),
                DIRECTORY_FLAGS,
                Mode::empty(),
            )
            .map_err(os_error)?;
            for component in path.components() {
                match component {
                    Component::RootDir => (),
                    Component::Normal(name) => {
                        directory =
                            openat(&directory, Path::new(name), DIRECTORY_FLAGS, Mode::empty())
                                .map_err(os_error)?;
                    }
                    _ => return Err(invalid()),
                }
            }
            Ok(Self {
                path: path.into(),
                directory: File::from(directory),
            })
        }
        #[cfg(not(unix))]
        {
            super::config::check_ancestors(path).map_err(io::Error::other)?;
            if !path.is_dir() {
                return Err(invalid());
            }
            Ok(Self { path: path.into() })
        }
    }

    pub(crate) fn create(&self, destination: &Path) -> io::Result<File> {
        use std::path::Component;
        let relative = destination
            .strip_prefix(&self.path)
            .map_err(|_| invalid())?;
        let components: Vec<_> = relative.components().collect();
        if components.is_empty()
            || components
                .iter()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use nix::{
                fcntl::{OFlag, openat},
                sys::stat::{Mode, mkdirat},
            };
            let mut parent = self.directory.try_clone()?;
            for (index, component) in components.iter().enumerate() {
                let Component::Normal(name) = component else {
                    return Err(invalid());
                };
                let name = Path::new(name);
                if index + 1 == components.len() {
                    return openat(
                        &parent,
                        name,
                        OFlag::O_WRONLY
                            | OFlag::O_CREAT
                            | OFlag::O_EXCL
                            | OFlag::O_NOFOLLOW
                            | OFlag::O_CLOEXEC,
                        Mode::from_bits_truncate(0o600),
                    )
                    .map(File::from)
                    .map_err(os_error);
                }
                let next = match openat(&parent, name, DIRECTORY_FLAGS, Mode::empty()) {
                    Ok(next) => next,
                    Err(nix::errno::Errno::ENOENT) => {
                        match mkdirat(&parent, name, Mode::from_bits_truncate(0o755)) {
                            Ok(()) | Err(nix::errno::Errno::EEXIST) => (),
                            Err(error) => return Err(os_error(error)),
                        }
                        openat(&parent, name, DIRECTORY_FLAGS, Mode::empty()).map_err(os_error)?
                    }
                    Err(error) => return Err(os_error(error)),
                };
                parent = File::from(next);
            }
            Err(invalid())
        }
        #[cfg(not(unix))]
        {
            super::config::check_ancestors(destination).map_err(io::Error::other)?;
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            super::config::check_ancestors(destination).map_err(io::Error::other)?;
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn pinned_root_never_follows_replacement_or_creates_outside_directories() {
        let root = tempfile::tempdir().expect("owned fixture");
        let target = root.path().join("target");
        let retained = root.path().join("retained");
        let outside = root.path().join("outside");
        std::fs::create_dir(&target).expect("fixture");
        std::fs::create_dir(&outside).expect("fixture");
        let pinned = CopyTarget::open(&target).expect("pin root");
        std::fs::rename(&target, &retained).expect("fixture");
        std::os::unix::fs::symlink(&outside, &target).expect("fixture");
        pinned
            .create(&target.join("nested/file"))
            .expect("owned inode remains usable");
        assert!(retained.join("nested/file").is_file());
        assert!(!outside.join("nested").exists());
        assert!(CopyTarget::open(&target).is_err());
        std::os::unix::fs::symlink(&outside, retained.join("alias")).expect("fixture");
        assert!(pinned.create(&target.join("alias/new/file")).is_err());
        assert!(!outside.join("new").exists());
    }
    #[test]
    fn final_existing_file_and_symlink_are_preserved() {
        let root = tempfile::tempdir().expect("owned fixture");
        let target = CopyTarget::open(root.path()).expect("pin root");
        let precious = root.path().join("precious");
        std::fs::write(&precious, "preserve").expect("fixture");
        std::os::unix::fs::symlink(&precious, root.path().join("alias")).expect("fixture");
        for name in ["precious", "alias"] {
            assert_eq!(
                target
                    .create(&root.path().join(name))
                    .expect_err("no overwrite")
                    .kind(),
                io::ErrorKind::AlreadyExists
            );
        }
        assert_eq!(
            std::fs::read_to_string(precious).expect("fixture"),
            "preserve"
        );
        assert!(target.create(&root.path().join("../escape")).is_err());
    }
}
