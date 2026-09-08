//! Bounded, no-follow project-definition snapshots. Only a native ProjectLease
//! can select the root. The retained bytes/objects are never renderer authority.
use super::project_probe::ProjectLease;
use crate::core::manifest::relative_source;
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
pub struct ProjectFiles {
    lease: ProjectLease,
    objects: BTreeMap<PathBuf, Object>,
    absent: BTreeSet<PathBuf>,
    files: BTreeMap<String, Contents>,
    total_bytes: usize,
}
impl ProjectFiles {
    pub fn new(lease: ProjectLease) -> Result<Self> {
        lease.revalidate()?;
        let mut result = Self {
            lease,
            objects: BTreeMap::new(),
            absent: BTreeSet::new(),
            files: BTreeMap::new(),
            total_bytes: 0,
        };
        let root = PathBuf::from(&result.lease.binding().root);
        result.pin(&root, true)?;
        if result.objects[&root].identity != result.lease.native_root_identity() {
            return Err("project_definition_changed");
        }
        Ok(result)
    }
    pub fn lease(&self) -> &ProjectLease {
        &self.lease
    }
    fn pin(&mut self, path: &Path, directory: bool) -> Result<()> {
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
        let mut path = PathBuf::from(&self.lease.binding().root);
        let parts: Vec<_> = relative.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            path.push(part);
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && optional => {
                    self.absent.insert(path);
                    self.lease.revalidate()?;
                    return Ok(None);
                }
                Err(_) => return Err("project_definition_unavailable"),
                Ok(_) => self.pin(&path, index + 1 < parts.len())?,
            }
        }
        let (handle, identity) =
            open_filesystem_object(&path, false).map_err(|_| "project_definition_unavailable")?;
        let mut bytes = Vec::new();
        handle
            .take(MAX_FILE + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "project_definition_unavailable")?;
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
        self.validate_objects()?;
        Ok(Some(bytes))
    }
    fn validate_objects(&self) -> Result<()> {
        self.lease.revalidate()?;
        for (path, object) in &self.objects {
            ensure_no_links(path).map_err(|_| "unsafe_project_definition")?;
            if filesystem_identity(path, object.directory)
                .map_err(|_| "project_definition_changed")?
                != object.identity
            {
                return Err("project_definition_changed");
            }
        }
        for path in &self.absent {
            if !matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("project_definition_changed");
            }
        }
        Ok(())
    }
    pub fn revalidate(&self) -> Result<()> {
        self.validate_objects()?;
        for (relative, expected) in &self.files {
            let path = Path::new(&self.lease.binding().root).join(relative);
            let (handle, identity) =
                open_filesystem_object(&path, false).map_err(|_| "project_definition_changed")?;
            let mut bytes = Vec::new();
            handle
                .take(MAX_FILE + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "project_definition_changed")?;
            if identity != expected.identity || bytes != expected.bytes {
                return Err("project_definition_changed");
            }
            if filesystem_identity(&path, false).map_err(|_| "project_definition_changed")?
                != identity
            {
                return Err("project_definition_changed");
            }
        }
        self.validate_objects()
    }
}

#[cfg(test)]
mod tests {
    use super::super::project_probe::probe_fixture;
    use super::*;
    #[test]
    fn same_object_changed_bytes_invalidate_a_reviewed_source() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("package.json"), b"first").unwrap();
        let mut files = ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap();
        assert_eq!(
            files.read("package.json", false).unwrap().unwrap(),
            b"first"
        );
        fs::write(root.path().join("package.json"), b"other").unwrap();
        assert_eq!(
            files.revalidate().unwrap_err(),
            "project_definition_changed"
        );
    }
    #[test]
    fn creating_an_absent_manifest_parent_requires_a_new_review() {
        let root = tempfile::tempdir().unwrap();
        let mut files = ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap();
        assert!(files.read(".devbox/project.json", true).unwrap().is_none());
        fs::create_dir(root.path().join(".devbox")).unwrap();
        assert_eq!(
            files.revalidate().unwrap_err(),
            "project_definition_changed"
        );
    }
    #[test]
    fn rejects_escape_and_bounds_source_contents() {
        let root = tempfile::tempdir().unwrap();
        let mut files = ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap();
        assert!(files.read("../outside", false).is_err());
        assert!(files.read("C:/outside", false).is_err());
        let file = File::create(root.path().join("large.json")).unwrap();
        file.set_len(MAX_FILE + 1).unwrap();
        assert_eq!(
            files.read("large.json", false).unwrap_err(),
            "project_definition_limit"
        );
    }
    #[cfg(unix)]
    #[test]
    fn linked_parent_is_rejected_before_reading_its_target() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("package.json"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
        let mut files = ProjectFiles::new(probe_fixture(root.path()).unwrap()).unwrap();
        assert!(files.read("linked/package.json", false).is_err());
    }
}
