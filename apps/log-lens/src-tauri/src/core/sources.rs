use super::buffer::RingBuffer;
use super::lifecycle::{CancellationToken, OperationRegistry};
use super::model::{
    run_source_parts, CoreError, FileCursor, FileIdentity, LogFormat, LogRecord, ReadStatus,
    SourceKind, SourceSnapshot, SourceSpec, MAX_SOURCE_BYTES,
};
use super::parser::parse_bytes;
use devbox_filesystem::{ensure_no_links, filesystem_identity, open_filesystem_object};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
#[cfg(windows)]
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

const READ_CHUNK_BYTES: usize = 64 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_DIRECTORY_FILES: usize = 256;
const MAX_CONTAINER_LINES: &str = "100000";
const CURSOR_ANCHOR_BYTES: u64 = 4 * 1024;
const TERMINATION_WAIT: Duration = Duration::from_secs(1);
const RUN_MANAGER_IDENTIFIER: &str = "com.devbox.runmanager";
const RUN_LOG_RELATIVE_ROOT: &str = "logs/runs";
const RUN_SEGMENT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_RUN_SEGMENTS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterPlan {
    pub program: String,
    pub args: Vec<String>,
    pub source_kind: SourceKind,
    pub read_only: bool,
}

pub struct LoadContext<'a> {
    pub operation_id: &'a str,
    pub generation: u64,
    pub token: &'a CancellationToken,
    pub registry: &'a OperationRegistry,
    pub deadline: Instant,
}

impl<'a> LoadContext<'a> {
    pub fn new(
        operation_id: &'a str,
        generation: u64,
        token: &'a CancellationToken,
        registry: &'a OperationRegistry,
    ) -> Self {
        Self {
            operation_id,
            generation,
            token,
            registry,
            deadline: Instant::now() + DEFAULT_TIMEOUT,
        }
    }

    pub fn check(&self) -> Result<(), CoreError> {
        if Instant::now() >= self.deadline {
            return Err(CoreError::Timeout);
        }
        self.registry
            .check_current(self.operation_id, self.generation, self.token)
    }
}

/// Return the exact fixed adapter command without executing it. This is also
/// used by tests and diagnostics so arbitrary command input can never enter a
/// WSL/container process boundary.
pub fn adapter_argv(source: &SourceSpec) -> Result<Option<AdapterPlan>, CoreError> {
    source.validate()?;
    let plan = match source {
        SourceSpec::LocalFile { .. } | SourceSpec::Directory { .. } => return Ok(None),
        SourceSpec::WslFile { distro, path } => AdapterPlan {
            program: "wsl.exe".to_string(),
            args: vec![
                "-d".to_string(),
                distro.clone(),
                "--".to_string(),
                "cat".to_string(),
                "--".to_string(),
                path.clone(),
            ],
            source_kind: SourceKind::WslFile,
            read_only: true,
        },
        SourceSpec::WslJournal { distro, unit } => {
            let mut args = vec![
                "-d".to_string(),
                distro.clone(),
                "--".to_string(),
                "journalctl".to_string(),
                "--no-pager".to_string(),
                "--output=short-iso".to_string(),
                "--lines=100000".to_string(),
            ];
            if let Some(unit) = unit {
                args.push(format!("--unit={unit}"));
            }
            AdapterPlan {
                program: "wsl.exe".to_string(),
                args,
                source_kind: SourceKind::WslJournal,
                read_only: true,
            }
        }
        SourceSpec::Container {
            engine,
            container_id,
        } => AdapterPlan {
            program: match engine {
                super::model::ContainerEngine::Docker => "docker",
                super::model::ContainerEngine::Podman => "podman",
            }
            .to_string(),
            args: vec![
                "logs".to_string(),
                "--timestamps".to_string(),
                "--tail".to_string(),
                MAX_CONTAINER_LINES.to_string(),
                "--".to_string(),
                container_id.clone(),
            ],
            source_kind: SourceKind::Container,
            read_only: true,
        },
        SourceSpec::Run { .. } | SourceSpec::WebhookCapture { .. } => return Ok(None),
    };
    Ok(Some(plan))
}

pub fn load_source(
    source: &SourceSpec,
    cursor: Option<&FileCursor>,
    sequence_start: u64,
    context: &LoadContext<'_>,
) -> Result<SourceSnapshot, CoreError> {
    context.check()?;
    let summary = source.summary()?;
    if let SourceSpec::WebhookCapture { capture } = source {
        return load_webhook_capture(capture, summary, sequence_start, context);
    }
    let (bytes, next_cursor, status, initial_truncated) = match source {
        SourceSpec::LocalFile { path } => {
            let read = read_file(Path::new(path), cursor, context)?;
            (read.bytes, Some(read.cursor), read.status, read.truncated)
        }
        SourceSpec::Directory { path, pattern } => {
            let read = read_directory(Path::new(path), pattern, context)?;
            (read.bytes, None, read.status, read.truncated)
        }
        SourceSpec::WslFile { .. }
        | SourceSpec::WslJournal { .. }
        | SourceSpec::Container { .. } => {
            let plan = adapter_argv(source)?.ok_or(CoreError::AdapterUnavailable)?;
            (
                run_fixed_adapter(&plan, context)?,
                None,
                ReadStatus::Initial,
                false,
            )
        }
        SourceSpec::Run { source_id } => {
            let read = read_run_source(source_id, cursor, context)?;
            (read.bytes, Some(read.cursor), read.status, read.truncated)
        }
        SourceSpec::WebhookCapture { .. } => unreachable!("handled before byte readers"),
    };
    context.check()?;
    let batch = parse_bytes(&bytes, &summary.source_id, sequence_start)?;
    context.check()?;
    let mut ring = RingBuffer::default();
    let push = ring.extend(batch.records);
    let final_status = if status == ReadStatus::Rotated {
        ReadStatus::Rotated
    } else if batch.truncated || initial_truncated {
        ReadStatus::Truncated
    } else {
        status
    };
    Ok(SourceSnapshot {
        operation_id: context.operation_id.to_string(),
        generation: context.generation,
        source: summary,
        records: ring.snapshot(),
        next_cursor,
        status: final_status,
        truncated: batch.truncated || initial_truncated,
        dropped_records: push.dropped_records,
        dropped_bytes: push.dropped_bytes,
    })
}

fn load_webhook_capture(
    capture: &devbox_applink::WebhookLogPayload,
    summary: super::model::SourceSummary,
    sequence_start: u64,
    context: &LoadContext<'_>,
) -> Result<SourceSnapshot, CoreError> {
    devbox_applink::validate_webhook_log_payload(capture).map_err(|_| CoreError::InvalidSource)?;
    context.check()?;
    let mut fields = BTreeMap::new();
    fields.insert("method".to_string(), capture.method.clone());
    fields.insert("target".to_string(), capture.target.clone());
    if !capture.header_names.is_empty() {
        fields.insert("headerNames".to_string(), capture.header_names.join(", "));
    }
    if !capture.body_preview.is_empty() {
        fields.insert("bodyPreview".to_string(), capture.body_preview.clone());
    }
    fields.insert("redacted".to_string(), capture.redacted.to_string());
    fields.insert("truncated".to_string(), capture.truncated.to_string());
    let record = LogRecord {
        source_id: summary.source_id.clone(),
        sequence: sequence_start,
        timestamp_millis: Some(capture.received_at_ms),
        level: None,
        message: format!("{} {}", capture.method, capture.target),
        fields,
        format: LogFormat::Plain,
        truncated: capture.truncated,
    };
    record.validate()?;
    Ok(SourceSnapshot {
        operation_id: context.operation_id.to_string(),
        generation: context.generation,
        source: summary,
        records: vec![record],
        next_cursor: None,
        status: ReadStatus::Initial,
        truncated: capture.truncated,
        dropped_records: 0,
        dropped_bytes: 0,
    })
}

#[derive(Debug)]
struct RunSegment {
    generation: u64,
    start: u64,
    end: u64,
    path: PathBuf,
}

fn run_manager_data_root() -> Result<PathBuf, CoreError> {
    dirs::data_local_dir()
        .map(|root| root.join(RUN_MANAGER_IDENTIFIER))
        .ok_or(CoreError::AdapterUnavailable)
}

fn read_run_source(
    source_id: &str,
    previous: Option<&FileCursor>,
    context: &LoadContext<'_>,
) -> Result<ReadResult, CoreError> {
    read_run_source_in(&run_manager_data_root()?, source_id, previous, context)
}

/// Read Run Manager's fixed app-owned rotation format without accepting a
/// producer path. The source identity determines the exact app data root,
/// run directory, stream prefix, and logical cursor; no arbitrary path or
/// command can enter this adapter.
fn read_run_source_in(
    app_data_root: &Path,
    source_id: &str,
    previous: Option<&FileCursor>,
    context: &LoadContext<'_>,
) -> Result<ReadResult, CoreError> {
    let (run_id, stream) = run_source_parts(source_id)?;
    if previous.is_some_and(|cursor| cursor.identity.is_some() || cursor.anchor_hash.is_some()) {
        return Err(CoreError::InvalidInput);
    }

    let run_directory = app_data_root.join(RUN_LOG_RELATIVE_ROOT).join(run_id);
    ensure_no_links(&run_directory).map_err(|_| CoreError::AdapterUnavailable)?;
    let canonical_root =
        fs::canonicalize(app_data_root).map_err(|_| CoreError::AdapterUnavailable)?;
    let canonical_run =
        fs::canonicalize(&run_directory).map_err(|_| CoreError::AdapterUnavailable)?;
    if !canonical_run.starts_with(&canonical_root)
        || canonical_run.file_name().and_then(|name| name.to_str()) != Some(run_id)
    {
        return Err(CoreError::InvalidSource);
    }
    let directory_identity =
        filesystem_identity(&canonical_run, true).map_err(|_| CoreError::AdapterUnavailable)?;

    let mut segments = Vec::new();
    for entry in fs::read_dir(&canonical_run).map_err(|_| CoreError::AdapterUnavailable)? {
        context.check()?;
        let entry = entry.map_err(|_| CoreError::Io)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some((generation, start, end)) = parse_run_segment_name(&name, stream)? else {
            continue;
        };
        if segments.len() >= MAX_RUN_SEGMENTS {
            return Err(CoreError::OutputLimit);
        }
        let path = entry.path();
        ensure_no_links(&path).map_err(|_| CoreError::InvalidSource)?;
        segments.push(RunSegment {
            generation,
            start,
            end,
            path,
        });
    }
    if segments.is_empty() {
        return Err(CoreError::AdapterUnavailable);
    }
    segments.sort_by_key(|segment| (segment.start, segment.generation, segment.end));
    for (index, segment) in segments.iter().enumerate() {
        let length = segment
            .end
            .checked_sub(segment.start)
            .ok_or(CoreError::InvalidSource)?;
        if length > RUN_SEGMENT_BYTES
            || segments.len() > 1 && length == 0
            || index > 0 && segments[index - 1].end != segment.start
            || index > 0 && segments[index - 1].generation >= segment.generation
        {
            return Err(CoreError::InvalidSource);
        }
    }

    let retained_start = segments.first().map(|segment| segment.start).unwrap_or(0);
    let snapshot_end = segments.last().map(|segment| segment.end).unwrap_or(0);
    let requested = previous
        .map(|cursor| {
            cursor
                .offset
                .parse::<u64>()
                .map_err(|_| CoreError::InvalidInput)
        })
        .transpose()?
        .unwrap_or(retained_start);
    let (mut position, status, mut truncated) = if requested > snapshot_end {
        (retained_start, ReadStatus::Rotated, true)
    } else if requested < retained_start {
        (retained_start, ReadStatus::Truncated, true)
    } else if previous.is_some() {
        (requested, ReadStatus::Advanced, false)
    } else {
        (requested, ReadStatus::Initial, false)
    };

    let mut bytes = Vec::with_capacity(
        usize::try_from(snapshot_end.saturating_sub(position))
            .unwrap_or(MAX_SOURCE_BYTES)
            .min(MAX_SOURCE_BYTES),
    );
    for segment in &segments {
        context.check()?;
        if segment.end <= position || segment.start >= snapshot_end {
            continue;
        }
        if bytes.len() >= MAX_SOURCE_BYTES {
            truncated = true;
            break;
        }
        let read_start = position.max(segment.start);
        let read_end = segment.end.min(snapshot_end);
        let available = MAX_SOURCE_BYTES - bytes.len();
        let read_length = usize::try_from(read_end - read_start)
            .map_err(|_| CoreError::OutputLimit)?
            .min(available);
        let (mut file, _) =
            open_filesystem_object(&segment.path, false).map_err(|_| CoreError::Io)?;
        let metadata = file.metadata().map_err(|_| CoreError::Io)?;
        if metadata.len() != segment.end - segment.start {
            return Err(CoreError::Io);
        }
        file.seek(SeekFrom::Start(read_start - segment.start))
            .map_err(|_| CoreError::Io)?;
        let old_length = bytes.len();
        bytes.resize(old_length + read_length, 0);
        if file.read_exact(&mut bytes[old_length..]).is_err() {
            bytes.truncate(old_length);
            return Err(CoreError::Io);
        }
        position = read_start + read_length as u64;
        if position < read_end {
            truncated = true;
            break;
        }
    }
    if filesystem_identity(&canonical_run, true).map_err(|_| CoreError::Io)? != directory_identity {
        return Err(CoreError::Io);
    }

    Ok(ReadResult {
        bytes,
        cursor: FileCursor {
            identity: None,
            offset: position.to_string(),
            anchor_hash: None,
        },
        status,
        truncated,
    })
}

fn parse_run_segment_name(name: &str, stream: &str) -> Result<Option<(u64, u64, u64)>, CoreError> {
    let prefix = format!("{stream}.g");
    let Some(rest) = name.strip_prefix(&prefix) else {
        return Ok(None);
    };
    let Some((generation, range)) = rest.split_once(".o") else {
        return Err(CoreError::InvalidSource);
    };
    let Some(range) = range.strip_suffix(".log") else {
        return Err(CoreError::InvalidSource);
    };
    let Some((start, end)) = range.split_once('-') else {
        return Err(CoreError::InvalidSource);
    };
    if [generation, start, end]
        .iter()
        .any(|value| value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(CoreError::InvalidSource);
    }
    let generation = generation
        .parse::<u64>()
        .map_err(|_| CoreError::InvalidSource)?;
    let start = start.parse::<u64>().map_err(|_| CoreError::InvalidSource)?;
    let end = end.parse::<u64>().map_err(|_| CoreError::InvalidSource)?;
    if end < start {
        return Err(CoreError::InvalidSource);
    }
    Ok(Some((generation, start, end)))
}

struct ReadResult {
    bytes: Vec<u8>,
    cursor: FileCursor,
    status: ReadStatus,
    truncated: bool,
}

fn read_file(
    path: &Path,
    previous: Option<&FileCursor>,
    context: &LoadContext<'_>,
) -> Result<ReadResult, CoreError> {
    read_file_with_limit(path, previous, MAX_SOURCE_BYTES, context)
}

/// Read at most `byte_limit` bytes from a local file. Directory sources pass
/// their remaining aggregate budget here so one large member cannot allocate
/// a second 64 MiB buffer before its bytes are copied into the directory
/// output. The source window still starts at the same 64 MiB tail boundary as
/// a regular local read; a small remaining budget only limits how much of that
/// window is materialized.
fn read_file_with_limit(
    path: &Path,
    previous: Option<&FileCursor>,
    byte_limit: usize,
    context: &LoadContext<'_>,
) -> Result<ReadResult, CoreError> {
    let byte_limit = byte_limit.min(MAX_SOURCE_BYTES);
    let mut file = File::open(path).map_err(|_| CoreError::Io)?;
    let metadata = file.metadata().map_err(|_| CoreError::Io)?;
    let identity = file_identity(&file, &metadata);
    let (mut offset, mut status, mut truncated) = match previous {
        None => (
            metadata.len().saturating_sub(MAX_SOURCE_BYTES as u64),
            ReadStatus::Initial,
            metadata.len() > MAX_SOURCE_BYTES as u64,
        ),
        Some(previous) => {
            previous.validate()?;
            let previous_offset = previous
                .offset
                .parse::<u64>()
                .map_err(|_| CoreError::InvalidInput)?;
            let same_identity = same_file_identity(previous.identity.as_ref(), &identity)
                && previous
                    .identity
                    .as_ref()
                    .is_none_or(|old| identity.modified_millis >= old.modified_millis);
            let anchor_matches = if previous_offset > metadata.len() {
                false
            } else {
                match previous.anchor_hash.as_deref() {
                    Some(expected) => cursor_anchor_hash(&mut file, previous_offset)? == expected,
                    None => true,
                }
            };
            if !same_identity {
                (0, ReadStatus::Rotated, true)
            } else if previous
                .identity
                .as_ref()
                .is_some_and(|old| identity.size < old.size)
                || previous_offset > metadata.len()
                || !anchor_matches
            {
                (0, ReadStatus::Truncated, true)
            } else {
                (previous_offset, ReadStatus::Advanced, false)
            }
        }
    };
    if offset > metadata.len() {
        offset = 0;
        status = ReadStatus::Rotated;
        truncated = true;
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| CoreError::Io)?;
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    while bytes.len() < byte_limit {
        context.check()?;
        let remaining = byte_limit - bytes.len();
        let read_len = remaining.min(chunk.len());
        let read = file
            .read(&mut chunk[..read_len])
            .map_err(|_| CoreError::Io)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    if bytes.len() == byte_limit {
        let mut probe = [0_u8; 1];
        if file.read(&mut probe).map_err(|_| CoreError::Io)? > 0 {
            truncated = true;
        }
    }
    let next_offset = offset.saturating_add(bytes.len() as u64);
    let anchor_hash = cursor_anchor_hash(&mut file, next_offset)?;
    Ok(ReadResult {
        bytes,
        cursor: FileCursor {
            identity: Some(identity),
            offset: next_offset.to_string(),
            anchor_hash: Some(anchor_hash),
        },
        status,
        truncated,
    })
}

fn read_directory(
    path: &Path,
    pattern: &str,
    context: &LoadContext<'_>,
) -> Result<ReadResult, CoreError> {
    let entries = fs::read_dir(path).map_err(|_| CoreError::Io)?;
    // Keep only the lexicographically first bounded set while scanning. A
    // directory may contain an unbounded number of entries, so collecting all
    // matches before truncating would defeat the source memory bound.
    let mut paths = BTreeMap::new();
    let mut matched_file_limit = false;
    let mut scan_partial = false;
    for entry in entries {
        context.check()?;
        let Ok(entry) = entry else {
            scan_partial = true;
            continue;
        };
        let entry_path = entry.path();
        let Some(metadata) = fs::symlink_metadata(&entry_path).ok() else {
            scan_partial = true;
            continue;
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            scan_partial = true;
            continue;
        };
        if wildcard_match(pattern, &name) {
            paths.insert(name, entry_path);
            if paths.len() > MAX_DIRECTORY_FILES {
                paths.pop_last();
                matched_file_limit = true;
            }
        }
    }
    let mut output = Vec::new();
    let mut truncated = matched_file_limit || scan_partial;
    for (_, path) in paths {
        context.check()?;
        if output.len() >= MAX_SOURCE_BYTES {
            truncated = true;
            break;
        }
        // Directory members are one logical source, but a file without a
        // trailing newline must not run into the first line of the next
        // member. Add only the missing delimiter and charge it to the same
        // aggregate byte budget.
        if output.last().is_some_and(|byte| *byte != b'\n') {
            output.push(b'\n');
        }
        let remaining = MAX_SOURCE_BYTES - output.len();
        let read = match read_file_with_limit(&path, None, remaining, context) {
            Ok(read) => read,
            // A single unreadable member should not hide every other matching
            // file. The directory is visibly partial, while cancellation and
            // deadline errors still abort the whole operation below.
            Err(CoreError::Io) => {
                truncated = true;
                continue;
            }
            Err(error) => return Err(error),
        };
        output.extend_from_slice(&read.bytes);
        truncated |= read.truncated;
    }
    Ok(ReadResult {
        bytes: output,
        cursor: FileCursor {
            identity: None,
            offset: "0".to_string(),
            anchor_hash: None,
        },
        status: ReadStatus::Initial,
        truncated,
    })
}

fn run_fixed_adapter(plan: &AdapterPlan, context: &LoadContext<'_>) -> Result<Vec<u8>, CoreError> {
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // A fixed adapter can still create descendants (for example a WSL
    // helper). Give it a private process group on Unix; Windows assigns the
    // child to a kill-on-close Job Object below. Both boundaries cover the
    // complete adapter tree without passing user input to a shell utility.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let (mut child, mut process_tree) =
        ProcessTree::spawn(&mut command).map_err(|_| CoreError::AdapterUnavailable)?;
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            process_tree.terminate(&mut child);
            return Err(CoreError::AdapterUnavailable);
        }
    };
    // Keep the reader thread back-pressured. An unbounded channel would let a
    // fast adapter enqueue arbitrary output before the main loop observes the
    // 64 MiB safety cap.
    let (sender, receiver) = mpsc::sync_channel::<Result<Vec<u8>, ()>>(4);
    let reader = match thread::Builder::new()
        .name("log-lens-adapter-reader".to_string())
        .spawn(move || {
            let mut chunk = [0_u8; READ_CHUNK_BYTES];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        if sender.send(Ok(chunk[..read].to_vec())).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = sender.send(Err(()));
                        break;
                    }
                }
            }
        }) {
        Ok(reader) => reader,
        Err(_) => {
            drop(receiver);
            process_tree.terminate(&mut child);
            return Err(CoreError::Io);
        }
    };
    let mut bytes = Vec::new();
    let mut exit_status = None;
    loop {
        if let Err(error) = context.check() {
            terminate_child(&mut child, &mut process_tree, receiver, reader);
            return Err(error);
        }
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(chunk)) => {
                if bytes.len().saturating_add(chunk.len()) > MAX_SOURCE_BYTES {
                    terminate_child(&mut child, &mut process_tree, receiver, reader);
                    return Err(CoreError::OutputLimit);
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(Err(())) => {
                terminate_child(&mut child, &mut process_tree, receiver, reader);
                return Err(CoreError::Io);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if exit_status.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // The root may exit while one of its helpers still owns
                    // stdout. Close the owned tree immediately so the reader
                    // can disconnect instead of waiting until the operation
                    // deadline with a live inherited pipe.
                    process_tree.terminate_descendants();
                    exit_status = Some(status);
                }
                Ok(None) => {}
                Err(_) => {
                    terminate_child(&mut child, &mut process_tree, receiver, reader);
                    return Err(CoreError::Io);
                }
            }
        }
        if exit_status.is_some() {
            match receiver.try_recv() {
                Ok(Ok(chunk)) => {
                    if bytes.len().saturating_add(chunk.len()) > MAX_SOURCE_BYTES {
                        terminate_child(&mut child, &mut process_tree, receiver, reader);
                        return Err(CoreError::OutputLimit);
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok(Err(())) => {
                    terminate_child(&mut child, &mut process_tree, receiver, reader);
                    return Err(CoreError::Io);
                }
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
        }
    }
    let status = match exit_status {
        Some(status) => status,
        None => loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    process_tree.terminate_descendants();
                    break status;
                }
                Ok(None) => {}
                Err(_) => {
                    terminate_child(&mut child, &mut process_tree, receiver, reader);
                    return Err(CoreError::Io);
                }
            }
            if let Err(error) = context.check() {
                terminate_child(&mut child, &mut process_tree, receiver, reader);
                return Err(error);
            }
            thread::sleep(Duration::from_millis(10));
        },
    };
    drop(receiver);
    join_reader_bounded(reader);
    if !status.success() {
        return Err(CoreError::AdapterUnavailable);
    }
    Ok(bytes)
}

fn terminate_child(
    child: &mut Child,
    process_tree: &mut ProcessTree,
    receiver: mpsc::Receiver<Result<Vec<u8>, ()>>,
    reader: thread::JoinHandle<()>,
) {
    drop(receiver);
    process_tree.terminate(child);
    join_reader_bounded(reader);
}

/// A descendant that inherited stdout can keep the pipe open after the root
/// exits. The process-tree boundary normally closes it, but joining without a
/// bound would turn a cleanup failure into a permanently stuck worker. A
/// finished reader is still joined so its resources are reclaimed; an
/// unfinished reader is detached after the bounded grace period.
fn join_reader_bounded(reader: thread::JoinHandle<()>) {
    let wait_until = Instant::now() + TERMINATION_WAIT;
    while !reader.is_finished() && Instant::now() < wait_until {
        thread::sleep(Duration::from_millis(10));
    }
    if reader.is_finished() {
        let _ = reader.join();
    }
}

/// Reap a child without an unbounded `wait()`. The second bounded pass is a
/// last-resort root kill; platform process-tree ownership is closed by the
/// caller so descendants cannot remain attached to the reader pipe.
fn reap_child_bounded(child: &mut Child) {
    let wait_until = Instant::now() + TERMINATION_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Err(_) => {
                let _ = child.kill();
                return;
            }
            Ok(None) if Instant::now() >= wait_until => break,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
    let _ = child.kill();
    let wait_until = Instant::now() + TERMINATION_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if Instant::now() >= wait_until => return,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn kill_child_and_reap(child: &mut Child) {
    let _ = child.kill();
    reap_child_bounded(child);
}

/// Own an adapter's complete process tree. Windows uses a Job Object with
/// kill-on-close; Unix uses the process group assigned before spawn.
#[cfg(windows)]
struct ProcessTree {
    handle: HANDLE,
}

#[cfg(windows)]
impl ProcessTree {
    fn spawn(command: &mut Command) -> Result<(Child, Self), ()> {
        use std::os::windows::process::CommandExt;
        use windows::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

        // A normally-running child could create an unowned descendant in the
        // interval between spawn and Job assignment. Start the sole primary
        // thread suspended, establish exact Job ownership, prove the Job
        // contains exactly this root, and only then resume it.
        command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|_| ())?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .is_err()
        {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(());
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                return Err(());
            }
        };
        let process = HANDLE(child.as_raw_handle());
        if unsafe { AssignProcessToJobObject(handle, process) }.is_err() {
            kill_child_and_reap(&mut child);
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(());
        }
        if query_job_active_processes(handle) != Some(1)
            || resume_primary_thread(child.id(), handle).is_err()
        {
            let _ = unsafe { TerminateJobObject(handle, 1) };
            reap_child_bounded(&mut child);
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(());
        }
        Ok((child, Self { handle }))
    }

    fn terminate(&mut self, child: &mut Child) {
        self.terminate_descendants();
        reap_child_bounded(child);
    }

    fn terminate_descendants(&mut self) {
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(unix)]
struct ProcessTree {
    process_group: i32,
}

#[cfg(unix)]
impl ProcessTree {
    fn spawn(command: &mut Command) -> Result<(Child, Self), ()> {
        let mut child = command.spawn().map_err(|_| ())?;
        let process_group = match i32::try_from(child.id()) {
            Ok(process_group) => process_group,
            Err(_) => {
                kill_child_and_reap(&mut child);
                return Err(());
            }
        };
        if process_group <= 0 {
            kill_child_and_reap(&mut child);
            return Err(());
        }
        Ok((child, Self { process_group }))
    }

    fn terminate(&mut self, child: &mut Child) {
        self.terminate_descendants();
        reap_child_bounded(child);
    }

    fn terminate_descendants(&mut self) {
        // `Command::new("kill")` would trust the ambient PATH during cleanup
        // and can leave an inherited stdout pipe open if that utility is
        // replaced or unavailable. Kill the private process group directly.
        let _ = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
    }
}

#[cfg(not(any(windows, unix)))]
struct ProcessTree;

#[cfg(not(any(windows, unix)))]
impl ProcessTree {
    fn spawn(command: &mut Command) -> Result<(Child, Self), ()> {
        command.spawn().map(|child| (child, Self)).map_err(|_| ())
    }
    fn terminate(&mut self, child: &mut Child) {
        kill_child_and_reap(child);
    }
    fn terminate_descendants(&mut self) {}
}

#[cfg(windows)]
fn query_job_active_processes(handle: HANDLE) -> Option<u32> {
    use windows::Win32::System::JobObjects::{
        JobObjectBasicAccountingInformation, QueryInformationJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };
    let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            Some(handle),
            JobObjectBasicAccountingInformation,
            (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            None,
        )
    }
    .ok()
    .map(|_| accounting.ActiveProcesses)
}

#[cfg(windows)]
fn resume_primary_thread(pid: u32, job: HANDLE) -> Result<(), ()> {
    use windows::Win32::Foundation::{GetLastError, ERROR_NO_MORE_FILES};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(|_| ())?;
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut thread_id = None;
    if unsafe { Thread32First(snapshot, &mut entry) }.is_err() {
        unsafe {
            let _ = CloseHandle(snapshot);
        }
        return Err(());
    }
    loop {
        if entry.th32OwnerProcessID == pid && thread_id.replace(entry.th32ThreadID).is_some() {
            unsafe {
                let _ = CloseHandle(snapshot);
            }
            return Err(());
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        if unsafe { Thread32Next(snapshot, &mut entry) }.is_err() {
            let finished = unsafe { GetLastError() } == ERROR_NO_MORE_FILES;
            unsafe {
                let _ = CloseHandle(snapshot);
            }
            if !finished {
                return Err(());
            }
            break;
        }
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, thread_id.ok_or(())?) }
        .map_err(|_| ())?;
    if query_job_active_processes(job) != Some(1) {
        unsafe {
            let _ = CloseHandle(thread);
        }
        return Err(());
    }
    let previous_suspend_count = unsafe { ResumeThread(thread) };
    unsafe {
        let _ = CloseHandle(thread);
    }
    (previous_suspend_count == 1).then_some(()).ok_or(())
}

fn file_identity(_file: &File, metadata: &fs::Metadata) -> FileIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return FileIdentity {
            device: Some(metadata.dev()),
            inode: Some(metadata.ino()),
            size: metadata.len(),
            modified_millis: modified_millis(metadata),
        };
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };

        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        let handle = HANDLE(_file.as_raw_handle());
        let (device, inode) =
            if unsafe { GetFileInformationByHandle(handle, &mut information) }.is_ok() {
                (
                    Some(u64::from(information.dwVolumeSerialNumber)),
                    Some(
                        (u64::from(information.nFileIndexHigh) << 32)
                            | u64::from(information.nFileIndexLow),
                    ),
                )
            } else {
                (None, None)
            };
        return FileIdentity {
            device,
            inode,
            size: metadata.len(),
            modified_millis: modified_millis(metadata),
        };
    }
    #[allow(unreachable_code)]
    FileIdentity {
        device: None,
        inode: None,
        size: metadata.len(),
        modified_millis: modified_millis(metadata),
    }
}

fn modified_millis(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
}

fn same_file_identity(previous: Option<&FileIdentity>, current: &FileIdentity) -> bool {
    let Some(previous) = previous else {
        return true;
    };
    match (
        (previous.device, previous.inode),
        (current.device, current.inode),
    ) {
        ((Some(previous_device), Some(previous_inode)), (Some(device), Some(inode))) => {
            previous_device == device && previous_inode == inode
        }
        ((None, None), (None, None)) => true,
        // If only one side has a usable identity, conservative rotation is
        // safer than silently skipping bytes from a replacement file.
        _ => false,
    }
}

fn cursor_anchor_hash(file: &mut File, offset: u64) -> Result<String, CoreError> {
    let start = offset.saturating_sub(CURSOR_ANCHOR_BYTES);
    file.seek(SeekFrom::Start(start))
        .map_err(|_| CoreError::Io)?;
    let mut remaining = offset.saturating_sub(start);
    let mut hash = 0xcbf29ce484222325_u64;
    hash_bytes(&mut hash, &offset.to_le_bytes());
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    while remaining > 0 {
        let read_len = usize::try_from(remaining)
            .unwrap_or(READ_CHUNK_BYTES)
            .min(chunk.len());
        let read = file
            .read(&mut chunk[..read_len])
            .map_err(|_| CoreError::Io)?;
        if read == 0 {
            return Err(CoreError::Io);
        }
        hash_bytes(&mut hash, &chunk[..read]);
        remaining = remaining.saturating_sub(read as u64);
    }
    Ok(format!("{hash:016x}"))
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut table = vec![vec![false; value.len() + 1]; pattern.len() + 1];
    table[0][0] = true;
    for index in 0..pattern.len() {
        if pattern[index] == b'*' && table[index][0] {
            table[index + 1][0] = true;
        }
        for value_index in 0..value.len() {
            if !table[index][value_index] {
                continue;
            }
            match pattern[index] {
                b'*' => {
                    table[index + 1][value_index] = true;
                    table[index][value_index + 1] = true;
                }
                b'?' => {
                    table[index + 1][value_index + 1] = true;
                }
                character if character == value[value_index] => {
                    table[index + 1][value_index + 1] = true;
                }
                _ => {}
            }
        }
    }
    table[pattern.len()][value.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CancellationToken, OperationRegistry};
    use std::fs::OpenOptions;
    use std::io::Write;

    fn context<'a>(
        id: &'a str,
        generation: u64,
        token: &'a CancellationToken,
        registry: &'a OperationRegistry,
    ) -> LoadContext<'a> {
        LoadContext::new(id, generation, token, registry)
    }

    #[test]
    fn fixed_adapters_never_accept_a_command_string() {
        let plan = adapter_argv(&SourceSpec::WslFile {
            distro: "Ubuntu".to_string(),
            path: "/var/log/app.log".to_string(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(plan.program, "wsl.exe");
        assert_eq!(
            plan.args,
            vec!["-d", "Ubuntu", "--", "cat", "--", "/var/log/app.log"]
        );
        let journal = adapter_argv(&SourceSpec::WslJournal {
            distro: "Ubuntu".to_string(),
            unit: Some("sshd.service".to_string()),
        })
        .unwrap()
        .unwrap();
        assert!(journal.args.ends_with(&["--unit=sshd.service".to_string()]));
        let container = adapter_argv(&SourceSpec::Container {
            engine: super::super::model::ContainerEngine::Docker,
            container_id: "abc123".to_string(),
        })
        .unwrap()
        .unwrap();
        assert_eq!(container.args.last(), Some(&"abc123".to_string()));
        assert_eq!(container.args[container.args.len() - 2], "--");
        assert!(adapter_argv(&SourceSpec::Run {
            source_id: "run-manager:run-1:stdout".to_string(),
        })
        .unwrap()
        .is_none());
    }

    #[test]
    fn run_manager_root_identifier_matches_the_release_catalog() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../../../catalog.json")).unwrap();
        let identifier = catalog["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|app| app["id"] == "run-manager")
            .and_then(|app| app["identifier"].as_str());
        assert_eq!(identifier, Some(RUN_MANAGER_IDENTIFIER));
    }

    #[test]
    fn webhook_capture_loads_one_sanitized_ephemeral_record() {
        let capture = devbox_applink::webhook_log_payload(
            "POST",
            "/hooks?access_token=raw-query-secret",
            42,
            &[
                ("Authorization".into(), "Bearer raw-header-secret".into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            r#"{"password":"raw-body-secret"}"#,
        )
        .unwrap();
        let source = SourceSpec::WebhookCapture { capture };
        let registry = OperationRegistry::default();
        let token = registry.begin("webhook-read", 3).unwrap();

        let snapshot = load_source(
            &source,
            None,
            9,
            &context("webhook-read", 3, &token, &registry),
        )
        .unwrap();

        assert_eq!(snapshot.operation_id, "webhook-read");
        assert_eq!(snapshot.generation, 3);
        assert_eq!(snapshot.source.kind, SourceKind::WebhookCapture);
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.next_cursor, None);
        assert_eq!(snapshot.status, ReadStatus::Initial);
        let record = &snapshot.records[0];
        assert_eq!(record.sequence, 9);
        assert_eq!(record.timestamp_millis, Some(42));
        assert_eq!(
            record.fields.get("method").map(String::as_str),
            Some("POST")
        );
        assert_eq!(
            record.fields.get("headerNames").map(String::as_str),
            Some("Authorization, Content-Type")
        );
        assert_eq!(
            record.fields.get("redacted").map(String::as_str),
            Some("true")
        );
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(encoded.contains("[REDACTED]"));
        for secret in [
            "raw-query-secret",
            "raw-header-secret",
            "application/json",
            "raw-body-secret",
        ] {
            assert!(!encoded.contains(secret));
        }
    }

    #[test]
    fn run_source_reads_rotation_order_and_resumes_by_logical_offset() {
        let root = tempfile::tempdir().unwrap();
        let run_directory = root.path().join("logs/runs/run-1");
        fs::create_dir_all(&run_directory).unwrap();
        // Generation 10 sorts before 8 and 9 lexicographically. The adapter
        // must instead follow the encoded logical offsets.
        fs::write(run_directory.join("stdout.g8.o0-3.log"), b"old").unwrap();
        fs::write(run_directory.join("stdout.g9.o3-6.log"), b"mid").unwrap();
        fs::write(run_directory.join("stdout.g10.o6-9.log"), b"new").unwrap();
        fs::write(run_directory.join("stderr.g0.o0-5.log"), b"error").unwrap();
        fs::write(run_directory.join("stdout.manifest.json"), b"ignored").unwrap();

        let registry = OperationRegistry::default();
        let token = registry.begin("run-read", 1).unwrap();
        let first = read_run_source_in(
            root.path(),
            "run-manager:run-1:stdout",
            None,
            &context("run-read", 1, &token, &registry),
        )
        .unwrap();
        assert_eq!(first.bytes, b"oldmidnew");
        assert_eq!(first.status, ReadStatus::Initial);
        assert_eq!(first.cursor.offset, "9");
        assert!(first.cursor.identity.is_none());
        assert!(first.cursor.anchor_hash.is_none());

        let stderr = read_run_source_in(
            root.path(),
            "run-manager:run-1:stderr",
            None,
            &context("run-read", 1, &token, &registry),
        )
        .unwrap();
        assert_eq!(stderr.bytes, b"error");
        assert_eq!(stderr.cursor.offset, "5");

        fs::write(run_directory.join("stdout.g11.o9-13.log"), b"tail").unwrap();
        let second = read_run_source_in(
            root.path(),
            "run-manager:run-1:stdout",
            Some(&first.cursor),
            &context("run-read", 1, &token, &registry),
        )
        .unwrap();
        assert_eq!(second.bytes, b"tail");
        assert_eq!(second.status, ReadStatus::Advanced);
        assert_eq!(second.cursor.offset, "13");
    }

    #[test]
    fn run_source_reports_rotation_and_rejects_malformed_segments() {
        let root = tempfile::tempdir().unwrap();
        let run_directory = root.path().join("logs/runs/run-1");
        fs::create_dir_all(&run_directory).unwrap();
        fs::write(run_directory.join("stdout.g4.o10-13.log"), b"new").unwrap();
        let registry = OperationRegistry::default();
        let token = registry.begin("run-rotation", 1).unwrap();
        let stale = FileCursor {
            identity: None,
            offset: "4".to_string(),
            anchor_hash: None,
        };
        let rotated = read_run_source_in(
            root.path(),
            "run-manager:run-1:stdout",
            Some(&stale),
            &context("run-rotation", 1, &token, &registry),
        )
        .unwrap();
        assert_eq!(rotated.bytes, b"new");
        assert_eq!(rotated.status, ReadStatus::Truncated);
        assert!(rotated.truncated);
        assert_eq!(rotated.cursor.offset, "13");

        let malformed = run_directory.join("stdout.gbad.o13-14.log");
        fs::write(&malformed, b"x").unwrap();
        assert!(matches!(
            read_run_source_in(
                root.path(),
                "run-manager:run-1:stdout",
                None,
                &context("run-rotation", 1, &token, &registry),
            ),
            Err(CoreError::InvalidSource)
        ));
        fs::remove_file(malformed).unwrap();
        fs::write(run_directory.join("stdout.g5.o12-14.log"), b"xx").unwrap();
        assert!(matches!(
            read_run_source_in(
                root.path(),
                "run-manager:run-1:stdout",
                None,
                &context("run-rotation", 1, &token, &registry),
            ),
            Err(CoreError::InvalidSource)
        ));
    }

    #[test]
    fn run_source_enforces_segment_count_and_size_before_reading() {
        let root = tempfile::tempdir().unwrap();
        let run_directory = root.path().join("logs/runs/run-1");
        fs::create_dir_all(&run_directory).unwrap();
        for index in 0..=MAX_RUN_SEGMENTS {
            fs::write(
                run_directory.join(format!("stdout.g{index}.o{index}-{}.log", index + 1)),
                b"x",
            )
            .unwrap();
        }
        let registry = OperationRegistry::default();
        let token = registry.begin("run-limits", 1).unwrap();
        assert!(matches!(
            read_run_source_in(
                root.path(),
                "run-manager:run-1:stdout",
                None,
                &context("run-limits", 1, &token, &registry),
            ),
            Err(CoreError::OutputLimit)
        ));

        let oversized_root = tempfile::tempdir().unwrap();
        let oversized_directory = oversized_root.path().join("logs/runs/run-1");
        fs::create_dir_all(&oversized_directory).unwrap();
        let oversized_length = RUN_SEGMENT_BYTES + 1;
        File::create(oversized_directory.join(format!("stdout.g0.o0-{oversized_length}.log")))
            .unwrap()
            .set_len(oversized_length)
            .unwrap();
        assert!(matches!(
            read_run_source_in(
                oversized_root.path(),
                "run-manager:run-1:stdout",
                None,
                &context("run-limits", 1, &token, &registry),
            ),
            Err(CoreError::InvalidSource)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn run_source_rejects_a_linked_run_directory() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("logs/runs")).unwrap();
        fs::write(outside.path().join("stdout.g0.o0-3.log"), b"bad").unwrap();
        symlink(outside.path(), root.path().join("logs/runs/run-1")).unwrap();
        let registry = OperationRegistry::default();
        let token = registry.begin("run-link", 1).unwrap();
        assert!(matches!(
            read_run_source_in(
                root.path(),
                "run-manager:run-1:stdout",
                None,
                &context("run-link", 1, &token, &registry),
            ),
            Err(CoreError::AdapterUnavailable)
        ));
    }

    #[test]
    fn local_cursor_detects_append_and_truncate() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.log");
        let mut file = File::create(&path).unwrap();
        writeln!(file, "INFO first").unwrap();
        let registry = OperationRegistry::default();
        let token = registry.begin("read-1", 1).unwrap();
        let first = load_source(
            &SourceSpec::LocalFile {
                path: path.to_string_lossy().into_owned(),
            },
            None,
            0,
            &context("read-1", 1, &token, &registry),
        )
        .unwrap();
        assert_eq!(first.status, ReadStatus::Initial);
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "INFO second").unwrap();
        let second = load_source(
            &SourceSpec::LocalFile {
                path: path.to_string_lossy().into_owned(),
            },
            first.next_cursor.as_ref(),
            1,
            &context("read-1", 1, &token, &registry),
        )
        .unwrap();
        assert_eq!(second.status, ReadStatus::Advanced);
        assert_eq!(second.records.len(), 1);
        std::fs::write(&path, b"INFO reset\n").unwrap();
        let third = load_source(
            &SourceSpec::LocalFile {
                path: path.to_string_lossy().into_owned(),
            },
            second.next_cursor.as_ref(),
            2,
            &context("read-1", 1, &token, &registry),
        )
        .unwrap();
        assert_eq!(third.status, ReadStatus::Truncated);
        assert_eq!(third.records[0].message, "INFO reset");
    }

    #[test]
    fn local_cursor_detects_truncate_and_regrow_with_same_inode() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app.log");
        std::fs::write(&path, b"INFO first\nINFO second\n").unwrap();
        let registry = OperationRegistry::default();
        let token = registry.begin("read-regrow", 1).unwrap();
        let source = SourceSpec::LocalFile {
            path: path.to_string_lossy().into_owned(),
        };
        let first = load_source(
            &source,
            None,
            0,
            &context("read-regrow", 1, &token, &registry),
        )
        .unwrap();
        let old_inode = first
            .next_cursor
            .as_ref()
            .and_then(|cursor| cursor.identity.as_ref().and_then(|identity| identity.inode));

        // The replacement is longer than the old file, so an offset-only
        // cursor would incorrectly classify it as an append. `write` keeps
        // the inode while changing the bytes before the cursor anchor.
        std::fs::write(
            &path,
            b"INFO reset after truncate\nINFO second replacement\nINFO third\n",
        )
        .unwrap();
        let second = load_source(
            &source,
            first.next_cursor.as_ref(),
            2,
            &context("read-regrow", 1, &token, &registry),
        )
        .unwrap();
        assert_eq!(
            second.next_cursor.as_ref().and_then(|cursor| {
                cursor.identity.as_ref().and_then(|identity| identity.inode)
            }),
            old_inode
        );
        assert_eq!(second.status, ReadStatus::Truncated);
        assert_eq!(second.records[0].message, "INFO reset after truncate");
    }

    #[test]
    fn deadline_is_checked_before_adapter_work() {
        let registry = OperationRegistry::default();
        let token = registry.begin("deadline", 1).unwrap();
        let mut context = context("deadline", 1, &token, &registry);
        context.deadline = Instant::now();
        assert_eq!(context.check(), Err(CoreError::Timeout));
    }

    #[cfg(unix)]
    #[test]
    fn adapter_root_exit_closes_a_descendant_inherited_pipe() {
        let registry = OperationRegistry::default();
        let token = registry.begin("adapter-descendant", 1).unwrap();
        let mut context = context("adapter-descendant", 1, &token, &registry);
        context.deadline = Instant::now() + Duration::from_secs(2);
        let plan = AdapterPlan {
            program: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                "sleep 30 & printf 'INFO root\\n'".to_string(),
            ],
            source_kind: SourceKind::WslJournal,
            read_only: true,
        };

        let started = Instant::now();
        let bytes = run_fixed_adapter(&plan, &context).expect("adapter output");

        assert_eq!(bytes, b"INFO root\n");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn directory_pattern_is_sorted_and_bounded() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("b.log"), b"b\n").unwrap();
        std::fs::write(directory.path().join("a.log"), b"a\n").unwrap();
        assert!(wildcard_match("*.log", "a.log"));
        assert!(!wildcard_match("*.log", "a.txt"));
    }

    #[test]
    fn directory_members_are_separated_when_a_file_has_no_newline() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("a.log"), b"INFO first").unwrap();
        std::fs::write(directory.path().join("b.log"), b"INFO second").unwrap();
        let source = SourceSpec::Directory {
            path: directory.path().to_string_lossy().into_owned(),
            pattern: "*.log".to_string(),
        };
        let registry = OperationRegistry::default();
        let token = registry.begin("directory-separator", 1).unwrap();
        let snapshot = load_source(
            &source,
            None,
            0,
            &context("directory-separator", 1, &token, &registry),
        )
        .unwrap();

        assert_eq!(
            snapshot
                .records
                .iter()
                .map(|record| record.message.as_str())
                .collect::<Vec<_>>(),
            vec!["INFO first", "INFO second"]
        );
    }

    #[test]
    fn directory_source_reports_matching_file_cap() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..=MAX_DIRECTORY_FILES {
            std::fs::write(
                directory.path().join(format!("{index:03}.log")),
                format!("INFO entry {index}\n"),
            )
            .unwrap();
        }
        let source = SourceSpec::Directory {
            path: directory.path().to_string_lossy().into_owned(),
            pattern: "*.log".to_string(),
        };
        let registry = OperationRegistry::default();
        let token = registry.begin("directory-cap", 1).unwrap();
        let snapshot = load_source(
            &source,
            None,
            0,
            &context("directory-cap", 1, &token, &registry),
        )
        .unwrap();

        assert_eq!(snapshot.records.len(), MAX_DIRECTORY_FILES);
        assert_eq!(snapshot.status, ReadStatus::Truncated);
        assert!(snapshot.truncated);
    }

    #[test]
    fn cancellation_is_observed_before_read() {
        let registry = OperationRegistry::default();
        let token = registry.begin("read-1", 1).unwrap();
        token.cancel();
        let context = context("read-1", 1, &token, &registry);
        assert_eq!(context.check(), Err(CoreError::OperationCancelled));
    }

    #[test]
    fn partial_file_identity_fails_closed_to_rotation() {
        let previous = FileIdentity {
            device: Some(1),
            inode: Some(2),
            size: 10,
            modified_millis: None,
        };
        let current = FileIdentity {
            device: None,
            inode: None,
            size: 10,
            modified_millis: None,
        };
        assert!(!same_file_identity(Some(&previous), &current));
    }
}
