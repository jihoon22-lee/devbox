//! Pin every ancestor and unlink relative to its descriptor, never a mutable path.
use super::UninstallError;
use nix::{
    fcntl::{AtFlags, OFlag, openat},
    sys::stat::{Mode, fstatat},
    unistd::{UnlinkatFlags, unlinkat},
};
use std::{
    ffi::OsString,
    fs::{File, Metadata},
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

struct Binding {
    parent: Arc<File>,
    name: OsString,
    child: Arc<File>,
}
pub(super) struct Directory {
    file: Arc<File>,
    bindings: Vec<Binding>,
}
fn io(error: nix::errno::Errno) -> UninstallError {
    std::io::Error::from_raw_os_error(error as i32).into()
}
impl Directory {
    pub(super) fn open(path: &Path) -> Result<Self, UninstallError> {
        if !path.is_absolute() {
            return Err(UninstallError::ForeignFile);
        }
        let mut file = Arc::new(File::open("/")?);
        let mut bindings = Vec::new();
        for component in path.components() {
            let name = match component {
                Component::RootDir => continue,
                Component::Normal(name) => name,
                _ => return Err(UninstallError::ForeignFile),
            };
            let fd = openat(
                file.as_ref(),
                name,
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(io)?;
            let child = Arc::new(File::from(fd));
            bindings.push(Binding {
                parent: file,
                name: name.to_owned(),
                child: child.clone(),
            });
            file = child;
        }
        let result = Self { file, bindings };
        result.validate()?;
        Ok(result)
    }
    pub(super) fn validate(&self) -> Result<(), UninstallError> {
        for binding in &self.bindings {
            let stat = fstatat(
                binding.parent.as_ref(),
                binding.name.as_os_str(),
                AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map_err(io)?;
            let meta = binding.child.metadata()?;
            if stat.st_dev != meta.dev() || stat.st_ino != meta.ino() {
                return Err(UninstallError::ForeignFile);
            }
        }
        Ok(())
    }
    pub(super) fn remove(self) -> Result<(), UninstallError> {
        self.validate()?;
        let binding = self.bindings.last().ok_or(UninstallError::ForeignFile)?;
        unlinkat(
            binding.parent.as_ref(),
            binding.name.as_os_str(),
            UnlinkatFlags::RemoveDir,
        )
        .map_err(io)?;
        Ok(())
    }
}
pub(super) struct OwnedFile {
    parent: Directory,
    name: OsString,
    dev: u64,
    ino: u64,
}
impl OwnedFile {
    pub(super) fn new(path: PathBuf, metadata: &Metadata) -> Result<Self, UninstallError> {
        let parent = Directory::open(path.parent().ok_or(UninstallError::ForeignFile)?)?;
        let name = path
            .file_name()
            .ok_or(UninstallError::ForeignFile)?
            .to_owned();
        let result = Self {
            parent,
            name,
            dev: metadata.dev(),
            ino: metadata.ino(),
        };
        result.validate()?;
        Ok(result)
    }
    pub(super) fn validate(&self) -> Result<(), UninstallError> {
        self.parent.validate()?;
        let stat = fstatat(
            self.parent.file.as_ref(),
            self.name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(io)?;
        if stat.st_dev != self.dev || stat.st_ino != self.ino {
            return Err(UninstallError::ForeignFile);
        }
        Ok(())
    }
    pub(super) fn remove(self) -> Result<(), UninstallError> {
        self.validate()?;
        unlinkat(
            self.parent.file.as_ref(),
            self.name.as_os_str(),
            UnlinkatFlags::NoRemoveDir,
        )
        .map_err(io)?;
        Ok(())
    }
}
