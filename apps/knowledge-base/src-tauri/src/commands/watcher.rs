//! KnowledgeRoot 감시 — 외부 편집을 재시작 없이 검색·태그 인덱스에 반영한다.
//!
//! # crates/watcher 추출 판단 (세 번째 소비자)
//! code-pad / everything-plus / knowledge-base 세 watcher의 요구를 비교했다.
//!
//! | | code-pad | everything-plus | knowledge-base |
//! |---|---|---|---|
//! | 감시 범위 | 부모 디렉터리, 비재귀, 파일명 집합 | 루트 재귀 | 루트 재귀 |
//! | 적용 대상 | 이벤트 전송(file-changed) | 파일 인덱스 upsert/delete | 문서 인덱스(frontmatter·태그) + tree 갱신 |
//! | 수명 관리 | generation 무효화 | 루트 추가/제거 | 루트 1개 |
//! | DB 스키마 | 없음 | files/file_content | docs |
//!
//! 공통 부분은 디바운스(수십 줄)뿐이고, 감시 범위·적용 대상·수명 관리가 모두 달라
//! 공용 크레이트로 묶으면 세 구현이 오히려 얽힌다. **추출하지 않는다.** 디바운스는
//! 각 앱에 두고, 셋째 소비자가 같은 형태로 다시 필요해지면 그때 추출을 재검토한다.

use crate::commands::docs::AppState;
use crate::core::db::{index_doc, remove_doc};
use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
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
const OVERFLOW_POLL: Duration = Duration::from_millis(500);

/// 경로별 quiet-period 디바운스 (순수).
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
        if path_byte_len(path) > MAX_WATCH_PATH_BYTES {
            self.overflow = true;
            return;
        }
        if !self.pending.contains_key(path) && self.pending.len() >= MAX_PENDING_WATCH_PATHS {
            self.overflow = true;
            return;
        }
        self.pending.insert(path.to_path_buf(), now);
    }
    fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|seen| *seen + self.window).min()
    }
    fn take_ready(&mut self, now: Instant) -> Vec<PathBuf> {
        let mut ready: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, seen)| now.saturating_duration_since(**seen) >= self.window)
            .map(|(path, _)| path.clone())
            .collect();
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
    Event(notify::Event),
    Shutdown,
}

/// 루트 감시와 워커를 관리한다.
pub struct KnowledgeWatcher {
    _app: AppHandle,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    sender: SyncSender<Message>,
    worker: Mutex<Option<JoinHandle<()>>>,
    root: Mutex<Option<PathBuf>>,
    overflow: Arc<AtomicBool>,
}

impl KnowledgeWatcher {
    pub fn new(app: AppHandle, state: Arc<AppState>) -> Arc<Self> {
        let (sender, receiver) = mpsc::sync_channel(MAX_WATCH_MESSAGES);
        let overflow = Arc::new(AtomicBool::new(false));
        let worker_state = Arc::clone(&state);
        let worker_app = app.clone();
        let worker_overflow = Arc::clone(&overflow);
        let worker = thread::Builder::new()
            .name("knowledge-base-watcher".to_string())
            .spawn(move || watcher_worker(worker_state, worker_app, receiver, worker_overflow))
            .expect("knowledge-base watcher worker should start");
        Arc::new(Self {
            _app: app,
            watcher: Mutex::new(None),
            sender,
            worker: Mutex::new(Some(worker)),
            root: Mutex::new(None),
            overflow,
        })
    }

    /// 루트를 감시한다. 이미 같은 루트면 no-op. 다른 루트면 재시작한다.
    pub fn set_root(&self, root: &Path) -> Result<(), String> {
        let normalized = root.to_path_buf();
        {
            let current = self.root.lock().unwrap();
            if current.as_ref() == Some(&normalized) {
                return Ok(());
            }
        }
        let sender = self.sender.clone();
        let overflow = Arc::clone(&self.overflow);
        let mut watcher = notify::recommended_watcher(move |result| match result {
            Ok(event) => {
                if sender.try_send(Message::Event(event)).is_err() {
                    overflow.store(true, Ordering::Release);
                }
            }
            Err(error) => {
                eprintln!("knowledge-base watcher error: {error}");
            }
        })
        .map_err(|e| format!("watcher 생성 실패: {e}"))?;
        watcher
            .watch(&normalized, RecursiveMode::Recursive)
            .map_err(|e| format!("watcher 등록 실패: {e}"))?;
        *self.watcher.lock().unwrap() = Some(watcher);
        *self.root.lock().unwrap() = Some(normalized);
        Ok(())
    }
}

impl Drop for KnowledgeWatcher {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Shutdown);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

fn watcher_worker(
    state: Arc<AppState>,
    app: AppHandle,
    receiver: Receiver<Message>,
    overflow: Arc<AtomicBool>,
) {
    let mut debouncer = Debouncer::new(DEBOUNCE_WINDOW);
    loop {
        if overflow.swap(false, Ordering::AcqRel) {
            debouncer.mark_overflow();
            if debouncer.next_deadline().is_none() {
                deliver_ready(&state, &app, &mut debouncer);
            }
        }
        let message = match debouncer.next_deadline() {
            Some(deadline) => {
                match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(message) => message,
                    Err(RecvTimeoutError::Timeout) => {
                        deliver_ready(&state, &app, &mut debouncer);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            None => match receiver.recv_timeout(OVERFLOW_POLL) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            },
        };
        match message {
            Message::Event(event) => {
                if event.paths.len() > MAX_EVENT_PATHS {
                    debouncer.mark_overflow();
                    continue;
                }
                for path in &event.paths {
                    debouncer.record(path, Instant::now());
                }
            }
            Message::Shutdown => break,
        }
    }
}

fn deliver_ready(state: &Arc<AppState>, app: &AppHandle, debouncer: &mut Debouncer) {
    let ready = debouncer.take_ready(Instant::now());
    let overflowed = debouncer.take_overflow();
    if ready.is_empty() && !overflowed {
        return;
    }
    let root = {
        let conn = state.db.lock().unwrap();
        let Ok(path) = crate::commands::docs::resolve_root(&conn) else {
            return;
        };
        path.canonicalize().unwrap_or(path)
    };
    let conn = state.db.lock().unwrap();
    let mut changed = false;
    for path in &ready {
        let event_path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let Ok(rel) = event_path.strip_prefix(&root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.is_empty() {
            continue;
        }
        if path.is_dir() {
            continue;
        }
        match read_watched_file(&event_path) {
            Ok(content) => {
                if index_doc(&conn, &rel, &content).is_ok() {
                    changed = true;
                }
            }
            Err(_) => {
                // 삭제/이름변경(이전 경로) — 인덱스에서 제거
                if remove_doc(&conn, &rel).is_ok() {
                    changed = true;
                }
            }
        }
    }
    if overflowed {
        changed |= reconcile_root(&conn, &root);
    }
    drop(conn);
    if changed {
        // 외부 editor에서 생긴 변경도 앱 내부 저장과 같은 activity 계약으로 발행한다.
        let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
        let _ = app.emit("docs-changed", ());
    }
}

fn read_watched_file(path: &Path) -> Result<String, ()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) || metadata.len() > MAX_WATCH_FILE_BYTES
    {
        return Err(());
    }
    let file = File::open(path).map_err(|_| ())?;
    let mut content = String::new();
    file.take(MAX_WATCH_FILE_BYTES + 1)
        .read_to_string(&mut content)
        .map_err(|_| ())?;
    if content.len() as u64 > MAX_WATCH_FILE_BYTES {
        return Err(());
    }
    Ok(content)
}

fn reconcile_root(conn: &rusqlite::Connection, root: &Path) -> bool {
    let mut pending = vec![root.to_path_buf()];
    let mut directories = 0usize;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut changed = false;
    while let Some(directory) = pending.pop() {
        directories += 1;
        if directories > MAX_RECONCILE_DIRECTORIES {
            return changed;
        }
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if files >= MAX_RECONCILE_FILES || bytes >= MAX_RECONCILE_BYTES {
                return changed;
            }
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if is_link_or_reparse(&metadata) {
                continue;
            }
            if metadata.is_dir() {
                if directories.saturating_add(pending.len()) >= MAX_RECONCILE_DIRECTORIES {
                    return changed;
                }
                pending.push(path);
                continue;
            }
            if !metadata.is_file() || metadata.len() > MAX_WATCH_FILE_BYTES {
                continue;
            }
            // Count every regular file examined, not only Markdown hits. A
            // directory full of unrelated files must not turn overflow
            // reconciliation into an unbounded scan.
            if !consume_reconcile_file(&mut files) {
                return changed;
            }
            if bytes.saturating_add(metadata.len()) > MAX_RECONCILE_BYTES {
                return changed;
            }
            let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
                continue;
            };
            if !extension.eq_ignore_ascii_case("md") {
                continue;
            }
            let Ok(content) = read_watched_file(&path) else {
                continue;
            };
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            bytes = bytes.saturating_add(content.len() as u64);
            if index_doc(conn, &relative, &content).is_ok() {
                changed = true;
            }
        }
    }
    changed
}

fn consume_reconcile_file(files: &mut usize) -> bool {
    if *files >= MAX_RECONCILE_FILES {
        return false;
    }
    *files += 1;
    true
}

fn is_link_or_reparse(metadata: &std::fs::Metadata) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debouncer_caps_pending_paths_and_marks_overflow() {
        let mut debouncer = Debouncer::new(Duration::from_millis(1));
        let now = Instant::now();
        for index in 0..=MAX_PENDING_WATCH_PATHS {
            debouncer.record(Path::new(&format!("note-{index}.md")), now);
        }
        assert!(debouncer.overflow);
        assert!(debouncer.pending.len() <= MAX_PENDING_WATCH_PATHS);
    }

    #[test]
    fn debouncer_rejects_oversized_paths_without_retaining_them() {
        let mut debouncer = Debouncer::new(Duration::from_millis(1));
        debouncer.record(
            Path::new(&"x".repeat(MAX_WATCH_PATH_BYTES + 1)),
            Instant::now(),
        );
        assert!(debouncer.pending.is_empty());
        assert!(debouncer.take_overflow());
    }

    #[test]
    fn watcher_file_reader_is_bounded() {
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(MAX_WATCH_FILE_BYTES + 1).unwrap();
        assert!(read_watched_file(file.path()).is_err());
    }

    #[test]
    fn reconciliation_budget_counts_unrelated_regular_files() {
        let mut files = 0;
        for _ in 0..MAX_RECONCILE_FILES {
            assert!(consume_reconcile_file(&mut files));
        }
        assert!(!consume_reconcile_file(&mut files));
        assert_eq!(files, MAX_RECONCILE_FILES);
    }
}
