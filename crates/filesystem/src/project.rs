//! Retained native root/Git observations shared by Windows Workspace and its
//! Linux helper. A caller supplies transport admission before every path IO.
use crate::{ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity};
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;
pub type Admission = fn(&Path) -> Result<()>;
pub const MAX_GIT_POINTER_BYTES: u64 = 8192;
struct Object {
    path: PathBuf,
    directory: bool,
    identity: FilesystemIdentity,
    handle: File,
    bytes: Option<Vec<u8>>,
    admit: Admission,
}
impl Object {
    fn open(path: &Path, directory: bool, admit: Admission) -> Result<Self> {
        admit(path)?;
        ensure_no_links(path).map_err(|_| "unsafe_project_object")?;
        let (handle, identity) =
            open_filesystem_object(path, directory).map_err(|_| "project_object_unavailable")?;
        Ok(Self {
            path: path.into(),
            directory,
            identity,
            handle,
            bytes: None,
            admit,
        })
    }
    fn pointer(path: &Path, admit: Admission) -> Result<Self> {
        let mut object = Self::open(path, false, admit)?;
        if object
            .handle
            .metadata()
            .map_err(|_| "project_object_unavailable")?
            .len()
            > MAX_GIT_POINTER_BYTES
        {
            return Err("git_pointer_limit");
        }
        let mut bytes = Vec::new();
        (&object.handle)
            .take(MAX_GIT_POINTER_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "project_object_unavailable")?;
        if bytes.len() as u64 > MAX_GIT_POINTER_BYTES {
            return Err("git_pointer_limit");
        }
        object.bytes = Some(bytes);
        object.revalidate()?;
        Ok(object)
    }
    fn revalidate(&self) -> Result<()> {
        (self.admit)(&self.path)?;
        ensure_no_links(&self.path).map_err(|_| "project_object_changed")?;
        if filesystem_identity(&self.path, self.directory).map_err(|_| "project_object_changed")?
            != self.identity
        {
            return Err("project_object_changed");
        }
        if let Some(expected) = &self.bytes {
            let (handle, identity) =
                open_filesystem_object(&self.path, false).map_err(|_| "project_object_changed")?;
            let mut current = Vec::new();
            handle
                .take(MAX_GIT_POINTER_BYTES + 1)
                .read_to_end(&mut current)
                .map_err(|_| "project_object_changed")?;
            if identity != self.identity || current != *expected {
                return Err("project_object_changed");
            }
        }
        Ok(())
    }
}

/// Keeps root and Git metadata handles alive. Serialized evidence cannot recreate this owner.
pub struct ProjectObservation {
    objects: Vec<Object>,
    absent: Vec<PathBuf>,
    git: Option<(PathBuf, PathBuf)>,
    repository_identity: Option<FilesystemIdentity>,
    admit: Admission,
}
impl ProjectObservation {
    pub fn capture(root: &Path, admit: Admission) -> Result<Self> {
        if !root.is_absolute() {
            return Err("invalid_root");
        }
        capture(root, admit)
    }
    pub fn root(&self) -> &Path {
        &self.objects[0].path
    }
    pub fn root_handle(&self) -> &File {
        &self.objects[0].handle
    }
    pub fn root_identity(&self) -> FilesystemIdentity {
        self.objects[0].identity
    }
    pub fn repository_identity(&self) -> Option<FilesystemIdentity> {
        self.repository_identity
    }
    pub fn repository_handle(&self) -> Option<&File> {
        self.git.as_ref().map(|_| {
            &self
                .objects
                .last()
                .expect("observed common directory")
                .handle
        })
    }
    pub fn git_directories(&self) -> Option<(&Path, &Path)> {
        self.git
            .as_ref()
            .map(|(git, common)| (git.as_path(), common.as_path()))
    }
    pub fn revalidate(&self) -> Result<()> {
        for object in &self.objects {
            object.revalidate()?;
        }
        for path in &self.absent {
            if !missing(path, self.admit)? {
                return Err("project_object_changed");
            }
        }
        Ok(())
    }
}
fn missing(path: &Path, admit: Admission) -> Result<bool> {
    admit(path)?;
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err("project_object_unavailable"),
    }
}
fn pointer_scope(root: &str, candidate: &str) -> Result<()> {
    use crate::{parse_safe_project_path, ProjectPathKind};
    let root = parse_safe_project_path(root).ok_or("unsafe_git_pointer")?;
    let candidate = parse_safe_project_path(candidate).ok_or("unsafe_git_pointer")?;
    match (root.kind(), candidate.kind()) {
        (ProjectPathKind::WindowsDrive, ProjectPathKind::WindowsDrive)
        | (ProjectPathKind::Posix, ProjectPathKind::Posix) => Ok(()),
        (ProjectPathKind::WindowsUnc, ProjectPathKind::WindowsUnc) => {
            let authority = |value: &str| {
                value
                    .replace('\\', "/")
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .take(2)
                    .map(str::to_ascii_lowercase)
                    .collect::<Vec<_>>()
            };
            if authority(root.identity()) == authority(candidate.identity()) {
                Ok(())
            } else {
                Err("unsafe_git_pointer")
            }
        }
        _ => Err("unsafe_git_pointer"),
    }
}
fn pointer_path(root: &Path, object: &Object, prefix: &str, admit: Admission) -> Result<PathBuf> {
    let text = std::str::from_utf8(object.bytes.as_deref().ok_or("invalid_git_pointer")?)
        .map_err(|_| "invalid_git_pointer")?;
    let value = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text)
        .strip_prefix(prefix)
        .ok_or("invalid_git_pointer")?;
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("invalid_git_pointer");
    }
    let path = Path::new(value);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        object
            .path
            .parent()
            .ok_or("invalid_git_pointer")?
            .join(path)
    };
    // Resolve '..' lexically before IO, but inspect each traversed prefix so a
    // symlink followed by '..' cannot change the meaning of the pointer.
    let mut normalized = PathBuf::new();
    let mut traversed = Vec::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                traversed.push(normalized.clone());
                if !normalized.pop() {
                    return Err("invalid_git_pointer");
                }
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    traversed.push(normalized.clone());
    // Validate every transport before any IO: an untrusted .git pointer may
    // not contact a new UNC host or start WSL via its filesystem redirector.
    for path in &traversed {
        pointer_scope(
            root.to_str().ok_or("unsafe_git_pointer")?,
            path.to_str().ok_or("unsafe_git_pointer")?,
        )?;
    }
    for path in &traversed {
        admit(path)?;
    }
    for path in &traversed {
        ensure_no_links(path).map_err(|_| "unsafe_git_pointer")?;
    }
    Ok(normalized)
}

fn capture(root: &Path, admit: Admission) -> Result<ProjectObservation> {
    let root_object = Object::open(root, true, admit)?;
    let mut objects = vec![root_object];
    let mut absent = vec![];
    let mut git_directories = None;
    let dot_git = root.join(".git");
    let repository_object = if missing(&dot_git, admit)? {
        absent.push(dot_git);
        None
    } else {
        let metadata = fs::symlink_metadata(&dot_git).map_err(|_| "project_object_unavailable")?;
        let gitdir = if metadata.is_dir() {
            dot_git.clone()
        } else {
            let pointer = Object::pointer(&dot_git, admit)?;
            let gitdir = pointer_path(root, &pointer, "gitdir: ", admit)?;
            objects.push(pointer);
            gitdir
        };
        let git = Object::open(&gitdir, true, admit)?;
        objects.push(git);
        let commondir = gitdir.join("commondir");
        let common = if missing(&commondir, admit)? {
            absent.push(commondir);
            gitdir.clone()
        } else {
            let pointer = Object::pointer(&commondir, admit)?;
            let common = pointer_path(root, &pointer, "", admit)?;
            objects.push(pointer);
            let backlink = Object::pointer(&gitdir.join("gitdir"), admit)?;
            let back = pointer_path(root, &backlink, "", admit)?;
            if filesystem_identity(&back, false).map_err(|_| "invalid_git_backlink")?
                != filesystem_identity(&dot_git, false).map_err(|_| "invalid_git_backlink")?
            {
                return Err("invalid_git_backlink");
            }
            objects.push(backlink);
            common
        };
        // A failed/malformed Git observation never silently becomes a plain
        // folder. Inspect standard metadata without invoking git or hooks.
        objects.push(Object::open(&gitdir.join("HEAD"), false, admit)?);
        objects.push(Object::open(&common.join("objects"), true, admit)?);
        git_directories = Some((gitdir, common.clone()));
        let common = Object::open(&common, true, admit)?;
        let stamp = common.identity;
        objects.push(common);
        Some(stamp)
    };
    let lease = ProjectObservation {
        objects,
        absent,
        git: git_directories,
        repository_identity: repository_object,
        admit,
    };
    lease.revalidate()?;
    Ok(lease)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn git_pointers_cannot_select_a_distro_device_or_new_network_authority() {
        for candidate in [
            r"\\wsl.localhost\Stopped\home\repo\.git",
            r"\\server\share\repo",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\repo",
            r"C:\repo:stream",
        ] {
            assert!(pointer_scope(r"C:\repo", candidate).is_err(), "{candidate}");
        }
        assert!(pointer_scope(r"C:\repo", r"D:\main\.git").is_ok());
        assert!(pointer_scope(r"\\server\share\linked", r"\\SERVER\share\main\.git").is_ok());
        assert!(pointer_scope(r"\\server\share\linked", r"\\other\share\main\.git").is_err());
        assert!(pointer_scope(r"\\server\share\linked", r"\\server\other\main\.git").is_err());
    }
}
