//! 등록 루트별 `notify` watcher와 증분 인덱싱 워커.
//!
//! 네이티브 notify 콜백은 이벤트만 채널에 넣는다 (IO 없음). 별도 워커 스레드가
//! 디바운스 후 DB에 반영한다. full re-index가 도는 동안은 증분 반영을 건너뛴다
//! (full re-index가 루트를 통째로 다시 쓰므로 generation 대신 `indexing` 플래그로
//! 배타 제어한다 — §8.3).

use crate::commands::indexing::{spawn_index, validate_root, AppState};
use crate::core::content::{extract_file, is_content_candidate};
use crate::core::db::{
    delete_content, delete_file, find_root_for, root_row_for, upsert_content_record, upsert_file,
};
use crate::core::models::RootStatus;
use crate::core::watcher::{classify_event, is_within_root, Debouncer, DEBOUNCE_WINDOW};
use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;
use tauri::AppHandle;

/// 루트 하나에 붙는 네이티브 watcher.
struct RootWatcher {
    _watcher: notify::RecommendedWatcher,
}

enum WatcherMessage {
    Event {
        root: String,
        event: notify::Event,
    },
    /// watcher overflow/backend 오류 → 해당 루트 reconciliation scan
    Rescan {
        root: String,
    },
    Shutdown,
}

type SharedStatus = Arc<Mutex<HashMap<String, RootStatus>>>;

/// 앱 수명 동안 루트 watcher와 워커를 관리한다.
pub struct WatcherManager {
    _app: AppHandle,
    state: Arc<AppState>,
    roots: Mutex<HashMap<String, RootWatcher>>,
    status: SharedStatus,
    sender: Sender<WatcherMessage>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl WatcherManager {
    pub fn new(app: AppHandle, state: Arc<AppState>) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel();
        let status: SharedStatus = Arc::new(Mutex::new(HashMap::new()));
        let worker_state = Arc::clone(&state);
        let worker_status = Arc::clone(&status);
        let worker = thread::Builder::new()
            .name("everything-plus-watcher".to_string())
            .spawn(move || watcher_worker(worker_state, worker_status, receiver))
            .expect("everything-plus watcher worker should start");
        Arc::new(Self {
            _app: app,
            state,
            roots: Mutex::new(HashMap::new()),
            status,
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
            if let Ok(validated) = validate_root(&root.path) {
                let _ = self.add(&validated);
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
        let sender = self.sender.clone();
        let root_for_cb = normalized.clone();
        let mut watcher = notify::recommended_watcher(move |result| match result {
            Ok(event) => {
                let _ = sender.send(WatcherMessage::Event {
                    root: root_for_cb.clone(),
                    event,
                });
            }
            Err(error) => {
                // overflow 등 — 해당 루트 reconciliation scan으로 수렴한다
                let _ = error;
                eprintln!("everything-plus: watcher requested reconciliation");
                let _ = sender.send(WatcherMessage::Rescan {
                    root: root_for_cb.clone(),
                });
            }
        })
        .map_err(|_| "검색 감시를 시작할 수 없습니다.".to_string())?;
        watcher
            .watch(std::path::Path::new(&normalized), RecursiveMode::Recursive)
            .map_err(|_| "검색 감시를 등록할 수 없습니다.".to_string())?;

        self.roots
            .lock()
            .unwrap()
            .insert(normalized.clone(), RootWatcher { _watcher: watcher });
        self.status.lock().unwrap().insert(
            normalized.clone(),
            RootStatus {
                root: normalized,
                last_synced_at: None,
                pending: 0,
                error: None,
            },
        );
        Ok(())
    }

    /// 루트 제거 시 watcher와 상태를 함께 해제한다.
    pub fn remove(&self, root_path: &str) {
        let normalized = crate::core::db::normalize_path(root_path);
        self.roots.lock().unwrap().remove(&normalized);
        self.status.lock().unwrap().remove(&normalized);
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
        let _ = self.sender.send(WatcherMessage::Shutdown);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

fn watcher_worker(state: Arc<AppState>, status: SharedStatus, receiver: Receiver<WatcherMessage>) {
    let mut debouncer = Debouncer::new(DEBOUNCE_WINDOW);
    loop {
        let message = match debouncer.next_deadline() {
            Some(deadline) => {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(message) => message,
                    Err(RecvTimeoutError::Timeout) => {
                        deliver_ready(&state, &status, &mut debouncer);
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
            WatcherMessage::Event { root, event } => {
                let class = classify_event(&event.kind);
                let paths: Vec<String> = event
                    .paths
                    .iter()
                    .map(|p| crate::core::db::normalize_path(&p.to_string_lossy()))
                    .filter(|p| is_within_root(&root, p))
                    .collect();
                if let Some(entry) = status.lock().unwrap().get_mut(&root) {
                    entry.pending = entry.pending.saturating_add(paths.len() as u32);
                    entry.error = None;
                }
                for path in paths {
                    debouncer.record(&PathBuf::from(path), Instant::now());
                }
                let _ = class;
            }
            WatcherMessage::Rescan { root } => {
                spawn_index(Arc::clone(&state), vec![root]);
            }
            WatcherMessage::Shutdown => break,
        }
    }
}

fn deliver_ready(state: &Arc<AppState>, status: &SharedStatus, debouncer: &mut Debouncer) {
    let ready = debouncer.take_ready(Instant::now());
    if ready.is_empty() {
        return;
    }
    // full re-index가 도는 동안에는 증분 반영을 건너뛴다 (배타 제어)
    if state.indexing.load(Ordering::SeqCst) {
        // Do not drop events that arrived while the full scan owned the index.
        // Re-arm the quiet window so the next worker observes the final file
        // state after the full scan has finished.
        let now = Instant::now();
        for path in ready {
            debouncer.record(&path, now);
        }
        return;
    }
    for path in &ready {
        let path_str = path.to_string_lossy().into_owned();
        if apply_incremental(state, &path_str).is_err() {
            eprintln!("everything-plus: incremental index failed");
            if let Ok(conn) = state.db.lock() {
                if let Ok(Some(root)) = find_root_for(&conn, &path_str) {
                    if let Some(entry) = status.lock().unwrap().get_mut(&root.path) {
                        entry.error = Some("incremental_index_failed".to_string());
                    }
                }
            }
        }
    }

    let now = now_ms();
    for entry in status.lock().unwrap().values_mut() {
        entry.last_synced_at = Some(now);
        entry.pending = 0;
    }
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
