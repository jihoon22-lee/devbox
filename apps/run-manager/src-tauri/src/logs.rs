//! App-owned stdout/stderr log streams with bounded rotation and lossless tailing.
//!
//! A stream assigns every byte a monotonically increasing logical offset. The
//! on-disk segment ranges use half-open intervals (`[start, end)`), so a
//! cursor can be resumed at `end` without repeating the last byte from the
//! preceding segment. Each stream has its own async mutex; stdout and stderr
//! can therefore be drained independently while each stream's append/read
//! snapshot remains serialized.

use crate::platform;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// The production segment size required by the Run Manager specification.
pub const DEFAULT_SEGMENT_BYTES: u64 = 10 * 1024 * 1024;
/// The production number of retained segments for each stream.
pub const DEFAULT_MAX_SEGMENTS: usize = 5;
/// The maximum number of bytes one tail response can return.
pub const MAX_TAIL_BYTES: usize = 256 * 1024;

const LOG_RELATIVE_ROOT: &str = "logs/runs";
const SEGMENT_EXTENSION: &str = ".log";
const MANIFEST_SCHEMA_VERSION: u32 = 1;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;

/// The two process output streams are separate files and separate locks.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl LogStream {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

impl fmt::Display for LogStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Limits are configurable so pure Rust tests can exercise rotation with a
/// few bytes while production uses the fixed 10 MiB x5 policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogLimits {
    pub segment_bytes: u64,
    pub max_segments: usize,
}

impl Default for LogLimits {
    fn default() -> Self {
        Self {
            segment_bytes: DEFAULT_SEGMENT_BYTES,
            max_segments: DEFAULT_MAX_SEGMENTS,
        }
    }
}

impl LogLimits {
    fn validate(self) -> Result<Self, LogError> {
        if self.segment_bytes == 0 || self.max_segments == 0 {
            return Err(LogError::InvalidLimits);
        }
        Ok(self)
    }
}

/// A tail request. `cursor` is a decimal logical byte offset, never a native
/// file offset; `None` starts at the oldest currently retained byte.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TailRequest {
    pub stream: LogStream,
    pub cursor: Option<String>,
    pub max_bytes: usize,
}

/// A byte-preserving tail response. Cursor fields are strings because the
/// frontend must not round-trip logical offsets through a JavaScript number.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TailResponse {
    pub data: Vec<u8>,
    pub retained_start_offset: String,
    pub next_cursor: String,
    pub truncated: bool,
}

/// Errors intentionally avoid embedding user-controlled absolute paths in
/// their display text. This keeps command-layer errors safe to show in UI or
/// logs while preserving enough structure for retry/diagnostic decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogError {
    InvalidLimits,
    InvalidRunId,
    InvalidLogPath,
    PathOutsideRoot,
    RunDirectoryMissing,
    InvalidCursor,
    CursorOverflow,
    SegmentCorrupt,
    SegmentConflict,
    ManifestCorrupt,
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
}

impl LogError {
    fn io(operation: &'static str, error: io::Error) -> Self {
        Self::Io {
            operation,
            kind: error.kind(),
        }
    }
}

impl fmt::Display for LogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimits => formatter.write_str("invalid log rotation limits"),
            Self::InvalidRunId => formatter.write_str("invalid run identifier"),
            Self::InvalidLogPath => formatter.write_str("invalid log path"),
            Self::PathOutsideRoot => formatter.write_str("log path is outside the app data root"),
            Self::RunDirectoryMissing => formatter.write_str("run log directory is missing"),
            Self::InvalidCursor => formatter.write_str("cursor must be a decimal string"),
            Self::CursorOverflow => formatter.write_str("cursor is outside the supported range"),
            Self::SegmentCorrupt => formatter.write_str("log segment metadata is invalid"),
            Self::SegmentConflict => formatter.write_str("log segment path already exists"),
            Self::ManifestCorrupt => formatter.write_str("log manifest metadata is invalid"),
            Self::Io { operation, kind } => {
                write!(formatter, "log I/O failed while {operation}: {kind}")
            }
        }
    }
}

impl std::error::Error for LogError {}

/// Validate an app-generated run identifier before it participates in a path.
/// UUIDs and similarly conservative identifiers are accepted; separators,
/// dot components, NUL bytes, and all other path syntax are rejected.
pub fn validate_run_id(run_id: &str) -> Result<(), LogError> {
    if run_id.is_empty()
        || run_id == "."
        || run_id == ".."
        || run_id
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'))
    {
        return Err(LogError::InvalidRunId);
    }
    Ok(())
}

/// The relative DB identifier stored for a run's logs.
pub fn relative_log_dir(run_id: &str) -> Result<PathBuf, LogError> {
    validate_run_id(run_id)?;
    Ok(Path::new(LOG_RELATIVE_ROOT).join(run_id))
}

/// Resolve and validate the app-owned run directory referenced by `log_dir`.
///
/// The caller supplies the app-local data root and the relative value read
/// from SQLite. Requiring the exact generated `logs/runs/<run_id>` shape keeps
/// a valid-looking path for another run from being used accidentally. The
/// canonical containment check also rejects a pre-existing symlink that points
/// outside the app data root.
pub fn resolve_run_directory(
    app_data_root: impl AsRef<Path>,
    log_dir: impl AsRef<Path>,
    run_id: &str,
) -> Result<PathBuf, LogError> {
    validate_run_id(run_id)?;
    let log_dir = log_dir.as_ref();
    let expected_relative = relative_log_dir(run_id)?;
    if log_dir.is_absolute()
        || log_dir.components().any(|component| {
            matches!(
                component,
                Component::RootDir
                    | Component::Prefix(_)
                    | Component::ParentDir
                    | Component::CurDir
            )
        })
        || log_dir != expected_relative
    {
        return Err(LogError::InvalidLogPath);
    }

    let root = fs::canonicalize(app_data_root).map_err(|_| LogError::RunDirectoryMissing)?;
    let candidate = root.join(log_dir);
    reject_symlink_components(&root, log_dir)?;
    let resolved = fs::canonicalize(&candidate).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            LogError::RunDirectoryMissing
        } else {
            LogError::io("resolving run directory", error)
        }
    })?;
    if !resolved.starts_with(&root)
        || resolved.file_name().and_then(|name| name.to_str()) != Some(run_id)
    {
        return Err(LogError::PathOutsideRoot);
    }
    Ok(resolved)
}

fn reject_symlink_components(root: &Path, relative: &Path) -> Result<(), LogError> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(LogError::InvalidLogPath);
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(LogError::PathOutsideRoot);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(LogError::io("validating log path", error)),
        }
    }
    Ok(())
}

/// One run's two independently locked output streams.
#[derive(Clone)]
pub struct LogStreams {
    run_id: String,
    directory: Arc<PathBuf>,
    stdout: LogStreamHandle,
    stderr: LogStreamHandle,
}

/// Alias for callers that prefer the storage-oriented name.
pub type LogStreamStore = LogStreams;

impl LogStreams {
    /// Open or create `app_data_root/logs/runs/<run_id>` and reconstruct any
    /// valid retained segments already present there.
    pub fn open(
        app_data_root: impl AsRef<Path>,
        run_id: impl Into<String>,
        limits: LogLimits,
    ) -> Result<Self, LogError> {
        let limits = limits.validate()?;
        let run_id = run_id.into();
        validate_run_id(&run_id)?;
        fs::create_dir_all(app_data_root.as_ref())
            .map_err(|error| LogError::io("creating app data root", error))?;
        let root =
            fs::canonicalize(app_data_root.as_ref()).map_err(|_| LogError::RunDirectoryMissing)?;
        let relative = relative_log_dir(&run_id)?;
        reject_symlink_components(&root, &relative)?;
        let directory = root.join(&relative);
        fs::create_dir_all(&directory)
            .map_err(|error| LogError::io("creating run log directory", error))?;
        let directory = resolve_run_directory(&root, &relative, &run_id)?;
        let directory = Arc::new(directory);

        let stdout = LogStreamHandle::new(StreamState::load(
            LogStream::Stdout,
            Arc::clone(&directory),
            &run_id,
            limits,
        )?);
        let stderr = LogStreamHandle::new(StreamState::load(
            LogStream::Stderr,
            Arc::clone(&directory),
            &run_id,
            limits,
        )?);

        Ok(Self {
            run_id,
            directory,
            stdout,
            stderr,
        })
    }

    /// Open with the production 10 MiB x5 limits.
    pub fn open_default(
        app_data_root: impl AsRef<Path>,
        run_id: impl Into<String>,
    ) -> Result<Self, LogError> {
        Self::open(app_data_root, run_id, LogLimits::default())
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn run_directory(&self) -> &Path {
        self.directory.as_path()
    }

    pub fn log_dir(&self) -> PathBuf {
        Path::new(LOG_RELATIVE_ROOT).join(&self.run_id)
    }

    /// Obtain a cloneable handle with an async lock dedicated to one stream.
    pub fn handle(&self, stream: LogStream) -> LogStreamHandle {
        match stream {
            LogStream::Stdout => self.stdout.clone(),
            LogStream::Stderr => self.stderr.clone(),
        }
    }

    pub async fn append(&self, stream: LogStream, bytes: &[u8]) -> Result<String, LogError> {
        self.handle(stream).append(bytes).await
    }

    pub async fn tail(&self, request: TailRequest) -> Result<TailResponse, LogError> {
        self.handle(request.stream).tail(request).await
    }

    pub async fn tail_log(
        &self,
        stream: LogStream,
        cursor: Option<&str>,
        max_bytes: usize,
    ) -> Result<TailResponse, LogError> {
        self.tail(TailRequest {
            stream,
            cursor: cursor.map(str::to_string),
            max_bytes,
        })
        .await
    }
}

/// A cloneable process-adapter-facing stream handle. All methods take the
/// stream mutex before touching segment metadata or bytes on disk.
#[derive(Clone)]
pub struct LogStreamHandle {
    stream: LogStream,
    state: Arc<Mutex<StreamState>>,
}

impl LogStreamHandle {
    fn new(state: StreamState) -> Self {
        Self {
            stream: state.stream,
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn stream(&self) -> LogStream {
        self.stream
    }

    /// Append bytes and return the next logical cursor as a canonical decimal
    /// string. A write error leaves the in-memory range unchanged; successful
    /// chunks before a later rotation error remain durable by design.
    pub async fn append(&self, bytes: &[u8]) -> Result<String, LogError> {
        let mut state = self.state.lock().await;
        let next_offset = state.append(bytes)?;
        Ok(next_offset.to_string())
    }

    /// Tail at a snapshot end boundary taken while holding this stream's
    /// mutex. Appenders cannot advance or rotate the stream until all bytes in
    /// this response have been read, so the response cannot mix generations.
    pub async fn tail(&self, request: TailRequest) -> Result<TailResponse, LogError> {
        if request.stream != self.stream {
            return Err(LogError::InvalidLogPath);
        }
        let state = self.state.lock().await;
        state.tail(&request)
    }
}

#[derive(Debug, Clone)]
struct Segment {
    generation: u64,
    start: u64,
    end: u64,
    checksum: u64,
    path: PathBuf,
}

impl Segment {
    fn len(&self) -> u64 {
        self.end - self.start
    }
}

fn checksum_bytes(bytes: &[u8]) -> u64 {
    checksum_extend(FNV_OFFSET_BASIS, bytes)
}

fn checksum_extend(mut checksum: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        checksum ^= u64::from(*byte);
        checksum = checksum.wrapping_mul(FNV_PRIME);
    }
    checksum
}

fn checksum_hex(checksum: u64) -> String {
    format!("{checksum:016x}")
}

fn checksum_file(path: &Path) -> Result<u64, LogError> {
    let mut file = File::open(path).map_err(|error| LogError::io("reading log segment", error))?;
    let mut checksum = FNV_OFFSET_BASIS;
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| LogError::io("reading log segment", error))?;
        if read == 0 {
            break;
        }
        checksum = checksum_extend(checksum, &buffer[..read]);
    }
    Ok(checksum)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ManifestSegment {
    file_name: String,
    start_offset: u64,
    end_offset: u64,
    byte_length: u64,
    checksum: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ManifestUnsigned {
    schema_version: u32,
    run_id: String,
    stream: LogStream,
    retained_start_offset: u64,
    next_offset: u64,
    rotation_generation: u64,
    segments: Vec<ManifestSegment>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Manifest {
    #[serde(flatten)]
    unsigned: ManifestUnsigned,
    checksum: String,
}

struct StreamState {
    stream: LogStream,
    directory: Arc<PathBuf>,
    run_id: String,
    limits: LogLimits,
    segments: Vec<Segment>,
    next_offset: u64,
    next_generation: u64,
}

impl StreamState {
    fn load(
        stream: LogStream,
        directory: Arc<PathBuf>,
        run_id: &str,
        limits: LogLimits,
    ) -> Result<Self, LogError> {
        let mut segments = Vec::new();
        let entries = fs::read_dir(directory.as_path())
            .map_err(|error| LogError::io("listing log segments", error))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| LogError::io("reading log directory entry", error))?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let Some((generation, start, end)) = parse_segment_name(stream, name)? else {
                continue;
            };
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| LogError::io("reading log segment", error))?;
            if !metadata.file_type().is_file() {
                if metadata.file_type().is_symlink() {
                    return Err(LogError::PathOutsideRoot);
                }
                return Err(LogError::SegmentCorrupt);
            }
            let encoded_len = end.checked_sub(start).ok_or(LogError::SegmentCorrupt)?;
            let actual_len = metadata.len();
            if actual_len < encoded_len || actual_len > limits.segment_bytes {
                return Err(LogError::SegmentCorrupt);
            }

            // A process can crash after the bytes reach disk but before the
            // range-encoded filename is renamed and the manifest is swapped.
            // Recover that durable suffix instead of discarding it or treating
            // a valid crash window as an unrecoverable corrupt segment.
            let (path, end) = if actual_len == encoded_len {
                (path, end)
            } else {
                let recovered_end = start
                    .checked_add(actual_len)
                    .ok_or(LogError::CursorOverflow)?;
                let recovered_path = directory.as_path().join(format!(
                    "{}.g{generation}.o{start}-{recovered_end}{SEGMENT_EXTENSION}",
                    stream.as_str()
                ));
                if recovered_path != path {
                    match fs::symlink_metadata(&recovered_path) {
                        Ok(metadata) if metadata.file_type().is_symlink() => {
                            return Err(LogError::PathOutsideRoot);
                        }
                        Ok(_) => return Err(LogError::SegmentConflict),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(LogError::io("recovering log segment", error));
                        }
                    }
                    fs::rename(&path, &recovered_path)
                        .map_err(|error| LogError::io("recovering log segment", error))?;
                }
                (recovered_path, recovered_end)
            };
            let resolved = fs::canonicalize(&path)
                .map_err(|error| LogError::io("validating log segment", error))?;
            if !resolved.starts_with(directory.as_path()) {
                return Err(LogError::PathOutsideRoot);
            }
            let checksum = checksum_file(&path)?;
            segments.push(Segment {
                generation,
                start,
                end,
                checksum,
                path,
            });
        }

        segments.sort_by_key(|segment| (segment.start, segment.generation, segment.end));

        // Segment creation happens before the first write so the child can
        // always inherit a ready path. A crash in that small window leaves an
        // empty newest segment; discard it when older data exists, while
        // retaining a sole empty segment as the append point for a stream
        // whose entire history has rotated away.
        if segments.len() > 1
            && segments
                .last()
                .is_some_and(|segment| segment.start == segment.end)
        {
            let empty = segments.pop().expect("checked above");
            match fs::remove_file(&empty.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(LogError::io("recovering empty log segment", error)),
            }
        }
        let mut previous_end = None;
        let mut previous_generation = None;
        let mut next_generation = 0;
        for segment in &segments {
            if segment.end < segment.start
                || previous_end.is_some_and(|end| end != segment.start)
                || previous_generation.is_some_and(|generation| generation == segment.generation)
                || (segments.len() > 1 && segment.start == segment.end)
            {
                return Err(LogError::SegmentCorrupt);
            }
            previous_end = Some(segment.end);
            previous_generation = Some(segment.generation);
            next_generation = next_generation.max(
                segment
                    .generation
                    .checked_add(1)
                    .ok_or(LogError::SegmentCorrupt)?,
            );
        }

        let mut state = Self {
            stream,
            directory,
            run_id: run_id.to_string(),
            limits,
            next_offset: segments.last().map_or(0, |segment| segment.end),
            next_generation,
            segments,
        };
        if state.segments.is_empty() {
            // Keep a concrete append point available immediately after a run
            // is opened. The process adapter can attach its readers before
            // the first child byte arrives, and the manifest then describes
            // an empty but valid stream rather than an absent stream.
            state.create_segment()?;
        }
        while state.segments.len() > state.limits.max_segments {
            state.remove_oldest_segment()?;
        }
        state.repair_manifest()?;
        Ok(state)
    }

    fn manifest_path(&self) -> PathBuf {
        self.directory
            .join(format!("{}.manifest.json", self.stream.as_str()))
    }

    fn manifest_temp_path(&self) -> PathBuf {
        self.directory
            .join(format!("{}.manifest.json.tmp", self.stream.as_str()))
    }

    fn rotation_generation(&self) -> u64 {
        self.segments.last().map_or(0, |segment| segment.generation)
    }

    fn read_manifest(&self) -> Result<Option<Manifest>, LogError> {
        let path = self.manifest_path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(LogError::io("reading log manifest", error)),
        };
        if !metadata.file_type().is_file() {
            return Err(if metadata.file_type().is_symlink() {
                LogError::PathOutsideRoot
            } else {
                LogError::ManifestCorrupt
            });
        }
        let resolved = fs::canonicalize(&path)
            .map_err(|error| LogError::io("validating log manifest", error))?;
        if !resolved.starts_with(self.directory.as_path()) {
            return Err(LogError::PathOutsideRoot);
        }
        let bytes = fs::read(&path).map_err(|error| LogError::io("reading log manifest", error))?;
        let manifest =
            serde_json::from_slice::<Manifest>(&bytes).map_err(|_| LogError::ManifestCorrupt)?;
        Ok(Some(manifest))
    }

    fn manifest_matches(&self, manifest: &Manifest) -> bool {
        let expected_checksum = serde_json::to_vec(&manifest.unsigned)
            .ok()
            .map(|bytes| checksum_hex(checksum_bytes(&bytes)));
        if expected_checksum.as_deref() != Some(manifest.checksum.as_str())
            || manifest.unsigned.schema_version != MANIFEST_SCHEMA_VERSION
            || manifest.unsigned.run_id != self.run_id
            || manifest.unsigned.stream != self.stream
            || manifest.unsigned.next_offset != self.next_offset
            || manifest.unsigned.rotation_generation != self.rotation_generation()
        {
            return false;
        }

        let retained_start = self
            .segments
            .first()
            .map_or(self.next_offset, |segment| segment.start);
        if manifest.unsigned.retained_start_offset != retained_start
            || manifest.unsigned.segments.len() != self.segments.len()
        {
            return false;
        }

        manifest
            .unsigned
            .segments
            .iter()
            .zip(&self.segments)
            .all(|(manifest_segment, segment)| {
                let Some(file_name) = segment.path.file_name().and_then(|name| name.to_str())
                else {
                    return false;
                };
                manifest_segment.file_name == file_name
                    && manifest_segment.start_offset == segment.start
                    && manifest_segment.end_offset == segment.end
                    && manifest_segment.byte_length == segment.len()
                    && manifest_segment.checksum == checksum_hex(segment.checksum)
            })
    }

    fn repair_manifest(&mut self) -> Result<(), LogError> {
        match self.read_manifest() {
            Ok(Some(manifest)) if self.manifest_matches(&manifest) => Ok(()),
            Ok(Some(_)) | Ok(None) | Err(LogError::ManifestCorrupt) => self.persist_manifest(),
            Err(error) => Err(error),
        }
    }

    fn persist_manifest(&self) -> Result<(), LogError> {
        let segments = self
            .segments
            .iter()
            .map(|segment| {
                let file_name = segment
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(LogError::ManifestCorrupt)?
                    .to_string();
                Ok(ManifestSegment {
                    file_name,
                    start_offset: segment.start,
                    end_offset: segment.end,
                    byte_length: segment.len(),
                    checksum: checksum_hex(segment.checksum),
                })
            })
            .collect::<Result<Vec<_>, LogError>>()?;
        let unsigned = ManifestUnsigned {
            schema_version: MANIFEST_SCHEMA_VERSION,
            run_id: self.run_id.clone(),
            stream: self.stream,
            retained_start_offset: self
                .segments
                .first()
                .map_or(self.next_offset, |segment| segment.start),
            next_offset: self.next_offset,
            rotation_generation: self.rotation_generation(),
            segments,
        };
        let unsigned_bytes =
            serde_json::to_vec(&unsigned).map_err(|_| LogError::ManifestCorrupt)?;
        let manifest = Manifest {
            unsigned,
            checksum: checksum_hex(checksum_bytes(&unsigned_bytes)),
        };
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(|_| LogError::ManifestCorrupt)?;
        let temp_path = self.manifest_temp_path();
        match fs::symlink_metadata(&temp_path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(if metadata.file_type().is_symlink() {
                    LogError::PathOutsideRoot
                } else {
                    LogError::ManifestCorrupt
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(LogError::io("preparing log manifest", error)),
        }

        let write_result = (|| {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(&temp_path)
                .map_err(|error| LogError::io("creating log manifest", error))?;
            file.write_all(&bytes)
                .and_then(|_| file.flush())
                .and_then(|_| file.sync_all())
                .map_err(|error| LogError::io("writing log manifest", error))?;
            drop(file);
            match platform::replace_file_atomic(&temp_path, &self.manifest_path()) {
                Ok(()) => Ok(()),
                Err(error) => Err(LogError::io("renaming log manifest", error)),
            }
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }
        write_result
    }

    fn append(&mut self, bytes: &[u8]) -> Result<u64, LogError> {
        let mut written = 0usize;
        while written < bytes.len() {
            let needs_new_segment = self
                .segments
                .last()
                .is_none_or(|segment| segment.len() == self.limits.segment_bytes);
            if needs_new_segment {
                self.create_segment()?;
            }

            let Some(current) = self.segments.last() else {
                return Err(LogError::SegmentCorrupt);
            };
            let available = self.limits.segment_bytes.saturating_sub(current.len());
            if available == 0 {
                return Err(LogError::SegmentCorrupt);
            }
            let chunk_len = available.min((bytes.len() - written) as u64) as usize;
            self.append_chunk(&bytes[written..written + chunk_len])?;
            written += chunk_len;
        }
        Ok(self.next_offset)
    }

    fn create_segment(&mut self) -> Result<(), LogError> {
        let generation = self.next_generation;
        let next_generation = generation.checked_add(1).ok_or(LogError::SegmentCorrupt)?;
        let start = self.next_offset;
        let path = self.segment_path(generation, start, start);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    LogError::SegmentConflict
                } else {
                    LogError::io("creating log segment", error)
                }
            })?;

        // Create the replacement before removing the oldest retained file so
        // a rotation-time creation failure cannot discard already retained
        // bytes. If removal fails, clean up the empty replacement and leave
        // the in-memory ranges unchanged.
        if self.segments.len() >= self.limits.max_segments {
            if let Err(error) = self.remove_oldest_segment() {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
        }
        self.next_generation = next_generation;
        self.segments.push(Segment {
            generation,
            start,
            end: start,
            checksum: FNV_OFFSET_BASIS,
            path,
        });
        Ok(())
    }

    fn append_chunk(&mut self, bytes: &[u8]) -> Result<(), LogError> {
        debug_assert!(!bytes.is_empty());
        let Some(index) = self.segments.len().checked_sub(1) else {
            return Err(LogError::SegmentCorrupt);
        };
        let segment = &self.segments[index];
        let old_path = segment.path.clone();
        let old_end = segment.end;
        let old_len = segment.len();
        let old_checksum = segment.checksum;
        let new_end = old_end
            .checked_add(bytes.len() as u64)
            .ok_or(LogError::CursorOverflow)?;
        if new_end - segment.start > self.limits.segment_bytes {
            return Err(LogError::SegmentCorrupt);
        }
        let metadata = fs::symlink_metadata(&old_path)
            .map_err(|error| LogError::io("opening log segment", error))?;
        if !metadata.file_type().is_file() || metadata.len() != old_len {
            return Err(LogError::SegmentCorrupt);
        }
        let mut file = OpenOptions::new()
            .append(true)
            .open(&old_path)
            .map_err(|error| LogError::io("opening log segment", error))?;

        if let Err(error) = file.write_all(bytes).and_then(|_| file.flush()) {
            drop(file);
            let _ = truncate_file(&old_path, old_len);
            return Err(LogError::io("writing log segment", error));
        }
        drop(file);

        let new_path = self.segment_path(segment.generation, segment.start, new_end);
        if new_path != old_path && new_path.exists() {
            let _ = truncate_file(&old_path, old_len);
            return Err(LogError::SegmentConflict);
        }
        if new_path != old_path {
            if let Err(error) = fs::rename(&old_path, &new_path) {
                let _ = truncate_file(&old_path, old_len);
                return Err(LogError::io("renaming log segment", error));
            }
        }

        let segment = &mut self.segments[index];
        segment.end = new_end;
        segment.checksum = checksum_extend(segment.checksum, bytes);
        segment.path = new_path;
        self.next_offset = new_end;
        if let Err(error) = self.persist_manifest() {
            let current_path = self.segments[index].path.clone();
            let restored = if current_path != old_path {
                fs::rename(&current_path, &old_path).is_ok()
            } else {
                true
            };
            if restored {
                let _ = truncate_file(&old_path, old_len);
            }
            let segment = &mut self.segments[index];
            segment.end = old_end;
            segment.checksum = old_checksum;
            segment.path = old_path;
            self.next_offset = old_end;
            return Err(error);
        }
        Ok(())
    }

    fn remove_oldest_segment(&mut self) -> Result<(), LogError> {
        let oldest = self
            .segments
            .first()
            .ok_or(LogError::SegmentCorrupt)?
            .path
            .clone();
        match fs::remove_file(&oldest) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(LogError::io("rotating log segments", error)),
        }
        self.segments.remove(0);
        Ok(())
    }

    fn tail(&self, request: &TailRequest) -> Result<TailResponse, LogError> {
        let snapshot_end = self.next_offset;
        let retained_start = self
            .segments
            .first()
            .map_or(snapshot_end, |segment| segment.start);
        let requested = request.cursor.as_deref().map(parse_cursor).transpose()?;
        let requested = requested.unwrap_or(retained_start);
        let truncated = requested < retained_start;
        let cursor = requested.max(retained_start).min(snapshot_end);
        let max_bytes = request.max_bytes.min(MAX_TAIL_BYTES);
        let mut data =
            Vec::with_capacity(max_bytes.min(snapshot_end.saturating_sub(cursor) as usize));
        let mut position = cursor;

        for segment in &self.segments {
            if data.len() == max_bytes || segment.end <= position {
                continue;
            }
            if segment.start >= snapshot_end {
                break;
            }
            let read_start = position.max(segment.start);
            let read_end = segment.end.min(snapshot_end);
            if read_start >= read_end {
                continue;
            }
            let allowed = (max_bytes - data.len()) as u64;
            let read_len = (read_end - read_start).min(allowed);
            read_segment(self, segment, read_start, read_len, &mut data)?;
            position = read_start + read_len;
            if data.len() == max_bytes {
                break;
            }
        }

        Ok(TailResponse {
            data,
            retained_start_offset: retained_start.to_string(),
            next_cursor: position.to_string(),
            truncated,
        })
    }

    fn segment_path(&self, generation: u64, start: u64, end: u64) -> PathBuf {
        self.directory.join(format!(
            "{}.g{generation}.o{start}-{end}{SEGMENT_EXTENSION}",
            self.stream.as_str()
        ))
    }
}

fn parse_segment_name(stream: LogStream, name: &str) -> Result<Option<(u64, u64, u64)>, LogError> {
    let prefix = format!("{}.g", stream.as_str());
    let Some(rest) = name.strip_prefix(&prefix) else {
        return Ok(None);
    };
    let Some((generation, rest)) = rest.split_once(".o") else {
        return Err(LogError::SegmentCorrupt);
    };
    let range = rest
        .strip_suffix(SEGMENT_EXTENSION)
        .ok_or(LogError::SegmentCorrupt)?;
    let Some((start, end)) = range.split_once('-') else {
        return Err(LogError::SegmentCorrupt);
    };
    let generation = generation.parse().map_err(|_| LogError::SegmentCorrupt)?;
    let start = start.parse().map_err(|_| LogError::SegmentCorrupt)?;
    let end = end.parse().map_err(|_| LogError::SegmentCorrupt)?;
    if end < start {
        return Err(LogError::SegmentCorrupt);
    }
    Ok(Some((generation, start, end)))
}

fn parse_cursor(cursor: &str) -> Result<u64, LogError> {
    if cursor.is_empty() || !cursor.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(LogError::InvalidCursor);
    }
    cursor.parse().map_err(|_| LogError::CursorOverflow)
}

fn truncate_file(path: &Path, length: u64) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.set_len(length))
}

fn read_segment(
    state: &StreamState,
    segment: &Segment,
    start: u64,
    length: u64,
    output: &mut Vec<u8>,
) -> Result<(), LogError> {
    let resolved = fs::canonicalize(&segment.path)
        .map_err(|error| LogError::io("opening log segment", error))?;
    if !resolved.starts_with(state.directory.as_path()) {
        return Err(LogError::PathOutsideRoot);
    }
    let metadata = fs::symlink_metadata(&segment.path)
        .map_err(|error| LogError::io("reading log segment", error))?;
    if !metadata.file_type().is_file() || metadata.len() != segment.len() {
        return Err(if metadata.file_type().is_symlink() {
            LogError::PathOutsideRoot
        } else {
            LogError::SegmentCorrupt
        });
    }
    if checksum_file(&segment.path)? != segment.checksum {
        return Err(LogError::SegmentCorrupt);
    }
    let mut file =
        File::open(&segment.path).map_err(|error| LogError::io("opening log segment", error))?;
    file.seek(SeekFrom::Start(start - segment.start))
        .map_err(|error| LogError::io("seeking log segment", error))?;
    let length = usize::try_from(length).map_err(|_| LogError::CursorOverflow)?;
    let old_len = output.len();
    output.resize(old_len + length, 0);
    if let Err(error) = file.read_exact(&mut output[old_len..]) {
        output.truncate(old_len);
        return Err(LogError::io("reading log segment", error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store(limits: LogLimits) -> (TempDir, LogStreams) {
        let root = tempfile::tempdir().expect("temp root");
        let streams = LogStreams::open(root.path(), "run-1", limits).expect("log store");
        (root, streams)
    }

    fn request(stream: LogStream, cursor: Option<&str>, max_bytes: usize) -> TailRequest {
        TailRequest {
            stream,
            cursor: cursor.map(str::to_string),
            max_bytes,
        }
    }

    async fn tail_all(
        streams: &LogStreams,
        stream: LogStream,
        cursor: Option<&str>,
    ) -> TailResponse {
        streams
            .tail(request(stream, cursor, usize::MAX))
            .await
            .expect("tail")
    }

    #[test]
    fn production_limits_match_spec() {
        assert_eq!(LogLimits::default().segment_bytes, 10 * 1024 * 1024);
        assert_eq!(LogLimits::default().max_segments, 5);
        assert_eq!(MAX_TAIL_BYTES, 256 * 1024);
    }

    #[tokio::test]
    async fn rotates_at_boundaries_and_preserves_decimal_ranges() {
        let (_root, streams) = store(LogLimits {
            segment_bytes: 3,
            max_segments: 3,
        });
        assert_eq!(
            streams.append(LogStream::Stdout, b"abc").await.unwrap(),
            "3"
        );
        assert_eq!(
            streams.append(LogStream::Stdout, b"defg").await.unwrap(),
            "7"
        );
        let response = tail_all(&streams, LogStream::Stdout, None).await;
        assert_eq!(response.data, b"abcdefg");
        assert_eq!(response.retained_start_offset, "0");
        assert_eq!(response.next_cursor, "7");
        assert!(!response.truncated);

        let names = fs::read_dir(streams.run_directory())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("stdout."))
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "stdout.g0.o0-3.log"));
        assert!(names.iter().any(|name| name == "stdout.g1.o3-6.log"));
        assert!(names.iter().any(|name| name == "stdout.g2.o6-7.log"));
    }

    #[tokio::test]
    async fn multi_rotation_tail_is_lossless_without_duplicates() {
        let (_root, streams) = store(LogLimits {
            segment_bytes: 2,
            max_segments: 3,
        });
        streams
            .append(LogStream::Stdout, b"0123456789")
            .await
            .unwrap();
        let first = streams
            .tail(request(LogStream::Stdout, Some("0"), 3))
            .await
            .unwrap();
        assert_eq!(first.data, b"456");
        assert_eq!(first.retained_start_offset, "4");
        assert_eq!(first.next_cursor, "7");
        let second = streams
            .tail(request(LogStream::Stdout, Some(&first.next_cursor), 3))
            .await
            .unwrap();
        assert_eq!(second.data, b"789");
        let third = streams
            .tail(request(LogStream::Stdout, Some(&second.next_cursor), 3))
            .await
            .unwrap();
        assert_eq!(third.next_cursor, "10");
        assert!(third.data.is_empty());
        assert_eq!(
            format!(
                "{}{}{}",
                String::from_utf8_lossy(&first.data),
                String::from_utf8_lossy(&second.data),
                String::from_utf8_lossy(&third.data)
            ),
            "456789"
        );
    }

    #[tokio::test]
    async fn stale_cursor_reports_truncation_and_starts_at_retained_offset() {
        let (_root, streams) = store(LogLimits {
            segment_bytes: 2,
            max_segments: 2,
        });
        streams.append(LogStream::Stdout, b"abcdef").await.unwrap();
        let response = tail_all(&streams, LogStream::Stdout, Some("0")).await;
        assert_eq!(response.data, b"cdef");
        assert_eq!(response.retained_start_offset, "2");
        assert_eq!(response.next_cursor, "6");
        assert!(response.truncated);
    }

    #[tokio::test]
    async fn tail_is_snapshot_bounded_and_cursor_can_resume_after_append() {
        let (_root, streams) = store(LogLimits {
            segment_bytes: 4,
            max_segments: 3,
        });
        streams.append(LogStream::Stdout, b"before").await.unwrap();
        let snapshot = streams
            .tail(request(LogStream::Stdout, Some("0"), 4))
            .await
            .unwrap();
        assert_eq!(snapshot.data, b"befo");
        assert_eq!(snapshot.next_cursor, "4");
        streams.append(LogStream::Stdout, b"-after").await.unwrap();
        let resumed = streams
            .tail(request(
                LogStream::Stdout,
                Some(&snapshot.next_cursor),
                usize::MAX,
            ))
            .await
            .unwrap();
        assert_eq!(resumed.data, b"re-after");
        assert_eq!(resumed.next_cursor, "12");
        assert!(!snapshot.data.windows(2).any(|window| window == b"-a"));
    }

    #[tokio::test]
    async fn stdout_and_stderr_have_independent_ranges_and_locks() {
        let (_root, streams) = store(LogLimits {
            segment_bytes: 3,
            max_segments: 2,
        });
        let (stdout, stderr) = tokio::join!(
            streams.append(LogStream::Stdout, b"out"),
            streams.append(LogStream::Stderr, b"error"),
        );
        assert_eq!(stdout.unwrap(), "3");
        assert_eq!(stderr.unwrap(), "5");
        assert_eq!(
            tail_all(&streams, LogStream::Stdout, None).await.data,
            b"out"
        );
        assert_eq!(
            tail_all(&streams, LogStream::Stderr, None).await.data,
            b"error"
        );
    }

    #[tokio::test]
    async fn max_bytes_is_clamped_and_binary_bytes_are_lossless() {
        let (_root, streams) = store(LogLimits::default());
        let bytes = vec![0; MAX_TAIL_BYTES + 17];
        streams.append(LogStream::Stdout, &bytes).await.unwrap();
        let response = streams
            .tail(request(LogStream::Stdout, Some("0"), usize::MAX))
            .await
            .unwrap();
        assert_eq!(response.data.len(), MAX_TAIL_BYTES);
        assert_eq!(response.next_cursor, MAX_TAIL_BYTES.to_string());
    }

    #[tokio::test]
    async fn invalid_cursor_values_are_rejected_without_path_or_io_leaks() {
        let (_root, streams) = store(LogLimits::default());
        for cursor in [
            "",
            "-1",
            "+1",
            "1.0",
            "not-a-number",
            "18446744073709551616",
        ] {
            let error = streams
                .tail(request(LogStream::Stdout, Some(cursor), 10))
                .await
                .unwrap_err();
            assert!(matches!(
                error,
                LogError::InvalidCursor | LogError::CursorOverflow
            ));
            assert!(!error.to_string().contains("/"));
        }
    }

    #[test]
    fn invalid_paths_and_run_ids_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        for run_id in ["../escape", "nested/run", r"nested\\run", ".", "..", ""] {
            assert!(matches!(
                LogStreams::open(root.path(), run_id, LogLimits::default()),
                Err(LogError::InvalidRunId)
            ));
        }
        let outside = tempfile::tempdir().unwrap();
        let run_dir = outside.path().join("run-1");
        fs::create_dir_all(&run_dir).unwrap();
        let relative = relative_log_dir("run-1").unwrap();
        assert_eq!(
            resolve_run_directory(root.path(), outside.path().join("run-1"), "run-1"),
            Err(LogError::InvalidLogPath)
        );
        assert_eq!(
            resolve_run_directory(root.path(), relative, "run-1"),
            Err(LogError::RunDirectoryMissing)
        );
    }

    #[tokio::test]
    async fn write_failures_return_safe_errors_and_keep_cursor_unchanged() {
        let (_root, streams) = store(LogLimits {
            segment_bytes: 4,
            max_segments: 2,
        });
        streams.append(LogStream::Stdout, b"ok").await.unwrap();
        let current = fs::read_dir(streams.run_directory())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("stdout.")
                    && path.extension().is_some_and(|extension| extension == "log")
            })
            .unwrap();
        fs::remove_file(&current).unwrap();
        fs::create_dir(&current).unwrap();
        let error = streams
            .append(LogStream::Stdout, b"fail")
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            LogError::Io { .. } | LogError::SegmentCorrupt
        ));
        fs::remove_dir(&current).unwrap();
        fs::write(&current, b"ok").unwrap();
        let response = tail_all(&streams, LogStream::Stdout, None).await;
        assert_eq!(response.data, b"ok");
        assert_eq!(response.next_cursor, "2");
        assert!(!error
            .to_string()
            .contains(current.to_string_lossy().as_ref()));
    }

    #[tokio::test]
    async fn reopening_reconstructs_ranges_and_continues_generation() {
        let root = tempfile::tempdir().unwrap();
        let limits = LogLimits {
            segment_bytes: 2,
            max_segments: 3,
        };
        let streams = LogStreams::open(root.path(), "run-1", limits).unwrap();
        streams.append(LogStream::Stdout, b"abcd").await.unwrap();
        drop(streams);
        let reopened = LogStreams::open(root.path(), "run-1", limits).unwrap();
        assert_eq!(
            tail_all(&reopened, LogStream::Stdout, None).await.data,
            b"abcd"
        );
        assert_eq!(reopened.append(LogStream::Stdout, b"e").await.unwrap(), "5");
        let names = fs::read_dir(reopened.run_directory())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "stdout.g2.o4-5.log"));
    }

    #[tokio::test]
    async fn reopening_recovers_bytes_written_before_range_rename() {
        let root = tempfile::tempdir().unwrap();
        let limits = LogLimits {
            segment_bytes: 4,
            max_segments: 3,
        };
        let streams = LogStreams::open(root.path(), "run-1", limits).unwrap();
        streams.append(LogStream::Stdout, b"ab").await.unwrap();
        let current = streams.run_directory().join("stdout.g0.o0-2.log");
        let crash_window = streams.run_directory().join("stdout.g0.o0-0.log");
        fs::rename(&current, &crash_window).unwrap();
        fs::write(
            streams.run_directory().join("stdout.manifest.json"),
            b"broken",
        )
        .unwrap();
        drop(streams);

        let reopened = LogStreams::open(root.path(), "run-1", limits).unwrap();
        assert_eq!(
            tail_all(&reopened, LogStream::Stdout, None).await.data,
            b"ab"
        );
        assert!(reopened
            .run_directory()
            .join("stdout.g0.o0-2.log")
            .is_file());
        assert!(!reopened.run_directory().join("stdout.g0.o0-0.log").exists());
    }

    #[tokio::test]
    async fn reopening_discards_an_empty_newest_segment_from_a_crash_window() {
        let root = tempfile::tempdir().unwrap();
        let limits = LogLimits {
            segment_bytes: 2,
            max_segments: 3,
        };
        let streams = LogStreams::open(root.path(), "run-1", limits).unwrap();
        streams.append(LogStream::Stdout, b"abcd").await.unwrap();
        File::create(streams.run_directory().join("stdout.g2.o4-4.log")).unwrap();
        drop(streams);

        let reopened = LogStreams::open(root.path(), "run-1", limits).unwrap();
        assert_eq!(
            tail_all(&reopened, LogStream::Stdout, None).await.data,
            b"abcd"
        );
        assert!(!reopened.run_directory().join("stdout.g2.o4-4.log").exists());
        assert_eq!(reopened.append(LogStream::Stdout, b"e").await.unwrap(), "5");
        assert!(reopened
            .run_directory()
            .join("stdout.g2.o4-5.log")
            .is_file());
    }

    #[tokio::test]
    async fn corrupt_manifest_is_rebuilt_from_segment_names_and_lengths() {
        let root = tempfile::tempdir().unwrap();
        let limits = LogLimits {
            segment_bytes: 2,
            max_segments: 2,
        };
        let streams = LogStreams::open(root.path(), "run-1", limits).unwrap();
        streams.append(LogStream::Stdout, b"abcdef").await.unwrap();
        let manifest_path = streams.run_directory().join("stdout.manifest.json");
        fs::write(&manifest_path, b"not-json").unwrap();
        drop(streams);

        let reopened = LogStreams::open(root.path(), "run-1", limits).unwrap();
        assert_eq!(
            tail_all(&reopened, LogStream::Stdout, None).await.data,
            b"cdef"
        );
        let manifest = fs::read_to_string(manifest_path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(value["schema_version"], MANIFEST_SCHEMA_VERSION);
        assert_eq!(value["run_id"], "run-1");
        assert_eq!(value["stream"], "stdout");
        assert_eq!(value["next_offset"], 6);
    }

    #[test]
    fn zero_rotation_limits_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            LogStreams::open(
                root.path(),
                "run-1",
                LogLimits {
                    segment_bytes: 0,
                    max_segments: 1,
                }
            ),
            Err(LogError::InvalidLimits)
        ));
        assert!(matches!(
            LogStreams::open(
                root.path(),
                "run-1",
                LogLimits {
                    segment_bytes: 1,
                    max_segments: 0,
                }
            ),
            Err(LogError::InvalidLimits)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn segment_symlinks_cannot_escape_the_run_directory() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_segment = outside.path().join("outside.log");
        fs::write(&outside_segment, b"escape").unwrap();
        let run_dir = root.path().join("logs/runs/run-1");
        fs::create_dir_all(&run_dir).unwrap();
        symlink(&outside_segment, run_dir.join("stdout.g0.o0-6.log")).unwrap();
        let error = match LogStreams::open(root.path(), "run-1", LogLimits::default()) {
            Err(error) => error,
            Ok(_) => panic!("outside symlink should be rejected"),
        };
        assert_eq!(error, LogError::PathOutsideRoot);
        assert!(!error
            .to_string()
            .contains(outside.path().to_string_lossy().as_ref()));
    }
}
