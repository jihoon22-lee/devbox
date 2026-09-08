//! Read-only filesystem observation. No Git executable, repository config,
//! hooks, package manager or distro command is run during project discovery.
//! The command owner must run these blocking probes in its bounded IO worker.
use crate::core::registry::{Binding, ObjectStamp};
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use product_contract::ExecutionTarget;
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_POINTER: u64 = 8192;

struct Object {
    path: PathBuf,
    directory: bool,
    identity: FilesystemIdentity,
    handle: File,
    bytes: Option<Vec<u8>>,
}
impl Object {
    fn open(path: &Path, directory: bool) -> Result<Self> {
        ensure_no_links(path).map_err(|_| "unsafe_project_object")?;
        let (handle, identity) =
            open_filesystem_object(path, directory).map_err(|_| "project_object_unavailable")?;
        Ok(Self {
            path: path.into(),
            directory,
            identity,
            handle,
            bytes: None,
        })
    }
    fn pointer(path: &Path) -> Result<Self> {
        let mut object = Self::open(path, false)?;
        if object
            .handle
            .metadata()
            .map_err(|_| "project_object_unavailable")?
            .len()
            > MAX_POINTER
        {
            return Err("git_pointer_limit");
        }
        let mut bytes = Vec::new();
        (&object.handle)
            .take(MAX_POINTER + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "project_object_unavailable")?;
        if bytes.len() as u64 > MAX_POINTER {
            return Err("git_pointer_limit");
        }
        object.bytes = Some(bytes);
        object.revalidate()?;
        Ok(object)
    }
    fn revalidate(&self) -> Result<()> {
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
                .take(MAX_POINTER + 1)
                .read_to_end(&mut current)
                .map_err(|_| "project_object_changed")?;
            if identity != self.identity || current != *expected {
                return Err("project_object_changed");
            }
        }
        Ok(())
    }
}

/// Keeps all observed objects alive, including .git/commondir/backlink files.
/// Only the native owner retains this value; a serialized Binding cannot
/// reconstruct the lease or satisfy operation admission.
pub struct ProjectLease {
    binding: Binding,
    objects: Vec<Object>,
    absent: Vec<PathBuf>,
}
impl ProjectLease {
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn native_root_identity(&self) -> FilesystemIdentity {
        self.objects[0].identity
    }
    pub fn revalidate(&self) -> Result<()> {
        for object in &self.objects {
            object.revalidate()?;
        }
        for path in &self.absent {
            if !missing(path)? {
                return Err("project_object_changed");
            }
        }
        Ok(())
    }
}

/// Windows-local/ordinary UNC discovery only. WSL requires a separate native
/// distro binding and running-state admission before even opening its UNC root.
pub fn probe_windows(root: &str) -> Result<ProjectLease> {
    #[cfg(windows)]
    {
        let parsed = devbox_filesystem::parse_safe_project_path(root).ok_or("invalid_root")?;
        if !matches!(
            parsed.kind(),
            devbox_filesystem::ProjectPathKind::WindowsDrive
                | devbox_filesystem::ProjectPathKind::WindowsUnc
        ) || devbox_wsl::path::parse_wsl_unc_path(root)
            .map_err(|_| "invalid_root")?
            .is_some()
        {
            return Err("invalid_target");
        }
        let root = PathBuf::from(root);
        ensure_no_links(&root).map_err(|_| "unsafe_project_object")?;
        let canonical = fs::canonicalize(&root).map_err(|_| "project_object_unavailable")?;
        let spelling = canonical.to_str().ok_or("invalid_root")?;
        let spelling = if let Some(unc) = spelling.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else {
            spelling
                .strip_prefix(r"\\?\")
                .unwrap_or(spelling)
                .to_owned()
        };
        probe(
            Path::new(&spelling),
            ExecutionTarget::Windows,
            spelling.clone(),
        )
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Err("windows_required")
    }
}

fn missing(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(_) => Err("project_object_unavailable"),
    }
}
fn pointer_scope(root: &str, candidate: &str) -> Result<()> {
    use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
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
fn pointer_path(root: &Path, object: &Object, prefix: &str) -> Result<PathBuf> {
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
        ensure_no_links(path).map_err(|_| "unsafe_git_pointer")?;
    }
    Ok(normalized)
}

#[cfg_attr(not(windows), allow(dead_code))]
fn probe(root: &Path, target: ExecutionTarget, spelling: String) -> Result<ProjectLease> {
    let root_object = Object::open(root, true)?;
    let root_stamp = stamp(&root_object.handle)?;
    let mut objects = vec![root_object];
    let mut absent = vec![];
    let dot_git = root.join(".git");
    let repository_object = if missing(&dot_git)? {
        absent.push(dot_git);
        None
    } else {
        let metadata = fs::symlink_metadata(&dot_git).map_err(|_| "project_object_unavailable")?;
        let gitdir = if metadata.is_dir() {
            dot_git.clone()
        } else {
            let pointer = Object::pointer(&dot_git)?;
            let gitdir = pointer_path(root, &pointer, "gitdir: ")?;
            objects.push(pointer);
            gitdir
        };
        let git = Object::open(&gitdir, true)?;
        objects.push(git);
        let commondir = gitdir.join("commondir");
        let common = if missing(&commondir)? {
            absent.push(commondir);
            gitdir.clone()
        } else {
            let pointer = Object::pointer(&commondir)?;
            let common = pointer_path(root, &pointer, "")?;
            objects.push(pointer);
            let backlink = Object::pointer(&gitdir.join("gitdir"))?;
            let back = pointer_path(root, &backlink, "")?;
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
        objects.push(Object::open(&gitdir.join("HEAD"), false)?);
        objects.push(Object::open(&common.join("objects"), true)?);
        let common = Object::open(&common, true)?;
        let stamp = stamp(&common.handle)?;
        objects.push(common);
        Some(stamp)
    };
    let binding = Binding {
        target,
        root: spelling,
        root_object: root_stamp,
        repository_object,
    };
    binding.validate()?;
    let lease = ProjectLease {
        binding,
        objects,
        absent,
    };
    lease.revalidate()?;
    Ok(lease)
}

fn stamp(handle: &File) -> Result<ObjectStamp> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = handle
            .metadata()
            .map_err(|_| "project_object_unavailable")?;
        Ok(ObjectStamp {
            scope: format!("{:x}", metadata.dev()),
            object: format!("{:x}", metadata.ino()),
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(handle.as_raw_handle()), &mut info) }
            .map_err(|_| "project_object_unavailable")?;
        Ok(ObjectStamp {
            scope: format!("{:x}", info.dwVolumeSerialNumber),
            object: format!(
                "{:x}",
                (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)
            ),
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = handle;
        Err("unsupported_platform")
    }
}

#[cfg(test)]
pub(crate) fn probe_fixture(root: &Path) -> Result<ProjectLease> {
    #[cfg(unix)]
    let target = ExecutionTarget::Wsl {
        distro_id: "native-test-fixture".into(),
    };
    #[cfg(windows)]
    let target = ExecutionTarget::Windows;
    probe(root, target, root.to_str().unwrap().into())
}

#[cfg(test)]
mod tests {
    use super::probe_fixture as observe;
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
    use super::*;
    fn repository(root: &Path) {
        fs::create_dir_all(root.join(".git/objects")).unwrap();
        fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    }
    #[test]
    fn directory_replacement_and_new_git_metadata_revoke_observation() {
        let owner = tempfile::tempdir().unwrap();
        let root = owner.path().join("프로젝트 space");
        fs::create_dir(&root).unwrap();
        let plain = observe(&root).unwrap();
        assert!(plain.binding().repository_object.is_none());
        repository(&root);
        assert!(plain.revalidate().is_err());
        let repo = observe(&root).unwrap();
        assert!(repo.binding().repository_object.is_some());
        let before = repo.binding().clone();
        match fs::rename(&root, owner.path().join("old")) {
            Ok(()) => {
                repository(&root);
                assert!(repo.revalidate().is_err());
            }
            #[cfg(windows)]
            Err(error) => {
                // Windows can forbid renaming a directory while its Git
                // children have open handles. The reviewed objects remain
                // valid; release the native lease before testing replacement.
                assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
                repo.revalidate().unwrap();
                drop(repo);
                drop(plain);
                fs::rename(&root, owner.path().join("old")).unwrap();
                repository(&root);
            }
            #[cfg(not(windows))]
            Err(error) => panic!("fixture rename failed: {error}"),
        }
        assert_ne!(&before, observe(&root).unwrap().binding());
    }
    #[test]
    fn linked_worktrees_share_common_object_and_pointer_edits_are_detected() {
        let owner = tempfile::tempdir().unwrap();
        let main = owner.path().join("main");
        repository(&main);
        let linked = owner.path().join("linked 한글");
        fs::create_dir(&linked).unwrap();
        let metadata = main.join(".git/worktrees/linked");
        fs::create_dir_all(&metadata).unwrap();
        fs::write(
            linked.join(".git"),
            "gitdir: ../main/.git/worktrees/linked\n",
        )
        .unwrap();
        fs::write(metadata.join("commondir"), "../..\n").unwrap();
        fs::write(metadata.join("gitdir"), "../../../../linked 한글/.git\n").unwrap();
        fs::write(metadata.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
        let first = observe(&main).unwrap();
        let second = observe(&linked).unwrap();
        assert_eq!(
            first.binding().repository_object,
            second.binding().repository_object
        );
        assert_ne!(first.binding().root_object, second.binding().root_object);
        fs::write(metadata.join("commondir"), "../../elsewhere\n").unwrap();
        assert!(second.revalidate().is_err());
        assert!(observe(&linked).is_err());
    }
    #[test]
    fn invalid_git_metadata_does_not_fall_back_to_plain_folder() {
        let owner = tempfile::tempdir().unwrap();
        for content in [
            "gitdir: missing\n",
            "gitdir: ../a\ninjected\n",
            "arbitrary\n",
        ] {
            fs::write(owner.path().join(".git"), content).unwrap();
            assert!(observe(owner.path()).is_err());
        }
        fs::write(
            owner.path().join(".git"),
            vec![b'a'; MAX_POINTER as usize + 1],
        )
        .unwrap();
        assert!(observe(owner.path()).is_err());
    }
    #[test]
    #[cfg(unix)]
    fn symlink_roots_and_git_pointers_fail_without_following_them() {
        use std::os::unix::fs::symlink;
        let owner = tempfile::tempdir().unwrap();
        let real = owner.path().join("real");
        repository(&real);
        let alias = owner.path().join("alias");
        symlink(&real, &alias).unwrap();
        assert!(observe(&alias).is_err());
        let linked = owner.path().join("linked");
        fs::create_dir(&linked).unwrap();
        fs::write(linked.join(".git"), "gitdir: ../alias/.git\n").unwrap();
        assert!(observe(&linked).is_err());
    }
}
