//! Retained native objects and bounded streaming hashes for reviewed LSP code.
//! Unlike Git text evidence, a native server executable can exceed 48 MiB.
use crate::{files::Admission, git_files, storage_paths::display};
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_OBJECTS: usize = 20_000;
const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;

pub fn transport(root: &Path, path: &Path) -> Result<()> {
    transport_with_admission(root, path, crate::windows_path::admit)
}
pub fn transport_with_admission(root: &Path, path: &Path, admission: Admission) -> Result<()> {
    git_files::transport_with_admission(root, path, admission)
        .map_err(|_| "lsp_source_transport_denied")
}

/// Check every candidate before any resolver may follow one of them. A bare
/// command can select a project file or a PATH entry, including an .exe suffix.
pub fn inspect_candidates(
    root: &Path,
    paths: &BTreeSet<PathBuf>,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    inspect_candidates_with_admission(root, paths, check, crate::windows_path::admit)
}
pub fn inspect_candidates_with_admission(
    root: &Path,
    paths: &BTreeSet<PathBuf>,
    check: &dyn Fn() -> Result<()>,
    admission: Admission,
) -> Result<()> {
    if paths.len() > 4096 {
        return Err("lsp_source_limit");
    }
    for path in paths {
        transport_with_admission(root, path, admission)?;
    }
    for path in paths {
        check()?;
        match ensure_no_links(path) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) => {}
            Err(_) => return Err("lsp_source_path_invalid"),
        }
    }
    Ok(())
}

fn hash(mut file: File, check: &dyn Fn() -> Result<()>) -> Result<(String, u64)> {
    let size = file.metadata().map_err(|_| "lsp_source_unavailable")?.len();
    if size > MAX_FILE {
        return Err("lsp_source_limit");
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0u64;
    loop {
        check()?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| "lsp_source_unavailable")?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_FILE {
            return Err("lsp_source_limit");
        }
        hasher.update(&buffer[..count]);
    }
    if total != size {
        return Err("lsp_sources_changed");
    }
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        total,
    ))
}
fn names(path: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(path).map_err(|_| "lsp_source_unavailable")? {
        if names.len() >= MAX_OBJECTS {
            return Err("lsp_source_limit");
        }
        names.push(
            entry
                .map_err(|_| "lsp_source_unavailable")?
                .file_name()
                .into_string()
                .map_err(|_| "lsp_source_path_invalid")?,
        );
    }
    names.sort();
    Ok(names)
}
struct Object {
    identity: FilesystemIdentity,
    directory: bool,
    _handle: File,
    #[cfg(unix)]
    mode: u32,
    digest: Option<String>,
    names: Option<Vec<String>>,
}
pub struct Evidence {
    admission: Admission,
    objects: BTreeMap<PathBuf, Object>,
    total: u64,
}
impl Default for Evidence {
    fn default() -> Self {
        Self::with_admission(crate::windows_path::admit)
    }
}
impl Evidence {
    pub fn with_admission(admission: Admission) -> Self {
        Self {
            admission,
            objects: BTreeMap::new(),
            total: 0,
        }
    }
    fn object(
        &mut self,
        path: &Path,
        directory: bool,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        check()?;
        (self.admission)(path).map_err(|_| "lsp_source_transport_denied")?;
        ensure_no_links(path).map_err(|_| "lsp_source_path_invalid")?;
        if !directory
            && !fs::symlink_metadata(path)
                .map_err(|_| "lsp_source_unavailable")?
                .is_file()
        {
            return Err("lsp_source_path_invalid");
        }
        if let Some(object) = self.objects.get(path) {
            if object.directory != directory
                || filesystem_identity(path, directory).ok() != Some(object.identity)
            {
                return Err("lsp_sources_changed");
            }
            return Ok(());
        }
        if self.objects.len() >= MAX_OBJECTS {
            return Err("lsp_source_limit");
        }
        let (handle, identity) =
            open_filesystem_object(path, directory).map_err(|_| "lsp_source_unavailable")?;
        self.objects.insert(
            path.to_owned(),
            Object {
                identity,
                directory,
                #[cfg(unix)]
                mode: handle
                    .metadata()
                    .map_err(|_| "lsp_source_unavailable")?
                    .permissions()
                    .mode(),
                _handle: handle,
                digest: None,
                names: None,
            },
        );
        Ok(())
    }
    pub fn path(
        &mut self,
        root: &Path,
        path: &Path,
        directory: bool,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        let path = display(path)?;
        transport_with_admission(root, &path, self.admission)?;
        let mut parts: Vec<_> = path
            .ancestors()
            .filter(|path| !path.as_os_str().is_empty())
            .collect();
        if parts.len() > 128 {
            return Err("lsp_source_limit");
        }
        parts.reverse();
        for part in parts {
            self.object(part, part != path || directory, check)?;
        }
        if !directory
            && self
                .objects
                .get(&path)
                .is_some_and(|object| object.digest.is_none())
        {
            let (handle, identity) =
                open_filesystem_object(&path, false).map_err(|_| "lsp_source_unavailable")?;
            let (digest, size) = hash(handle, check)?;
            let object = self
                .objects
                .get_mut(&path)
                .ok_or("lsp_source_unavailable")?;
            if identity != object.identity
                || filesystem_identity(&path, false).ok() != Some(identity)
            {
                return Err("lsp_sources_changed");
            }
            self.total = self.total.checked_add(size).ok_or("lsp_source_limit")?;
            if self.total > MAX_TOTAL {
                return Err("lsp_source_limit");
            }
            object.digest = Some(digest);
        }
        Ok(())
    }
    pub fn tree(&mut self, root: &Path, path: &Path, check: &dyn Fn() -> Result<()>) -> Result<()> {
        let mut pending = vec![display(path)?];
        while let Some(path) = pending.pop() {
            self.path(root, &path, true, check)?;
            let children = names(&path)?;
            for name in &children {
                check()?;
                let child = path.join(name);
                let metadata =
                    fs::symlink_metadata(&child).map_err(|_| "lsp_source_unavailable")?;
                if metadata.is_dir() {
                    pending.push(child);
                } else {
                    self.path(root, &child, false, check)?;
                }
                if pending.len() + self.objects.len() > MAX_OBJECTS {
                    return Err("lsp_source_limit");
                }
            }
            self.objects
                .get_mut(&path)
                .ok_or("lsp_source_unavailable")?
                .names = Some(children);
        }
        Ok(())
    }
    pub fn revalidate(&self, root: &Path, check: &dyn Fn() -> Result<()>) -> Result<()> {
        transport_with_admission(root, root, self.admission)?;
        for (path, object) in &self.objects {
            check()?;
            (self.admission)(path).map_err(|_| "lsp_source_transport_denied")?;
            ensure_no_links(path).map_err(|_| "lsp_sources_changed")?;
            let (handle, identity) = open_filesystem_object(path, object.directory)
                .map_err(|_| "lsp_sources_changed")?;
            if identity != object.identity {
                return Err("lsp_sources_changed");
            }
            #[cfg(unix)]
            if handle
                .metadata()
                .map_err(|_| "lsp_sources_changed")?
                .permissions()
                .mode()
                != object.mode
            {
                return Err("lsp_sources_changed");
            }
            if let Some(expected) = &object.digest {
                if hash(handle, check)?.0 != *expected {
                    return Err("lsp_sources_changed");
                }
            }
            if let Some(expected) = &object.names {
                if names(path)? != *expected {
                    return Err("lsp_sources_changed");
                }
            }
            if filesystem_identity(path, object.directory).ok() != Some(object.identity) {
                return Err("lsp_sources_changed");
            }
        }
        check()
    }
    pub fn digest(&self) -> Result<String> {
        let objects = self
            .objects
            .iter()
            .map(|(path, object)| {
                (
                    path,
                    format!("{:?}", object.identity),
                    &object.digest,
                    &object.names,
                    #[cfg(unix)]
                    object.mode,
                )
            })
            .collect::<Vec<_>>();
        let bytes = serde_json::to_vec(&objects).map_err(|_| "lsp_source_path_invalid")?;
        Ok(Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_unapproved_transport_stops_all_candidate_inspection() {
        let root = tempfile::tempdir().unwrap();
        let paths = BTreeSet::from([
            root.path().join("missing.exe"),
            PathBuf::from(r"\\wsl.localhost\Unrequested\server.exe"),
        ]);
        let error = inspect_candidates(root.path(), &paths, &|| {
            panic!("candidate IO must not begin")
        })
        .unwrap_err();
        assert_eq!(error, "lsp_source_transport_denied");
    }
    #[test]
    fn execution_evidence_detects_in_place_edits_added_modules_and_replaced_objects() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let entry = root.join("entry.js");
        fs::write(&entry, b"original").unwrap();
        let mut evidence = Evidence::default();
        evidence.tree(root, root, &|| Ok(())).unwrap();
        evidence.revalidate(root, &|| Ok(())).unwrap();
        let digest = evidence.digest().unwrap();
        fs::write(&entry, b"modified").unwrap();
        assert!(evidence.revalidate(root, &|| Ok(())).is_err());
        fs::write(&entry, b"original").unwrap();
        evidence.revalidate(root, &|| Ok(())).unwrap();
        fs::write(root.join("extra.js"), b"unreviewed").unwrap();
        assert!(evidence.revalidate(root, &|| Ok(())).is_err());
        fs::remove_file(root.join("extra.js")).unwrap();
        fs::rename(&entry, root.join("former.js")).unwrap();
        fs::write(&entry, b"original").unwrap();
        assert!(evidence.revalidate(root, &|| Ok(())).is_err());
        assert_eq!(evidence.digest().unwrap(), digest);
    }
}
