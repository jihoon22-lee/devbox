//! Thin Tauri file commands. Encoding and line-ending policy lives in `core`;
//! this module owns filesystem metadata, canonical paths, and atomic replacement.

use crate::core::{
    encoding::{self, DecodeError, EncodeError, Encoding},
    guard,
    line_ending::{self, LineEnding},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[derive(Debug, Clone, Copy)]
pub struct ExpectedFileSnapshot<'a> {
    pub mtime: i64,
    pub size: u64,
    pub content_hash: &'a str,
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
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { operation, source } => write!(f, "{operation} failed: {source}"),
            Self::InvalidPath(path) => write!(f, "not a regular file: {path}"),
            Self::InvalidMtime(value) => {
                write!(f, "invalid epoch nanoseconds decimal string: {value:?}")
            }
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
    let canonical = canonical_file(path)?;
    let (metadata, bytes) = read_stable(&canonical)?;
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
    if source_lossy {
        return Err(FileError::LossySource);
    }
    let canonical = canonical_file(path)?;
    let canonical_string = path_string(&canonical)?;
    let (current, current_bytes) = read_stable(&canonical)?;
    let actual_mtime = modified_epoch_nanos(&current)?;
    let actual_size = current.len();
    if actual_mtime != expected.mtime
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

    let restored = line_ending::restore(text, line_ending);
    let bytes = encoding::encode(&restored, encoding).map_err(FileError::Encode)?;
    let saved_content_hash = content_hash(&bytes);
    let permissions = current.permissions();
    let (temporary, prepared_metadata) = write_sibling_temp(&canonical, &bytes, &permissions)?;
    let fallback_mtime = modified_epoch_nanos(&prepared_metadata)?;
    let fallback_size = prepared_metadata.len();

    // The user may have edited the file while the temporary replacement was
    // being prepared. Recheck the exact bytes immediately before commit.
    let before_replace = read_stable(&canonical)?;
    let replacement_is_still_current = modified_epoch_nanos(&before_replace.0)? == expected.mtime
        && before_replace.0.len() == expected.size
        && content_hash(&before_replace.1) == expected.content_hash;
    if !replacement_is_still_current {
        let _ = fs::remove_file(&temporary);
        return Err(FileError::Conflict {
            expected_mtime: expected.mtime,
            actual_mtime: modified_epoch_nanos(&before_replace.0)?,
            expected_size: expected.size,
            actual_size: before_replace.0.len(),
        });
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
    })
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
            },
            request.source_lossy,
        )
        .map(SavedFileWire::from)
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("save file worker failed: {error}"))?
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
    Ok(canonical)
}

fn path_string(path: &Path) -> Result<String, FileError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| FileError::InvalidPath(format!("non-Unicode path: {path:?}")))
}

fn content_hash(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

/// Reads bytes only when the metadata snapshot is stable for the entire read.
/// A concurrent writer is surfaced instead of producing a mixed buffer/hash.
fn read_stable(path: &Path) -> Result<(fs::Metadata, Vec<u8>), FileError> {
    let before = metadata(path)?;
    if guard::should_reject_open(before.len()) {
        return Err(FileError::TooLargeToOpen(before.len()));
    }
    let bytes = fs::read(path).map_err(|source| FileError::Io {
        operation: "read file",
        source,
    })?;
    let after = metadata(path)?;
    if before.len() != after.len()
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

fn modified_epoch_nanos(metadata: &fs::Metadata) -> Result<i64, FileError> {
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
    permissions: &std::fs::Permissions,
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
            file.set_permissions(permissions.clone())
                .map_err(|source| FileError::Io {
                    operation: "preserve file permissions",
                    source,
                })?;
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
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        let prepared_metadata = file.metadata().map_err(|source| FileError::Io {
            operation: "read temporary file metadata",
            source,
        })?;
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

    fn snapshot(opened: &OpenedFile) -> ExpectedFileSnapshot<'_> {
        ExpectedFileSnapshot {
            mtime: opened.mtime,
            size: opened.size,
            content_hash: &opened.content_hash,
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
            },
            false,
        )
        .unwrap_err();
        assert!(matches!(error, FileError::Conflict { .. }));
        assert_eq!(fs::read(&path).unwrap(), b"one");
    }
}
