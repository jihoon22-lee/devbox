//! Identity-pinned private generation metadata shared by native feature owners.
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use std::{
    fs::{self, File},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_METADATA: u64 = 8 * 1024 * 1024;
pub(crate) struct MetadataRoot {
    path: PathBuf,
    identity: FilesystemIdentity,
    _handle: File,
}
impl MetadataRoot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn open(path: &Path) -> Result<Self> {
        ensure_no_links(path).map_err(|_| "invalid_files_store")?;
        let (handle, identity) =
            open_filesystem_object(path, true).map_err(|_| "invalid_files_store")?;
        Ok(Self {
            path: path.into(),
            identity,
            _handle: handle,
        })
    }
    pub(crate) fn child(&self, name: &str) -> Result<Self> {
        self.revalidate()?;
        let path = self.path.join(name);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return Err("files_store_unavailable"),
        }
        let child = Self::open(&path)?;
        self.revalidate()?;
        Ok(child)
    }
    pub(crate) fn revalidate(&self) -> Result<()> {
        ensure_no_links(&self.path).map_err(|_| "files_store_changed")?;
        if filesystem_identity(&self.path, true).map_err(|_| "files_store_changed")?
            != self.identity
        {
            return Err("files_store_changed");
        }
        Ok(())
    }
    pub(crate) fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.revalidate()?;
        let path = self.path.join(name);
        let (mut handle, identity) = match open_filesystem_object(&path, false) {
            Ok(value) => value,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("invalid_files_store"),
        };
        let mut bytes = Vec::new();
        (&mut handle)
            .take(MAX_METADATA + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "invalid_files_store")?;
        if bytes.len() as u64 > MAX_METADATA
            || filesystem_identity(&path, false).map_err(|_| "files_store_changed")? != identity
        {
            return Err("invalid_files_store");
        }
        self.revalidate()?;
        Ok(Some(bytes))
    }
    pub(crate) fn write(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if bytes.len() as u64 > MAX_METADATA {
            return Err("files_store_limit");
        }
        self.revalidate()?;
        let path = self.path.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                ensure_no_links(&path).map_err(|_| "invalid_files_store")?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err("invalid_files_store"),
        }
        devbox_filesystem::atomic_write(&path, bytes).map_err(|_| "files_store_unavailable")?;
        self.revalidate()
    }
}
