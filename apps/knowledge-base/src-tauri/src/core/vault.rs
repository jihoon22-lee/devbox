//! Knowledge vault identity and child-path boundary.
//!
//! A configured vault is an existing, ordinary directory.  Quick capture
//! snapshots its canonical path and filesystem identity during preview, then
//! requires the same identity immediately before every save-side mutation.
//! This is deliberately app-local: it protects the fixed Inbox writer without
//! widening the shared filesystem crate into a Knowledge-specific vault API.

use std::fmt;
use std::path::{Component, Path, PathBuf};

const INVALID_ROOT: &str = "빠른 캡처 저장 위치를 사용할 수 없습니다";
const INVALID_ENTRY: &str = "Knowledge 항목 경로가 올바르지 않습니다";
const STALE_ROOT: &str = "빠른 캡처 미리보기가 오래되어 다시 확인하세요";

#[derive(Clone, PartialEq, Eq)]
pub struct VaultIdentity {
    canonical_path: PathBuf,
    marker: FileIdentity,
}

/// Filesystem identity captured for one regular file.  Rollback callers must
/// provide this token so a path replaced by another writer is never deleted
/// merely because it still resolves inside the same vault.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct EntryIdentity(FileIdentity);

impl EntryIdentity {
    /// Compare identities across a publication rename. Unix and Windows have
    /// stable filesystem IDs; the fallback platform only has a path marker,
    /// so it cannot prove a rename preserved the same object.
    pub(crate) fn matches(&self, other: &Self) -> bool {
        #[cfg(any(unix, windows))]
        {
            self.0.is_usable() && other.0.is_usable() && self == other
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (self, other);
            // A path-only fallback cannot prove that the same filesystem
            // object survived a rename or replacement. Cleanup and dedupe
            // must fail closed instead of deleting or reusing by path.
            false
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum FileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows {
        volume: Option<u32>,
        file_index: Option<u64>,
    },
    #[cfg(not(any(unix, windows)))]
    Path(PathBuf),
}

impl FileIdentity {
    fn is_usable(&self) -> bool {
        #[cfg(windows)]
        {
            matches!(
                self,
                Self::Windows {
                    volume: Some(_),
                    file_index: Some(_),
                }
            )
        }
        #[cfg(not(windows))]
        {
            true
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VaultError {
    InvalidRoot,
    InvalidEntry,
    Stale,
}

impl VaultError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidRoot => INVALID_ROOT,
            Self::InvalidEntry => INVALID_ENTRY,
            Self::Stale => STALE_ROOT,
        }
    }
}

impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl fmt::Debug for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for VaultError {}

impl VaultIdentity {
    /// Inspect an already configured vault without creating it or any child.
    pub fn inspect(path: &Path) -> Result<Self, VaultError> {
        if path.as_os_str().is_empty() {
            return Err(VaultError::InvalidRoot);
        }
        reject_root_link_components(path)?;
        let metadata = std::fs::symlink_metadata(path).map_err(|_| VaultError::InvalidRoot)?;
        if !is_plain_directory(&metadata) {
            return Err(VaultError::InvalidRoot);
        }
        let canonical_path = path.canonicalize().map_err(|_| VaultError::InvalidRoot)?;
        // Re-check the canonical spelling as well.  A parent directory can be
        // replaced by a link after the first check while the final directory
        // keeps the same inode; accepting that path would reintroduce a
        // reparse boundary on the next save-side operation.
        reject_root_link_components(&canonical_path)?;
        let canonical_metadata =
            std::fs::symlink_metadata(&canonical_path).map_err(|_| VaultError::InvalidRoot)?;
        if !is_plain_directory(&canonical_metadata) {
            return Err(VaultError::InvalidRoot);
        }
        let marker = file_identity(&canonical_path, &canonical_metadata);
        let original_marker = file_identity(path, &metadata);
        if !marker.is_usable() || !original_marker.is_usable() {
            return Err(VaultError::InvalidRoot);
        }
        // A path that resolves to a different object after canonicalization is
        // treated as a reparse/symlink boundary, even on platforms where the
        // standard library exposes no richer reparse metadata.
        if marker != original_marker {
            return Err(VaultError::InvalidRoot);
        }
        Ok(Self {
            canonical_path,
            marker,
        })
    }

    #[cfg(test)]
    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }

    /// Re-read the canonical root and compare both path and filesystem ID.
    /// The check is intentionally cheap enough to run around each write step.
    pub fn revalidate(&self) -> Result<(), VaultError> {
        let current = Self::inspect(&self.canonical_path)?;
        if current != *self {
            return Err(VaultError::Stale);
        }
        Ok(())
    }

    /// Resolve a root-relative child while rejecting traversal, symlink and
    /// Windows reparse components.  Missing descendants are allowed so a
    /// caller can create a fixed child after validating its nearest existing
    /// ancestor.
    pub fn new_entry(&self, relative: &str) -> Result<PathBuf, VaultError> {
        validate_relative(relative)?;
        self.revalidate()?;

        let entry = self.canonical_path.join(relative);
        self.reject_link_components_path(Path::new(relative))?;

        let mut ancestor = entry.parent().ok_or(VaultError::InvalidEntry)?;
        loop {
            match std::fs::symlink_metadata(ancestor) {
                Ok(metadata) => {
                    if !is_plain_directory(&metadata) {
                        return Err(VaultError::InvalidEntry);
                    }
                    let canonical = ancestor
                        .canonicalize()
                        .map_err(|_| VaultError::InvalidEntry)?;
                    if !canonical.starts_with(&self.canonical_path) {
                        return Err(VaultError::InvalidEntry);
                    }
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    ancestor = ancestor.parent().ok_or(VaultError::InvalidEntry)?;
                }
                Err(_) => return Err(VaultError::InvalidEntry),
            }
        }
        Ok(entry)
    }

    /// Resolve an existing root-relative entry after checking every component
    /// and the final object's identity.  Callers use this for read/compare
    /// operations where a missing child is not acceptable.
    pub fn existing_entry(&self, relative: &str) -> Result<PathBuf, VaultError> {
        validate_relative(relative)?;
        self.existing_path(&self.canonical_path.join(relative))
    }

    /// Validate an already-constructed path under this vault.  This avoids
    /// converting platform paths to lossy strings during rollback/cleanup.
    pub fn existing_path(&self, path: &Path) -> Result<PathBuf, VaultError> {
        self.revalidate()?;
        let relative = path
            .strip_prefix(&self.canonical_path)
            .map_err(|_| VaultError::InvalidEntry)?;
        validate_relative_path(relative)?;
        self.reject_link_components_path(relative)?;

        let metadata = std::fs::symlink_metadata(path).map_err(|_| VaultError::InvalidEntry)?;
        if is_link_or_reparse(&metadata) {
            return Err(VaultError::InvalidEntry);
        }
        let canonical = path.canonicalize().map_err(|_| VaultError::InvalidEntry)?;
        if canonical == self.canonical_path || !canonical.starts_with(&self.canonical_path) {
            return Err(VaultError::InvalidEntry);
        }
        Ok(canonical)
    }

    /// Capture the identity of an existing regular file after the same root,
    /// ancestor, and reparse checks used by all other vault operations.
    pub(crate) fn existing_file_identity(&self, path: &Path) -> Result<EntryIdentity, VaultError> {
        let canonical = self.existing_path(path)?;
        let metadata = std::fs::symlink_metadata(path).map_err(|_| VaultError::InvalidEntry)?;
        if !metadata.is_file() {
            return Err(VaultError::InvalidEntry);
        }
        Ok(Self::entry_identity_from_metadata(&canonical, &metadata))
    }

    /// Capture a file identity from an already-open file's metadata.  The
    /// caller still revalidates the vault before using the token for cleanup.
    pub(crate) fn entry_identity_from_metadata(
        path: &Path,
        metadata: &std::fs::Metadata,
    ) -> EntryIdentity {
        EntryIdentity(file_identity(path, metadata))
    }

    fn reject_link_components_path(&self, relative: &Path) -> Result<(), VaultError> {
        let mut cursor = self.canonical_path.clone();
        for component in relative.components() {
            let Component::Normal(segment) = component else {
                return Err(VaultError::InvalidEntry);
            };
            cursor.push(segment);
            match std::fs::symlink_metadata(&cursor) {
                Ok(metadata) if is_link_or_reparse(&metadata) => {
                    return Err(VaultError::InvalidEntry)
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(VaultError::InvalidEntry),
            }
        }
        Ok(())
    }
}

/// Reject links/reparse points in the configured root spelling, not only at
/// the final directory. `symlink_metadata(path)` follows links in ancestors,
/// so checking just the final object would accept e.g. `alias/Knowledge`.
fn reject_root_link_components(path: &Path) -> Result<(), VaultError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| VaultError::InvalidRoot)?
            .join(path)
    };
    let mut cursor = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => cursor.push(prefix.as_os_str()),
            Component::RootDir => cursor.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return Err(VaultError::InvalidRoot),
            Component::Normal(segment) => {
                cursor.push(segment);
                let metadata =
                    std::fs::symlink_metadata(&cursor).map_err(|_| VaultError::InvalidRoot)?;
                if is_link_or_reparse(&metadata) {
                    return Err(VaultError::InvalidRoot);
                }
            }
        }
    }
    Ok(())
}

fn validate_relative(relative: &str) -> Result<(), VaultError> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || relative.split('/').any(str::is_empty)
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(VaultError::InvalidEntry);
    }
    Ok(())
}

fn validate_relative_path(relative: &Path) -> Result<(), VaultError> {
    let mut has_component = false;
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(VaultError::InvalidEntry);
        }
        has_component = true;
    }
    has_component.then_some(()).ok_or(VaultError::InvalidEntry)
}

fn is_plain_directory(metadata: &std::fs::Metadata) -> bool {
    metadata.is_dir() && !is_link_or_reparse(metadata)
}

fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT.  Using the std metadata extension
        // keeps junction detection in this app without another dependency.
        return metadata.file_attributes() & 0x400 != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn file_identity(path: &Path, metadata: &std::fs::Metadata) -> FileIdentity {
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        FileIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        FileIdentity::Windows {
            volume: metadata.volume_serial_number(),
            file_index: metadata.file_index(),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        FileIdentity::Path(path.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn captures_existing_directory_identity_without_mutating_children() {
        let root = tempfile::tempdir().unwrap();
        let identity = VaultIdentity::inspect(root.path()).unwrap();
        assert_eq!(
            identity.canonical_path(),
            root.path().canonicalize().unwrap()
        );
        assert!(!root.path().join("Inbox").exists());
        identity.revalidate().unwrap();
    }

    #[test]
    fn rejects_root_file_and_traversal_without_echoing_values() {
        let file = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(
            VaultIdentity::inspect(file.path()).err(),
            Some(VaultError::InvalidRoot)
        );
        let root = tempfile::tempdir().unwrap();
        let identity = VaultIdentity::inspect(root.path()).unwrap();
        let error = identity.new_entry("../outside").err().unwrap();
        assert_eq!(error, VaultError::InvalidEntry);
        assert!(!error.to_string().contains("outside"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_root_and_child_symlinks() {
        use std::os::unix::fs::symlink;

        let actual = tempfile::tempdir().unwrap();
        let parent = tempfile::tempdir().unwrap();
        let alias = parent.path().join("alias");
        symlink(actual.path(), &alias).unwrap();
        assert_eq!(
            VaultIdentity::inspect(&alias).err(),
            Some(VaultError::InvalidRoot)
        );

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join("Inbox")).unwrap();
        let identity = VaultIdentity::inspect(root.path()).unwrap();
        assert_eq!(
            identity.new_entry("Inbox/note.md").err(),
            Some(VaultError::InvalidEntry)
        );
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_root_ancestor_even_when_final_directory_is_plain() {
        use std::os::unix::fs::symlink;

        let actual = tempfile::tempdir().unwrap();
        let actual_root = actual.path().join("Knowledge");
        fs::create_dir(&actual_root).unwrap();
        let parent = tempfile::tempdir().unwrap();
        let alias = parent.path().join("alias");
        symlink(actual.path(), &alias).unwrap();

        assert_eq!(
            VaultIdentity::inspect(&alias.join("Knowledge")).err(),
            Some(VaultError::InvalidRoot)
        );
    }

    #[test]
    fn detects_replaced_root_identity() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("vault");
        fs::create_dir(&root).unwrap();
        let identity = VaultIdentity::inspect(&root).unwrap();
        fs::remove_dir(&root).unwrap();
        fs::create_dir(&root).unwrap();
        assert_eq!(identity.revalidate().err(), Some(VaultError::Stale));
    }

    #[test]
    fn existing_entry_returns_canonical_regular_child_and_rejects_missing() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("note.md"), "note").unwrap();
        let identity = VaultIdentity::inspect(root.path()).unwrap();
        let note = identity.existing_entry("note.md").unwrap();
        assert_eq!(note, root.path().join("note.md").canonicalize().unwrap());
        assert_eq!(
            identity.existing_entry("missing.md").err(),
            Some(VaultError::InvalidEntry)
        );
    }
}
