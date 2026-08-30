//! Parent-directory watcher for open documents.
//!
//! A document's parent is watched instead of the file itself so editors that
//! save by delete/create (and tools such as `git checkout`) do not silently
//! lose the registration.  Native callbacks only enqueue paths; one
//! application-lifetime worker owns the quiet-period debounce and delivery.

use crate::core::guard::MAX_OPENABLE_BYTES;
use devbox_filesystem::{filesystem_identity, FilesystemIdentity};
use notify::event::{CreateKind, EventKind, ModifyKind, RemoveKind};
use notify::{Event, RecursiveMode, Watcher};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    Arc, Mutex, Weak,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);
pub const MAX_PENDING_WATCH_PATHS: usize = 4_096;
pub const MAX_READY_WATCH_PATHS: usize = 512;
pub const MAX_WATCH_REGISTRATIONS: usize = 512;
pub const MAX_EVENT_PATHS: usize = 256;
pub const MAX_WATCH_PATH_BYTES: usize = 32 * 1024;
const MAX_WATCH_MESSAGES: usize = 1_024;
const WORKER_TICK: Duration = Duration::from_millis(500);
const WSL_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileChangedEvent {
    pub path: String,
    /// Decimal epoch nanoseconds preserve the full i64 timestamp at the JS
    /// boundary; JavaScript numbers are not lossless for current epoch values.
    pub mtime_nanos: String,
    pub content_hash: String,
    pub size: u64,
}

#[derive(Debug)]
pub struct Debouncer {
    window: Duration,
    pending: HashMap<PathBuf, Instant>,
    overflow: bool,
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
            overflow: false,
        }
    }

    /// Records an event. A later event resets the quiet-period clock.
    pub fn record(&mut self, path: &Path, now: Instant) -> bool {
        if path_byte_len(path) > MAX_WATCH_PATH_BYTES
            || (!self.pending.contains_key(path) && self.pending.len() >= MAX_PENDING_WATCH_PATHS)
        {
            self.overflow = true;
            return false;
        }
        self.pending.insert(path.to_path_buf(), now);
        true
    }

    pub fn cancel(&mut self, path: &Path) {
        self.pending.remove(path);
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|seen| *seen + self.window).min()
    }

    /// Removes paths that have been quiet for the configured window.
    pub fn take_ready(&mut self, now: Instant) -> Vec<PathBuf> {
        let mut ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, seen)| now.saturating_duration_since(**seen) >= self.window)
            .map(|(path, _)| path.clone())
            .collect();
        ready.sort();
        if ready.len() > MAX_READY_WATCH_PATHS {
            ready.truncate(MAX_READY_WATCH_PATHS);
            self.overflow = true;
        }
        for path in &ready {
            self.pending.remove(path);
        }
        ready
    }

    fn take_overflow(&mut self) -> bool {
        std::mem::take(&mut self.overflow)
    }
}

/// Mutation events include remove/create pairs, which is important for atomic
/// replacement saves. Access-only notifications are intentionally ignored.
pub fn is_mutation_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(CreateKind::Any | CreateKind::File)
            | EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Metadata(_))
            | EventKind::Modify(ModifyKind::Name(_))
            | EventKind::Modify(ModifyKind::Other)
            | EventKind::Remove(RemoveKind::Any | RemoveKind::File)
    )
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left_text = left.to_string_lossy();
    let right_text = right.to_string_lossy();
    let left_wsl = devbox_wsl::path::parse_wsl_unc_path(&left_text)
        .ok()
        .flatten();
    let right_wsl = devbox_wsl::path::parse_wsl_unc_path(&right_text)
        .ok()
        .flatten();
    match (left_wsl, right_wsl) {
        (Some(left), Some(right)) => return left.identity() == right.identity(),
        (Some(_), None) | (None, Some(_)) => return false,
        (None, None) => {}
    }
    #[cfg(windows)]
    {
        left_text.eq_ignore_ascii_case(&right_text)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn same_filename(left: &OsStr, right: &OsStr) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// Returns only event paths belonging to the watched parent and filename set.
/// Paths are kept as received so this helper contains no filesystem I/O.
pub fn matching_event_paths(
    event: &Event,
    parent: &Path,
    watched_filenames: &HashMap<OsString, usize>,
) -> Vec<PathBuf> {
    if !is_mutation_event(&event.kind) {
        return Vec::new();
    }
    let mut paths = Vec::new();
    for path in event.paths.iter().take(MAX_EVENT_PATHS) {
        if path_byte_len(path) > MAX_WATCH_PATH_BYTES {
            continue;
        }
        let Some(event_parent) = path.parent() else {
            continue;
        };
        if !same_path(event_parent, parent) {
            continue;
        }
        let Some(filename) = path.file_name() else {
            continue;
        };
        if watched_filenames
            .keys()
            .any(|watched| same_filename(filename, watched))
            && !paths.iter().any(|existing| existing == path)
        {
            paths.push(path.clone());
        }
    }
    paths
}

/// Reads a replacement-safe snapshot. A metadata/hash event is emitted only
/// when the file was the same regular file before and after the byte read. A
/// short retry budget handles an editor's final write without blocking the
/// watcher forever.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StableSnapshot {
    event: FileChangedEvent,
    identity: FilesystemIdentity,
    mtime_nanos: i64,
}

fn metadata_snapshot(path: &Path) -> Option<StableSnapshot> {
    for _ in 0..3 {
        let canonical = path.canonicalize().ok()?;
        let before = std::fs::metadata(&canonical).ok()?;
        if !before.is_file() || before.len() > MAX_OPENABLE_BYTES {
            return None;
        }
        let before_identity = filesystem_identity(&canonical, false).ok()?;
        let file = std::fs::File::open(&canonical).ok()?;
        // A file can grow after the metadata check. Read one byte beyond the
        // shared open limit so growth is rejected without an unbounded
        // allocation or hash event.
        let bytes = read_bounded(file, MAX_OPENABLE_BYTES)?;
        let after = std::fs::metadata(&canonical).ok()?;
        if !after.is_file() || after.len() > MAX_OPENABLE_BYTES || before.len() != after.len() {
            continue;
        }
        let after_identity = filesystem_identity(&canonical, false).ok()?;
        let before_mtime = modified_epoch_nanos(&before)?;
        let after_mtime = modified_epoch_nanos(&after)?;
        if before_mtime != after_mtime
            || before_identity != after_identity
            || bytes.len() as u64 != after.len()
        {
            continue;
        }
        let content_hash = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        return Some(StableSnapshot {
            event: FileChangedEvent {
                path: canonical.to_string_lossy().into_owned(),
                mtime_nanos: after_mtime.to_string(),
                content_hash,
                size: after.len(),
            },
            identity: after_identity,
            mtime_nanos: after_mtime,
        });
    }
    None
}

/// Fast WSL polling probe. File bytes are hashed only after size, mtime, or
/// filesystem identity changes from the last delivered stable snapshot.
fn snapshot_metadata_unchanged(path: &Path, previous: &StableSnapshot) -> bool {
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    let Ok(metadata) = std::fs::metadata(&canonical) else {
        return false;
    };
    metadata.is_file()
        && metadata.len() == previous.event.size
        && modified_epoch_nanos(&metadata).is_some_and(|mtime| {
            mtime == previous.mtime_nanos
                && filesystem_identity(&canonical, false).ok().as_ref() == Some(&previous.identity)
        })
}

fn path_byte_len(path: &Path) -> usize {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().len()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().count().saturating_mul(2)
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.to_string_lossy().len()
    }
}

fn is_wsl_path(path: &Path) -> bool {
    devbox_wsl::path::parse_wsl_unc_path(&path.to_string_lossy())
        .ok()
        .flatten()
        .is_some()
}

fn read_bounded(mut reader: impl Read, max_bytes: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= max_bytes).then_some(bytes)
}

fn modified_epoch_nanos(metadata: &std::fs::Metadata) -> Option<i64> {
    let nanos = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    i64::try_from(nanos).ok()
}

struct DirectoryRegistration {
    #[allow(dead_code)]
    watcher: notify::RecommendedWatcher,
    /// A registration is reference-counted because the same path may be
    /// opened in both fixed views. The parent watcher is dropped only after
    /// the final file registration is removed.
    filenames: HashMap<OsString, usize>,
}

struct PollRegistration {
    references: usize,
    snapshot: Option<StableSnapshot>,
}

struct WatcherState {
    registrations: HashMap<PathBuf, DirectoryRegistration>,
    polling: HashMap<PathBuf, PollRegistration>,
    /// Generation changes invalidate messages already queued for an
    /// unregistering file. A path can be registered again with a new gen.
    generations: HashMap<PathBuf, u64>,
    next_generation: u64,
}

enum WatcherMessage {
    Candidate { path: PathBuf, generation: u64 },
    Invalidate { path: PathBuf, generation: u64 },
    Wake,
    Shutdown,
}

/// Application-lifetime manager. Each parent directory has one native watcher
/// and a filename refcount map, while one worker handles all debounce timers.
pub struct WatcherManager {
    app: AppHandle,
    state: Arc<Mutex<WatcherState>>,
    sender: SyncSender<WatcherMessage>,
    recheck_all: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl WatcherManager {
    pub fn new(app: AppHandle) -> Arc<Self> {
        let state = Arc::new(Mutex::new(WatcherState {
            registrations: HashMap::new(),
            polling: HashMap::new(),
            generations: HashMap::new(),
            next_generation: 1,
        }));
        let (sender, receiver) = mpsc::sync_channel(MAX_WATCH_MESSAGES);
        let recheck_all = Arc::new(AtomicBool::new(false));
        let worker_state = Arc::downgrade(&state);
        let worker_app = app.clone();
        let worker_recheck = Arc::clone(&recheck_all);
        let worker = thread::Builder::new()
            .name("code-pad-watcher".to_string())
            .spawn(move || watcher_worker(worker_state, worker_app, receiver, worker_recheck))
            .expect("code-pad watcher worker should start");
        Arc::new(Self {
            app,
            state,
            sender,
            recheck_all,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Registers a canonical file's parent. If native watcher creation or
    /// this registration fails, existing registrations remain untouched.
    pub fn register(&self, path: &Path) -> Result<(), String> {
        let canonical = path
            .canonicalize()
            .map_err(|_| "파일 감시 경로를 확인할 수 없습니다".to_string())?;
        if !canonical.is_file() {
            return Err("파일 감시 대상이 일반 파일이 아닙니다".to_string());
        }
        if path_byte_len(&canonical) > MAX_WATCH_PATH_BYTES {
            return Err("파일 감시 경로가 너무 깁니다".to_string());
        }

        if is_wsl_path(&canonical) {
            let baseline = metadata_snapshot(&canonical);
            let mut state = self
                .state
                .lock()
                .map_err(|_| "파일 감시 상태가 손상되었습니다".to_string())?;
            let existing_key = state
                .polling
                .keys()
                .find(|registered| same_path(registered, &canonical))
                .cloned();
            if let Some(key) = existing_key {
                let registration = state.polling.get_mut(&key).expect("key exists");
                registration.references = registration.references.saturating_add(1);
            } else {
                ensure_registration_capacity(&state, &canonical)?;
                state.polling.insert(
                    canonical.clone(),
                    PollRegistration {
                        references: 1,
                        snapshot: baseline,
                    },
                );
                allocate_generation(&mut state, canonical);
            }
            drop(state);
            let _ = self.sender.try_send(WatcherMessage::Wake);
            return Ok(());
        }

        let parent = canonical
            .parent()
            .ok_or_else(|| "파일 부모 폴더를 확인할 수 없습니다".to_string())?
            .to_path_buf();
        let filename = canonical
            .file_name()
            .ok_or_else(|| "파일 이름을 확인할 수 없습니다".to_string())?
            .to_os_string();

        let mut state = self
            .state
            .lock()
            .map_err(|_| "파일 감시 상태가 손상되었습니다".to_string())?;
        let registration_capacity_full = state.generations.len() >= MAX_WATCH_REGISTRATIONS;
        if let Some(registration) = state.registrations.get_mut(&parent) {
            let is_new_file = !registration
                .filenames
                .keys()
                .any(|existing| same_filename(existing, &filename));
            if is_new_file && registration_capacity_full {
                return Err("동시에 감시할 수 있는 파일 수를 초과했습니다".to_string());
            }
            let added = increment_filename(&mut registration.filenames, filename);
            if added {
                allocate_generation(&mut state, canonical);
            }
            return Ok(());
        }

        ensure_registration_capacity(&state, &canonical)?;

        let weak_state: Weak<Mutex<WatcherState>> = Arc::downgrade(&self.state);
        let sender = self.sender.clone();
        let recheck_all = Arc::clone(&self.recheck_all);
        let mut watcher = notify::recommended_watcher(move |result| {
            handle_notify_result(&weak_state, &sender, &recheck_all, result);
        })
        .map_err(|_| "파일 감시를 시작할 수 없습니다".to_string())?;
        watcher
            .watch(&parent, RecursiveMode::NonRecursive)
            .map_err(|_| "파일 감시를 등록할 수 없습니다".to_string())?;

        let mut filenames = HashMap::new();
        increment_filename(&mut filenames, filename);
        state
            .registrations
            .insert(parent, DirectoryRegistration { watcher, filenames });
        allocate_generation(&mut state, canonical);
        Ok(())
    }

    /// Removes one file reference and drops the parent watcher when it has no
    /// open documents left. Delivery already queued for the final unregister
    /// is invalidated by removing its generation before the command returns;
    /// a later registration receives a fresh application-lifetime value.
    pub fn unregister(&self, path: &Path) -> Result<(), String> {
        let candidate = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let polling_invalidation = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "파일 감시 상태가 손상되었습니다".to_string())?;
            let key = state
                .polling
                .keys()
                .find(|registered| same_path(registered, &candidate))
                .cloned();
            match key {
                Some(key) => {
                    let registration = state.polling.get_mut(&key).expect("key exists");
                    if registration.references > 1 {
                        registration.references -= 1;
                        return Ok(());
                    }
                    state.polling.remove(&key);
                    let previous_generation = state.generations.remove(&key).unwrap_or(0);
                    Some((key, previous_generation))
                }
                None => None,
            }
        };
        if let Some((path, generation)) = polling_invalidation {
            let _ = self
                .sender
                .try_send(WatcherMessage::Invalidate { path, generation });
            return Ok(());
        }

        let (parent, filename) = unregister_target(path)
            .ok_or_else(|| "파일 부모 폴더를 확인할 수 없습니다".to_string())?;
        let (canonical, generation) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "파일 감시 상태가 손상되었습니다".to_string())?;
            let Some(registration) = state.registrations.get_mut(&parent) else {
                return Ok(());
            };
            let key = registration
                .filenames
                .keys()
                .find(|existing| same_filename(existing, &filename))
                .cloned();
            let Some(key) = key else {
                return Ok(());
            };
            let count = registration.filenames.get_mut(&key).expect("key exists");
            if *count > 1 {
                *count -= 1;
                return Ok(());
            }
            registration.filenames.remove(&key);
            if registration.filenames.is_empty() {
                state.registrations.remove(&parent);
            }
            let canonical = parent.join(&key);
            let previous_generation = state.generations.remove(&canonical).unwrap_or(0);
            (canonical, previous_generation)
        };
        let _ = self.sender.try_send(WatcherMessage::Invalidate {
            path: canonical,
            generation,
        });
        Ok(())
    }

    pub fn registration_count(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.registrations.len() + state.polling.len())
            .unwrap_or_default()
    }
}

impl Drop for WatcherManager {
    fn drop(&mut self) {
        let _ = self.sender.send(WatcherMessage::Shutdown);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
        // Keep the AppHandle field explicit: the worker owns its clone, and
        // this manager owns the application lifetime registration.
        let _ = &self.app;
    }
}

/// Returns true when this is a new watched path rather than another reference.
fn increment_filename(filenames: &mut HashMap<OsString, usize>, filename: OsString) -> bool {
    if let Some((key, count)) = filenames
        .iter_mut()
        .find(|(existing, _)| same_filename(existing, &filename))
    {
        let _ = key;
        *count = count.saturating_add(1);
        return false;
    }
    filenames.insert(filename, 1);
    true
}

fn allocate_generation(state: &mut WatcherState, path: PathBuf) {
    let generation = state.next_generation;
    state.next_generation = state.next_generation.wrapping_add(1).max(1);
    state.generations.insert(path, generation);
}

fn ensure_registration_capacity(state: &WatcherState, path: &Path) -> Result<(), String> {
    if !state.generations.contains_key(path) && state.generations.len() >= MAX_WATCH_REGISTRATIONS {
        return Err("동시에 감시할 수 있는 파일 수를 초과했습니다".to_string());
    }
    Ok(())
}

/// Resolves the registration key even when the file itself has already been
/// deleted. The parent directory is still the key owned by the manager.
fn unregister_target(path: &Path) -> Option<(PathBuf, OsString)> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let parent = path.parent()?.to_path_buf();
    let parent = parent.canonicalize().unwrap_or(parent);
    Some((parent, path.file_name()?.to_os_string()))
}

fn handle_notify_result(
    weak_state: &Weak<Mutex<WatcherState>>,
    sender: &SyncSender<WatcherMessage>,
    recheck_all: &AtomicBool,
    result: notify::Result<Event>,
) {
    let Ok(event) = result else {
        recheck_all.store(true, Ordering::Release);
        let _ = sender.try_send(WatcherMessage::Wake);
        return;
    };
    if event.paths.len() > MAX_EVENT_PATHS
        || event
            .paths
            .iter()
            .any(|path| path_byte_len(path) > MAX_WATCH_PATH_BYTES)
    {
        recheck_all.store(true, Ordering::Release);
    }
    let Some(state_arc) = weak_state.upgrade() else {
        return;
    };
    let candidates = {
        let Ok(state) = state_arc.lock() else {
            return;
        };
        state
            .registrations
            .iter()
            .flat_map(|(parent, registration)| {
                matching_event_paths(&event, parent, &registration.filenames)
                    .into_iter()
                    .filter_map(|path| {
                        let filename = path.file_name()?;
                        // Notify may preserve a different case spelling on
                        // Windows. Reuse the registration's key so the
                        // worker's generation and state lookups use one
                        // canonical identity (including delete events).
                        let registered_filename = registration
                            .filenames
                            .keys()
                            .find(|watched| same_filename(watched, filename))?;
                        let canonical_key = parent.join(registered_filename);
                        let generation = *state.generations.get(&canonical_key).unwrap_or(&0);
                        Some((canonical_key, generation))
                    })
            })
            .collect::<Vec<_>>()
    };
    for (path, generation) in candidates {
        if sender
            .try_send(WatcherMessage::Candidate { path, generation })
            .is_err()
        {
            recheck_all.store(true, Ordering::Release);
            break;
        }
    }
}

fn watcher_worker(
    weak_state: Weak<Mutex<WatcherState>>,
    app: AppHandle,
    receiver: Receiver<WatcherMessage>,
    recheck_all: Arc<AtomicBool>,
) {
    let mut debouncer = Debouncer::new(DEBOUNCE_WINDOW);
    let mut generations = HashMap::<PathBuf, u64>::new();
    let mut next_poll = Instant::now() + WSL_POLL_INTERVAL;
    loop {
        let now = Instant::now();
        let mut wait = WORKER_TICK.min(next_poll.saturating_duration_since(now));
        if let Some(deadline) = debouncer.next_deadline() {
            wait = wait.min(deadline.saturating_duration_since(now));
        }
        let message = match receiver.recv_timeout(wait) {
            Ok(message) => Some(message),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match message {
            Some(WatcherMessage::Candidate { path, generation }) => {
                if is_current_registration(&weak_state, &path, generation) {
                    if debouncer.record(&path, Instant::now()) {
                        generations.insert(path, generation);
                    } else {
                        recheck_all.store(true, Ordering::Release);
                    }
                }
            }
            Some(WatcherMessage::Invalidate { path, generation }) => {
                // The message names the generation that was just removed,
                // not the newly allocated generation.  Equality matters:
                // unregister and re-register may overlap, and an old
                // invalidation must never cancel the new registration's
                // queued candidate.
                if generations.get(&path).copied() == Some(generation) {
                    debouncer.cancel(&path);
                    generations.remove(&path);
                }
            }
            Some(WatcherMessage::Wake) | None => {}
            Some(WatcherMessage::Shutdown) => break,
        }

        if debouncer.take_overflow() {
            recheck_all.store(true, Ordering::Release);
        }
        if recheck_all.swap(false, Ordering::AcqRel) {
            emit_current_snapshots(&weak_state, &app);
        }
        if Instant::now() >= next_poll {
            poll_wsl_files(&weak_state, &app);
            next_poll = Instant::now() + WSL_POLL_INTERVAL;
        }
        deliver_ready(&weak_state, &app, &mut debouncer, &mut generations);
    }
}

fn emit_current_snapshots(weak_state: &Weak<Mutex<WatcherState>>, app: &AppHandle) {
    let registrations = registered_paths(weak_state);
    for (path, generation) in registrations {
        let Some(snapshot) = metadata_snapshot(&path) else {
            continue;
        };
        if is_current_registration(weak_state, &path, generation) {
            let _ = app.emit("file-changed", snapshot.event);
        }
    }
}

fn registered_paths(weak_state: &Weak<Mutex<WatcherState>>) -> Vec<(PathBuf, u64)> {
    let Some(state) = weak_state.upgrade() else {
        return Vec::new();
    };
    let Ok(state) = state.lock() else {
        return Vec::new();
    };
    let mut paths = state
        .registrations
        .iter()
        .flat_map(|(parent, registration)| {
            registration.filenames.keys().map(|filename| {
                let path = parent.join(filename);
                let generation = state.generations.get(&path).copied().unwrap_or(0);
                (path, generation)
            })
        })
        .chain(state.polling.keys().map(|path| {
            let generation = state.generations.get(path).copied().unwrap_or(0);
            (path.clone(), generation)
        }))
        .collect::<Vec<_>>();
    paths.truncate(MAX_READY_WATCH_PATHS);
    paths
}

fn poll_wsl_files(weak_state: &Weak<Mutex<WatcherState>>, app: &AppHandle) {
    let Some(state_arc) = weak_state.upgrade() else {
        return;
    };
    let registrations = {
        let Ok(state) = state_arc.lock() else {
            return;
        };
        state
            .polling
            .iter()
            .take(MAX_READY_WATCH_PATHS)
            .map(|(path, registration)| {
                (
                    path.clone(),
                    state.generations.get(path).copied().unwrap_or(0),
                    registration.snapshot.clone(),
                )
            })
            .collect::<Vec<_>>()
    };

    for (path, generation, previous) in registrations {
        if previous
            .as_ref()
            .is_some_and(|snapshot| snapshot_metadata_unchanged(&path, snapshot))
        {
            continue;
        }
        let Some(current) = metadata_snapshot(&path) else {
            // An offline distribution or a temporarily missing file is not
            // deletion authority. Keep the last known-good stamp.
            continue;
        };
        let changed = previous.as_ref() != Some(&current);
        let accepted = {
            let Ok(mut state) = state_arc.lock() else {
                return;
            };
            if state.generations.get(&path).copied().unwrap_or(0) != generation {
                false
            } else if let Some(registration) = state.polling.get_mut(&path) {
                registration.snapshot = Some(current.clone());
                true
            } else {
                false
            }
        };
        if accepted && changed {
            let _ = app.emit("file-changed", current.event);
        }
    }
}

fn deliver_ready(
    weak_state: &Weak<Mutex<WatcherState>>,
    app: &AppHandle,
    debouncer: &mut Debouncer,
    generations: &mut HashMap<PathBuf, u64>,
) {
    let ready = debouncer.take_ready(Instant::now());
    for path in ready {
        let Some(generation) = generations.remove(&path) else {
            continue;
        };
        if let Some(snapshot) = metadata_snapshot(&path) {
            if is_current_registration(weak_state, &path, generation) {
                let _ = app.emit("file-changed", snapshot.event);
            }
        }
    }
}

fn is_current_registration(
    weak_state: &Weak<Mutex<WatcherState>>,
    path: &Path,
    generation: u64,
) -> bool {
    let Some(state) = weak_state.upgrade() else {
        return false;
    };
    let Ok(state) = state.lock() else {
        return false;
    };
    if state
        .polling
        .keys()
        .any(|registered| same_path(registered, path))
    {
        return state.generations.get(path).copied().unwrap_or(0) == generation;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(filename) = path.file_name() else {
        return false;
    };
    let registered = state.registrations.get(parent).is_some_and(|registration| {
        registration
            .filenames
            .keys()
            .any(|watched| same_filename(watched, filename))
    });
    registered && state.generations.get(path).copied().unwrap_or(0) == generation
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn filters_by_mutation_kind_parent_and_filename() {
        let parent = Path::new("/workspace");
        let watched = HashMap::from([(OsString::from("main.rs"), 1)]);
        let event = Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .add_path(PathBuf::from("/workspace/main.rs"))
        .add_path(PathBuf::from("/workspace/other.rs"))
        .add_path(PathBuf::from("/workspace/src/main.rs"));
        assert_eq!(
            matching_event_paths(&event, parent, &watched),
            vec![PathBuf::from("/workspace/main.rs")]
        );

        let access = Event::new(EventKind::Access(notify::event::AccessKind::Read))
            .add_path(PathBuf::from("/workspace/main.rs"));
        assert!(matching_event_paths(&access, parent, &watched).is_empty());
    }

    #[test]
    fn debounce_waits_for_a_quiet_period_and_coalesces_bursts() {
        let mut debouncer = Debouncer::new(Duration::from_millis(100));
        let path = Path::new("/workspace/main.rs");
        let start = Instant::now();
        debouncer.record(path, start);
        assert!(debouncer
            .take_ready(start + Duration::from_millis(99))
            .is_empty());
        debouncer.record(path, start + Duration::from_millis(99));
        assert!(debouncer
            .take_ready(start + Duration::from_millis(150))
            .is_empty());
        assert_eq!(
            debouncer.take_ready(start + Duration::from_millis(199)),
            vec![PathBuf::from("/workspace/main.rs")]
        );
    }

    #[test]
    fn debounce_and_event_payloads_are_bounded() {
        let mut debouncer = Debouncer::new(Duration::from_millis(1));
        let now = Instant::now();
        for index in 0..=MAX_PENDING_WATCH_PATHS {
            let _ = debouncer.record(Path::new(&format!("note-{index}.md")), now);
        }
        assert!(debouncer.pending.len() <= MAX_PENDING_WATCH_PATHS);
        assert!(debouncer.take_overflow());

        let watched = HashMap::from([(OsString::from("main.rs"), 1)]);
        let mut event = Event::new(EventKind::Modify(ModifyKind::Any));
        for _ in 0..=MAX_EVENT_PATHS {
            event = event.add_path(PathBuf::from("/workspace/main.rs"));
        }
        assert!(matching_event_paths(&event, Path::new("/workspace"), &watched).len() <= 1);
    }

    #[test]
    fn wsl_alias_identity_keeps_linux_tail_case_sensitive() {
        assert!(same_path(
            Path::new("//wsl$/Ubuntu/home/jihoon/프로젝트/Main.rs"),
            Path::new("//?/UNC/wsl.localhost/ubuntu/home/jihoon/프로젝트/Main.rs"),
        ));
        assert!(!same_path(
            Path::new("//wsl$/Ubuntu/home/jihoon/Project/Main.rs"),
            Path::new("//wsl.localhost/ubuntu/home/jihoon/project/Main.rs"),
        ));
        assert!(is_wsl_path(Path::new(
            "//wsl$/Ubuntu/home/jihoon/프로젝트/Main.rs"
        )));
    }

    #[test]
    fn unregister_key_survives_file_deletion() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("main.rs");
        std::fs::write(&path, "fn main() {}\n").unwrap();
        let canonical_parent = directory.path().canonicalize().unwrap();
        let canonical_path = path.canonicalize().unwrap();
        std::fs::remove_file(&path).unwrap();

        let (parent, filename) = unregister_target(&canonical_path).unwrap();
        assert_eq!(parent, canonical_parent);
        assert_eq!(filename, OsString::from("main.rs"));
    }

    #[test]
    fn bounded_snapshot_reader_rejects_growth_past_the_limit() {
        let growing = std::io::Cursor::new(vec![0_u8; 17]);
        assert!(read_bounded(growing, 16).is_none());
        assert_eq!(
            read_bounded(std::io::Cursor::new(vec![0_u8; 16]), 16)
                .unwrap()
                .len(),
            16
        );
    }

    #[test]
    fn oversized_snapshot_is_rejected_before_reading_file_contents() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.bin");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_OPENABLE_BYTES + 1).unwrap();
        assert!(metadata_snapshot(&path).is_none());
    }

    #[test]
    fn metadata_probe_avoids_rehashing_an_unchanged_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        std::fs::write(&path, "unchanged").unwrap();
        let snapshot = metadata_snapshot(&path).unwrap();
        assert!(snapshot_metadata_unchanged(&path, &snapshot));

        std::fs::write(&path, "changed-size").unwrap();
        assert!(!snapshot_metadata_unchanged(&path, &snapshot));
    }

    #[test]
    fn generation_state_is_bounded_to_live_registrations() {
        let mut state = WatcherState {
            registrations: HashMap::new(),
            polling: HashMap::new(),
            generations: HashMap::new(),
            next_generation: 1,
        };
        for index in 0..10_000 {
            let path = PathBuf::from(format!("/workspace/file-{index}.rs"));
            allocate_generation(&mut state, path.clone());
            state.generations.remove(&path);
        }
        assert!(state.generations.is_empty());
        assert_eq!(state.next_generation, 10_001);
    }

    #[test]
    fn registration_capacity_fails_explicitly_instead_of_starving_polling() {
        let mut state = WatcherState {
            registrations: HashMap::new(),
            polling: HashMap::new(),
            generations: HashMap::new(),
            next_generation: 1,
        };
        for index in 0..MAX_WATCH_REGISTRATIONS {
            allocate_generation(&mut state, PathBuf::from(format!("/workspace/{index}.md")));
        }
        assert_eq!(state.generations.len(), MAX_WATCH_REGISTRATIONS);
        assert_eq!(
            ensure_registration_capacity(&state, Path::new("/workspace/overflow.md")),
            Err("동시에 감시할 수 있는 파일 수를 초과했습니다".to_string())
        );
        assert!(ensure_registration_capacity(&state, Path::new("/workspace/0.md")).is_ok());
    }
}
