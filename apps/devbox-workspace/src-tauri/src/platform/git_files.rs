//! Bounded, no-follow execution evidence. These reads do not invoke Git or
//! contact a new UNC/WSL authority found in untrusted configuration.
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_OBJECTS: usize = 1024;
const MAX_BYTES: usize = 48 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 256;
struct Object {
    identity: FilesystemIdentity,
    directory: bool,
    _handle: File,
    bytes: Option<Vec<u8>>,
    names: Option<Vec<OsString>>,
}
#[derive(Default)]
pub struct GitFiles {
    objects: BTreeMap<PathBuf, Object>,
    absent: BTreeSet<PathBuf>,
    total: usize,
}
fn boundary(deadline: u64) -> Result<()> {
    crate::files_host::current_deadline(deadline)
}
fn ordinary(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or("git_source_path_invalid")?;
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        Ok(format!(r"\\{unc}"))
    } else {
        Ok(text.strip_prefix(r"\\?\").unwrap_or(text).to_owned())
    }
}
/// Read-only admission is lexical and precedes every filesystem call. Local
/// config may include another local drive. A UNC include must stay within the
/// already-selected native project's server/share; WSL is never auto-started.
pub fn transport(project: &Path, path: &Path) -> Result<()> {
    super::windows_path::admit(path).map_err(|_| "git_source_transport_denied")?;
    use devbox_filesystem::{parse_safe_project_path, ProjectPathKind};
    let text = ordinary(path)?;
    if devbox_wsl::path::parse_wsl_unc_path(&text)
        .map_err(|_| "git_source_path_invalid")?
        .is_some()
    {
        return Err("git_source_transport_denied");
    }
    let parsed = parse_safe_project_path(&text).ok_or("git_source_path_invalid")?;
    match parsed.kind() {
        ProjectPathKind::WindowsDrive => Ok(()),
        ProjectPathKind::WindowsUnc => {
            let root_text = ordinary(project)?;
            let root = parse_safe_project_path(&root_text).ok_or("git_source_path_invalid")?;
            let share = |text: &str| {
                text.replace('\\', "/")
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .take(2)
                    .map(str::to_ascii_lowercase)
                    .collect::<Vec<_>>()
            };
            if root.kind() == ProjectPathKind::WindowsUnc
                && share(root.identity()) == share(parsed.identity())
            {
                Ok(())
            } else {
                Err("git_source_transport_denied")
            }
        }
        ProjectPathKind::Posix => {
            #[cfg(unix)]
            {
                Ok(())
            }
            #[cfg(not(unix))]
            {
                Err("git_source_transport_denied")
            }
        }
    }
}
/// Resolve '..' only after retaining its traversed path for no-link checks.
/// Tilde/prefix interpolation is performed by the Git config owner beforehand.
pub fn resolve(project: &Path, base: &Path, value: &Path, deadline: u64) -> Result<PathBuf> {
    boundary(deadline)?;
    let joined = if value.is_absolute() {
        value.to_owned()
    } else {
        base.join(value)
    };
    let mut path = PathBuf::new();
    let mut traversed = Vec::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                traversed.push(path.clone());
                if !path.pop() {
                    return Err("git_source_path_invalid");
                }
            }
            Component::CurDir => {}
            part => path.push(part),
        }
    }
    // All transports are checked first, including a prefix removed by '..'.
    transport(project, &path)?;
    for prefix in &traversed {
        transport(project, prefix)?;
    }
    for prefix in &traversed {
        boundary(deadline)?;
        ensure_no_links(prefix).map_err(|_| "git_source_path_invalid")?;
    }
    boundary(deadline)?;
    Ok(path)
}
impl GitFiles {
    fn pin(&mut self, path: &Path, directory: bool, deadline: u64) -> Result<()> {
        boundary(deadline)?;
        super::windows_path::admit(path).map_err(|_| "git_source_transport_denied")?;
        if let Some(object) = self.objects.get(path) {
            if object.directory != directory
                || filesystem_identity(path, directory).ok() != Some(object.identity)
            {
                return Err("git_sources_changed");
            }
            return Ok(());
        }
        if self.objects.len() + self.absent.len() >= MAX_OBJECTS {
            return Err("git_source_limit");
        }
        ensure_no_links(path).map_err(|_| "git_source_path_invalid")?;
        let (handle, identity) =
            open_filesystem_object(path, directory).map_err(|_| "git_source_unavailable")?;
        self.objects.insert(
            path.to_owned(),
            Object {
                identity,
                directory,
                _handle: handle,
                bytes: None,
                names: None,
            },
        );
        boundary(deadline)
    }
    fn ancestors(
        &mut self,
        project: &Path,
        path: &Path,
        directory: bool,
        deadline: u64,
    ) -> Result<bool> {
        transport(project, path)?;
        let mut paths = path.ancestors().collect::<Vec<_>>();
        paths.reverse();
        for part in paths {
            if part.as_os_str().is_empty() {
                continue;
            }
            boundary(deadline)?;
            super::windows_path::admit(part).map_err(|_| "git_source_transport_denied")?;
            match fs::symlink_metadata(part) {
                Ok(_) => self.pin(part, part != path || directory, deadline)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if self.objects.len() + self.absent.len() >= MAX_OBJECTS {
                        return Err("git_source_limit");
                    }
                    self.absent.insert(part.to_owned());
                    return Ok(false);
                }
                Err(_) => return Err("git_source_unavailable"),
            }
        }
        Ok(true)
    }
    pub fn file(
        &mut self,
        project: &Path,
        path: &Path,
        limit: usize,
        deadline: u64,
    ) -> Result<Option<Vec<u8>>> {
        if !self.ancestors(project, path, false, deadline)? {
            return Ok(None);
        }
        let object = self.objects.get_mut(path).ok_or("git_source_unavailable")?;
        if let Some(bytes) = &object.bytes {
            if bytes.len() > limit {
                return Err("git_source_limit");
            }
            return Ok(Some(bytes.clone()));
        }
        let (handle, identity) =
            open_filesystem_object(path, false).map_err(|_| "git_source_unavailable")?;
        if handle
            .metadata()
            .map_err(|_| "git_source_unavailable")?
            .len()
            > limit as u64
        {
            return Err("git_source_limit");
        }
        let mut bytes = Vec::new();
        handle
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "git_source_unavailable")?;
        if bytes.len() > limit || self.total.saturating_add(bytes.len()) > MAX_BYTES {
            return Err("git_source_limit");
        }
        if identity != object.identity || filesystem_identity(path, false).ok() != Some(identity) {
            return Err("git_sources_changed");
        }
        self.total += bytes.len();
        object.bytes = Some(bytes.clone());
        boundary(deadline)?;
        Ok(Some(bytes))
    }
    pub fn hooks(&mut self, project: &Path, path: &Path, deadline: u64) -> Result<Vec<PathBuf>> {
        if !self.ancestors(project, path, true, deadline)? {
            return Ok(vec![]);
        }
        let names = directory_names(path)?;
        self.objects
            .get_mut(path)
            .ok_or("git_source_unavailable")?
            .names = Some(names.clone());
        let mut files = Vec::new();
        for name in names {
            boundary(deadline)?;
            let child = path.join(name);
            let metadata = fs::symlink_metadata(&child).map_err(|_| "git_sources_changed")?;
            if metadata.is_dir() {
                self.pin(&child, true, deadline)?;
            } else {
                self.file(project, &child, 2 * 1024 * 1024, deadline)?
                    .ok_or("git_sources_changed")?;
                files.push(child);
            }
        }
        Ok(files)
    }
    pub fn revalidate(&self, deadline: u64) -> Result<()> {
        for (path, object) in &self.objects {
            boundary(deadline)?;
            super::windows_path::admit(path).map_err(|_| "git_source_transport_denied")?;
            ensure_no_links(path).map_err(|_| "git_sources_changed")?;
            if filesystem_identity(path, object.directory).ok() != Some(object.identity) {
                return Err("git_sources_changed");
            }
            if let Some(expected) = &object.bytes {
                let (handle, identity) =
                    open_filesystem_object(path, false).map_err(|_| "git_sources_changed")?;
                let mut bytes = Vec::new();
                handle
                    .take(expected.len() as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "git_sources_changed")?;
                if identity != object.identity || bytes != *expected {
                    return Err("git_sources_changed");
                }
            }
            if let Some(expected) = &object.names {
                if directory_names(path)? != *expected {
                    return Err("git_sources_changed");
                }
            }
        }
        for path in &self.absent {
            boundary(deadline)?;
            super::windows_path::admit(path).map_err(|_| "git_source_transport_denied")?;
            if !matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            {
                return Err("git_sources_changed");
            }
        }
        boundary(deadline)
    }
    pub fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        let mut part = |bytes: &[u8]| {
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        };
        for (path, object) in &self.objects {
            part(b"object");
            part(path.as_os_str().as_encoded_bytes());
            part(format!("{:?}", object.identity).as_bytes());
            part(if object.directory {
                b"directory"
            } else {
                b"file"
            });
            if let Some(bytes) = &object.bytes {
                part(b"contents");
                part(bytes);
            }
            if let Some(names) = &object.names {
                part(b"entries");
                part(&(names.len() as u64).to_be_bytes());
                for name in names {
                    part(name.as_encoded_bytes());
                }
            }
        }
        part(b"absent");
        for path in &self.absent {
            part(path.as_os_str().as_encoded_bytes());
        }
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
fn directory_names(path: &Path) -> Result<Vec<OsString>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(path)
        .map_err(|_| "git_source_unavailable")?
        .take(MAX_DIRECTORY_ENTRIES + 1)
    {
        names.push(entry.map_err(|_| "git_source_unavailable")?.file_name());
    }
    if names.len() > MAX_DIRECTORY_ENTRIES {
        return Err("git_source_limit");
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_include_and_replaced_same_bytes_require_new_evidence() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        fs::write(&config, b"[core]\n").unwrap();
        let mut files = GitFiles::default();
        files.file(root.path(), &config, 1024, u64::MAX).unwrap();
        assert!(files
            .file(
                root.path(),
                &root.path().join("missing/config"),
                1024,
                u64::MAX
            )
            .unwrap()
            .is_none());
        assert!(files.revalidate(u64::MAX).is_ok());
        fs::create_dir(root.path().join("missing")).unwrap();
        assert_eq!(
            files.revalidate(u64::MAX).unwrap_err(),
            "git_sources_changed"
        );
        let mut files = GitFiles::default();
        files.file(root.path(), &config, 1024, u64::MAX).unwrap();
        fs::rename(&config, root.path().join("previous")).unwrap();
        fs::write(&config, b"[core]\n").unwrap();
        assert!(files.revalidate(u64::MAX).is_err());
    }
    #[test]
    fn new_or_changed_hooks_invalidate_the_approved_directory() {
        let root = tempfile::tempdir().unwrap();
        let hooks = root.path().join("hooks");
        fs::create_dir(&hooks).unwrap();
        fs::write(hooks.join("pre-commit"), b"original").unwrap();
        let mut files = GitFiles::default();
        files.hooks(root.path(), &hooks, u64::MAX).unwrap();
        let digest = files.digest();
        fs::write(hooks.join("pre-commit"), b"modified").unwrap();
        assert!(files.revalidate(u64::MAX).is_err());
        let mut files = GitFiles::default();
        files.hooks(root.path(), &hooks, u64::MAX).unwrap();
        assert_ne!(digest, files.digest());
        fs::write(hooks.join("post-commit"), b"new").unwrap();
        assert!(files.revalidate(u64::MAX).is_err());
    }
    #[test]
    fn untrusted_paths_cannot_contact_other_unc_hosts_or_wsl() {
        assert!(transport(
            Path::new(r"C:\project"),
            Path::new(r"\\unreviewed\share\config")
        )
        .is_err());
        assert!(transport(
            Path::new(r"C:\project"),
            Path::new(r"\\wsl.localhost\Ubuntu\home\config")
        )
        .is_err());
        assert!(transport(
            Path::new(r"\\owned\share\project"),
            Path::new(r"\\owned\share\config")
        )
        .is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn a_link_before_parent_traversal_is_rejected_before_reading() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
        assert!(resolve(
            root.path(),
            root.path(),
            Path::new("linked/../config"),
            u64::MAX
        )
        .is_err());
    }
}
