//! Knowledge vault watcher with native notifications and bounded WSL polling.
//!
//! Native callbacks enqueue only bounded path batches. WSL vaults use a
//! metadata scan because recursive Windows notifications are not reliable for
//! the WSL UNC provider. Only a complete scan is deletion authority.

use crate::commands::docs::AppState;
use crate::core::db::{index_doc_in_transaction, list_doc_paths, remove_doc};
use crate::core::vault::VaultIdentity;
use notify::{EventKind, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(600);
pub const MAX_PENDING_WATCH_PATHS: usize = 4_096;
pub const MAX_EVENT_PATHS: usize = 256;
pub const MAX_READY_WATCH_PATHS: usize = 512;
pub const MAX_WATCH_PATH_BYTES: usize = 32 * 1024;
pub const MAX_WATCH_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_RECONCILE_FILES: usize = 4_096;
pub const MAX_RECONCILE_DIRECTORIES: usize = 4_096;
pub const MAX_RECONCILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_WATCH_MESSAGES: usize = 1_024;
const WORKER_TICK: Duration = Duration::from_millis(500);
const WSL_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeWatcherStatus {
    pub source_kind: String,
    pub watch_mode: String,
    pub last_synced_at: Option<u64>,
    pub error: Option<String>,
}

impl Default for KnowledgeWatcherStatus {
    fn default() -> Self {
        Self {
            source_kind: "native".to_string(),
            watch_mode: "unavailable".to_string(),
            last_synced_at: None,
            error: Some("vault_unconfigured".to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    modified_nanos: i64,
}

#[derive(Debug, Clone)]
struct ScannedDocument {
    stamp: FileStamp,
}

#[derive(Debug)]
struct VaultScan {
    docs: HashMap<String, ScannedDocument>,
    complete: bool,
    error: Option<&'static str>,
}

#[derive(Debug)]
struct Debouncer {
    window: Duration,
    pending: HashMap<PathBuf, Instant>,
    overflow: bool,
}

impl Debouncer {
    fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
            overflow: false,
        }
    }

    fn record(&mut self, path: &Path, now: Instant) {
        if path_byte_len(path) > MAX_WATCH_PATH_BYTES
            || (!self.pending.contains_key(path) && self.pending.len() >= MAX_PENDING_WATCH_PATHS)
        {
            self.overflow = true;
            return;
        }
        self.pending.insert(path.to_path_buf(), now);
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|seen| *seen + self.window).min()
    }

    fn take_ready(&mut self, now: Instant) -> Vec<PathBuf> {
        let mut ready = self
            .pending
            .iter()
            .filter(|(_, seen)| now.saturating_duration_since(**seen) >= self.window)
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
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

    fn mark_overflow(&mut self) {
        self.overflow = true;
    }

    fn take_overflow(&mut self) -> bool {
        std::mem::take(&mut self.overflow)
    }
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

enum Message {
    Event(Vec<PathBuf>),
    Wake,
    Shutdown,
}

type SharedRoot = Arc<Mutex<Option<PathBuf>>>;
type SharedStatus = Arc<Mutex<KnowledgeWatcherStatus>>;

pub struct KnowledgeWatcher {
    app: AppHandle,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    sender: SyncSender<Message>,
    worker: Mutex<Option<JoinHandle<()>>>,
    root: SharedRoot,
    status: SharedStatus,
    reconcile: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
}

impl KnowledgeWatcher {
    pub fn new(app: AppHandle, state: Arc<AppState>) -> Arc<Self> {
        let (sender, receiver) = mpsc::sync_channel(MAX_WATCH_MESSAGES);
        let root: SharedRoot = Arc::new(Mutex::new(None));
        let status: SharedStatus = Arc::new(Mutex::new(KnowledgeWatcherStatus::default()));
        let reconcile = Arc::new(AtomicBool::new(false));
        let overflow = Arc::new(AtomicBool::new(false));
        let worker = {
            let worker_state = Arc::clone(&state);
            let worker_app = app.clone();
            let worker_root = Arc::clone(&root);
            let worker_status = Arc::clone(&status);
            let worker_reconcile = Arc::clone(&reconcile);
            let worker_overflow = Arc::clone(&overflow);
            thread::Builder::new()
                .name("knowledge-base-watcher".to_string())
                .spawn(move || {
                    watcher_worker(
                        worker_state,
                        worker_app,
                        receiver,
                        worker_root,
                        worker_status,
                        worker_reconcile,
                        worker_overflow,
                    )
                })
                .expect("knowledge-base watcher worker should start")
        };
        Arc::new(Self {
            app,
            watcher: Mutex::new(None),
            sender,
            worker: Mutex::new(Some(worker)),
            root,
            status,
            reconcile,
            overflow,
        })
    }

    pub fn set_root(&self, root: &Path) -> Result<(), String> {
        let vault = VaultIdentity::inspect(root)
            .map_err(|_| "Knowledge watcher 저장 위치를 확인할 수 없습니다".to_string())?;
        let canonical = vault.canonical_path().to_path_buf();
        if path_byte_len(&canonical) > MAX_WATCH_PATH_BYTES {
            return Err("Knowledge watcher 저장 위치가 너무 깁니다".to_string());
        }
        let source_kind = root_source_kind(&canonical);
        let next_watcher = if source_kind == "wsl" {
            None
        } else {
            let sender = self.sender.clone();
            let overflow = Arc::clone(&self.overflow);
            let reconcile = Arc::clone(&self.reconcile);
            let mut watcher =
                notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    match result {
                        Ok(event) => {
                            if !is_mutation_event(&event.kind) {
                                return;
                            }
                            if event.paths.is_empty() {
                                reconcile.store(true, Ordering::Release);
                                let _ = sender.try_send(Message::Wake);
                                return;
                            }
                            let (paths, truncated) = bounded_event_paths(&event.paths);
                            if truncated {
                                overflow.store(true, Ordering::Release);
                                reconcile.store(true, Ordering::Release);
                            }
                            if !paths.is_empty() && sender.try_send(Message::Event(paths)).is_err()
                            {
                                overflow.store(true, Ordering::Release);
                                reconcile.store(true, Ordering::Release);
                            }
                        }
                        Err(_) => {
                            reconcile.store(true, Ordering::Release);
                            let _ = sender.try_send(Message::Wake);
                        }
                    }
                })
                .map_err(|_| "Knowledge watcher를 시작할 수 없습니다".to_string())?;
            watcher
                .watch(&canonical, RecursiveMode::Recursive)
                .map_err(|_| "Knowledge watcher를 등록할 수 없습니다".to_string())?;
            Some(watcher)
        };

        *self.watcher.lock().unwrap() = next_watcher;
        *self.root.lock().unwrap() = Some(canonical);
        set_status(
            &self.status,
            &self.app,
            KnowledgeWatcherStatus {
                source_kind: source_kind.to_string(),
                watch_mode: if source_kind == "wsl" {
                    "polling".to_string()
                } else {
                    "native".to_string()
                },
                last_synced_at: None,
                error: None,
            },
        );
        self.reconcile.store(true, Ordering::Release);
        let _ = self.sender.try_send(Message::Wake);
        Ok(())
    }

    /// Restores a previously configured root without discarding an offline
    /// WSL vault. The path was already accepted and persisted by `set_root`;
    /// retaining only a syntactically valid, bounded WSL UNC path lets the
    /// worker reconnect when the distro becomes available again while every
    /// filesystem mutation remains gated by `VaultIdentity::inspect`.
    pub fn restore_root(&self, root: &Path) {
        if self.set_root(root).is_ok() || !is_restorable_wsl_root(root) {
            return;
        }

        *self.watcher.lock().unwrap() = None;
        *self.root.lock().unwrap() = Some(root.to_path_buf());
        set_status(
            &self.status,
            &self.app,
            KnowledgeWatcherStatus {
                source_kind: "wsl".to_string(),
                watch_mode: "unavailable".to_string(),
                last_synced_at: None,
                error: Some("vault_unavailable".to_string()),
            },
        );
        self.reconcile.store(true, Ordering::Release);
        let _ = self.sender.try_send(Message::Wake);
    }

    pub fn status(&self) -> KnowledgeWatcherStatus {
        self.status.lock().unwrap().clone()
    }
}

#[tauri::command]
pub fn knowledge_watcher_status(
    watcher: tauri::State<'_, Arc<KnowledgeWatcher>>,
) -> KnowledgeWatcherStatus {
    watcher.status()
}

impl Drop for KnowledgeWatcher {
    fn drop(&mut self) {
        if let Ok(mut watcher) = self.watcher.lock() {
            watcher.take();
        }
        let _ = self.sender.send(Message::Shutdown);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn watcher_worker(
    state: Arc<AppState>,
    app: AppHandle,
    receiver: Receiver<Message>,
    root: SharedRoot,
    status: SharedStatus,
    reconcile: Arc<AtomicBool>,
    overflow: Arc<AtomicBool>,
) {
    let mut debouncer = Debouncer::new(DEBOUNCE_WINDOW);
    let mut previous = HashMap::<String, FileStamp>::new();
    let mut observed_root = None::<PathBuf>;
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
            Some(Message::Event(paths)) => {
                for path in paths {
                    debouncer.record(&path, Instant::now());
                }
            }
            Some(Message::Wake) | None => {}
            Some(Message::Shutdown) => break,
        }

        let current_root = root.lock().unwrap().clone();
        if current_root != observed_root {
            observed_root = current_root;
            previous.clear();
            debouncer = Debouncer::new(DEBOUNCE_WINDOW);
            reconcile.store(true, Ordering::Release);
        }

        if overflow.swap(false, Ordering::AcqRel) {
            debouncer.mark_overflow();
        }
        let ready = debouncer.take_ready(Instant::now());
        if !ready.is_empty() {
            apply_incremental_paths(&state, &app, &root, &status, &reconcile, &ready);
        }
        if debouncer.take_overflow() {
            reconcile.store(true, Ordering::Release);
        }

        if reconcile.swap(false, Ordering::AcqRel) {
            reconcile_configured_root(&state, &app, &root, &status, &mut previous, true);
        }

        if Instant::now() >= next_poll {
            let polling = root
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|root| root_source_kind(root) == "wsl");
            let retry_unavailable = status.lock().unwrap().error.is_some();
            if polling || retry_unavailable {
                reconcile_configured_root(&state, &app, &root, &status, &mut previous, false);
            }
            next_poll = Instant::now() + WSL_POLL_INTERVAL;
        }
    }
}

fn apply_incremental_paths(
    state: &Arc<AppState>,
    app: &AppHandle,
    root: &SharedRoot,
    status: &SharedStatus,
    reconcile: &AtomicBool,
    paths: &[PathBuf],
) {
    let Some(root_path) = root.lock().unwrap().clone() else {
        return;
    };
    let Ok(vault) = VaultIdentity::inspect(&root_path) else {
        reconcile.store(true, Ordering::Release);
        update_status_error(status, app, &root_path, "vault_unavailable");
        return;
    };

    let mut upserts = Vec::new();
    for path in paths {
        let Some(relative) = safe_relative_path(&vault, path) else {
            reconcile.store(true, Ordering::Release);
            continue;
        };
        if !is_markdown(&relative) {
            reconcile.store(true, Ordering::Release);
            continue;
        }
        match read_bounded_note(&vault, &relative, None) {
            Ok(content) => upserts.push((relative, content)),
            Err(_) => reconcile.store(true, Ordering::Release),
        }
    }
    if upserts.is_empty() {
        return;
    }

    let applied = {
        let conn = state.db.lock().unwrap();
        let Ok(transaction) = conn.unchecked_transaction() else {
            update_status_error(status, app, &root_path, "vault_index_failed");
            return;
        };
        if upserts
            .iter()
            .any(|(path, content)| index_doc_in_transaction(&transaction, path, content).is_err())
        {
            update_status_error(status, app, &root_path, "vault_index_failed");
            return;
        }
        transaction.commit().is_ok()
    };
    if applied {
        publish_docs_changed(state, app);
        update_status_success(status, app, &root_path, None);
    } else {
        update_status_error(status, app, &root_path, "vault_index_failed");
    }
}

fn reconcile_configured_root(
    state: &Arc<AppState>,
    app: &AppHandle,
    root: &SharedRoot,
    status: &SharedStatus,
    previous: &mut HashMap<String, FileStamp>,
    force: bool,
) {
    let Some(root_path) = root.lock().unwrap().clone() else {
        return;
    };
    let vault = match VaultIdentity::inspect(&root_path) {
        Ok(vault) => vault,
        Err(_) => {
            update_status_error(status, app, &root_path, "vault_unavailable");
            return;
        }
    };
    let mut scan = scan_vault(&vault);
    let current = scan
        .docs
        .iter()
        .map(|(path, document)| (path.clone(), document.stamp))
        .collect::<HashMap<_, _>>();
    let changed = changed_paths(previous, &current, force);
    let recovered = status.lock().unwrap().error.is_some() && scan.complete;
    let needs_apply = force
        || recovered
        || !changed.is_empty()
        || (scan.complete && previous.keys().any(|path| !current.contains_key(path)));

    let mut applied = false;
    if needs_apply {
        match apply_scan(state, &vault, &mut scan, &changed, force || recovered) {
            Ok(changed) => applied = changed,
            Err(()) => {
                update_status_error(status, app, &root_path, "vault_index_failed");
                return;
            }
        }
    }

    // `apply_scan` removes documents that could not be read as one stable
    // filesystem object. Do not advance their stamps: the next retry must
    // attempt them again even if another unreadable subtree keeps the overall
    // scan incomplete.
    if scan.error != Some("vault_unavailable") {
        let accepted_current = scan
            .docs
            .iter()
            .map(|(path, document)| (path.clone(), document.stamp))
            .collect::<HashMap<_, _>>();
        if scan.complete {
            *previous = accepted_current;
        } else {
            previous.extend(accepted_current);
        }
    }
    if scan.error == Some("vault_unavailable") {
        update_status_error(status, app, &root_path, "vault_unavailable");
    } else {
        update_status_success(status, app, &root_path, scan.error);
    }
    if applied {
        publish_docs_changed(state, app);
    }
}

fn changed_paths(
    previous: &HashMap<String, FileStamp>,
    current: &HashMap<String, FileStamp>,
    force: bool,
) -> HashSet<String> {
    current
        .iter()
        .filter(|(path, stamp)| force || previous.get(*path) != Some(*stamp))
        .map(|(path, _)| path.clone())
        .collect()
}

fn apply_scan(
    state: &Arc<AppState>,
    vault: &VaultIdentity,
    scan: &mut VaultScan,
    changed: &HashSet<String>,
    upsert_all: bool,
) -> Result<bool, ()> {
    let mut contents = Vec::new();
    let mut unreadable = Vec::new();
    for (relative, document) in &scan.docs {
        if !upsert_all && !changed.contains(relative) {
            continue;
        }
        match read_bounded_note(vault, relative, Some(document.stamp)) {
            Ok(content) => contents.push((relative.clone(), content)),
            Err(_) => {
                scan.complete = false;
                if scan.error != Some("vault_unavailable") {
                    scan.error = Some("vault_scan_incomplete");
                }
                unreadable.push(relative.clone());
            }
        }
    }
    for relative in unreadable {
        scan.docs.remove(&relative);
    }

    if vault.revalidate().is_err() {
        scan.complete = false;
        scan.error = Some("vault_unavailable");
        scan.docs.clear();
        return Ok(false);
    }

    let conn = state.db.lock().map_err(|_| ())?;
    let transaction = conn.unchecked_transaction().map_err(|_| ())?;
    for (path, content) in &contents {
        index_doc_in_transaction(&transaction, path, content).map_err(|_| ())?;
    }
    let mut deleted = false;
    if scan.complete {
        let present = scan.docs.keys().map(String::as_str).collect::<HashSet<_>>();
        for path in list_doc_paths(&transaction).map_err(|_| ())? {
            if !present.contains(path.as_str()) {
                remove_doc(&transaction, &path).map_err(|_| ())?;
                deleted = true;
            }
        }
    }
    // Keep the transaction uncommitted until the source root still matches
    // the identity that authorized this scan. Dropping it here rolls every
    // upsert/deletion back if the vault was replaced during reconciliation.
    if vault.revalidate().is_err() {
        scan.complete = false;
        scan.error = Some("vault_unavailable");
        scan.docs.clear();
        return Ok(false);
    }
    transaction.commit().map_err(|_| ())?;
    Ok(!contents.is_empty() || deleted)
}

fn scan_vault(vault: &VaultIdentity) -> VaultScan {
    let root = vault.canonical_path();
    let mut pending = vec![root.to_path_buf()];
    let mut directories = 0usize;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut docs = HashMap::new();
    let mut complete = true;
    let mut error = None;

    if vault.revalidate().is_err() {
        return VaultScan {
            docs,
            complete: false,
            error: Some("vault_unavailable"),
        };
    }

    while let Some(directory) = pending.pop() {
        match fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.is_dir() && !is_link_or_reparse(&metadata) => {}
            _ => {
                complete = false;
                error.get_or_insert("vault_scan_incomplete");
                continue;
            }
        }
        directories = directories.saturating_add(1);
        if directories > MAX_RECONCILE_DIRECTORIES {
            complete = false;
            error = Some("vault_scan_limit");
            break;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                complete = false;
                error.get_or_insert("vault_scan_incomplete");
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    complete = false;
                    error.get_or_insert("vault_scan_incomplete");
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    complete = false;
                    error.get_or_insert("vault_scan_incomplete");
                    continue;
                }
            };
            if is_link_or_reparse(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                if directories.saturating_add(pending.len()) >= MAX_RECONCILE_DIRECTORIES {
                    complete = false;
                    error = Some("vault_scan_limit");
                    break;
                }
                pending.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            files = files.saturating_add(1);
            if files > MAX_RECONCILE_FILES {
                complete = false;
                error = Some("vault_scan_limit");
                break;
            }
            let Some(relative) = scanned_relative_path(root, &path) else {
                complete = false;
                error.get_or_insert("vault_scan_incomplete");
                continue;
            };
            if !is_markdown(&relative) || metadata.len() > MAX_WATCH_FILE_BYTES {
                continue;
            }
            if bytes.saturating_add(metadata.len()) > MAX_RECONCILE_BYTES {
                complete = false;
                error = Some("vault_scan_limit");
                break;
            }
            let Some(stamp) = stamp_for_metadata(&metadata) else {
                complete = false;
                error.get_or_insert("vault_scan_incomplete");
                continue;
            };
            bytes = bytes.saturating_add(metadata.len());
            docs.insert(relative, ScannedDocument { stamp });
        }
        if error == Some("vault_scan_limit") {
            break;
        }
    }
    if vault.revalidate().is_err() {
        docs.clear();
        complete = false;
        error = Some("vault_unavailable");
    }
    VaultScan {
        docs,
        complete,
        error,
    }
}

fn bounded_event_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, bool) {
    let bounded = paths
        .iter()
        .take(MAX_EVENT_PATHS)
        .filter(|path| path_byte_len(path) <= MAX_WATCH_PATH_BYTES)
        .cloned()
        .collect::<Vec<_>>();
    let truncated = bounded.len() != paths.len();
    (bounded, truncated)
}

fn is_mutation_event(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

fn root_source_kind(path: &Path) -> &'static str {
    if devbox_wsl::path::parse_wsl_unc_path(&path.to_string_lossy())
        .ok()
        .flatten()
        .is_some()
    {
        "wsl"
    } else {
        "native"
    }
}

fn is_restorable_wsl_root(path: &Path) -> bool {
    path_byte_len(path) <= MAX_WATCH_PATH_BYTES
        && devbox_wsl::path::parse_wsl_unc_path(&path.to_string_lossy())
            .ok()
            .flatten()
            .is_some()
}

fn watch_mode(path: &Path) -> &'static str {
    if root_source_kind(path) == "wsl" {
        "polling"
    } else {
        "native"
    }
}

fn set_status(status: &SharedStatus, app: &AppHandle, next: KnowledgeWatcherStatus) {
    *status.lock().unwrap() = next.clone();
    let _ = app.emit("knowledge-watcher-status", next);
}

fn update_status_success(status: &SharedStatus, app: &AppHandle, root: &Path, error: Option<&str>) {
    set_status(
        status,
        app,
        KnowledgeWatcherStatus {
            source_kind: root_source_kind(root).to_string(),
            watch_mode: watch_mode(root).to_string(),
            last_synced_at: Some(now_ms()),
            error: error.map(str::to_string),
        },
    );
}

fn update_status_error(status: &SharedStatus, app: &AppHandle, root: &Path, error: &'static str) {
    let last_synced_at = status.lock().unwrap().last_synced_at;
    set_status(
        status,
        app,
        KnowledgeWatcherStatus {
            source_kind: root_source_kind(root).to_string(),
            watch_mode: if error == "vault_unavailable" {
                "unavailable".to_string()
            } else {
                watch_mode(root).to_string()
            },
            last_synced_at,
            error: Some(error.to_string()),
        },
    );
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn publish_docs_changed(state: &Arc<AppState>, app: &AppHandle) {
    let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
    let _ = app.emit("docs-changed", ());
}

fn is_markdown(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn safe_relative_path(vault: &VaultIdentity, path: &Path) -> Option<String> {
    let scoped_path = vault
        .existing_path(path)
        .unwrap_or_else(|_| path.to_path_buf());
    let relative = scoped_path.strip_prefix(vault.canonical_path()).ok()?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    let relative = relative.to_string_lossy().replace('\\', "/");
    let candidate = vault.new_entry(&relative).ok()?;
    if let Ok(metadata) = fs::symlink_metadata(&candidate) {
        if metadata.file_type().is_symlink() || metadata.is_dir() || !metadata.is_file() {
            return None;
        }
        vault.existing_path(&candidate).ok()?;
    }
    Some(relative)
}

fn scanned_relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn stamp_for_metadata(metadata: &fs::Metadata) -> Option<FileStamp> {
    let modified_nanos = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(FileStamp {
        size: metadata.len(),
        modified_nanos: i64::try_from(modified_nanos).ok()?,
    })
}

fn read_bounded_note(
    vault: &VaultIdentity,
    relative: &str,
    expected: Option<FileStamp>,
) -> Result<String, ()> {
    let safe_path = vault.existing_entry(relative).map_err(|_| ())?;
    let before = fs::symlink_metadata(&safe_path).map_err(|_| ())?;
    let before_stamp = stamp_for_metadata(&before).ok_or(())?;
    if !before.is_file()
        || is_link_or_reparse(&before)
        || before.len() > MAX_WATCH_FILE_BYTES
        || expected.is_some_and(|stamp| stamp != before_stamp)
    {
        return Err(());
    }
    let file = File::open(&safe_path).map_err(|_| ())?;
    let opened_stamp = stamp_for_metadata(&file.metadata().map_err(|_| ())?).ok_or(())?;
    if opened_stamp != before_stamp {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    file.take(MAX_WATCH_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_WATCH_FILE_BYTES {
        return Err(());
    }
    let after = fs::symlink_metadata(&safe_path).map_err(|_| ())?;
    if !after.is_file()
        || is_link_or_reparse(&after)
        || stamp_for_metadata(&after) != Some(before_stamp)
        || bytes.len() as u64 != after.len()
    {
        return Err(());
    }
    String::from_utf8(bytes).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db;

    #[test]
    fn debouncer_caps_pending_paths_and_marks_overflow() {
        let mut debouncer = Debouncer::new(Duration::from_millis(1));
        let now = Instant::now();
        for index in 0..=MAX_PENDING_WATCH_PATHS {
            debouncer.record(Path::new(&format!("note-{index}.md")), now);
        }
        assert!(debouncer.pending.len() <= MAX_PENDING_WATCH_PATHS);
        assert!(debouncer.take_overflow());
    }

    #[test]
    fn event_path_payload_is_bounded_before_queueing() {
        let mut paths = vec![PathBuf::from("x".repeat(MAX_WATCH_PATH_BYTES + 1))];
        paths.extend((0..MAX_EVENT_PATHS + 1).map(|index| PathBuf::from(format!("note-{index}"))));
        let (bounded, truncated) = bounded_event_paths(&paths);
        assert!(bounded.len() <= MAX_EVENT_PATHS);
        assert!(truncated);
        assert!(bounded
            .iter()
            .all(|path| path_byte_len(path) <= MAX_WATCH_PATH_BYTES));
    }

    #[test]
    fn watcher_ignores_access_only_events() {
        assert!(!is_mutation_event(&EventKind::Access(
            notify::event::AccessKind::Read,
        )));
        assert!(is_mutation_event(&EventKind::Modify(
            notify::event::ModifyKind::Any,
        )));
    }

    #[test]
    fn incomplete_snapshots_keep_missing_last_known_good_paths() {
        let previous = HashMap::from([
            (
                "Notes/kept.md".to_string(),
                FileStamp {
                    size: 4,
                    modified_nanos: 1,
                },
            ),
            (
                "Notes/missing.md".to_string(),
                FileStamp {
                    size: 7,
                    modified_nanos: 2,
                },
            ),
        ]);
        let current = HashMap::from([(
            "Notes/kept.md".to_string(),
            FileStamp {
                size: 5,
                modified_nanos: 3,
            },
        )]);
        let mut retained = previous.clone();
        retained.extend(current.clone());
        assert!(retained.contains_key("Notes/missing.md"));
        assert_eq!(changed_paths(&previous, &current, false).len(), 1);
    }

    #[test]
    fn complete_scan_removes_stale_rows_but_incomplete_scan_preserves_them() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("Notes")).unwrap();
        fs::write(root.path().join("Notes/kept.md"), "# kept").unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let connection = db::init(Path::new(":memory:")).unwrap();
        db::index_doc(&connection, "Notes/kept.md", "# old").unwrap();
        db::index_doc(&connection, "Notes/stale.md", "# stale").unwrap();

        let mut complete = scan_vault(&vault);
        let state = Arc::new(AppState {
            db: Mutex::new(connection),
            rename_plans: Mutex::new(crate::core::rename::RenamePlanStore::default()),
            quick_capture_previews: Mutex::new(
                crate::commands::docs::QuickCapturePreviewStore::default(),
            ),
            template_previews: Mutex::new(
                crate::commands::templates::TemplatePreviewStore::default(),
            ),
            image_cache: Mutex::new(HashMap::new()),
        });
        let all = complete.docs.keys().cloned().collect::<HashSet<_>>();
        assert!(apply_scan(&state, &vault, &mut complete, &all, true).unwrap());
        assert_eq!(
            list_doc_paths(&state.db.lock().unwrap()).unwrap(),
            vec!["Notes/kept.md"]
        );

        db::index_doc(
            &state.db.lock().unwrap(),
            "Notes/retain-on-partial.md",
            "# retain",
        )
        .unwrap();
        let mut incomplete = scan_vault(&vault);
        incomplete.complete = false;
        incomplete.error = Some("vault_scan_incomplete");
        let all = incomplete.docs.keys().cloned().collect::<HashSet<_>>();
        apply_scan(&state, &vault, &mut incomplete, &all, true).unwrap();
        assert!(list_doc_paths(&state.db.lock().unwrap())
            .unwrap()
            .contains(&"Notes/retain-on-partial.md".to_string()));
    }

    #[test]
    fn file_disappearing_after_scan_is_not_deletion_authority() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("Notes")).unwrap();
        let note = root.path().join("Notes/raced.md");
        fs::write(&note, "# before").unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        let connection = db::init(Path::new(":memory:")).unwrap();
        db::index_doc(&connection, "Notes/raced.md", "# indexed").unwrap();
        let state = Arc::new(AppState {
            db: Mutex::new(connection),
            rename_plans: Mutex::new(crate::core::rename::RenamePlanStore::default()),
            quick_capture_previews: Mutex::new(
                crate::commands::docs::QuickCapturePreviewStore::default(),
            ),
            template_previews: Mutex::new(
                crate::commands::templates::TemplatePreviewStore::default(),
            ),
            image_cache: Mutex::new(HashMap::new()),
        });

        let mut scan = scan_vault(&vault);
        let changed = scan.docs.keys().cloned().collect::<HashSet<_>>();
        fs::remove_file(note).unwrap();
        assert!(!apply_scan(&state, &vault, &mut scan, &changed, true).unwrap());
        assert!(!scan.complete);
        assert_eq!(scan.error, Some("vault_scan_incomplete"));
        assert_eq!(
            list_doc_paths(&state.db.lock().unwrap()).unwrap(),
            vec!["Notes/raced.md"]
        );
    }

    #[test]
    fn vault_replacement_before_commit_rolls_back_authoritative_deletions() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("vault");
        fs::create_dir(&root).unwrap();
        let vault = VaultIdentity::inspect(&root).unwrap();
        let connection = db::init(Path::new(":memory:")).unwrap();
        db::index_doc(&connection, "Notes/last-known.md", "# retained").unwrap();
        let state = Arc::new(AppState {
            db: Mutex::new(connection),
            rename_plans: Mutex::new(crate::core::rename::RenamePlanStore::default()),
            quick_capture_previews: Mutex::new(
                crate::commands::docs::QuickCapturePreviewStore::default(),
            ),
            template_previews: Mutex::new(
                crate::commands::templates::TemplatePreviewStore::default(),
            ),
            image_cache: Mutex::new(HashMap::new()),
        });
        let mut scan = scan_vault(&vault);
        fs::rename(&root, parent.path().join("old-vault")).unwrap();
        fs::create_dir(&root).unwrap();

        assert!(!apply_scan(&state, &vault, &mut scan, &HashSet::new(), false).unwrap());
        assert_eq!(scan.error, Some("vault_unavailable"));
        assert_eq!(
            list_doc_paths(&state.db.lock().unwrap()).unwrap(),
            vec!["Notes/last-known.md"]
        );
    }

    #[test]
    fn safe_relative_path_rejects_directories_links_and_outside_paths() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Journal")).unwrap();
        fs::write(root.path().join("Journal/note.md"), b"ok").unwrap();
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        assert_eq!(
            safe_relative_path(&vault, &root.path().join("Journal/note.md")),
            Some("Journal/note.md".into())
        );
        assert_eq!(
            safe_relative_path(&vault, &vault.canonical_path().join("Journal/deleted.md")),
            Some("Journal/deleted.md".into())
        );
        assert!(safe_relative_path(&vault, &root.path().join("Journal")).is_none());
        assert!(safe_relative_path(&vault, Path::new("/tmp/outside.md")).is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("secret.md"), b"outside").unwrap();
            symlink(
                outside.path().join("secret.md"),
                root.path().join("Journal/link.md"),
            )
            .unwrap();
            assert!(safe_relative_path(&vault, &root.path().join("Journal/link.md")).is_none());
        }
    }

    #[test]
    fn wsl_root_is_reported_as_polling_without_folding_linux_case() {
        assert_eq!(
            root_source_kind(Path::new(
                "//?/UNC/wsl.localhost/Ubuntu/home/jihoon/Knowledge"
            )),
            "wsl"
        );
        assert_eq!(
            root_source_kind(Path::new("//wsl$/Ubuntu/home/jihoon/knowledge")),
            "wsl"
        );
    }

    #[test]
    fn offline_restore_candidate_requires_a_bounded_wsl_unc_root() {
        assert!(is_restorable_wsl_root(Path::new(
            "//wsl$/Ubuntu/home/jihoon/프로젝트"
        )));
        assert!(is_restorable_wsl_root(Path::new(
            "//?/UNC/wsl.localhost/ubuntu/home/jihoon/프로젝트"
        )));
        assert!(!is_restorable_wsl_root(Path::new(
            "C:/Users/jihoon/projects"
        )));
        assert!(!is_restorable_wsl_root(Path::new(&format!(
            "//wsl$/Ubuntu/{}",
            "x".repeat(MAX_WATCH_PATH_BYTES + 1)
        ))));
    }
}
