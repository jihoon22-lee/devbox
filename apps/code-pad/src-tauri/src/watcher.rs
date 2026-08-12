//! Parent-directory watcher for open documents.
//!
//! A document's parent is watched instead of the file itself so editors that
//! save by delete/create (and tools such as `git checkout`) do not silently
//! lose the registration.  Native callbacks only enqueue paths; one
//! application-lifetime worker owns the quiet-period debounce and delivery.

use crate::core::guard::MAX_OPENABLE_BYTES;
use notify::event::{CreateKind, EventKind, ModifyKind, RemoveKind};
use notify::{Event, RecursiveMode, Watcher};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{
    mpsc::{self, Receiver, RecvTimeoutError, Sender},
    Arc, Mutex, Weak,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);

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
}

impl Debouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
        }
    }

    /// Records an event. A later event resets the quiet-period clock.
    pub fn record(&mut self, path: &Path, now: Instant) {
        self.pending.insert(path.to_path_buf(), now);
    }

    pub fn cancel(&mut self, path: &Path) {
        self.pending.remove(path);
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|seen| *seen + self.window).min()
    }

    /// Removes paths that have been quiet for the configured window.
    pub fn take_ready(&mut self, now: Instant) -> Vec<PathBuf> {
        let ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, seen)| now.saturating_duration_since(**seen) >= self.window)
            .map(|(path, _)| path.clone())
            .collect();
        for path in &ready {
            self.pending.remove(path);
        }
        ready
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
    for path in &event.paths {
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
fn metadata_snapshot(path: &Path) -> Option<FileChangedEvent> {
    for _ in 0..3 {
        let canonical = path.canonicalize().ok()?;
        let before = std::fs::metadata(&canonical).ok()?;
        if !before.is_file() || before.len() > MAX_OPENABLE_BYTES {
            return None;
        }
        let file = std::fs::File::open(&canonical).ok()?;
        // A file can grow after the metadata check. Read one byte beyond the
        // shared open limit so growth is rejected without an unbounded
        // allocation or hash event.
        let bytes = read_bounded(file, MAX_OPENABLE_BYTES)?;
        let after = std::fs::metadata(&canonical).ok()?;
        if !after.is_file() || after.len() > MAX_OPENABLE_BYTES || before.len() != after.len() {
            continue;
        }
        let before_mtime = modified_epoch_nanos(&before)?;
        let after_mtime = modified_epoch_nanos(&after)?;
        if before_mtime != after_mtime || bytes.len() as u64 != after.len() {
            continue;
        }
        let content_hash = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        return Some(FileChangedEvent {
            path: canonical.to_string_lossy().into_owned(),
            mtime_nanos: after_mtime.to_string(),
            content_hash,
            size: after.len(),
        });
    }
    None
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

struct WatcherState {
    registrations: HashMap<PathBuf, DirectoryRegistration>,
    /// Generation changes invalidate messages already queued for an
    /// unregistering file. A path can be registered again with a new gen.
    generations: HashMap<PathBuf, u64>,
}

enum WatcherMessage {
    Candidate { path: PathBuf, generation: u64 },
    Invalidate { path: PathBuf, generation: u64 },
    Shutdown,
}

/// Application-lifetime manager. Each parent directory has one native watcher
/// and a filename refcount map, while one worker handles all debounce timers.
pub struct WatcherManager {
    app: AppHandle,
    state: Arc<Mutex<WatcherState>>,
    sender: Sender<WatcherMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl WatcherManager {
    pub fn new(app: AppHandle) -> Arc<Self> {
        let state = Arc::new(Mutex::new(WatcherState {
            registrations: HashMap::new(),
            generations: HashMap::new(),
        }));
        let (sender, receiver) = mpsc::channel();
        let worker_state = Arc::downgrade(&state);
        let worker_app = app.clone();
        let worker = thread::Builder::new()
            .name("code-pad-watcher".to_string())
            .spawn(move || watcher_worker(worker_state, worker_app, receiver))
            .expect("code-pad watcher worker should start");
        Arc::new(Self {
            app,
            state,
            sender,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Registers a canonical file's parent. If native watcher creation or
    /// this registration fails, existing registrations remain untouched.
    pub fn register(&self, path: &Path) -> Result<(), String> {
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("파일 감시 경로를 확인할 수 없습니다: {error}"))?;
        if !canonical.is_file() {
            return Err("파일 감시 대상이 일반 파일이 아닙니다".to_string());
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
        if let Some(registration) = state.registrations.get_mut(&parent) {
            increment_filename(&mut registration.filenames, filename);
            return Ok(());
        }

        let weak_state: Weak<Mutex<WatcherState>> = Arc::downgrade(&self.state);
        let sender = self.sender.clone();
        let mut watcher = notify::recommended_watcher(move |result| {
            handle_notify_result(&weak_state, &sender, result);
        })
        .map_err(|error| format!("파일 감시를 시작할 수 없습니다: {error}"))?;
        watcher
            .watch(&parent, RecursiveMode::NonRecursive)
            .map_err(|error| format!("파일 감시를 등록할 수 없습니다: {error}"))?;

        let mut filenames = HashMap::new();
        increment_filename(&mut filenames, filename);
        state
            .registrations
            .insert(parent, DirectoryRegistration { watcher, filenames });
        state.generations.entry(canonical).or_insert(0);
        Ok(())
    }

    /// Removes one file reference and drops the parent watcher when it has no
    /// open documents left. Delivery already queued for the final unregister
    /// is invalidated by a generation bump before the command returns.
    pub fn unregister(&self, path: &Path) -> Result<(), String> {
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
            let entry = state.generations.entry(canonical.clone()).or_insert(0);
            let previous_generation = *entry;
            *entry = entry.wrapping_add(1);
            (canonical, previous_generation)
        };
        let _ = self.sender.send(WatcherMessage::Invalidate {
            path: canonical,
            generation,
        });
        Ok(())
    }

    pub fn registration_count(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.registrations.len())
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

fn increment_filename(filenames: &mut HashMap<OsString, usize>, filename: OsString) {
    if let Some((key, count)) = filenames
        .iter_mut()
        .find(|(existing, _)| same_filename(existing, &filename))
    {
        let _ = key;
        *count = count.saturating_add(1);
        return;
    }
    filenames.insert(filename, 1);
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
    sender: &Sender<WatcherMessage>,
    result: notify::Result<Event>,
) {
    let Ok(event) = result else {
        // A backend error affects this watcher only. The mtime/size/hash check
        // in save_file remains the safety fallback for edits while it is down.
        return;
    };
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
        let _ = sender.send(WatcherMessage::Candidate { path, generation });
    }
}

fn watcher_worker(
    weak_state: Weak<Mutex<WatcherState>>,
    app: AppHandle,
    receiver: Receiver<WatcherMessage>,
) {
    let mut debouncer = Debouncer::new(DEBOUNCE_WINDOW);
    let mut generations = HashMap::<PathBuf, u64>::new();
    loop {
        let message = match debouncer.next_deadline() {
            Some(deadline) => {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(message) => message,
                    Err(RecvTimeoutError::Timeout) => {
                        deliver_ready(&weak_state, &app, &mut debouncer, &mut generations);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match receiver.recv() {
                Ok(message) => message,
                Err(_) => break,
            },
        };
        match message {
            WatcherMessage::Candidate { path, generation } => {
                if is_current_registration(&weak_state, &path, generation) {
                    debouncer.record(&path, Instant::now());
                    generations.insert(path, generation);
                }
            }
            WatcherMessage::Invalidate { path, generation } => {
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
            WatcherMessage::Shutdown => break,
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
        if let Some(payload) = metadata_snapshot(&path) {
            if is_current_registration(weak_state, &path, generation) {
                let _ = app.emit("file-changed", payload);
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
}
