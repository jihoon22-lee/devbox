//! Native archive choices are transient read capabilities, never editor grants.
use crate::{
    files_host::current_deadline,
    platform::{
        storage_paths::{display, ProtectedStorage},
        windows_path,
    },
    private_metadata::MetadataRoot,
};
use code_pad_lib::lsp::{installer::validate_external_archive, RequestCancellation};
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, open_filesystem_object, FilesystemIdentity,
};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

type Result<T> = std::result::Result<T, &'static str>;
const MAX_FILES: usize = 32;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const TTL: Duration = Duration::from_secs(180);
struct Object {
    path: PathBuf,
    identity: FilesystemIdentity,
    _handle: File,
}
pub(super) struct Archive {
    path: PathBuf,
    parents: Vec<Object>,
    file: File,
    identity: FilesystemIdentity,
    size: u64,
    modified: SystemTime,
    extension: String,
    created: Instant,
}
impl Archive {
    fn capture(path: &Path, protected: &ProtectedStorage, deadline: u64) -> Result<Self> {
        current_deadline(deadline)?;
        windows_path::admit(path)?;
        protected.ensure_user_path(path)?;
        validate_external_archive(path).map_err(|_| "lsp_archive_unsafe")?;
        let path = display(&fs::canonicalize(path).map_err(|_| "lsp_archive_unavailable")?)?;
        windows_path::admit(&path)?;
        protected.ensure_user_path(&path)?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .ok_or("lsp_archive_unsafe")?
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "zip" | "tgz" | "gz") {
            return Err("lsp_archive_unsafe");
        }
        let mut parents = Vec::new();
        for path in path.ancestors().skip(1) {
            if parents.len() >= 64 {
                return Err("lsp_archive_limit");
            }
            current_deadline(deadline)?;
            ensure_no_links(path).map_err(|_| "lsp_archive_unsafe")?;
            let (handle, identity) =
                open_filesystem_object(path, true).map_err(|_| "lsp_archive_unavailable")?;
            parents.push(Object {
                path: path.into(),
                identity,
                _handle: handle,
            });
        }
        let (file, identity) =
            open_filesystem_object(&path, false).map_err(|_| "lsp_archive_unavailable")?;
        let metadata = file.metadata().map_err(|_| "lsp_archive_unavailable")?;
        let size = metadata.len();
        if size == 0 || size > MAX_BYTES {
            return Err("lsp_archive_limit");
        }
        let archive = Self {
            path,
            parents,
            file,
            identity,
            size,
            modified: metadata.modified().map_err(|_| "lsp_archive_unavailable")?,
            extension,
            created: Instant::now(),
        };
        archive.revalidate()?;
        Ok(archive)
    }
    fn revalidate(&self) -> Result<()> {
        windows_path::admit(&self.path)?;
        for parent in &self.parents {
            ensure_no_links(&parent.path).map_err(|_| "lsp_archive_changed")?;
            if filesystem_identity(&parent.path, true).ok() != Some(parent.identity) {
                return Err("lsp_archive_changed");
            }
        }
        validate_external_archive(&self.path).map_err(|_| "lsp_archive_changed")?;
        if filesystem_identity(&self.path, false).ok() != Some(self.identity) {
            return Err("lsp_archive_changed");
        }
        let metadata = self.file.metadata().map_err(|_| "lsp_archive_changed")?;
        if metadata.len() != self.size || metadata.modified().ok() != Some(self.modified) {
            return Err("lsp_archive_changed");
        }
        Ok(())
    }
}
pub(super) struct Selections {
    protected: ProtectedStorage,
    pending: HashMap<String, Archive>,
}
impl Selections {
    pub(super) fn new(protected: ProtectedStorage) -> Self {
        Self {
            protected,
            pending: HashMap::new(),
        }
    }
    pub(super) fn expire(&mut self) {
        self.pending
            .retain(|_, archive| archive.created.elapsed() < TTL);
    }
    pub(super) fn choose(&mut self, paths: &[PathBuf], deadline: u64) -> Result<Vec<String>> {
        self.expire();
        if paths.len() > MAX_FILES || self.pending.len() + paths.len() > MAX_FILES {
            return Err("lsp_archive_limit");
        }
        // Validate all native chooser transports before opening any selection.
        for path in paths {
            windows_path::admit(path)?;
            self.protected.ensure_user_path(path)?;
        }
        let mut archives = Vec::new();
        let mut bytes = self
            .pending
            .values()
            .map(|archive| archive.size)
            .sum::<u64>();
        let mut identities = HashSet::new();
        for path in paths {
            let archive = Archive::capture(path, &self.protected, deadline)?;
            bytes = bytes.checked_add(archive.size).ok_or("lsp_archive_limit")?;
            if bytes > MAX_BYTES || !identities.insert(archive.identity) {
                return Err("lsp_archive_limit");
            }
            archives.push(archive);
        }
        current_deadline(deadline)?;
        Ok(archives
            .into_iter()
            .map(|archive| {
                let token = uuid::Uuid::new_v4().to_string();
                self.pending.insert(token.clone(), archive);
                token
            })
            .collect())
    }
    fn validate_tokens(tokens: &[String]) -> Result<()> {
        let mut seen = HashSet::new();
        if tokens.len() > MAX_FILES
            || tokens
                .iter()
                .any(|token| uuid::Uuid::parse_str(token).is_err() || !seen.insert(token))
        {
            return Err("lsp_archive_selection_invalid");
        }
        Ok(())
    }
    pub(super) fn discard(&mut self, tokens: &[String]) -> Result<()> {
        Self::validate_tokens(tokens)?;
        for token in tokens {
            self.pending.remove(token);
        }
        Ok(())
    }
    pub(super) fn take(&mut self, tokens: &[String]) -> Result<Vec<Archive>> {
        self.expire();
        Self::validate_tokens(tokens)?;
        if tokens.is_empty() || tokens.iter().any(|token| !self.pending.contains_key(token)) {
            return Err("lsp_archive_selection_invalid");
        }
        Ok(tokens
            .iter()
            .map(|token| {
                self.pending
                    .remove(token)
                    .expect("validated native selection")
            })
            .collect())
    }
}
pub(super) struct Snapshots {
    _parent: Arc<MetadataRoot>,
    root: MetadataRoot,
    files: Vec<(PathBuf, FilesystemIdentity)>,
}
impl Snapshots {
    pub(super) fn create(
        parent: Arc<MetadataRoot>,
        archives: Vec<Archive>,
        cancellation: &RequestCancellation,
    ) -> Result<Self> {
        let root = parent.child(&uuid::Uuid::new_v4().to_string())?;
        let mut snapshots = Self {
            _parent: parent,
            root,
            files: Vec::new(),
        };
        for (index, mut archive) in archives.into_iter().enumerate() {
            archive.revalidate()?;
            snapshots.root.revalidate()?;
            if cancellation.is_cancelled() {
                return Err("lsp_operation_cancelled");
            }
            let path = snapshots
                .root
                .path()
                .join(format!("{index}.{}", archive.extension));
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| "lsp_archive_unavailable")?;
            let identity =
                filesystem_identity(&path, false).map_err(|_| "lsp_archive_unavailable")?;
            snapshots.files.push((path, identity));
            archive
                .file
                .seek(SeekFrom::Start(0))
                .map_err(|_| "lsp_archive_unavailable")?;
            let mut copied = 0u64;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                if cancellation.is_cancelled() {
                    return Err("lsp_operation_cancelled");
                }
                let read = archive
                    .file
                    .read(&mut buffer)
                    .map_err(|_| "lsp_archive_unavailable")?;
                if read == 0 {
                    break;
                }
                copied += read as u64;
                if copied > archive.size {
                    return Err("lsp_archive_changed");
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(|_| "lsp_archive_unavailable")?;
            }
            output.sync_all().map_err(|_| "lsp_archive_unavailable")?;
            if copied != archive.size {
                return Err("lsp_archive_changed");
            }
            archive.revalidate()?;
            snapshots.root.revalidate()?;
        }
        Ok(snapshots)
    }
    pub(super) fn paths(&self) -> Vec<PathBuf> {
        self.files.iter().map(|(path, _)| path.clone()).collect()
    }
}
impl Drop for Snapshots {
    fn drop(&mut self) {
        if self.root.revalidate().is_err() {
            return;
        }
        for (path, identity) in &self.files {
            if filesystem_identity(path, false).ok() == Some(*identity) {
                let _ = fs::remove_file(path);
            }
        }
        // Preserve unexpected entries; never recursively remove a changed tree.
        let _ = fs::remove_dir(self.root.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn deadline() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            + 29_000
    }
    fn fixture() -> (tempfile::TempDir, Selections, Arc<MetadataRoot>) {
        let root = tempfile::tempdir().unwrap();
        let product = root.path().join("product");
        fs::create_dir(&product).unwrap();
        let selections = Selections::new(ProtectedStorage::new(&product, vec![]).unwrap());
        let snapshots = Arc::new(
            MetadataRoot::open(&product)
                .unwrap()
                .child("snapshots")
                .unwrap(),
        );
        (root, selections, snapshots)
    }
    #[test]
    fn native_tokens_are_single_use_and_unknown_tokens_do_not_consume_other_choices() {
        let (root, mut selections, snapshots) = fixture();
        let chosen = root.path().join("한글 selected.tgz");
        fs::write(&chosen, b"synthetic archive").unwrap();
        let tokens = selections
            .choose(std::slice::from_ref(&chosen), deadline())
            .unwrap();
        assert!(uuid::Uuid::parse_str(&tokens[0]).is_ok());
        assert!(selections
            .take(&[tokens[0].clone(), chosen.display().to_string()])
            .is_err());
        assert!(selections
            .take(&[tokens[0].clone(), uuid::Uuid::new_v4().to_string()])
            .is_err());
        let copy = Snapshots::create(
            snapshots.clone(),
            selections.take(&tokens).unwrap(),
            &RequestCancellation::new(),
        )
        .unwrap();
        assert!(selections.take(&tokens).is_err());
        assert_eq!(fs::read(&copy.paths()[0]).unwrap(), b"synthetic archive");
        assert_eq!(copy.paths()[0].extension().unwrap(), "tgz");
        let directory = copy.root.path().to_owned();
        drop(copy);
        assert!(!directory.exists());
        assert_eq!(fs::read(chosen).unwrap(), b"synthetic archive");
    }
    #[test]
    fn discard_and_expiry_release_choices_without_reopening_unavailable_sources() {
        let (root, mut selections, _) = fixture();
        let chosen = root.path().join("selected.zip");
        fs::write(&chosen, b"synthetic").unwrap();
        let tokens = selections
            .choose(std::slice::from_ref(&chosen), deadline())
            .unwrap();
        fs::remove_file(&chosen).unwrap();
        selections.discard(&tokens).unwrap();
        assert!(selections.take(&tokens).is_err());
        fs::write(&chosen, b"another").unwrap();
        let tokens = selections.choose(&[chosen], deadline()).unwrap();
        selections.pending.get_mut(&tokens[0]).unwrap().created -= TTL;
        assert!(selections.take(&tokens).is_err());
        assert!(selections.pending.is_empty());
    }
    #[test]
    fn replaced_files_changed_contents_and_protected_paths_never_reach_import() {
        let (root, mut selections, snapshots) = fixture();
        let chosen = root.path().join("selected.zip");
        fs::write(&chosen, b"first").unwrap();
        let tokens = selections
            .choose(std::slice::from_ref(&chosen), deadline())
            .unwrap();
        fs::rename(&chosen, root.path().join("old.zip")).unwrap();
        fs::write(&chosen, b"first").unwrap();
        assert!(Snapshots::create(
            snapshots.clone(),
            selections.take(&tokens).unwrap(),
            &RequestCancellation::new()
        )
        .is_err());
        let tokens = selections
            .choose(std::slice::from_ref(&chosen), deadline())
            .unwrap();
        fs::write(&chosen, b"changed contents").unwrap();
        assert!(Snapshots::create(
            snapshots.clone(),
            selections.take(&tokens).unwrap(),
            &RequestCancellation::new()
        )
        .is_err());
        let private = root.path().join("product/private.zip");
        fs::write(&private, b"private").unwrap();
        assert!(selections.choose(&[private], deadline()).is_err());
        let hardlink = root.path().join("hardlink.zip");
        fs::hard_link(&chosen, &hardlink).unwrap();
        assert!(selections.choose(&[hardlink], deadline()).is_err());
        assert_eq!(fs::read(chosen).unwrap(), b"changed contents");
        assert!(fs::read_dir(snapshots.path()).unwrap().next().is_none());
    }
    #[test]
    fn cancellation_and_changed_snapshot_directories_preserve_source_and_unknown_data() {
        let (root, mut selections, snapshots) = fixture();
        let chosen = root.path().join("selected.zip");
        fs::write(&chosen, b"synthetic").unwrap();
        let tokens = selections
            .choose(std::slice::from_ref(&chosen), deadline())
            .unwrap();
        let cancelled = RequestCancellation::new();
        cancelled.cancel();
        assert!(Snapshots::create(
            snapshots.clone(),
            selections.take(&tokens).unwrap(),
            &cancelled
        )
        .is_err());
        assert!(fs::read_dir(snapshots.path()).unwrap().next().is_none());
        let tokens = selections
            .choose(std::slice::from_ref(&chosen), deadline())
            .unwrap();
        let copy = Snapshots::create(
            snapshots,
            selections.take(&tokens).unwrap(),
            &RequestCancellation::new(),
        )
        .unwrap();
        let directory = copy.root.path().to_owned();
        let unknown = directory.join("unexpected.txt");
        fs::write(&unknown, b"preserve").unwrap();
        drop(copy);
        assert_eq!(fs::read(unknown).unwrap(), b"preserve");
        assert_eq!(fs::read(chosen).unwrap(), b"synthetic");
    }
}
