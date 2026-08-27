//! Knowledge vault identity and child-path boundary.
//!
//! A configured vault is an existing, ordinary directory.  Quick capture
//! snapshots its canonical path and filesystem identity during preview, then
//! requires the same identity immediately before every save-side mutation.
//! This is deliberately app-local: it protects the fixed Inbox writer without
//! widening the shared filesystem crate into a Knowledge-specific vault API.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const INVALID_ROOT: &str = "빠른 캡처 저장 위치를 사용할 수 없습니다";
const INVALID_ENTRY: &str = "Knowledge 항목 경로가 올바르지 않습니다";
const STALE_ROOT: &str = "빠른 캡처 미리보기가 오래되어 다시 확인하세요";

#[derive(Clone)]
pub struct VaultIdentity {
    canonical_path: PathBuf,
    marker: FileIdentity,
    // Keep the original directory object alive from preview through save.
    // This prevents Unix inode / Windows file-index reuse from making a
    // delete-and-recreate replacement compare equal to the preview snapshot.
    _root_lease: Arc<RootLease>,
}

impl PartialEq for VaultIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.canonical_path == other.canonical_path && self.marker == other.marker
    }
}

impl Eq for VaultIdentity {}

/// Filesystem identity captured for one regular file or directory. Rollback
/// and publication callers use it so a path replaced by another writer is
/// never treated as the object they previously validated.
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

/// Keeps a validated directory object alive while a caller performs a
/// path-based publication beneath it. Holding the object prevents its stable
/// filesystem identifier from being recycled after a delete-and-recreate race.
pub(crate) struct EntryLease {
    marker: EntryIdentity,
    _lease: RootLease,
}

impl EntryLease {
    pub(crate) fn identity(&self) -> &EntryIdentity {
        &self.marker
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

#[cfg(unix)]
struct RootLease {
    _file: std::fs::File,
}

#[cfg(windows)]
struct RootLease {
    handle: ::windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for RootLease {}
#[cfg(windows)]
unsafe impl Sync for RootLease {}

#[cfg(windows)]
impl Drop for RootLease {
    fn drop(&mut self) {
        unsafe {
            let _ = ::windows::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(not(any(unix, windows)))]
struct RootLease;

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
        let (root_lease, lease_marker) = open_root_lease(&canonical_path)?;
        if !lease_marker.is_usable() || lease_marker != marker {
            return Err(VaultError::InvalidRoot);
        }
        Ok(Self {
            canonical_path,
            marker,
            _root_lease: Arc::new(root_lease),
        })
    }

    pub(crate) fn canonical_path(&self) -> &Path {
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
        let metadata = std::fs::symlink_metadata(path).map_err(|_| VaultError::InvalidEntry)?;
        if is_link_or_reparse(&metadata) {
            return Err(VaultError::InvalidEntry);
        }
        // Reject links/reparse points in the caller's spelling before
        // canonicalization. On Windows `Path::canonicalize` commonly adds a
        // verbatim (`\\?\`) prefix, so the raw path cannot be compared
        // lexically with the canonical vault root first. Walking the original
        // spelling closes the ancestor-link boundary without depending on
        // those equivalent prefix forms.
        reject_root_link_components(path).map_err(|_| VaultError::InvalidEntry)?;
        let canonical = path.canonicalize().map_err(|_| VaultError::InvalidEntry)?;
        if canonical == self.canonical_path || !canonical.starts_with(&self.canonical_path) {
            return Err(VaultError::InvalidEntry);
        }
        let relative = canonical
            .strip_prefix(&self.canonical_path)
            .map_err(|_| VaultError::InvalidEntry)?;
        validate_relative_path(relative)?;
        self.reject_link_components_path(relative)?;
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

    /// Validate and hold an existing ordinary directory for the duration of a
    /// child publication. The returned lease must stay in scope until the
    /// path-based operation has completed.
    pub(crate) fn lease_existing_directory(&self, path: &Path) -> Result<EntryLease, VaultError> {
        let canonical = self.existing_path(path)?;
        let (lease, marker) = open_root_lease(&canonical).map_err(|_| VaultError::InvalidEntry)?;
        Ok(EntryLease {
            marker: EntryIdentity(marker),
            _lease: lease,
        })
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

/// Validate the existing portion of a root before the explicit root-selection
/// command creates a missing tail. Read-only previews always require
/// `VaultIdentity::inspect` on an already existing configured root.
pub(crate) fn validate_root_for_creation(path: &Path) -> Result<(), VaultError> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(VaultError::InvalidRoot);
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(VaultError::InvalidRoot);
    }
    let mut cursor = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => cursor.push(prefix.as_os_str()),
            Component::RootDir => cursor.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => return Err(VaultError::InvalidRoot),
            Component::Normal(segment) => {
                cursor.push(segment);
                match std::fs::symlink_metadata(&cursor) {
                    Ok(metadata) if is_link_or_reparse(&metadata) => {
                        return Err(VaultError::InvalidRoot)
                    }
                    Ok(metadata) if !metadata.is_dir() => return Err(VaultError::InvalidRoot),
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    Err(_) => return Err(VaultError::InvalidRoot),
                }
            }
        }
    }
    Ok(())
}

/// Publish a fully flushed file without replacing an existing target.
/// The caller owns cleanup of the private temporary sibling on failure.
pub(crate) fn publish_new_file(temporary: &Path, target: &Path) -> Result<(), std::io::Error> {
    if temporary.parent() != target.parent() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "publication paths must share a parent",
        ));
    }

    #[cfg(windows)]
    {
        use ::windows::core::PCWSTR;
        use ::windows::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, WIN32_ERROR,
        };
        use ::windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
        use std::os::windows::ffi::OsStrExt;

        let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        let result = unsafe {
            MoveFileExW(
                PCWSTR(temporary.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        };
        return match result {
            Ok(()) => Ok(()),
            Err(error)
                if WIN32_ERROR::from_error(&error).is_some_and(|code| {
                    code == ERROR_ACCESS_DENIED
                        || code == ERROR_ALREADY_EXISTS
                        || code == ERROR_FILE_EXISTS
                }) =>
            {
                Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "publication target already exists",
                ))
            }
            Err(error) => Err(std::io::Error::other(error)),
        };
    }

    #[cfg(not(windows))]
    {
        // A same-directory hard link gives no-replace publication on Unix.
        // A plain rename could replace a competing target.
        std::fs::hard_link(temporary, target)?;
        if sync_parent(target).is_err() {
            let _ = std::fs::remove_file(target);
            let _ = std::fs::remove_file(temporary);
            let _ = sync_parent(target);
            return Err(std::io::Error::other(
                "publication directory could not be synced",
            ));
        }
        if std::fs::remove_file(temporary).is_err() {
            let _ = std::fs::remove_file(target);
            let _ = sync_parent(target);
            return Err(std::io::Error::other(
                "publication temporary file could not be removed",
            ));
        }
        if sync_parent(target).is_err() {
            let _ = std::fs::remove_file(target);
            let _ = sync_parent(target);
            return Err(std::io::Error::other(
                "publication directory could not be synced",
            ));
        }
        Ok(())
    }
}

pub(crate) fn cleanup_file(vault: &VaultIdentity, path: &Path, expected: &EntryIdentity) {
    if vault.revalidate().is_err() {
        return;
    }
    let Ok(current) = vault.existing_file_identity(path) else {
        return;
    };
    if expected.matches(&current) {
        let _ = std::fs::remove_file(path);
        let _ = sync_parent(path);
    }
}

/// Remove a private temporary file only if it still has the object identity
/// captured by this process. A changed path is left untouched.
pub(crate) fn cleanup_file_by_identity(path: &Path, expected: &EntryIdentity) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return;
    };
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return;
    }
    let current = EntryIdentity(file_identity(path, &metadata));
    if expected.matches(&current) {
        let _ = std::fs::remove_file(path);
        let _ = sync_parent(path);
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
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn file_identity(path: &Path, metadata: &std::fs::Metadata) -> FileIdentity {
    let _ = (path, metadata);
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
        let Ok(handle) = open_windows_path_handle(path) else {
            return FileIdentity::Windows {
                volume: None,
                file_index: None,
            };
        };
        let identity = windows_handle_identity(handle);
        let close_result = unsafe { ::windows::Win32::Foundation::CloseHandle(handle) };
        if close_result.is_err() {
            return FileIdentity::Windows {
                volume: None,
                file_index: None,
            };
        }
        identity
    }
    #[cfg(not(any(unix, windows)))]
    {
        FileIdentity::Path(path.to_path_buf())
    }
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("publication path has no parent"))?;
    std::fs::File::open(parent).and_then(|directory| directory.sync_all())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
fn open_root_lease(path: &Path) -> Result<(RootLease, FileIdentity), VaultError> {
    use std::os::unix::fs::MetadataExt;

    let file = std::fs::File::open(path).map_err(|_| VaultError::InvalidRoot)?;
    let metadata = file.metadata().map_err(|_| VaultError::InvalidRoot)?;
    if !metadata.is_dir() {
        return Err(VaultError::InvalidRoot);
    }
    let identity = FileIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    Ok((RootLease { _file: file }, identity))
}

#[cfg(windows)]
fn open_root_lease(path: &Path) -> Result<(RootLease, FileIdentity), VaultError> {
    let handle = open_windows_path_handle(path).map_err(|_| VaultError::InvalidRoot)?;
    let identity = windows_handle_identity(handle);
    if !identity.is_usable() || !windows_handle_is_directory(handle) {
        unsafe {
            let _ = ::windows::Win32::Foundation::CloseHandle(handle);
        }
        return Err(VaultError::InvalidRoot);
    }
    Ok((RootLease { handle }, identity))
}

#[cfg(windows)]
fn open_windows_path_handle(
    path: &Path,
) -> Result<::windows::Win32::Foundation::HANDLE, ::windows::core::Error> {
    use ::windows::core::PCWSTR;
    use ::windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use std::os::windows::ffi::OsStrExt;

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
}

#[cfg(windows)]
fn windows_handle_identity(handle: ::windows::Win32::Foundation::HANDLE) -> FileIdentity {
    use ::windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle, &mut information) }.is_err() {
        return FileIdentity::Windows {
            volume: None,
            file_index: None,
        };
    }
    FileIdentity::Windows {
        volume: Some(information.dwVolumeSerialNumber),
        file_index: Some(
            (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        ),
    }
}

#[cfg(windows)]
fn windows_handle_is_directory(handle: ::windows::Win32::Foundation::HANDLE) -> bool {
    use ::windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(handle, &mut information) }.is_ok()
        && information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0
}

#[cfg(not(any(unix, windows)))]
fn open_root_lease(path: &Path) -> Result<(RootLease, FileIdentity), VaultError> {
    Ok((RootLease, FileIdentity::Path(path.to_path_buf())))
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
