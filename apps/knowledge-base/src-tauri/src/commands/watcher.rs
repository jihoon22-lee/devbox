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
use crate::core::vault::VaultIdentity;
use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::fs::{self, File};
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
    Event(Vec<PathBuf>),
    Shutdown,
}

/// 루트 감시와 워커를 관리한다.
pub struct KnowledgeWatcher {
    _app: AppHandle,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    sender: SyncSender<Message>,
    worker: Mutex<Option<JoinHandle<()>>>,
    root: Mutex<Option<VaultIdentity>>,
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
        let vault = VaultIdentity::inspect(root)
            .map_err(|_| "Knowledge watcher 저장 위치를 확인할 수 없습니다".to_string())?;
        let normalized = vault.canonical_path().to_path_buf();
        {
            let current = self.root.lock().unwrap();
            if current.as_ref() == Some(&vault) {
                return Ok(());
            }
        }
        let sender = self.sender.clone();
        let overflow = Arc::clone(&self.overflow);
        let mut watcher = notify::recommended_watcher(
            move |result: notify::Result<notify::Event>| match result {
                Ok(event) => {
                    let (paths, truncated) = bounded_event_paths(&event.paths);
                    if truncated {
                        overflow.store(true, Ordering::Release);
                    }
                    if !paths.is_empty() && sender.try_send(Message::Event(paths)).is_err() {
                        overflow.store(true, Ordering::Release);
                    }
                }
                Err(error) => {
                    eprintln!("knowledge-base watcher error: {error}");
                }
            },
        )
        .map_err(|e| format!("watcher 생성 실패: {e}"))?;
        watcher
            .watch(&normalized, RecursiveMode::Recursive)
            .map_err(|e| format!("watcher 등록 실패: {e}"))?;
        *self.watcher.lock().unwrap() = Some(watcher);
        *self.root.lock().unwrap() = Some(vault);
        Ok(())
    }
}

impl Drop for KnowledgeWatcher {
    fn drop(&mut self) {
        // Stop notify callbacks before waiting for the bounded queue to drain;
        // otherwise a busy editor can keep refilling the queue while shutdown
        // is trying to enqueue its sentinel.
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
                for path in &event {
                    debouncer.record(path, Instant::now());
                }
            }
            Message::Shutdown => break,
        }
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

fn deliver_ready(state: &Arc<AppState>, app: &AppHandle, debouncer: &mut Debouncer) {
    let ready = debouncer.take_ready(Instant::now());
    let overflowed = debouncer.take_overflow();
    if ready.is_empty() && !overflowed {
        return;
    }
    let vault = {
        let conn = state.db.lock().unwrap();
        crate::commands::docs::resolve_configured_root(&conn)
            .map(|root| VaultIdentity::inspect(&root))
    };
    let Ok(Ok(vault)) = vault else { return };
    let conn = state.db.lock().unwrap();
    let mut changed = false;
    for path in &ready {
        let Some(rel) = safe_relative_path(&vault, path) else {
            continue;
        };
        match read_bounded_note(&vault, path) {
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
        changed |= reconcile_root(&conn, &vault);
    }
    drop(conn);
    if changed {
        // 외부 editor에서 생긴 변경도 앱 내부 저장과 같은 activity 계약으로 발행한다.
        let _ = crate::integration::write_snapshot(&state.db.lock().unwrap());
        let _ = app.emit("docs-changed", ());
    }
}

fn reconcile_root(conn: &rusqlite::Connection, vault: &VaultIdentity) -> bool {
    let root = vault.canonical_path();
    let mut pending = vec![root.to_path_buf()];
    let mut directories = 0usize;
    let mut files = 0usize;
    let mut bytes = 0u64;
    let mut changed = false;
    while let Some(directory) = pending.pop() {
        if vault.revalidate().is_err() {
            return changed;
        }
        if directory != root {
            let Ok(relative) = directory.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if vault.new_entry(&relative).is_err() {
                continue;
            }
        }
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
            let Some(relative) = safe_relative_path(vault, &path) else {
                continue;
            };
            let Ok(content) = read_bounded_note(vault, &path) else {
                continue;
            };
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

/// Resolve a notify path to a strict vault-relative document path.  Missing
/// final components are allowed so delete/rename events can remove an old
/// index row, but existing links/reparse points are never followed.
fn safe_relative_path(vault: &VaultIdentity, path: &Path) -> Option<String> {
    // Windows canonical paths normally use a verbatim prefix (`\\?\`) while
    // notify/tests may provide the ordinary drive spelling. Resolve an
    // existing entry through the vault first so both spellings converge on
    // the same canonical root. Missing canonical event paths are retained for
    // delete/rename index cleanup.
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

fn read_bounded_note(vault: &VaultIdentity, path: &Path) -> Result<String, ()> {
    if safe_relative_path(vault, path).is_none() {
        return Err(());
    }
    let safe_path = vault.existing_path(path).map_err(|_| ())?;
    let metadata = fs::symlink_metadata(&safe_path).map_err(|_| ())?;
    if metadata.len() > MAX_WATCH_FILE_BYTES {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(&safe_path)
        .map_err(|_| ())?
        .take(MAX_WATCH_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_WATCH_FILE_BYTES {
        return Err(());
    }
    String::from_utf8(bytes).map_err(|_| ())
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
    fn reconciliation_budget_counts_unrelated_regular_files() {
        let mut files = 0;
        for _ in 0..MAX_RECONCILE_FILES {
            assert!(consume_reconcile_file(&mut files));
        }
        assert!(!consume_reconcile_file(&mut files));
        assert_eq!(files, MAX_RECONCILE_FILES);
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
    fn bounded_reader_accepts_regular_documents_and_rejects_oversized_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("note.txt"), b"text").unwrap();
        fs::write(root.path().join("note.md"), b"# ok").unwrap();
        let oversized = File::create(root.path().join("oversized.bin")).unwrap();
        oversized
            .set_len(MAX_WATCH_FILE_BYTES + 1)
            .expect("fixture should be sparse");
        let vault = VaultIdentity::inspect(root.path()).unwrap();
        assert_eq!(
            read_bounded_note(&vault, &root.path().join("note.txt")).unwrap(),
            "text"
        );
        assert_eq!(
            read_bounded_note(&vault, &root.path().join("note.md")).unwrap(),
            "# ok"
        );
        assert!(read_bounded_note(&vault, &root.path().join("oversized.bin")).is_err());
    }
}
