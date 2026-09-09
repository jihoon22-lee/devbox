//! Bounded, no-follow project-definition snapshots. Only a native ProjectLease
//! can select the root. The retained bytes/objects are never renderer authority.
use crate::files::{Admission, RootLease};
use crate::manifest::relative_source;
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_FILE: u64 = 2 * 1024 * 1024;
const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_OBJECTS: usize = 1024;
const MAX_FILES: usize = 130;

struct Object {
    identity: FilesystemIdentity,
    directory: bool,
    _handle: File,
}
struct Contents {
    identity: FilesystemIdentity,
    bytes: Vec<u8>,
}
pub struct ProjectFiles<L: RootLease> {
    lease: L,
    admission: Admission,
    objects: BTreeMap<PathBuf, Object>,
    absent: BTreeSet<PathBuf>,
    files: BTreeMap<String, Contents>,
    total_bytes: usize,
}
impl<L: RootLease> ProjectFiles<L> {
    pub fn new(lease: L) -> Result<Self> {
        Self::new_with_admission(lease, crate::windows_path::admit, &|| Ok(()))
    }
    pub fn new_with_admission(
        lease: L,
        admission: Admission,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        guard()?;
        lease.revalidate()?;
        let mut result = Self {
            lease,
            admission,
            objects: BTreeMap::new(),
            absent: BTreeSet::new(),
            files: BTreeMap::new(),
            total_bytes: 0,
        };
        let root = result.lease.root().to_path_buf();
        result.pin(&root, true, guard)?;
        if result.objects[&root].identity != result.lease.native_root_identity() {
            return Err("project_definition_changed");
        }
        Ok(result)
    }
    pub fn lease(&self) -> &L {
        &self.lease
    }
    /// A writer may replace one reviewed absence with a directory it created
    /// exclusively. The retained native handle must still name that exact
    /// object. This does not adopt directories that appeared after review.
    pub fn retain_created_directory(
        &mut self,
        relative: &str,
        created: &File,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        if !relative_source(relative) || relative.split('/').count() > 64 {
            return Err("unsafe_project_definition");
        }
        let path = self.lease.root().join(relative);
        guard()?;
        if !self.absent.contains(&path)
            || !created.metadata().is_ok_and(|metadata| metadata.is_dir())
        {
            return Err("project_definition_changed");
        }
        self.absent.remove(&path);
        let result = (|| {
            self.validate_objects(guard)?;
            self.pin(&path, true, guard)?;
            if devbox_filesystem::opened_filesystem_identity(created, true)
                .map_err(|_| "project_definition_changed")?
                != self.objects[&path].identity
            {
                return Err("project_definition_changed");
            }
            self.validate_objects(guard)
        })();
        if result.is_err() {
            self.objects.remove(&path);
            self.absent.insert(path);
        }
        result
    }
    fn pin(&mut self, path: &Path, directory: bool, guard: &dyn Fn() -> Result<()>) -> Result<()> {
        guard()?;
        (self.admission)(path)?;
        if let Some(object) = self.objects.get(path) {
            if object.directory != directory
                || filesystem_identity(path, directory).map_err(|_| "project_definition_changed")?
                    != object.identity
            {
                return Err("project_definition_changed");
            }
            return Ok(());
        }
        if self.objects.len() >= MAX_OBJECTS {
            return Err("project_definition_limit");
        }
        ensure_no_links(path).map_err(|_| "unsafe_project_definition")?;
        let (handle, identity) = open_filesystem_object(path, directory)
            .map_err(|_| "project_definition_unavailable")?;
        self.objects.insert(
            path.into(),
            Object {
                identity,
                directory,
                _handle: handle,
            },
        );
        Ok(())
    }
    /// Missing optional files retain their first absent component. A folder
    /// created after preview is a conflict, even when its eventual file is absent.
    pub fn read(&mut self, relative: &str, optional: bool) -> Result<Option<Vec<u8>>> {
        self.read_guarded(relative, optional, &|| Ok(()))
    }
    pub fn read_guarded(
        &mut self,
        relative: &str,
        optional: bool,
        guard: &dyn Fn() -> Result<()>,
    ) -> Result<Option<Vec<u8>>> {
        guard()?;
        if !relative_source(relative) || relative.split('/').count() > 64 {
            return Err("unsafe_project_definition");
        }
        if self.files.len() + self.absent.len() >= MAX_FILES {
            return Err("project_definition_limit");
        }
        self.lease.revalidate()?;
        if let Some(contents) = self.files.get(relative) {
            return Ok(Some(contents.bytes.clone()));
        }
        let mut path = self.lease.root().to_path_buf();
        let parts: Vec<_> = relative.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            path.push(part);
            guard()?;
            (self.admission)(&path)?;
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && optional => {
                    self.absent.insert(path);
                    self.lease.revalidate()?;
                    return Ok(None);
                }
                Err(_) => return Err("project_definition_unavailable"),
                Ok(_) => self.pin(&path, index + 1 < parts.len(), guard)?,
            }
        }
        guard()?;
        (self.admission)(&path)?;
        let (handle, identity) =
            open_filesystem_object(&path, false).map_err(|_| "project_definition_unavailable")?;
        let bytes = read_contents(handle, guard)?;
        if bytes.len() as u64 > MAX_FILE || self.total_bytes + bytes.len() > MAX_BYTES {
            return Err("project_definition_limit");
        }
        if identity != self.objects[&path].identity {
            return Err("project_definition_changed");
        }
        self.total_bytes += bytes.len();
        self.files.insert(
            relative.into(),
            Contents {
                identity,
                bytes: bytes.clone(),
            },
        );
        self.validate_objects(guard)?;
        Ok(Some(bytes))
    }
    fn validate_objects(&self, guard: &dyn Fn() -> Result<()>) -> Result<()> {
        guard()?;
        self.lease.revalidate()?;
        for (path, object) in &self.objects {
            guard()?;
            (self.admission)(path)?;
            ensure_no_links(path).map_err(|_| "unsafe_project_definition")?;
            if filesystem_identity(path, object.directory)
                .map_err(|_| "project_definition_changed")?
                != object.identity
            {
                return Err("project_definition_changed");
            }
        }
        for path in &self.absent {
            guard()?;
            (self.admission)(path)?;
            if !matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("project_definition_changed");
            }
        }
        Ok(())
    }
    pub fn revalidate(&self) -> Result<()> {
        self.revalidate_guarded(&|| Ok(()))
    }
    pub fn revalidate_guarded(&self, guard: &dyn Fn() -> Result<()>) -> Result<()> {
        self.validate_objects(guard)?;
        for (relative, expected) in &self.files {
            let path = self.lease.root().join(relative);
            guard()?;
            (self.admission)(&path)?;
            let (handle, identity) =
                open_filesystem_object(&path, false).map_err(|_| "project_definition_changed")?;
            let bytes = read_contents(handle, guard)?;
            if identity != expected.identity || bytes != expected.bytes {
                return Err("project_definition_changed");
            }
            if filesystem_identity(&path, false).map_err(|_| "project_definition_changed")?
                != identity
            {
                return Err("project_definition_changed");
            }
        }
        self.validate_objects(guard)
    }
}

// Small regular-file reads check cancellation between bounded chunks. Admission
// rejects devices/FIFOs and foreign mounts before any contents are opened.
fn read_contents(mut file: File, guard: &dyn Fn() -> Result<()>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 16384];
    loop {
        guard()?;
        let count = file
            .read(&mut chunk)
            .map_err(|_| "project_definition_unavailable")?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > MAX_FILE as usize {
            return Err("project_definition_limit");
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    guard()?;
    Ok(bytes)
}
