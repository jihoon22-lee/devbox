//! Thin Tauri file commands. Encoding and line-ending policy lives in `core`;
//! this module owns filesystem metadata, canonical paths, and atomic replacement.

use crate::core::{
    encoding::{self, DecodeError, EncodeError, Encoding},
    guard,
    line_ending::{self, LineEnding},
};
use devbox_filesystem::{filesystem_identity, FilesystemIdentity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Serialize)]
pub struct OpenedFile {
    /// Canonical path, so later saves and watcher registrations use one identity.
    pub path: String,
    /// CodeMirror-compatible LF-only content.
    pub text: String,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    pub read_only: bool,
    pub size: u64,
    /// Epoch nanoseconds, as required by optimistic concurrency checks.
    pub mtime: i64,
    /// SHA-256 of the exact bytes read from disk. This supplements timestamp and
    /// size on filesystems whose timestamp precision is too coarse for conflict detection.
    pub content_hash: String,
    /// Identity of the exact regular file read for this snapshot. This never
    /// crosses the Tauri wire; it is used by native multi-file transactions to
    /// reject delete-and-recreate races that mtime/size/hash alone cannot
    /// prove safe.
    #[serde(skip)]
    pub(crate) identity: FilesystemIdentity,
    /// True when automatic detection had to use UTF-8 replacement characters.
    pub lossy: bool,
}

/// Tauri wire representation. Epoch nanoseconds are sent as decimal strings:
/// JavaScript `number` cannot represent an `i64` timestamp losslessly.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedFileWire {
    pub path: String,
    pub text: String,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    pub read_only: bool,
    pub size: u64,
    pub mtime_nanos: String,
    pub content_hash: String,
    pub lossy: bool,
}

impl From<OpenedFile> for OpenedFileWire {
    fn from(file: OpenedFile) -> Self {
        Self {
            path: file.path,
            text: file.text,
            encoding: file.encoding,
            line_ending: file.line_ending,
            read_only: file.read_only,
            size: file.size,
            mtime_nanos: file.mtime.to_string(),
            content_hash: file.content_hash,
            lossy: file.lossy,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SavedFile {
    pub path: String,
    pub mtime: i64,
    pub size: u64,
    pub content_hash: String,
    /// The replacement committed, but a best-effort durability/metadata refresh
    /// step failed. Callers must not report the save itself as failed.
    pub durability_warning: Option<String>,
    #[serde(skip)]
    pub(crate) identity: Option<FilesystemIdentity>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedFileWire {
    pub path: String,
    pub mtime_nanos: String,
    pub size: u64,
    pub content_hash: String,
    pub durability_warning: Option<String>,
}

impl From<SavedFile> for SavedFileWire {
    fn from(file: SavedFile) -> Self {
        Self {
            path: file.path,
            mtime_nanos: file.mtime.to_string(),
            size: file.size,
            content_hash: file.content_hash,
            durability_warning: file.durability_warning,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFileRequest {
    pub path: String,
    pub text: String,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
    /// Decimal epoch-nanoseconds string from the Tauri/JavaScript boundary.
    pub expected_mtime_nanos: String,
    pub expected_size: u64,
    pub expected_content_hash: String,
    /// A lossy fallback buffer cannot be saved. The caller must explicitly reopen
    /// with a supported encoding first, which returns `lossy = false`.
    pub source_lossy: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileActionRequest {
    pub path: String,
    pub expected_mtime_nanos: String,
    pub expected_size: u64,
    pub expected_content_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameFileRequest {
    #[serde(flatten)]
    pub file: FileActionRequest,
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RenamedFileWire {
    pub path: String,
    pub mtime_nanos: String,
    pub size: u64,
    pub content_hash: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ExpectedFileSnapshot<'a> {
    pub mtime: i64,
    pub size: u64,
    pub content_hash: &'a str,
    pub identity: Option<FilesystemIdentity>,
}

/// A backup that is still owned by one rename transaction. The identity and
/// content digest are retained so rollback never follows a path that was
/// replaced after the backup was created.
#[derive(Debug, Clone)]
pub(crate) struct CreatedBackup {
    pub(crate) path: PathBuf,
    pub(crate) identity: FilesystemIdentity,
    pub(crate) size: u64,
    pub(crate) content_hash: String,
}

/// Explicit request type for callers that prefer a struct over the command's
/// individual arguments. The Tauri command itself takes individual arguments so
/// `invoke("open_file", { path })` and `invoke("save_file", payload)` are natural.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFileRequest {
    pub path: String,
    /// `None` performs detection; `Some` performs strict explicit decoding.
    pub encoding: Option<Encoding>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateEncodingRequest {
    pub text: String,
    pub encoding: Encoding,
}

#[derive(Debug)]
pub enum FileError {
    Io {
        operation: &'static str,
        source: io::Error,
    },
    InvalidPath(String),
    InvalidMtime(String),
    InvalidFileName,
    DestinationExists,
    Conflict {
        expected_mtime: i64,
        actual_mtime: i64,
        expected_size: u64,
        actual_size: u64,
    },
    ReadOnly,
    LargeFile(u64),
    TooLargeToOpen(u64),
    LossySource,
    ChangedDuringRead,
    Decode(DecodeError),
    Encode(EncodeError),
    MetadataTime,
    BackupIntegrity,
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(f, "{operation} failed: {source}"),
            Self::InvalidPath(path) => write!(f, "not a regular file: {path}"),
            Self::InvalidMtime(value) => {
                write!(f, "invalid epoch nanoseconds decimal string: {value:?}")
            }
            Self::InvalidFileName => f.write_str("invalid sibling file name"),
            Self::DestinationExists => f.write_str("destination already exists"),
            Self::Conflict {
                expected_mtime,
                actual_mtime,
                expected_size,
                actual_size,
            } => write!(
                f,
                "file changed on disk (expected mtime={expected_mtime}, size={expected_size}; actual mtime={actual_mtime}, size={actual_size})"
            ),
            Self::ReadOnly => f.write_str("file is read-only"),
            Self::LargeFile(size) => write!(
                f,
                "file is larger than the 5 MiB editable limit ({size} bytes)"
            ),
            Self::TooLargeToOpen(size) => write!(
                f,
                "file is larger than the 64 MiB inspection limit ({size} bytes); use an external large-file tool"
            ),
            Self::LossySource => f.write_str(
                "lossy fallback content cannot be saved; reopen with an explicit encoding first",
            ),
            Self::ChangedDuringRead => {
                f.write_str("file changed on disk while it was being read")
            }
            Self::Decode(error) => write!(f, "decode failed: {error}"),
            Self::Encode(error) => write!(f, "encode failed: {error}"),
            Self::MetadataTime => f.write_str("file modification time is before the Unix epoch"),
            Self::BackupIntegrity => f.write_str("rename backup integrity check failed"),
        }
    }
}

impl std::error::Error for FileError {}

/// Opens a file from a path, applying canonicalization, encoding detection, LF
/// normalization, and the large/read-only policy.
pub fn open_path(path: &Path) -> Result<OpenedFile, FileError> {
    open_path_with_encoding(path, None)
}

/// Opens a file with either automatic detection or a strict user-selected
/// encoding. Explicit decoding is the only path from a lossy fallback buffer to
/// a saveable document.
pub fn open_path_with_encoding(
    path: &Path,
    selected_encoding: Option<Encoding>,
) -> Result<OpenedFile, FileError> {
    open_path_with_encoding_limit(path, selected_encoding, None)
}

/// Open a file with a consumer-specific byte cap. Multi-file LSP mutations use
/// this before decoding so a file that grows after its metadata preflight
/// cannot make the rename worker allocate the general 64 MiB open budget.
pub(crate) fn open_path_limited(path: &Path, max_bytes: u64) -> Result<OpenedFile, FileError> {
    open_path_with_encoding_limit(path, None, Some(max_bytes))
}

fn open_path_with_encoding_limit(
    path: &Path,
    selected_encoding: Option<Encoding>,
    max_bytes: Option<u64>,
) -> Result<OpenedFile, FileError> {
    let canonical = canonical_file(path)?;
    let (metadata, bytes) = read_stable_limited(&canonical, max_bytes)?;
    let decoded = match selected_encoding {
        Some(selected) => encoding::decode(&bytes, selected)
            .map(|text| encoding::DecodedText {
                text,
                encoding: selected,
                lossy: false,
            })
            .map_err(FileError::Decode)?,
        None => encoding::decode_detect(&bytes),
    };
    let line_ending = line_ending::detect(&decoded.text);
    let text = line_ending::normalize(&decoded.text);
    let read_only = metadata.permissions().readonly() || guard::is_large(metadata.len());

    Ok(OpenedFile {
        path: path_string(&canonical)?,
        text,
        encoding: decoded.encoding,
        line_ending,
        read_only,
        size: metadata.len(),
        mtime: modified_epoch_nanos(&metadata)?,
        content_hash: content_hash(&bytes),
        lossy: decoded.lossy,
        identity: filesystem_identity(&canonical, false).map_err(|source| FileError::Io {
            operation: "identify opened file",
            source,
        })?,
    })
}

/// Saves a normalized LF buffer after checking both optimistic-concurrency
/// fields. The write is performed to a sibling temporary file and replaced in a
/// single filesystem operation; the result contains a refreshed snapshot.
pub fn save_path(
    path: &Path,
    text: &str,
    encoding: Encoding,
    line_ending: LineEnding,
    expected: ExpectedFileSnapshot<'_>,
    source_lossy: bool,
) -> Result<SavedFile, FileError> {
    save_path_limited(
        path,
        text,
        encoding,
        line_ending,
        expected,
        source_lossy,
        None,
    )
}

/// Save with an optional read bound for multi-file transactions. The normal
/// editor save keeps the established inspection limit, while a rename worker
/// must not expand a preflighted small file into the larger general-open budget
/// if an external writer grows it while approval is pending.
pub(crate) fn save_path_limited(
    path: &Path,
    text: &str,
    encoding: Encoding,
    line_ending: LineEnding,
    expected: ExpectedFileSnapshot<'_>,
    source_lossy: bool,
    max_bytes: Option<u64>,
) -> Result<SavedFile, FileError> {
    if source_lossy {
        return Err(FileError::LossySource);
    }
    // Rename transactions pass canonical regular-file paths and a byte bound.
    // Requiring the final component to remain a non-reparse regular file at
    // each bounded-save checkpoint prevents a replacement symlink/reparse
    // point from redirecting an otherwise matching inode to another path.
    let bounded_path_identity = if max_bytes.is_some() {
        Some(
            filesystem_identity(path, false).map_err(|source| FileError::Io {
                operation: "identify bounded save target",
                source,
            })?,
        )
    } else {
        None
    };
    let canonical = canonical_file(path)?;
    let canonical_string = path_string(&canonical)?;
    if let Some(expected_identity) = bounded_path_identity {
        let actual_identity =
            filesystem_identity(path, false).map_err(|_| FileError::BackupIntegrity)?;
        if actual_identity != expected_identity {
            return Err(FileError::BackupIntegrity);
        }
    }
    let actual_identity =
        filesystem_identity(&canonical, false).map_err(|source| FileError::Io {
            operation: "identify file before save",
            source,
        })?;
    let (current, current_bytes) = read_stable_limited(&canonical, max_bytes)?;
    let actual_mtime = modified_epoch_nanos(&current)?;
    let actual_size = current.len();
    if expected
        .identity
        .is_some_and(|identity| identity != actual_identity)
        || actual_mtime != expected.mtime
        || actual_size != expected.size
        || content_hash(&current_bytes) != expected.content_hash
    {
        return Err(FileError::Conflict {
            expected_mtime: expected.mtime,
            actual_mtime,
            expected_size: expected.size,
            actual_size,
        });
    }
    if current.permissions().readonly() {
        return Err(FileError::ReadOnly);
    }
    if guard::is_large(actual_size) {
        return Err(FileError::LargeFile(actual_size));
    }

    let bytes = encode_for_save(text, encoding, line_ending)?;
    let saved_content_hash = content_hash(&bytes);
    let permissions = current.permissions();
    let (temporary, prepared_metadata) =
        write_sibling_temp(&canonical, &bytes, Some(&permissions))?;
    let fallback_mtime = modified_epoch_nanos(&prepared_metadata)?;
    let fallback_size = prepared_metadata.len();

    // The user may have edited the file while the temporary replacement was
    // being prepared. Recheck the exact bytes immediately before commit.
    let before_replace = match read_stable_limited(&canonical, max_bytes) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };
    let replacement_identity = match filesystem_identity(&canonical, false) {
        Ok(identity) => identity,
        Err(source) => {
            let _ = fs::remove_file(&temporary);
            return Err(FileError::Io {
                operation: "identify file before replacement",
                source,
            });
        }
    };
    let before_replace_mtime = match modified_epoch_nanos(&before_replace.0) {
        Ok(mtime) => mtime,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };
    let replacement_is_still_current = expected
        .identity
        .is_none_or(|identity| identity == replacement_identity)
        && before_replace_mtime == expected.mtime
        && before_replace.0.len() == expected.size
        && content_hash(&before_replace.1) == expected.content_hash;
    if !replacement_is_still_current {
        let _ = fs::remove_file(&temporary);
        return Err(FileError::Conflict {
            expected_mtime: expected.mtime,
            actual_mtime: before_replace_mtime,
            expected_size: expected.size,
            actual_size: before_replace.0.len(),
        });
    }

    if let Some(expected_identity) = bounded_path_identity {
        if !path_matches_identity(path, expected_identity) {
            let _ = fs::remove_file(&temporary);
            return Err(FileError::BackupIntegrity);
        }
    }

    if let Err(source) = replace_file(&temporary, &canonical) {
        let _ = fs::remove_file(&temporary);
        return Err(FileError::Io {
            operation: "replace file atomically",
            source,
        });
    }
    let mut warnings = Vec::new();
    if let Err(error) = sync_parent(&canonical) {
        warnings.push(error.to_string());
    }
    let (mtime, size) = match metadata(&canonical) {
        Ok(metadata) => match modified_epoch_nanos(&metadata) {
            Ok(mtime) => (mtime, metadata.len()),
            Err(error) => {
                warnings.push(error.to_string());
                (fallback_mtime, fallback_size)
            }
        },
        Err(error) => {
            warnings.push(error.to_string());
            (fallback_mtime, fallback_size)
        }
    };
    Ok(SavedFile {
        path: canonical_string,
        mtime,
        size,
        content_hash: saved_content_hash,
        durability_warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
        identity: filesystem_identity(&canonical, false).ok(),
    })
}

pub(crate) fn encode_for_save(
    text: &str,
    encoding: Encoding,
    line_ending: LineEnding,
) -> Result<Vec<u8>, FileError> {
    let restored = line_ending::restore(text, line_ending);
    encoding::encode(&restored, encoding).map_err(FileError::Encode)
}

fn create_directory_tree_no_follow(path: &Path) -> Result<(), FileError> {
    if path.as_os_str().is_empty() {
        return Err(FileError::InvalidPath(
            "empty private directory path".into(),
        ));
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(FileError::BackupIntegrity);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|source| FileError::Io {
                    operation: "create private directory component",
                    source,
                })?;
            }
            Err(source) => {
                return Err(FileError::Io {
                    operation: "inspect private directory component",
                    source,
                });
            }
        }
    }
    Ok(())
}

/// Create a private directory for one multi-file edit. The directory is kept
/// below app-local data rather than beside user files, preventing a backup
/// containing source/credential material from becoming a workspace artifact.
pub(crate) fn create_private_backup_dir(root: &Path, plan_id: &str) -> Result<PathBuf, FileError> {
    if plan_id.is_empty()
        || !plan_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(FileError::InvalidPath("invalid rename backup id".into()));
    }
    create_directory_tree_no_follow(root)?;
    filesystem_identity(root, true).map_err(|source| FileError::Io {
        operation: "identify rename backup root",
        source,
    })?;
    #[cfg(unix)]
    fs::set_permissions(root, std::os::unix::fs::PermissionsExt::from_mode(0o700)).map_err(
        |source| FileError::Io {
            operation: "protect rename backup root",
            source,
        },
    )?;
    let directory = root.join(plan_id);
    fs::create_dir(&directory).map_err(|source| FileError::Io {
        operation: "create rename backup directory",
        source,
    })?;
    #[cfg(unix)]
    fs::set_permissions(
        &directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .map_err(|source| FileError::Io {
        operation: "protect rename backup directory",
        source,
    })?;
    filesystem_identity(&directory, true).map_err(|source| FileError::Io {
        operation: "identify rename backup directory",
        source,
    })?;
    Ok(directory)
}

/// Create a durable backup for a multi-file edit in the transaction-private
/// directory. The caller owns cleanup of the returned path.
pub(crate) fn create_sibling_backup(
    backup_dir: &Path,
    _target: &Path,
    bytes: &[u8],
    permissions: &std::fs::Permissions,
    nonce: u128,
    index: usize,
) -> Result<CreatedBackup, FileError> {
    let parent = backup_dir;
    let parent_identity = filesystem_identity(parent, true).map_err(|source| FileError::Io {
        operation: "identify rename backup parent",
        source,
    })?;
    let process = std::process::id();
    let backup = parent.join(format!("backup-{process}-{nonce}-{index}.bak"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&backup)
        .map_err(|source| FileError::Io {
            operation: "create rename backup",
            source,
        })?;
    let result = (|| {
        file.set_permissions(permissions.clone())
            .map_err(|source| FileError::Io {
                operation: "preserve rename backup permissions",
                source,
            })?;
        file.write_all(bytes).map_err(|source| FileError::Io {
            operation: "write rename backup",
            source,
        })?;
        file.flush().map_err(|source| FileError::Io {
            operation: "flush rename backup",
            source,
        })?;
        file.sync_all().map_err(|source| FileError::Io {
            operation: "sync rename backup",
            source,
        })
    })();
    if let Err(error) = result {
        drop(file);
        let _ = fs::remove_file(&backup);
        return Err(error);
    }
    if filesystem_identity(parent, true).ok() != Some(parent_identity) {
        drop(file);
        let _ = fs::remove_file(&backup);
        return Err(FileError::BackupIntegrity);
    }
    drop(file);
    let identity = filesystem_identity(&backup, false).map_err(|source| FileError::Io {
        operation: "identify rename backup",
        source,
    })?;
    Ok(CreatedBackup {
        path: backup,
        identity,
        size: bytes.len() as u64,
        content_hash: content_hash(bytes),
    })
}

/// Restore a backup only while the target still has the caller's exact
/// snapshot. The check is repeated after the temporary restore bytes are
/// prepared, immediately before replacement, to close the rollback
/// check/write race with an external editor.
pub(crate) fn restore_sibling_backup_if_current(
    target: &Path,
    backup: &CreatedBackup,
    expected: Option<ExpectedFileSnapshot<'_>>,
) -> Result<(), FileError> {
    restore_sibling_backup_if_current_limited(target, backup, expected, None)
}

/// Bounded variant used by startup recovery. A journal is app-local state, but
/// it is still input that can be stale or corrupted; keep recovery from
/// allocating the general inspection budget for every recorded target/backup.
pub(crate) fn restore_sibling_backup_if_current_limited(
    target: &Path,
    backup: &CreatedBackup,
    expected: Option<ExpectedFileSnapshot<'_>>,
    max_bytes: Option<u64>,
) -> Result<(), FileError> {
    // The target must still be the regular path component approved by the
    // transaction. Do not canonicalize a replacement symlink/reparse point
    // into a different object before validating the snapshot.
    let target_identity =
        filesystem_identity(target, false).map_err(|_| FileError::BackupIntegrity)?;
    if let Some(expected) = expected {
        validate_file_snapshot_limited(target, expected, max_bytes)?;
    }
    let identity = filesystem_identity(&backup.path, false).map_err(|source| FileError::Io {
        operation: "identify rename backup before restore",
        source,
    })?;
    if identity != backup.identity {
        return Err(FileError::BackupIntegrity);
    }
    let (metadata, bytes) = read_stable_limited(&backup.path, max_bytes)?;
    if bytes.len() as u64 != backup.size || content_hash(&bytes) != backup.content_hash {
        return Err(FileError::BackupIntegrity);
    }
    let permissions = metadata.permissions();
    let (temporary, _) = write_sibling_temp(target, &bytes, Some(&permissions))?;
    if let Some(expected) = expected {
        if let Err(error) = validate_file_snapshot_limited(target, expected, max_bytes) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    if !path_matches_identity(target, target_identity) {
        let _ = fs::remove_file(&temporary);
        return Err(FileError::BackupIntegrity);
    }
    if let Err(source) = replace_file(&temporary, target) {
        let _ = fs::remove_file(&temporary);
        return Err(FileError::Io {
            operation: "restore rename backup atomically",
            source,
        });
    }
    sync_parent(target)
}

/// Parses the lossless decimal timestamp used by the Tauri wire contract.
/// Epoch timestamps produced by `open_path` are non-negative and fit in `i64`.
pub fn parse_epoch_nanos(value: &str) -> Result<i64, FileError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FileError::InvalidMtime(value.to_string()));
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| FileError::InvalidMtime(value.to_string()))?;
    i64::try_from(parsed).map_err(|_| FileError::InvalidMtime(value.to_string()))
}

fn expected_snapshot(request: &FileActionRequest) -> Result<ExpectedFileSnapshot<'_>, FileError> {
    Ok(ExpectedFileSnapshot {
        mtime: parse_epoch_nanos(&request.expected_mtime_nanos)?,
        size: request.expected_size,
        content_hash: &request.expected_content_hash,
        identity: None,
    })
}

fn validate_file_snapshot(
    path: &Path,
    expected: ExpectedFileSnapshot<'_>,
) -> Result<PathBuf, FileError> {
    validate_file_snapshot_limited(path, expected, None)
}

fn validate_file_snapshot_limited(
    path: &Path,
    expected: ExpectedFileSnapshot<'_>,
    max_bytes: Option<u64>,
) -> Result<PathBuf, FileError> {
    let canonical = canonical_file(path)?;
    let actual_identity =
        filesystem_identity(&canonical, false).map_err(|source| FileError::Io {
            operation: "identify file before mutation",
            source,
        })?;
    let (current, bytes) = read_stable_limited(&canonical, max_bytes)?;
    let actual_mtime = modified_epoch_nanos(&current)?;
    let actual_size = current.len();
    if expected
        .identity
        .is_some_and(|identity| identity != actual_identity)
        || actual_mtime != expected.mtime
        || actual_size != expected.size
        || content_hash(&bytes) != expected.content_hash
    {
        return Err(FileError::Conflict {
            expected_mtime: expected.mtime,
            actual_mtime,
            expected_size: expected.size,
            actual_size,
        });
    }
    Ok(canonical)
}

fn sibling_destination(source: &Path, new_name: &str) -> Result<PathBuf, FileError> {
    if new_name.is_empty()
        || new_name.trim() != new_name
        || new_name.contains('/')
        || new_name.contains('\\')
    {
        return Err(FileError::InvalidFileName);
    }
    let mut components = Path::new(new_name).components();
    let Some(std::path::Component::Normal(component)) = components.next() else {
        return Err(FileError::InvalidFileName);
    };
    if components.next().is_some() {
        return Err(FileError::InvalidFileName);
    }
    let parent = source.parent().ok_or(FileError::InvalidFileName)?;
    Ok(parent.join(component))
}

#[cfg(windows)]
fn rename_without_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::MoveFileW;

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe { MoveFileW(PCWSTR(source.as_ptr()), PCWSTR(destination.as_ptr())) }
        .map_err(io::Error::other)
}

#[cfg(not(windows))]
fn rename_without_replace(source: &Path, destination: &Path) -> io::Result<()> {
    // `std::fs::rename` may replace a concurrently-created destination on Unix.
    // A sibling hard link is create-new, so the move can never clobber data.
    fs::hard_link(source, destination)?;
    if let Err(error) = fs::remove_file(source) {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    Ok(())
}

pub fn rename_path(
    path: &Path,
    new_name: &str,
    expected: ExpectedFileSnapshot<'_>,
) -> Result<RenamedFileWire, FileError> {
    let canonical = canonical_file(path)?;
    let destination = sibling_destination(&canonical, new_name)?;
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err(FileError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(FileError::Io {
                operation: "inspect rename destination",
                source,
            })
        }
    }

    // Re-read immediately before the mutation, including a content digest, so
    // a stale tab can never rename a replaced file merely because metadata is equal.
    let canonical = validate_file_snapshot(&canonical, expected)?;
    rename_without_replace(&canonical, &destination).map_err(|source| FileError::Io {
        operation: "rename file without replacement",
        source,
    })?;
    // The namespace mutation already committed. A directory sync failure must
    // not make the frontend keep referring to the old path.
    let _ = sync_parent(&destination);
    Ok(RenamedFileWire {
        path: path_string(&destination)?,
        mtime_nanos: expected.mtime.to_string(),
        size: expected.size,
        content_hash: expected.content_hash.to_owned(),
    })
}

pub fn delete_path(path: &Path, expected: ExpectedFileSnapshot<'_>) -> Result<(), FileError> {
    let canonical = validate_file_snapshot(path, expected)?;
    fs::remove_file(&canonical).map_err(|source| FileError::Io {
        operation: "delete file",
        source,
    })?;
    // As with rename, deletion success is authoritative even when a best-effort
    // directory durability refresh is unavailable.
    let _ = sync_parent(&canonical);
    Ok(())
}

/// Tauri command for opening one file.
#[tauri::command]
pub async fn open_file(request: OpenFileRequest) -> Result<OpenedFileWire, String> {
    tauri::async_runtime::spawn_blocking(move || {
        open_path_with_encoding(Path::new(&request.path), request.encoding)
            .map(OpenedFileWire::from)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("open file worker failed: {error}"))?
}

/// Tauri command for saving one file. The timestamp is intentionally a decimal
/// string (`expectedMtimeNanos`) so JavaScript cannot round an epoch `i64`.
#[tauri::command]
pub async fn save_file(request: SaveFileRequest) -> Result<SavedFileWire, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let expected_mtime =
            parse_epoch_nanos(&request.expected_mtime_nanos).map_err(|error| error.to_string())?;
        save_path(
            Path::new(&request.path),
            &request.text,
            request.encoding,
            request.line_ending,
            ExpectedFileSnapshot {
                mtime: expected_mtime,
                size: request.expected_size,
                content_hash: &request.expected_content_hash,
                identity: None,
            },
            request.source_lossy,
        )
        .map(SavedFileWire::from)
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("save file worker failed: {error}"))?
}

/// Rename only the currently-open file, after an exact disk snapshot check.
/// Error strings are deliberately generic so arbitrary paths and OS details do
/// not cross the command boundary.
#[tauri::command]
pub async fn rename_file_action(request: RenameFileRequest) -> Result<RenamedFileWire, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let expected = expected_snapshot(&request.file)
            .map_err(|_| "파일 이름을 변경할 수 없습니다.".to_string())?;
        rename_path(Path::new(&request.file.path), &request.new_name, expected)
            .map_err(|_| "파일 이름을 변경할 수 없습니다.".to_string())
    })
    .await
    .map_err(|_| "파일 이름 변경 작업이 중단되었습니다.".to_string())?
}

/// Delete only the currently-open regular file after an exact snapshot check.
#[tauri::command]
pub async fn delete_file_action(request: FileActionRequest) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let expected =
            expected_snapshot(&request).map_err(|_| "파일을 삭제할 수 없습니다.".to_string())?;
        delete_path(Path::new(&request.path), expected)
            .map_err(|_| "파일을 삭제할 수 없습니다.".to_string())
    })
    .await
    .map_err(|_| "파일 삭제 작업이 중단되었습니다.".to_string())?
}

/// Reveal a canonical existing regular file without returning its path or the
/// platform opener's detailed error to the frontend.
#[tauri::command]
pub async fn reveal_file_action(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let canonical =
        canonical_file(Path::new(&path)).map_err(|_| "파일 위치를 열 수 없습니다.".to_string())?;
    app.opener()
        .reveal_item_in_dir(canonical)
        .map_err(|_| "파일 위치를 열 수 없습니다.".to_string())
}

/// Validates a prospective save encoding without touching the target file.
/// This is used by the status-bar conversion control so a metadata change is
/// only committed after CP949 (or another strict encoder) accepts the buffer.
#[tauri::command]
pub async fn validate_encoding(request: ValidateEncodingRequest) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        encoding::encode(&request.text, request.encoding)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("인코딩 검증 작업이 중단되었습니다: {error}"))?
}

pub fn canonical_file(path: &Path) -> Result<PathBuf, FileError> {
    let canonical = path.canonicalize().map_err(|source| FileError::Io {
        operation: "canonicalize path",
        source,
    })?;
    if !canonical.is_file() {
        return Err(FileError::InvalidPath(format!("{canonical:?}")));
    }
    filesystem_identity(&canonical, false)
        .map_err(|_| FileError::InvalidPath(format!("{canonical:?}")))?;
    Ok(canonical)
}

/// Read only the secure metadata size used to bound a multi-file operation
/// before any document contents are decoded or cloned.
pub(crate) fn preflight_size(path: &Path) -> Result<u64, FileError> {
    let canonical = canonical_file(path)?;
    let metadata = fs::metadata(canonical).map_err(|source| FileError::Io {
        operation: "read file size before rename",
        source,
    })?;
    Ok(metadata.len())
}

fn path_string(path: &Path) -> Result<String, FileError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| FileError::InvalidPath(format!("non-Unicode path: {path:?}")))
}

pub(crate) fn content_hash(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn path_matches_identity(path: &Path, expected: FilesystemIdentity) -> bool {
    filesystem_identity(path, false).ok() == Some(expected)
}

/// Reads bytes only when the metadata snapshot is stable for the entire read.
/// A concurrent writer is surfaced instead of producing a mixed buffer/hash.
pub(crate) fn read_stable_limited(
    path: &Path,
    max_bytes: Option<u64>,
) -> Result<(fs::Metadata, Vec<u8>), FileError> {
    // The identity helper opens the final component without following a
    // symlink/reparse point. Keep identities on both sides of the read so a
    // delete-and-recreate race cannot silently turn a snapshot into another
    // file with the same size and timestamp.
    let before_identity = filesystem_identity(path, false).map_err(|source| FileError::Io {
        operation: "identify file before read",
        source,
    })?;
    let before = metadata(path)?;
    if guard::should_reject_open(before.len()) {
        return Err(FileError::TooLargeToOpen(before.len()));
    }
    if max_bytes.is_some_and(|limit| before.len() > limit) {
        return Err(FileError::TooLargeToOpen(before.len()));
    }
    let mut file = File::open(path).map_err(|source| FileError::Io {
        operation: "read file",
        source,
    })?;
    let capacity = max_bytes
        .map(|limit| limit.min(before.len()))
        .unwrap_or(before.len())
        .try_into()
        .unwrap_or(usize::MAX);
    let mut bytes = Vec::with_capacity(capacity);
    let read_result = match max_bytes {
        Some(limit) => std::io::Read::by_ref(&mut file)
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes),
        None => file.read_to_end(&mut bytes),
    };
    read_result.map_err(|source| FileError::Io {
        operation: "read file",
        source,
    })?;
    let handle_metadata = file.metadata().map_err(|source| FileError::Io {
        operation: "read file metadata",
        source,
    })?;
    let after = metadata(path)?;
    let after_identity = filesystem_identity(path, false).map_err(|source| FileError::Io {
        operation: "identify file after read",
        source,
    })?;
    if before_identity != after_identity
        || max_bytes.is_some_and(|limit| after.len() > limit)
        || handle_metadata.len() != after.len()
        || modified_epoch_nanos(&handle_metadata)? != modified_epoch_nanos(&after)?
        || before.len() != after.len()
        || modified_epoch_nanos(&before)? != modified_epoch_nanos(&after)?
        || bytes.len() as u64 != after.len()
    {
        return Err(FileError::ChangedDuringRead);
    }
    Ok((after, bytes))
}

fn metadata(path: &Path) -> Result<fs::Metadata, FileError> {
    fs::metadata(path).map_err(|source| FileError::Io {
        operation: "read file metadata",
        source,
    })
}

pub(crate) fn modified_epoch_nanos(metadata: &fs::Metadata) -> Result<i64, FileError> {
    let duration = metadata
        .modified()
        .map_err(|_| FileError::MetadataTime)?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| FileError::MetadataTime)?;
    i64::try_from(duration.as_nanos()).map_err(|_| FileError::MetadataTime)
}

fn write_sibling_temp(
    target: &Path,
    bytes: &[u8],
    permissions: Option<&std::fs::Permissions>,
) -> Result<(PathBuf, fs::Metadata), FileError> {
    let parent = target
        .parent()
        .ok_or_else(|| FileError::InvalidPath(format!("{target:?}")))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let process = std::process::id();

    for attempt in 0..100u32 {
        let temporary = parent.join(format!(".code-pad-{process}-{nonce}-{attempt}.tmp"));
        let open = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match open {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(FileError::Io {
                    operation: "create temporary file",
                    source,
                })
            }
        };

        let result = (|| {
            if let Some(permissions) = permissions {
                file.set_permissions(permissions.clone())
                    .map_err(|source| FileError::Io {
                        operation: "preserve file permissions",
                        source,
                    })?;
            }
            file.write_all(bytes).map_err(|source| FileError::Io {
                operation: "write temporary file",
                source,
            })?;
            file.flush().map_err(|source| FileError::Io {
                operation: "flush temporary file",
                source,
            })?;
            file.sync_all().map_err(|source| FileError::Io {
                operation: "sync temporary file",
                source,
            })?;
            Ok::<(), FileError>(())
        })();
        if let Err(error) = result {
            drop(file);
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        let prepared_metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(source) => {
                drop(file);
                let _ = fs::remove_file(&temporary);
                return Err(FileError::Io {
                    operation: "read temporary file metadata",
                    source,
                });
            }
        };
        drop(file);
        return Ok((temporary, prepared_metadata));
    }

    Err(FileError::Io {
        operation: "create unique temporary file",
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "too many temporary-file collisions",
        ),
    })
}

/// Atomically publish a small app-private manifest. Unlike a normal document
/// save this helper does not require the destination to exist yet, but it still
/// uses a sibling temp file, flushes/syncs it, and syncs the parent directory.
pub(crate) fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), FileError> {
    let parent = path
        .parent()
        .ok_or_else(|| FileError::InvalidPath(format!("{path:?}")))?;
    let parent_identity = filesystem_identity(parent, true).map_err(|source| FileError::Io {
        operation: "identify private atomic-write parent",
        source,
    })?;
    // Keep the publish mode tied to the initial existence check. If the
    // journal was absent, a concurrent creator must win or make this write
    // fail; it must never be replaced by this transaction's stale decision.
    let target_identity = match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(FileError::BackupIntegrity);
            }
            Some(
                filesystem_identity(path, false).map_err(|source| FileError::Io {
                    operation: "identify private atomic-write target",
                    source,
                })?,
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(FileError::Io {
                operation: "inspect private atomic-write target",
                source,
            });
        }
    };
    let permissions = if target_identity.is_some() {
        Some(
            fs::metadata(path)
                .map_err(|source| FileError::Io {
                    operation: "read private atomic-write permissions",
                    source,
                })?
                .permissions(),
        )
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            Some(PermissionsExt::from_mode(0o600))
        }
        #[cfg(not(unix))]
        {
            // A newly created Windows file inherits the app-private
            // directory ACL. Copying directory `Permissions` onto the
            // file is not meaningful and can propagate the directory's
            // readonly attribute, making the journal unpublishable.
            None
        }
    };
    let (temporary, _) = write_sibling_temp(path, bytes, permissions.as_ref())?;
    // Never use overwrite-capable rename for the create branch: an attacker or
    // stale recovery process can create the journal path between the earlier
    // metadata check and this decision. The no-replace helper keeps that race
    // from clobbering an unrelated file.
    publish_private_temp(&temporary, path, target_identity)?;
    if filesystem_identity(parent, true).ok() != Some(parent_identity) {
        return Err(FileError::BackupIntegrity);
    }
    sync_parent(path)
}

fn publish_private_temp(
    temporary: &Path,
    target: &Path,
    target_identity: Option<FilesystemIdentity>,
) -> Result<(), FileError> {
    if target_identity.is_some_and(|expected| !path_matches_identity(target, expected)) {
        let _ = fs::remove_file(temporary);
        return Err(FileError::BackupIntegrity);
    }
    let result = if target_identity.is_some() {
        replace_file(temporary, target)
    } else {
        rename_without_replace(temporary, target)
    };
    if let Err(source) = result {
        let _ = fs::remove_file(temporary);
        return Err(FileError::Io {
            operation: "publish rename transaction journal",
            source,
        });
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temporary, target)
}

#[cfg(windows)]
fn replace_file(temporary: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        // ReplaceFile preserves the replaced file's ACLs and attributes while
        // atomically installing the prepared contents.
        ReplaceFileW(
            PCWSTR(target.as_ptr()),
            PCWSTR(temporary.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
    }
    .map_err(io::Error::other)
}

#[cfg(unix)]
fn sync_parent(target: &Path) -> Result<(), FileError> {
    let parent = target
        .parent()
        .ok_or_else(|| FileError::InvalidPath(format!("{target:?}")))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| FileError::Io {
            operation: "sync parent directory",
            source,
        })
}

#[cfg(not(unix))]
fn sync_parent(_target: &Path) -> Result<(), FileError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::encoding::EncodingKind;
    use std::fs;

    fn temp_file(name: &str, contents: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(name);
        fs::write(&path, contents).unwrap();
        (directory, path)
    }

    #[test]
    fn private_atomic_publish_never_overwrites_a_concurrent_path_owner() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("journal.json");

        // The caller approved a create, but another writer publishes first.
        let create_temp = directory.path().join("create.tmp");
        fs::write(&create_temp, b"stale create").unwrap();
        fs::write(&target, b"concurrent owner").unwrap();
        assert!(publish_private_temp(&create_temp, &target, None).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"concurrent owner");
        assert!(!create_temp.exists());

        // The caller approved an update, but that exact file is replaced
        // before publish. Allocate the replacement while the approved object
        // still exists so its filesystem identity is guaranteed to differ.
        let approved_identity = filesystem_identity(&target, false).unwrap();
        let replacement = directory.path().join("replacement.json");
        fs::write(&replacement, b"replacement owner").unwrap();
        fs::remove_file(&target).unwrap();
        fs::rename(&replacement, &target).unwrap();
        let update_temp = directory.path().join("update.tmp");
        fs::write(&update_temp, b"stale update").unwrap();
        assert!(matches!(
            publish_private_temp(&update_temp, &target, Some(approved_identity)),
            Err(FileError::BackupIntegrity)
        ));
        assert_eq!(fs::read(&target).unwrap(), b"replacement owner");
        assert!(!update_temp.exists());
    }

    fn snapshot(opened: &OpenedFile) -> ExpectedFileSnapshot<'_> {
        ExpectedFileSnapshot {
            mtime: opened.mtime,
            size: opened.size,
            content_hash: &opened.content_hash,
            identity: Some(opened.identity),
        }
    }

    #[test]
    fn open_returns_canonical_lf_buffer_and_snapshot() {
        let (_directory, path) = temp_file("sample.txt", b"one\r\ntwo\r\n");
        let opened = open_path(&path).unwrap();
        assert_eq!(opened.path, path.canonicalize().unwrap().to_string_lossy());
        assert_eq!(opened.text, "one\ntwo\n");
        assert_eq!(opened.line_ending, LineEnding::CrLf);
        assert_eq!(opened.size, 10);
        assert!(opened.mtime > 0);
        assert!(!opened.read_only);
    }

    #[test]
    fn timestamp_wire_parser_rejects_invalid_negative_and_overflow_values() {
        assert!(parse_epoch_nanos("").is_err());
        assert!(parse_epoch_nanos("-1").is_err());
        assert!(parse_epoch_nanos("12.3").is_err());
        assert!(parse_epoch_nanos("18446744073709551616").is_err());
        assert!(parse_epoch_nanos("9223372036854775808").is_err());
        assert_eq!(parse_epoch_nanos("9223372036854775807").unwrap(), i64::MAX);
    }

    #[test]
    fn open_wire_timestamp_roundtrips_into_save_without_number_conversion() {
        let (_directory, path) = temp_file("wire.txt", b"one\n");
        let opened = tauri::async_runtime::block_on(open_file(OpenFileRequest {
            path: path.to_string_lossy().into_owned(),
            encoding: None,
        }))
        .unwrap();
        assert!(opened.mtime_nanos.bytes().all(|byte| byte.is_ascii_digit()));
        let json = serde_json::to_value(&opened).unwrap();
        assert!(json.get("mtimeNanos").unwrap().is_string());
        assert!(json.get("mtime").is_none());
        let saved = tauri::async_runtime::block_on(save_file(SaveFileRequest {
            path: path.to_string_lossy().into_owned(),
            text: "two\n".into(),
            encoding: opened.encoding,
            line_ending: opened.line_ending,
            expected_mtime_nanos: opened.mtime_nanos.clone(),
            expected_size: opened.size,
            expected_content_hash: opened.content_hash.clone(),
            source_lossy: false,
        }))
        .unwrap();
        assert!(saved.mtime_nanos.bytes().all(|byte| byte.is_ascii_digit()));
        let saved_json = serde_json::to_value(&saved).unwrap();
        assert!(saved_json.get("mtimeNanos").unwrap().is_string());
        assert!(saved_json.get("mtime").is_none());
        assert_eq!(fs::read(&path).unwrap(), b"two\n");
    }

    #[test]
    fn save_restores_metadata_and_refreshes_snapshot() {
        let (_directory, path) = temp_file("sample.txt", b"one\r\ntwo\r\n");
        let opened = open_path(&path).unwrap();
        let saved = save_path(
            &path,
            "three\nfour\n",
            opened.encoding,
            opened.line_ending,
            snapshot(&opened),
            opened.lossy,
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"three\r\nfour\r\n");
        assert_eq!(saved.size, 13);
        let refreshed = open_path(&path).unwrap();
        assert_eq!(saved.mtime, refreshed.mtime);
        assert_eq!(saved.size, refreshed.size);
        assert_eq!(saved.content_hash, refreshed.content_hash);

        let second = save_path(
            &path,
            "five\nsix\n",
            refreshed.encoding,
            refreshed.line_ending,
            snapshot(&refreshed),
            refreshed.lossy,
        )
        .unwrap();
        assert_eq!(second.content_hash, open_path(&path).unwrap().content_hash);
    }

    #[test]
    fn save_rejects_external_snapshot_change() {
        let (_directory, path) = temp_file("sample.txt", b"one");
        let opened = open_path(&path).unwrap();
        fs::write(&path, b"changed").unwrap();
        let error = save_path(
            &path,
            "mine",
            opened.encoding,
            opened.line_ending,
            snapshot(&opened),
            opened.lossy,
        )
        .unwrap_err();
        assert!(matches!(error, FileError::Conflict { .. }));
        assert_eq!(fs::read(&path).unwrap(), b"changed");
    }

    #[test]
    fn rename_backup_restores_the_exact_bytes_and_keeps_rollback_retryable() {
        let (_directory, path) = temp_file("rename.txt", b"before\r\n");
        let opened = open_path(&path).unwrap();
        let backup_directory = tempfile::tempdir().unwrap();
        let backup = create_sibling_backup(
            backup_directory.path(),
            &path,
            &fs::read(&path).unwrap(),
            &fs::metadata(&path).unwrap().permissions(),
            17,
            0,
        )
        .unwrap();
        fs::write(&path, b"partial write").unwrap();

        restore_sibling_backup_if_current(&path, &backup, None).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"before\r\n");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().readonly(),
            opened.read_only
        );
        assert!(
            backup.path.exists(),
            "rollback keeps the backup until apply cleanup"
        );
        fs::remove_file(backup.path).unwrap();
    }

    #[test]
    fn rename_backup_rejects_tampered_or_replaced_backup_paths() {
        let (_directory, path) = temp_file("rename-integrity.txt", b"before");
        let backup_directory = tempfile::tempdir().unwrap();
        let original = fs::read(&path).unwrap();
        let permissions = fs::metadata(&path).unwrap().permissions();
        let backup = create_sibling_backup(
            backup_directory.path(),
            &path,
            &original,
            &permissions,
            23,
            0,
        )
        .unwrap();
        fs::write(&path, b"changed").unwrap();

        // Same backup identity, different bytes: the digest check must stop
        // rollback before it prepares a replacement.
        fs::write(&backup.path, b"tampered").unwrap();
        assert!(matches!(
            restore_sibling_backup_if_current(&path, &backup, None),
            Err(FileError::BackupIntegrity)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"changed");

        // A path replacement must fail even if an attacker restores the old
        // bytes in a newly-created file with the same pathname. Allocate the
        // replacement while the original still exists so the filesystem
        // cannot immediately recycle the original inode/file index and make
        // this identity regression test nondeterministic.
        let replacement = backup_directory.path().join("replacement.bak");
        fs::write(&replacement, &original).unwrap();
        fs::remove_file(&backup.path).unwrap();
        fs::rename(&replacement, &backup.path).unwrap();
        assert!(matches!(
            restore_sibling_backup_if_current(&path, &backup, None),
            Err(FileError::BackupIntegrity)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"changed");
    }

    #[test]
    fn save_rejects_unrepresentable_cp949_text_without_touching_file() {
        let (_directory, path) = temp_file("sample.txt", b"hello");
        let opened = open_path(&path).unwrap();
        let error = save_path(
            &path,
            "hello 🙂",
            Encoding::new(EncodingKind::Cp949, false),
            LineEnding::Lf,
            snapshot(&opened),
            opened.lossy,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            FileError::Encode(EncodeError::UnrepresentableCharacter)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"hello");
    }

    #[test]
    fn open_marks_large_file_read_only_at_strict_boundary() {
        let (_directory, path) = temp_file(
            "large.txt",
            &vec![b'a'; guard::MAX_EDITABLE_BYTES as usize + 1],
        );
        let opened = open_path(&path).unwrap();
        assert!(opened.read_only);
    }

    #[test]
    fn open_rejects_huge_sparse_file_before_reading_contents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("huge.log");
        let file = fs::File::create(&path).unwrap();
        file.set_len(guard::MAX_OPENABLE_BYTES + 1).unwrap();
        let error = open_path(&path).unwrap_err();
        assert!(matches!(error, FileError::TooLargeToOpen(_)));
    }

    #[test]
    fn save_rejects_lossy_fallback_without_touching_file() {
        let (_directory, path) = temp_file("lossy.txt", &[0xFF, 0xFE, 0xFA]);
        let opened = open_path(&path).unwrap();
        assert!(opened.lossy);
        let error = save_path(
            &path,
            &opened.text,
            opened.encoding,
            opened.line_ending,
            snapshot(&opened),
            true,
        )
        .unwrap_err();
        assert!(matches!(error, FileError::LossySource));
        assert_eq!(fs::read(&path).unwrap(), [0xFF, 0xFE, 0xFA]);
    }

    #[test]
    fn content_hash_detects_same_metadata_snapshot_change() {
        let (_directory, path) = temp_file("hash.txt", b"one");
        let opened = open_path(&path).unwrap();
        let mut wrong_hash = opened.content_hash.clone();
        wrong_hash.replace_range(
            ..1,
            if wrong_hash.starts_with('0') {
                "1"
            } else {
                "0"
            },
        );
        let error = save_path(
            &path,
            "two",
            opened.encoding,
            opened.line_ending,
            ExpectedFileSnapshot {
                mtime: opened.mtime,
                size: opened.size,
                content_hash: &wrong_hash,
                identity: Some(opened.identity),
            },
            false,
        )
        .unwrap_err();
        assert!(matches!(error, FileError::Conflict { .. }));
        assert_eq!(fs::read(&path).unwrap(), b"one");
    }

    #[test]
    fn final_identity_guard_rejects_a_same_path_regular_file_replacement() {
        let (_directory, path) = temp_file("identity-guard.txt", b"approved");
        let expected = filesystem_identity(&path, false).unwrap();
        let moved = path.with_file_name("identity-guard-original.txt");
        fs::rename(&path, &moved).unwrap();
        fs::write(&path, b"approved").unwrap();

        assert!(!path_matches_identity(&path, expected));
        assert_eq!(fs::read(&moved).unwrap(), b"approved");
        assert_eq!(fs::read(&path).unwrap(), b"approved");
    }

    #[test]
    fn rename_moves_only_to_a_new_sibling_and_preserves_the_snapshot() {
        let (_directory, path) = temp_file("before.txt", b"one");
        let opened = open_path(&path).unwrap();
        let renamed = rename_path(&path, "after.txt", snapshot(&opened)).unwrap();
        let destination = path.with_file_name("after.txt");

        assert!(!path.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"one");
        assert_eq!(
            Path::new(&renamed.path),
            fs::canonicalize(&destination).unwrap()
        );
        assert_eq!(renamed.mtime_nanos, opened.mtime.to_string());
        assert_eq!(renamed.size, opened.size);
        assert_eq!(renamed.content_hash, opened.content_hash);
    }

    #[test]
    fn rename_rejects_traversal_and_existing_destinations_without_touching_files() {
        let (_directory, path) = temp_file("before.txt", b"source");
        let destination = path.with_file_name("after.txt");
        fs::write(&destination, b"destination").unwrap();
        let opened = open_path(&path).unwrap();

        for invalid in [
            "",
            ".",
            "..",
            "../escape.txt",
            "sub/file.txt",
            "sub\\file.txt",
            " padded.txt",
        ] {
            assert!(matches!(
                rename_path(&path, invalid, snapshot(&opened)),
                Err(FileError::InvalidFileName)
            ));
        }
        assert!(matches!(
            rename_path(&path, "after.txt", snapshot(&opened)),
            Err(FileError::DestinationExists)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"source");
        assert_eq!(fs::read(&destination).unwrap(), b"destination");
    }

    #[test]
    fn rename_and_delete_reject_stale_content_snapshots() {
        let (_directory, path) = temp_file("before.txt", b"one");
        let opened = open_path(&path).unwrap();
        fs::write(&path, b"changed").unwrap();

        assert!(matches!(
            rename_path(&path, "after.txt", snapshot(&opened)),
            Err(FileError::Conflict { .. })
        ));
        assert!(matches!(
            delete_path(&path, snapshot(&opened)),
            Err(FileError::Conflict { .. })
        ));
        assert_eq!(fs::read(&path).unwrap(), b"changed");
    }

    #[test]
    fn delete_removes_only_the_snapshot_matched_regular_file() {
        let (_directory, path) = temp_file("delete.txt", b"one");
        let opened = open_path(&path).unwrap();
        delete_path(&path, snapshot(&opened)).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn mutation_commands_do_not_echo_untrusted_paths_in_errors() {
        let untrusted = "/secret/example.txt";
        let request = FileActionRequest {
            path: untrusted.into(),
            expected_mtime_nanos: "1".into(),
            expected_size: 1,
            expected_content_hash: "hash".into(),
        };
        let error = tauri::async_runtime::block_on(delete_file_action(request)).unwrap_err();
        assert_eq!(error, "파일을 삭제할 수 없습니다.");
        assert!(!error.contains(untrusted));
    }
}
