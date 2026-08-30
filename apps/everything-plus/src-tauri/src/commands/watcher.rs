//! 등록 루트별 `notify` watcher와 증분 인덱싱 워커.
//!
//! 네이티브 notify 콜백은 이벤트만 채널에 넣는다 (IO 없음). 별도 워커 스레드가
//! 디바운스 후 DB에 반영한다. full re-index가 도는 동안은 증분 반영을 건너뛴다
//! (full re-index가 루트를 통째로 다시 쓰므로 generation 대신 `indexing` 플래그로
//! 배타 제어한다 — §8.3).

use crate::commands::indexing::{collect_root_files, spawn_index, validate_root, AppState};
use crate::core::content::{extract_file, is_content_candidate};
use crate::core::db::{
    delete_content, delete_file, find_root_for, root_row_for, upsert_content_record, upsert_file,
};
use crate::core::models::{RootSourceKind, RootStatus, WatchMode};
use crate::core::watcher::{
    classify_event, is_within_root, Debouncer, EventClass, DEBOUNCE_WINDOW,
    MAX_PENDING_WATCH_PATHS, MAX_WATCH_PATH_BYTES,
};
use notify::{RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::AppHandle;

const MAX_WATCH_MESSAGES: usize = 1_024;
const MAX_EVENT_PATHS: usize = 256;
const WORKER_TICK: Duration = Duration::from_millis(500);
const WSL_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Native roots retain their notify handle. WSL roots deliberately use a
/// metadata poll because Windows notify backends do not provide a reliable
/// recursive contract for the WSL UNC provider.
enum RootWatcher {
    Native {
        _watcher: notify::RecommendedWatcher,
    },
    Polling,
}

enum WatcherMessage {
    Event { root: String, paths: Vec<PathBuf> },
    Wake,
    Shutdown,
}

type SharedStatus = Arc<Mutex<HashMap<String, RootStatus>>>;
type SharedRoots = Arc<Mutex<HashSet<String>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    size: i64,
    modified_ts: i64,
}

#[derive(Debug)]
struct PollSnapshot {
    files: HashMap<String, FileStamp>,
    truncated: bool,
    incomplete: bool,
}

/// 앱 수명 동안 루트 watcher와 워커를 관리한다.
pub struct WatcherManager {
    _app: AppHandle,
    state: Arc<AppState>,
    roots: Mutex<HashMap<String, RootWatcher>>,
    status: SharedStatus,
    polling_roots: SharedRoots,
    reconcile_roots: SharedRoots,
    sender: SyncSender<WatcherMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl WatcherManager {
    pub fn new(app: AppHandle, state: Arc<AppState>) -> Arc<Self> {
        let (sender, receiver) = mpsc::sync_channel(MAX_WATCH_MESSAGES);
        let status: SharedStatus = Arc::new(Mutex::new(HashMap::new()));
        let polling_roots: SharedRoots = Arc::new(Mutex::new(HashSet::new()));
        let reconcile_roots: SharedRoots = Arc::new(Mutex::new(HashSet::new()));
        let worker_state = Arc::clone(&state);
        let worker_status = Arc::clone(&status);
        let worker_polling_roots = Arc::clone(&polling_roots);
        let worker_reconcile_roots = Arc::clone(&reconcile_roots);
        let worker = thread::Builder::new()
            .name("everything-plus-watcher".to_string())
            .spawn(move || {
                watcher_worker(
                    worker_state,
                    worker_status,
                    worker_polling_roots,
                    worker_reconcile_roots,
                    receiver,
                )
            })
            .expect("everything-plus watcher worker should start");
        Arc::new(Self {
            _app: app,
            state,
            roots: Mutex::new(HashMap::new()),
            status,
            polling_roots,
            reconcile_roots,
            sender,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// 등록된 모든 루트에 watcher를 (재)설치한다. 앱 재시작 시 복원용.
    pub fn restore_all(&self) {
        let roots = {
            let conn = self.state.db.lock().unwrap();
            crate::core::db::list_roots(&conn).unwrap_or_default()
        };
        for root in roots {
            // A persisted WSL root must remain in the polling set while its
            // distribution is offline. `add` does not touch the filesystem
            // for WSL roots, so the next successful poll can reconnect it
            // without requiring an app restart or manual remove/add cycle.
            if root_source_kind(&root.path) == RootSourceKind::Wsl {
                match self.add(&root.path) {
                    Ok(()) => self.request_reconcile(&root.path),
                    Err(_) => self.record_unavailable(&root.path),
                }
                continue;
            }
            match validate_root(&root.path) {
                Ok(validated) => match self.add(&validated) {
                    Ok(()) => self.request_reconcile(&validated),
                    Err(_) => self.record_unavailable(&root.path),
                },
                Err(_) => self.record_unavailable(&root.path),
            }
        }
    }

    /// 루트 하나에 watcher를 추가한다.
    pub fn add(&self, root_path: &str) -> Result<(), String> {
        let normalized = crate::core::db::normalize_path(root_path);
        {
            let roots = self.roots.lock().unwrap();
            if roots.contains_key(&normalized) {
                return Ok(());
            }
            drop(roots);
        }
        let source_kind = root_source_kind(&normalized);
        let root_watcher = if source_kind == RootSourceKind::Wsl {
            self.polling_roots
                .lock()
                .map_err(|_| "검색 감시 상태를 준비할 수 없습니다.".to_string())?
                .insert(normalized.clone());
            RootWatcher::Polling
        } else {
            let sender = self.sender.clone();
            let root_for_cb = normalized.clone();
            let reconcile_roots = Arc::clone(&self.reconcile_roots);
            let mut watcher =
                notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    match result {
                        Ok(event) => {
                            if classify_event(&event.kind) == EventClass::Other {
                                return;
                            }
                            let (paths, truncated) = bounded_event_paths(&event.paths);
                            if truncated {
                                mark_reconcile(&reconcile_roots, &root_for_cb);
                            }
                            if !paths.is_empty()
                                && sender
                                    .try_send(WatcherMessage::Event {
                                        root: root_for_cb.clone(),
                                        paths,
                                    })
                                    .is_err()
                            {
                                mark_reconcile(&reconcile_roots, &root_for_cb);
                            }
                        }
                        Err(_) => {
                            eprintln!("everything-plus: watcher requested reconciliation");
                            mark_reconcile(&reconcile_roots, &root_for_cb);
                        }
                    }
                })
                .map_err(|_| "검색 감시를 시작할 수 없습니다.".to_string())?;
            watcher
                .watch(Path::new(&normalized), RecursiveMode::Recursive)
                .map_err(|_| "검색 감시를 등록할 수 없습니다.".to_string())?;
            RootWatcher::Native { _watcher: watcher }
        };

        self.roots
            .lock()
            .unwrap()
            .insert(normalized.clone(), root_watcher);
        self.status.lock().unwrap().insert(
            normalized.clone(),
            RootStatus {
                root: normalized,
                source_kind,
                watch_mode: if source_kind == RootSourceKind::Wsl {
                    WatchMode::Polling
                } else {
                    WatchMode::Native
                },
                last_synced_at: None,
                pending: 0,
                error: None,
            },
        );
        let _ = self.sender.try_send(WatcherMessage::Wake);
        Ok(())
    }

    fn request_reconcile(&self, root: &str) {
        mark_reconcile(&self.reconcile_roots, root);
        let _ = self.sender.try_send(WatcherMessage::Wake);
    }

    fn record_unavailable(&self, root: &str) {
        let normalized = crate::core::db::normalize_path(root);
        self.status.lock().unwrap().insert(
            normalized.clone(),
            RootStatus {
                root: normalized,
                source_kind: root_source_kind(root),
                watch_mode: WatchMode::Unavailable,
                last_synced_at: None,
                pending: 0,
                error: Some("root_unavailable".to_string()),
            },
        );
    }

    /// 루트 제거 시 watcher와 상태를 함께 해제한다.
    pub fn remove(&self, root_path: &str) {
        let normalized = crate::core::db::normalize_path(root_path);
        self.roots.lock().unwrap().remove(&normalized);
        self.polling_roots.lock().unwrap().remove(&normalized);
        self.reconcile_roots.lock().unwrap().remove(&normalized);
        self.status.lock().unwrap().remove(&normalized);
        let _ = self.sender.try_send(WatcherMessage::Wake);
    }

    /// 루트별 watcher 상태를 반환한다.
    pub fn statuses(&self) -> Vec<RootStatus> {
        let status = self.status.lock().unwrap();
        let mut list: Vec<RootStatus> = status.values().cloned().collect();
        list.sort_by(|a, b| a.root.cmp(&b.root));
        list
    }
}

/// 루트별 watcher 상태 (마지막 반영 시각, pending, 오류).
#[tauri::command]
pub fn watcher_statuses(watcher: tauri::State<'_, Arc<WatcherManager>>) -> Vec<RootStatus> {
    watcher.statuses()
}

impl Drop for WatcherManager {
    fn drop(&mut self) {
        if let Ok(roots) = self.roots.get_mut() {
            roots.clear();
        }
        let _ = self.sender.send(WatcherMessage::Shutdown);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

fn watcher_worker(
    state: Arc<AppState>,
    status: SharedStatus,
    polling_roots: SharedRoots,
    reconcile_roots: SharedRoots,
    receiver: Receiver<WatcherMessage>,
) {
    let mut debouncer = Debouncer::new(DEBOUNCE_WINDOW);
    let mut poll_snapshots = HashMap::<String, HashMap<String, FileStamp>>::new();
    let mut next_poll = Instant::now() + WSL_POLL_INTERVAL;
    loop {
        let now = Instant::now();
        let mut wait = WORKER_TICK;
        if let Some(deadline) = debouncer.next_deadline() {
            wait = wait.min(deadline.saturating_duration_since(now));
        }
        wait = wait.min(next_poll.saturating_duration_since(now));
        let message = match receiver.recv_timeout(wait) {
            Ok(message) => Some(message),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        match message {
            Some(WatcherMessage::Event { root, paths }) => {
                let mut accepted = 0u32;
                let mut overflowed = false;
                for path in paths {
                    let path = crate::core::db::normalize_path(&path.to_string_lossy());
                    if is_within_root(&root, &path) {
                        if debouncer.record(Path::new(&path), Instant::now()) {
                            accepted = accepted.saturating_add(1);
                        } else {
                            overflowed = true;
                        }
                    }
                }
                if let Some(entry) = status.lock().unwrap().get_mut(&root) {
                    entry.pending = entry.pending.saturating_add(accepted);
                    if entry.error.as_deref() == Some("incremental_index_failed") {
                        entry.error = None;
                    }
                }
                if overflowed {
                    mark_reconcile(&reconcile_roots, &root);
                }
            }
            Some(WatcherMessage::Wake) | None => {}
            Some(WatcherMessage::Shutdown) => break,
        }

        service_reconciliations(&state, &reconcile_roots);
        if Instant::now() >= next_poll {
            poll_wsl_roots(
                &status,
                &polling_roots,
                &reconcile_roots,
                &mut poll_snapshots,
                &mut debouncer,
            );
            next_poll = Instant::now() + WSL_POLL_INTERVAL;
        }
        deliver_ready(&state, &status, &reconcile_roots, &mut debouncer);
    }
}

fn service_reconciliations(state: &Arc<AppState>, reconcile_roots: &SharedRoots) {
    let pending = {
        let mut roots = reconcile_roots.lock().unwrap();
        std::mem::take(&mut *roots)
    };
    for root in pending {
        spawn_index(Arc::clone(state), vec![root]);
    }
}

fn poll_wsl_roots(
    status: &SharedStatus,
    polling_roots: &SharedRoots,
    reconcile_roots: &SharedRoots,
    snapshots: &mut HashMap<String, HashMap<String, FileStamp>>,
    debouncer: &mut Debouncer,
) {
    let roots = polling_roots.lock().unwrap().clone();
    snapshots.retain(|root, _| roots.contains(root));
    for root in roots {
        let scanned = match scan_poll_snapshot(&root) {
            Ok(scanned) => scanned,
            Err(code) => {
                if let Some(entry) = status.lock().unwrap().get_mut(&root) {
                    entry.error = Some(code.to_string());
                    entry.watch_mode = WatchMode::Polling;
                }
                continue;
            }
        };
        let previous = snapshots.get(&root);
        let (changed, overflowed) = previous.map_or_else(
            || (Vec::new(), false),
            |previous| {
                changed_poll_paths(
                    previous,
                    &scanned.files,
                    !scanned.truncated && !scanned.incomplete,
                    MAX_PENDING_WATCH_PATHS,
                )
            },
        );
        let mut accepted = 0u32;
        let mut debounce_overflow = overflowed;
        for path in changed {
            if debouncer.record(Path::new(&path), Instant::now()) {
                accepted = accepted.saturating_add(1);
            } else {
                debounce_overflow = true;
            }
        }
        if debounce_overflow {
            mark_reconcile(reconcile_roots, &root);
        }

        let next_snapshot = if scanned.truncated || scanned.incomplete {
            let mut retained = previous.cloned().unwrap_or_default();
            retained.extend(scanned.files);
            retained
        } else {
            scanned.files
        };
        snapshots.insert(root.clone(), next_snapshot);
        let recovered = if let Some(entry) = status.lock().unwrap().get_mut(&root) {
            let recovered = entry.error.is_some() && !scanned.truncated && !scanned.incomplete;
            entry.last_synced_at = Some(now_ms());
            entry.pending = accepted;
            entry.watch_mode = WatchMode::Polling;
            entry.error = if scanned.truncated {
                Some("root_scan_limit".to_string())
            } else if scanned.incomplete {
                Some("root_scan_incomplete".to_string())
            } else {
                None
            };
            recovered
        } else {
            false
        };
        if recovered {
            // A successful snapshot after an offline/incomplete period is a
            // new authority boundary. Reconcile once so stale rows and the
            // global indexing error converge even when no file stamp changed.
            mark_reconcile(reconcile_roots, &root);
        }

        // The initial add/restore already schedules a complete index scan.
        // A first polling snapshot is baseline evidence, not a second scan.
    }
}

fn deliver_ready(
    state: &Arc<AppState>,
    status: &SharedStatus,
    reconcile_roots: &SharedRoots,
    debouncer: &mut Debouncer,
) {
    let ready = debouncer.take_ready(Instant::now());
    if ready.is_empty() {
        return;
    }
    // full re-index가 도는 동안에는 증분 반영을 건너뛴다 (배타 제어)
    if state.indexing.load(Ordering::SeqCst) {
        // A fresh full scan owns the file state. Coalesce every ready path to
        // one root restart rather than rearming an already-ready queue in a
        // tight loop.
        if let Ok(conn) = state.db.lock() {
            for path in ready {
                if let Ok(Some(root)) = find_root_for(&conn, &path.to_string_lossy()) {
                    mark_reconcile(reconcile_roots, &root.path);
                }
            }
        }
        return;
    }
    let mut delivered_by_root = HashMap::<String, u32>::new();
    for path in &ready {
        let path_str = path.to_string_lossy().into_owned();
        let owning_root = state
            .db
            .lock()
            .ok()
            .and_then(|conn| find_root_for(&conn, &path_str).ok().flatten())
            .map(|root| root.path);
        if apply_incremental(state, &path_str).is_err() {
            eprintln!("everything-plus: incremental index failed");
            if let Ok(conn) = state.db.lock() {
                if let Ok(Some(root)) = find_root_for(&conn, &path_str) {
                    if let Some(entry) = status.lock().unwrap().get_mut(&root.path) {
                        entry.error = Some("incremental_index_failed".to_string());
                    }
                }
            }
        } else if let Some(root) = owning_root {
            *delivered_by_root.entry(root).or_default() += 1;
        }
    }

    let now = now_ms();
    let mut statuses = status.lock().unwrap();
    for (root, delivered) in delivered_by_root {
        if let Some(entry) = statuses.get_mut(&root) {
            entry.last_synced_at = Some(now);
            entry.pending = entry.pending.saturating_sub(delivered);
            if entry.error.as_deref() == Some("incremental_index_failed") {
                entry.error = None;
            }
        }
    }
}

fn root_source_kind(path: &str) -> RootSourceKind {
    if devbox_wsl::path::parse_wsl_unc_path(path)
        .ok()
        .flatten()
        .is_some()
    {
        RootSourceKind::Wsl
    } else {
        RootSourceKind::Native
    }
}

fn mark_reconcile(roots: &SharedRoots, root: &str) {
    if root.len() <= MAX_WATCH_PATH_BYTES {
        roots.lock().unwrap().insert(root.to_string());
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

fn scan_poll_snapshot(root: &str) -> Result<PollSnapshot, &'static str> {
    let walked = collect_root_files(Path::new(root))?;
    let files = walked
        .files
        .into_iter()
        .filter_map(|file| {
            let path = crate::core::db::normalize_path(&file.path.to_string_lossy());
            is_within_root(root, &path).then_some((
                path,
                FileStamp {
                    size: file.size,
                    modified_ts: file.modified_ts,
                },
            ))
        })
        .collect();
    Ok(PollSnapshot {
        files,
        truncated: walked.truncated,
        incomplete: walked.incomplete,
    })
}

fn changed_poll_paths(
    previous: &HashMap<String, FileStamp>,
    current: &HashMap<String, FileStamp>,
    include_deletions: bool,
    limit: usize,
) -> (Vec<String>, bool) {
    let mut paths = Vec::new();
    let mut overflowed = false;
    for (path, stamp) in current {
        if previous.get(path) == Some(stamp) {
            continue;
        }
        if paths.len() >= limit {
            overflowed = true;
            break;
        }
        paths.push(path.clone());
    }
    if include_deletions && !overflowed {
        for path in previous.keys().filter(|path| !current.contains_key(*path)) {
            if paths.len() >= limit {
                overflowed = true;
                break;
            }
            paths.push(path.clone());
        }
    }
    (paths, overflowed)
}

/// 경로 하나를 DB에 증분 반영한다.
/// - 일반 파일이면 upsert(크기·mtime) + 내용(설정·확장자·크기 조건부)
/// - 아니면(삭제·디렉터리·심링크) 이전 인덱스 정리
pub(crate) fn apply_incremental(state: &Arc<AppState>, path: &str) -> rusqlite::Result<()> {
    let meta = std::fs::symlink_metadata(path);
    match meta {
        Ok(m) if m.file_type().is_file() => {
            let size = i64::try_from(m.len()).unwrap_or(i64::MAX);
            let modified_ts = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let Some((root_id, content)) = ({
                let conn = state.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
                root_row_for(&conn, path)?
            }) else {
                return Ok(());
            };
            if size == i64::MAX {
                return Ok(());
            }
            let content_record = if content && is_content_candidate(std::path::Path::new(path)) {
                Some(extract_file(
                    std::path::Path::new(path),
                    size as u64,
                    Instant::now(),
                ))
            } else {
                None
            };
            let conn = state.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
            let Some((current_root_id, current_content)) = root_row_for(&conn, path)? else {
                // The root may have been removed while the bounded read was in
                // progress. Do not resurrect a file row after that deletion.
                return Ok(());
            };
            if current_root_id != root_id || current_content != content {
                // A root setting change owns a partial/full re-index. Let that
                // worker establish the new content policy instead of applying
                // a stale read under the old one.
                return Ok(());
            }
            let file_id = upsert_file(&conn, path, size, modified_ts, root_id)?;
            if let Some(record) = content_record {
                upsert_content_record(&conn, file_id, &record, now_ms())?;
            } else {
                delete_content(&conn, file_id)?;
            }
        }
        _ => {
            // 삭제·디렉터리·심링크 → 이전 인덱스 정리 (idempotent)
            let conn = state.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
            let _ = delete_file(&conn, path)?;
        }
    }
    state
        .last_indexed_at
        .store(now_ms().max(1), Ordering::SeqCst);
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(size: i64, modified_ts: i64) -> FileStamp {
        FileStamp { size, modified_ts }
    }

    #[test]
    fn callback_payload_is_bounded_before_queueing() {
        let mut paths = vec![PathBuf::from("x".repeat(MAX_WATCH_PATH_BYTES + 1))];
        paths.extend((0..=MAX_EVENT_PATHS).map(|index| PathBuf::from(format!("note-{index}"))));
        let (bounded, truncated) = bounded_event_paths(&paths);
        assert!(truncated);
        assert!(bounded.len() <= MAX_EVENT_PATHS);
        assert!(bounded
            .iter()
            .all(|path| path_byte_len(path) <= MAX_WATCH_PATH_BYTES));
    }

    #[test]
    fn polling_diff_preserves_deletions_until_snapshot_is_complete() {
        let previous = HashMap::from([
            ("//wsl$/Ubuntu/home/user/DevBox/A.rs".into(), stamp(1, 1)),
            ("//wsl$/Ubuntu/home/user/DevBox/B.rs".into(), stamp(1, 1)),
        ]);
        let current = HashMap::from([
            ("//wsl$/Ubuntu/home/user/DevBox/A.rs".into(), stamp(2, 2)),
            ("//wsl$/Ubuntu/home/user/DevBox/C.rs".into(), stamp(1, 1)),
        ]);

        let (partial, overflowed) = changed_poll_paths(&previous, &current, false, 10);
        assert!(!overflowed);
        assert_eq!(partial.len(), 2);
        assert!(!partial.iter().any(|path| path.ends_with("B.rs")));

        let (complete, overflowed) = changed_poll_paths(&previous, &current, true, 10);
        assert!(!overflowed);
        assert_eq!(complete.len(), 3);
        assert!(complete.iter().any(|path| path.ends_with("B.rs")));
    }

    #[test]
    fn polling_diff_reports_overflow_instead_of_allocating_every_change() {
        let current = (0..10)
            .map(|index| (format!("/root/{index}.rs"), stamp(1, index)))
            .collect::<HashMap<_, _>>();
        let (changed, overflowed) = changed_poll_paths(&HashMap::new(), &current, true, 3);
        assert_eq!(changed.len(), 3);
        assert!(overflowed);
    }

    #[test]
    fn canonical_wsl_roots_select_polling_without_touching_a_distro() {
        assert_eq!(
            root_source_kind("\\\\?\\UNC\\wsl.localhost\\Ubuntu\\home\\user\\한글 project"),
            RootSourceKind::Wsl
        );
        assert_eq!(
            root_source_kind("C:/projects/devbox"),
            RootSourceKind::Native
        );
    }

    #[test]
    fn missing_poll_root_is_unavailable_not_an_empty_snapshot() {
        let missing = std::env::temp_dir().join(format!(
            "everything-plus-missing-poll-root-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&missing);
        assert_eq!(
            scan_poll_snapshot(&missing.to_string_lossy()).unwrap_err(),
            "root_unavailable"
        );
    }
}
