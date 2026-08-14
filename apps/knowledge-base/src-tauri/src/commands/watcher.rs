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
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(600);

/// 경로별 quiet-period 디바운스 (순수).
struct Debouncer {
    window: Duration,
    pending: HashMap<PathBuf, Instant>,
}

impl Debouncer {
    fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
        }
    }
    fn record(&mut self, path: &Path, now: Instant) {
        self.pending.insert(path.to_path_buf(), now);
    }
    fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|seen| *seen + self.window).min()
    }
    fn take_ready(&mut self, now: Instant) -> Vec<PathBuf> {
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

enum Message {
    Event(notify::Event),
    Shutdown,
}

/// 루트 감시와 워커를 관리한다.
pub struct KnowledgeWatcher {
    _app: AppHandle,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    sender: Sender<Message>,
    worker: Mutex<Option<JoinHandle<()>>>,
    root: Mutex<Option<PathBuf>>,
}

impl KnowledgeWatcher {
    pub fn new(app: AppHandle, state: Arc<AppState>) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel();
        let worker_state = Arc::clone(&state);
        let worker_app = app.clone();
        let worker = thread::Builder::new()
            .name("knowledge-base-watcher".to_string())
            .spawn(move || watcher_worker(worker_state, worker_app, receiver))
            .expect("knowledge-base watcher worker should start");
        Arc::new(Self {
            _app: app,
            watcher: Mutex::new(None),
            sender,
            worker: Mutex::new(Some(worker)),
            root: Mutex::new(None),
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
        let mut watcher = notify::recommended_watcher(move |result| match result {
            Ok(event) => {
                let _ = sender.send(Message::Event(event));
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

fn watcher_worker(state: Arc<AppState>, app: AppHandle, receiver: Receiver<Message>) {
    let mut debouncer = Debouncer::new(DEBOUNCE_WINDOW);
    loop {
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
            None => match receiver.recv() {
                Ok(message) => message,
                Err(_) => break,
            },
        };
        match message {
            Message::Event(event) => {
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
    if ready.is_empty() {
        return;
    }
    let root = {
        let conn = state.db.lock().unwrap();
        crate::commands::docs::resolve_root(&conn)
    };
    let Ok(root) = root else { return };
    let conn = state.db.lock().unwrap();
    let mut changed = false;
    for path in &ready {
        let Ok(rel) = path.strip_prefix(&root) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.is_empty() {
            continue;
        }
        if path.is_dir() {
            continue;
        }
        match std::fs::read_to_string(path) {
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
    drop(conn);
    if changed {
        let _ = app.emit("docs-changed", ());
    }
}
