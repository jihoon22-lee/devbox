use super::buffer::RingBuffer;
use super::lifecycle::{CancellationToken, OperationRegistry};
use super::model::{
    CoreError, FileCursor, FileIdentity, ReadStatus, SourceKind, SourceSnapshot, SourceSpec,
    MAX_SOURCE_BYTES,
};
use super::parser::parse_bytes;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
#[cfg(windows)]
use std::mem::size_of;
use std::path::Path;
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
        SourceSpec::Run { .. } => return Ok(None),
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
        SourceSpec::Run { .. } => return Err(CoreError::AdapterUnavailable),
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
    let mut child = command.spawn().map_err(|_| CoreError::AdapterUnavailable)?;
    let mut process_tree = match ProcessTree::assign_to(&child) {
        Ok(process_tree) => process_tree,
        Err(()) => {
            // Fail closed if the platform cannot establish ownership. A
            // detached adapter must never survive a cancelled/read-limited
            // operation, even if only the root process can be reaped here.
            kill_child_and_reap(&mut child);
            return Err(CoreError::AdapterUnavailable);
        }
    };
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
    let mut child_done = false;
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
        if !child_done {
            child_done = match child.try_wait() {
                Ok(status) => status.is_some(),
                Err(_) => {
                    terminate_child(&mut child, &mut process_tree, receiver, reader);
                    return Err(CoreError::Io);
                }
            };
        }
        if child_done {
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
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
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
    };
    // The root adapter may exit while a helper still owns stdout. Terminate
    // only descendants owned by this operation before joining the reader so a
    // successful operation cannot leak a pipe/thread indefinitely.
    process_tree.terminate_descendants();
    drop(receiver);
    let _ = reader.join();
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
    let _ = reader.join();
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
    fn assign_to(child: &Child) -> Result<Self, ()> {
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
        let process = HANDLE(child.as_raw_handle());
        if unsafe { AssignProcessToJobObject(handle, process) }.is_err() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(());
        }
        Ok(Self { handle })
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
    fn assign_to(child: &Child) -> Result<Self, ()> {
        let process_group = i32::try_from(child.id()).map_err(|_| ())?;
        (process_group > 0)
            .then_some(Self { process_group })
            .ok_or(())
    }

    fn terminate(&mut self, child: &mut Child) {
        self.terminate_descendants();
        reap_child_bounded(child);
    }

    fn terminate_descendants(&mut self) {
        let process_group = format!("-{}", self.process_group);
        let result = Command::new("kill")
            .args(["-KILL", "--", process_group.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !result.is_ok_and(|status| status.success()) {
            // The group may already have disappeared after a normal exit.
        }
    }
}

#[cfg(not(any(windows, unix)))]
struct ProcessTree;

#[cfg(not(any(windows, unix)))]
impl ProcessTree {
    fn assign_to(_child: &Child) -> Result<Self, ()> {
        Ok(Self)
    }
    fn terminate(&mut self, child: &mut Child) {
        kill_child_and_reap(child);
    }
    fn terminate_descendants(&mut self) {}
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
