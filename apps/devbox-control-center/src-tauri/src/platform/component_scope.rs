//! Captured package files are review evidence, not a registry assertion or launch grant.
use devbox_filesystem::{
    ensure_no_links, filesystem_identity, opened_filesystem_identity, FilesystemIdentity,
};
use product_contract::installation::{Manifest, Member, MAX_MANIFEST_BYTES};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, &'static str>;
const MANIFEST: &str = "devbox-installation.json";
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

struct Directory {
    path: PathBuf,
    identity: FilesystemIdentity,
    _file: File,
}
impl Directory {
    fn open(path: &Path) -> Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .access_mode(0x80)
            .share_mode(3)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(path)
            .map_err(|_| "component_root_unavailable")?;
        let identity =
            opened_filesystem_identity(&file, true).map_err(|_| "component_root_unsafe")?;
        let directory = Self {
            path: path.into(),
            identity,
            _file: file,
        };
        directory.revalidate()?;
        Ok(directory)
    }
    fn revalidate(&self) -> Result<()> {
        if filesystem_identity(&self.path, true).map_err(|_| "component_root_changed")?
            != self.identity
        {
            return Err("component_root_changed");
        }
        Ok(())
    }
}
fn pin_directories(path: &Path) -> Result<Vec<Directory>> {
    use std::path::{Component, Prefix};
    if !path.is_absolute()
        || !matches!(path.components().next(),Some(Component::Prefix(prefix))
        if matches!(prefix.kind(),Prefix::Disk(_)|Prefix::VerbatimDisk(_)))
    {
        return Err("component_local_windows_root_required");
    }
    let ancestors: Vec<_> = path.ancestors().collect();
    if ancestors.len() > 64 {
        return Err("component_path_limit");
    }
    // Deny directory deletion/rename from the volume root down before traversing
    // the next component. Each opened object rejects a junction/reparse point.
    ancestors.into_iter().rev().map(Directory::open).collect()
}
struct PinnedFile {
    path: PathBuf,
    identity: FilesystemIdentity,
    file: File,
    digest: String,
    directories: Vec<Directory>,
}
impl PinnedFile {
    fn open(path: &Path, limit: u64) -> Result<Self> {
        let directories = pin_directories(path.parent().ok_or("component_path_unsafe")?)?;
        ensure_no_links(path).map_err(|_| "component_path_unsafe")?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1).custom_flags(0x0020_0000);
        }
        let mut file = options
            .open(path)
            .map_err(|_| "component_file_unavailable")?;
        let identity =
            opened_filesystem_identity(&file, false).map_err(|_| "component_file_invalid")?;
        if file.metadata().map_err(|_| "component_file_invalid")?.len() > limit {
            return Err("component_file_limit");
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut count = 0u64;
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| "component_file_unavailable")?;
            if read == 0 {
                break;
            }
            count += read as u64;
            if count > limit {
                return Err("component_file_limit");
            }
            hash.update(&buffer[..read]);
        }
        let digest = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let pinned = Self {
            path: path.into(),
            identity,
            file,
            digest,
            directories,
        };
        pinned.revalidate()?;
        Ok(pinned)
    }
    fn revalidate(&self) -> Result<()> {
        for directory in &self.directories {
            directory.revalidate()?;
        }
        ensure_no_links(&self.path).map_err(|_| "component_file_changed")?;
        if filesystem_identity(&self.path, false).map_err(|_| "component_file_changed")?
            != self.identity
        {
            return Err("component_file_changed");
        }
        Ok(())
    }
}

pub(crate) struct CapturedScope {
    retired: AtomicBool,
    root: PathBuf,
    root_identity: FilesystemIdentity,
    directories: Vec<Directory>,
    manifest_file: PinnedFile,
    pub(crate) manifest: Manifest,
    members: BTreeMap<String, PinnedFile>,
    pub(crate) issues: BTreeMap<String, &'static str>,
    pub(crate) installation_key: String,
    pub(crate) id: String,
}
impl CapturedScope {
    /// The caller uses only a native reviewed folder or its own package directory.
    pub(crate) fn capture(
        root: &Path,
        own_product: &str,
        current_executable: &Path,
        version: &str,
    ) -> Result<Self> {
        let directories = pin_directories(root)?;
        let root_identity = directories
            .last()
            .ok_or("component_root_unavailable")?
            .identity;
        let root = root
            .canonicalize()
            .map_err(|_| "component_root_unavailable")?;
        let mut manifest_file = PinnedFile::open(&root.join(MANIFEST), MAX_MANIFEST_BYTES as u64)?;
        manifest_file
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|_| "installation_manifest_invalid")?;
        let mut bytes = Vec::new();
        (&mut manifest_file.file)
            .take(MAX_MANIFEST_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "installation_manifest_invalid")?;
        if digest(&bytes) != manifest_file.digest {
            return Err("installation_manifest_changed");
        }
        let manifest = Manifest::parse(&bytes, version)?;
        let mut members = BTreeMap::new();
        let mut issues = BTreeMap::new();
        for member in &manifest.members {
            let path = member
                .executable
                .split('/')
                .fold(root.clone(), |path, part| path.join(part));
            match PinnedFile::open(&path, MAX_EXECUTABLE_BYTES).and_then(|file| {
                if file.digest == member.sha256 {
                    Ok(file)
                } else {
                    Err("component_digest_mismatch")
                }
            }) {
                Ok(file) => {
                    members.insert(member.product.clone(), file);
                }
                Err(issue) if member.product != own_product => {
                    issues.insert(member.product.clone(), issue);
                }
                Err(issue) => return Err(issue),
            }
        }
        let own = members.get(own_product).ok_or("component_self_missing")?;
        if filesystem_identity(current_executable, false).map_err(|_| "component_self_changed")?
            != own.identity
        {
            return Err("component_self_changed");
        }
        let installation_key = digest(
            &serde_json::to_vec(&(root_identity.components(), &manifest.installation_id))
                .map_err(|_| "installation_manifest_invalid")?,
        );
        let id = digest(
            &serde_json::to_vec(&(
                &installation_key,
                &manifest.generation,
                &manifest_file.digest,
            ))
            .map_err(|_| "installation_manifest_invalid")?,
        );
        let scope = Self {
            retired: AtomicBool::new(false),
            root,
            root_identity,
            directories,
            manifest_file,
            manifest,
            members,
            issues,
            installation_key,
            id,
        };
        scope.revalidate()?;
        Ok(scope)
    }
    pub(crate) fn retire(&self) {
        self.retired.store(true, Ordering::Release);
    }
    pub(crate) fn review_root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
    pub(crate) fn revalidate(&self) -> Result<()> {
        if self.retired.load(Ordering::Acquire) {
            return Err("suite_connection_retired");
        }
        for directory in &self.directories {
            directory.revalidate()?;
        }
        ensure_no_links(&self.root).map_err(|_| "component_root_changed")?;
        if filesystem_identity(&self.root, true).map_err(|_| "component_root_changed")?
            != self.root_identity
        {
            return Err("component_root_changed");
        }
        self.manifest_file.revalidate()?;
        for file in self.members.values() {
            file.revalidate()?;
        }
        Ok(())
    }
    pub(crate) fn product_for_image(&self, image: &Path) -> Result<String> {
        self.revalidate()?;
        let normalize = |path: &Path| {
            path.to_string_lossy()
                .replace('/', "\\")
                .trim_start_matches(r"\\?\")
                .to_lowercase()
        };
        let prefix = normalize(&self.root).trim_end_matches('\\').to_owned() + "\\";
        if !normalize(image).starts_with(&prefix) {
            return Err("peer_foreign_installation");
        }
        ensure_no_links(image).map_err(|_| "peer_image_unsafe")?;
        let identity = filesystem_identity(image, false).map_err(|_| "peer_image_unavailable")?;
        let parent = filesystem_identity(image.parent().ok_or("peer_image_unsafe")?, true)
            .map_err(|_| "peer_image_unavailable")?;
        let mut found = None;
        for (product, file) in &self.members {
            if file.identity == identity
                && image
                    .file_name()
                    .zip(file.path.file_name())
                    .is_some_and(|(a, b)| {
                        a.to_string_lossy()
                            .eq_ignore_ascii_case(&b.to_string_lossy())
                    })
                && filesystem_identity(file.path.parent().ok_or("peer_image_unsafe")?, true)
                    .map_err(|_| "peer_image_unavailable")?
                    == parent
            {
                if found.is_some() {
                    return Err("peer_image_ambiguous");
                }
                found = Some(product.clone());
            }
        }
        found.ok_or("peer_image_denied")
    }
    pub(crate) fn member(&self, product: &str) -> Result<(&Member, &Path, FilesystemIdentity)> {
        self.revalidate()?;
        let description = self
            .manifest
            .members
            .iter()
            .find(|member| member.product == product)
            .ok_or("component_missing")?;
        let file = self.members.get(product).ok_or("component_missing")?;
        Ok((description, &file.path, file.identity))
    }
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
