use crate::core::content::{extract_file, is_content_candidate, is_pdf_path};
use crate::core::db::{
    add_root as db_add_root, clear_all, clear_pdf, clear_root, content_status_summary,
    list_roots as db_list_roots, record_pdf_extractor_version, remove_root as db_remove_root,
    total_files, upsert_content_record, upsert_file,
};
use crate::core::models::IndexStatus;
use filesystem::collect;
use rusqlite::{Connection, OptionalExtension};
use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::Manager;

/// 한 트랜잭션에 담아 커밋할 파일 개수. 이 단위로 락을 반납해 `index_status`가
/// 인덱싱 도중에도 응답할 수 있게 한다.
const BATCH_SIZE: usize = 250;
const MAX_ROOT_BYTES: usize = 4 * 1024;
const ROOT_ERROR: &str = "검색 루트를 사용할 수 없습니다.";
const INDEX_ERROR: &str = "인덱스를 처리할 수 없습니다.";

/// 앱 전역 상태.
pub struct AppState {
    pub db: Mutex<Connection>,
    /// Serializes the indexing worker lifecycle with queued restart/cancel
    /// transitions. Without this gate a root change could race the worker's
    /// final `indexing = false` store and leave a restart request unclaimed.
    pub lifecycle: Mutex<()>,
    pub indexing: AtomicBool,
    pub cancel_requested: AtomicBool,
    pub restart_requested: AtomicBool,
    pub indexed: AtomicI64,
    pub total: AtomicI64,
    pub content_indexed: AtomicI64,
    pub content_truncated: AtomicI64,
    pub content_failed: AtomicI64,
    pub last_indexed_at: AtomicI64,
    pub last_error: Mutex<Option<String>>,
}

/// Scope used by a re-index worker.  `Pdf` is deliberately narrower than a
/// root re-index so changing the PDF extractor version cannot discard or
/// reread text/source/Markdown rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexFilter {
    All,
    Pdf,
}

impl IndexFilter {
    fn matches(self, path: &Path) -> bool {
        match self {
            Self::All => true,
            Self::Pdf => is_pdf_path(path),
        }
    }

    /// A queued request can represent a new root or a user-requested full
    /// rebuild.  Escalating to `All` is the only safe coalescing rule.
    fn queued_restart(self) -> Self {
        Self::All
    }
}

/// 인덱스 루트를 추가하고 인덱싱을 시작한다.
#[tauri::command]
pub fn add_root(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
    index_content: bool,
) -> Result<(), String> {
    let stored_path = validate_root(&path)?;
    let watcher = app.state::<Arc<crate::commands::watcher::WatcherManager>>();
    watcher.add(&stored_path)?;

    let result = {
        let conn = state.db.lock().map_err(|_| ROOT_ERROR.to_string())?;
        db_add_root(&conn, &stored_path, index_content)
    };
    let stored_path = match result {
        Ok(path) => path,
        Err(_) => {
            watcher.remove(&stored_path);
            return Err(ROOT_ERROR.to_string());
        }
    };
    // A root added while a full scan is running is picked up by the queued
    // restart rather than being lost to the current scan's snapshot.
    spawn_index(state.inner().clone(), vec![stored_path]);
    Ok(())
}

#[tauri::command]
pub fn remove_root(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    path: String,
) -> Result<(), String> {
    {
        let conn = state.db.lock().map_err(|_| ROOT_ERROR.to_string())?;
        db_remove_root(&conn, &path).map_err(|_| ROOT_ERROR.to_string())?;
    }
    // watcher 해제 (루트와 함께 pending 해제)
    let watcher = app.state::<Arc<crate::commands::watcher::WatcherManager>>();
    watcher.remove(&path);
    Ok(())
}

#[tauri::command]
pub fn list_roots(
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<Vec<crate::core::models::RootInfo>, String> {
    let conn = state.db.lock().map_err(|_| ROOT_ERROR.to_string())?;
    db_list_roots(&conn).map_err(|_| ROOT_ERROR.to_string())
}

/// 전체 루트를 다시 인덱싱한다. 이미 실행 중이면 현재 작업을 취소하고
/// 완료 후 한 번만 최신 상태로 다시 시작한다.
#[tauri::command]
pub fn index_now(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    spawn_index(state.inner().clone(), Vec::new());
    Ok(())
}

/// 진행 중인 색인을 협력적으로 중단한다. 파일 시스템 순회는 다음
/// 안전한 배치 경계에서 멈추며, 이미 커밋된 파일은 유효한 부분 색인으로
/// 남고 재시작하면 전체 루트가 다시 수렴한다.
#[tauri::command]
pub fn cancel_index(state: tauri::State<'_, Arc<AppState>>) -> Result<(), String> {
    let _lifecycle = state
        .lifecycle
        .lock()
        .map_err(|_| INDEX_ERROR.to_string())?;
    if state.indexing.load(Ordering::SeqCst) {
        state.restart_requested.store(false, Ordering::SeqCst);
        state.cancel_requested.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
pub fn index_status(state: tauri::State<'_, Arc<AppState>>) -> Result<IndexStatus, String> {
    let indexing = state.indexing.load(Ordering::SeqCst);
    let conn = state.db.lock().map_err(|_| INDEX_ERROR.to_string())?;
    let roots = db_list_roots(&conn).map_err(|_| INDEX_ERROR.to_string())?;
    let summary = content_status_summary(&conn).map_err(|_| INDEX_ERROR.to_string())?;
    let total = if indexing {
        state.total.load(Ordering::SeqCst)
    } else {
        total_files(&conn).map_err(|_| INDEX_ERROR.to_string())?
    };
    let last_indexed_at = state.last_indexed_at.load(Ordering::SeqCst);
    let last_error = state
        .last_error
        .lock()
        .map_err(|_| INDEX_ERROR.to_string())?
        .clone();
    Ok(IndexStatus {
        indexing,
        cancel_requested: state.cancel_requested.load(Ordering::SeqCst),
        total_files: total,
        indexed_files: state.indexed.load(Ordering::SeqCst),
        content_indexed_files: if indexing {
            state.content_indexed.load(Ordering::SeqCst)
        } else {
            summary.indexed_files
        },
        content_truncated_files: if indexing {
            state.content_truncated.load(Ordering::SeqCst)
        } else {
            summary.truncated_files
        },
        content_failed_files: if indexing {
            state.content_failed.load(Ordering::SeqCst)
        } else {
            summary.failed_files
        },
        roots: roots.len(),
        last_indexed_at: if last_indexed_at == 0 {
            summary.last_indexed_at
        } else {
            Some(last_indexed_at)
        },
        last_error,
    })
}

/// `only_roots`가 비어 있으면 전체 재인덱싱, 아니면 해당 루트만 부분 재인덱싱한다.
/// `pub(crate)`인 이유: 스키마 버전이 올라가 `migrate()`가 인덱스를 비운 직후
/// `lib.rs`의 setup에서 전체 재인덱싱을 걸 때도 이 함수를 재사용한다.
pub(crate) fn spawn_index(state: Arc<AppState>, only_roots: Vec<String>) {
    spawn_index_with_filter(state, only_roots, IndexFilter::All);
}

/// Rebuilds only PDF rows for every registered root.  This is used after a
/// PDF extractor version change and intentionally preserves ordinary content
/// rows.
pub(crate) fn spawn_pdf_reindex(state: Arc<AppState>) {
    spawn_index_with_filter(state, Vec::new(), IndexFilter::Pdf);
}

fn spawn_index_with_filter(state: Arc<AppState>, only_roots: Vec<String>, filter: IndexFilter) {
    {
        let _lifecycle = state
            .lifecycle
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.indexing.load(Ordering::SeqCst) {
            // A concurrent root change must not be lost.  A subsequent worker
            // snapshots all roots after the current worker reaches a safe stop.
            state.restart_requested.store(true, Ordering::SeqCst);
            state.cancel_requested.store(true, Ordering::SeqCst);
            return;
        }
        state.indexing.store(true, Ordering::SeqCst);
        reset_progress(&state);
    }

    let worker_state = Arc::clone(&state);
    let worker = std::thread::Builder::new()
        .name("everything-plus-indexer".to_string())
        .spawn(move || {
            let mut targets = only_roots;
            let mut active_filter = filter;
            loop {
                let result = run_index_with_filter(&worker_state, &targets, active_filter);
                if result.is_err() {
                    if let Ok(mut error) = worker_state.last_error.lock() {
                        *error = Some("indexing_failed".to_string());
                    }
                    // Do not expose the path or rusqlite/OS detail in logs.
                    eprintln!("everything-plus: indexing failed");
                }

                let restart = {
                    let _lifecycle = worker_state
                        .lifecycle
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    if worker_state.restart_requested.swap(false, Ordering::SeqCst) {
                        reset_progress(&worker_state);
                        true
                    } else {
                        worker_state.indexing.store(false, Ordering::SeqCst);
                        worker_state.cancel_requested.store(false, Ordering::SeqCst);
                        false
                    }
                };
                if !restart {
                    break;
                }
                // A queued restart always takes a fresh snapshot of every
                // registered root; this also incorporates roots added while
                // the previous scan was running.
                targets = Vec::new();
                active_filter = active_filter.queued_restart();
            }
        });
    if worker.is_err() {
        let _lifecycle = state
            .lifecycle
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.indexing.store(false, Ordering::SeqCst);
        state.cancel_requested.store(false, Ordering::SeqCst);
        state.restart_requested.store(false, Ordering::SeqCst);
        if let Ok(mut error) = state.last_error.lock() {
            *error = Some("indexing_failed".to_string());
        }
        eprintln!("everything-plus: indexer worker failed to start");
    }
}

fn reset_progress(state: &Arc<AppState>) {
    state.cancel_requested.store(false, Ordering::SeqCst);
    state.indexed.store(0, Ordering::SeqCst);
    state.total.store(0, Ordering::SeqCst);
    state.content_indexed.store(0, Ordering::SeqCst);
    state.content_truncated.store(0, Ordering::SeqCst);
    state.content_failed.store(0, Ordering::SeqCst);
    if let Ok(mut error) = state.last_error.lock() {
        *error = None;
    }
}

/// 실제 인덱싱. 파일 순회는 DB 락 밖에서 수행하고, 쓰기는 작은 트랜잭션으로
/// 분리해 검색/status command가 장시간 대기하지 않도록 한다.
fn run_index_with_filter(
    state: &Arc<AppState>,
    only_roots: &[String],
    filter: IndexFilter,
) -> rusqlite::Result<()> {
    let full_reindex = only_roots.is_empty();
    let targets: Vec<_> = {
        let conn = state.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let roots = db_list_roots(&conn)?;
        if full_reindex {
            match filter {
                IndexFilter::All => clear_all(&conn)?,
                IndexFilter::Pdf => {
                    for root in &roots {
                        clear_pdf(&conn, &root.path)?;
                    }
                }
            }
            roots
        } else {
            let targets: Vec<_> = roots
                .into_iter()
                .filter(|root| only_roots.contains(&root.path))
                .collect();
            for root in &targets {
                match filter {
                    IndexFilter::All => clear_root(&conn, &root.path)?,
                    IndexFilter::Pdf => clear_pdf(&conn, &root.path)?,
                }
            }
            targets
        }
    };

    for root in &targets {
        if state.cancel_requested.load(Ordering::SeqCst) {
            return Ok(());
        }
        let files: Vec<_> = collect(Path::new(&root.path))
            .into_iter()
            .filter(|file| filter.matches(&file.path))
            .collect();
        state.total.fetch_add(files.len() as i64, Ordering::SeqCst);
        for chunk in files.chunks(BATCH_SIZE) {
            if state.cancel_requested.load(Ordering::SeqCst) {
                return Ok(());
            }
            // Read/decode outside the DB mutex. A slow filesystem or a
            // 10-second candidate timeout must not block search/status calls.
            let mut prepared = Vec::with_capacity(chunk.len());
            for file in chunk {
                if state.cancel_requested.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let path = file.path.to_string_lossy().into_owned();
                let record = if root.content && is_content_candidate(&file.path) {
                    Some(extract_file(
                        &file.path,
                        file.size.max(0) as u64,
                        Instant::now(),
                    ))
                } else {
                    None
                };
                prepared.push((path, file.size, file.modified_ts, record));
            }

            if state.cancel_requested.load(Ordering::SeqCst) {
                return Ok(());
            }
            let conn = state.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
            let current_content: Option<bool> = conn
                .query_row(
                    "SELECT content != 0 FROM roots WHERE path = ?1",
                    [root.path.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            if current_content != Some(root.content) {
                // Root removal or a content-policy toggle won the race while
                // this chunk was being read. The queued re-index owns the
                // replacement state; do not commit a stale batch.
                continue;
            }
            conn.execute("BEGIN TRANSACTION", [])?;
            let mut failed = None;
            for (path, size, modified_ts, record) in &prepared {
                let file_id = match upsert_file(&conn, path, *size, *modified_ts, 0) {
                    Ok(file_id) => file_id,
                    Err(error) => {
                        failed = Some(error);
                        break;
                    }
                };
                if let Some(record) = record {
                    if let Err(error) = upsert_content_record(&conn, file_id, record, now_ms()) {
                        failed = Some(error);
                        break;
                    }
                }
            }
            if let Some(error) = failed {
                let _ = conn.execute("ROLLBACK", []);
                return Err(error);
            }
            conn.execute("COMMIT", [])?;
            state
                .indexed
                .fetch_add(prepared.len() as i64, Ordering::SeqCst);
            for (_, _, _, record) in prepared {
                if let Some(record) = record {
                    record_counters(state, record.status, record.truncated);
                }
            }
        }
    }
    if full_reindex {
        let conn = state.db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        record_pdf_extractor_version(&conn)?;
    }
    state
        .last_indexed_at
        .store(now_ms().max(1), Ordering::SeqCst);
    Ok(())
}

fn record_counters(
    state: &Arc<AppState>,
    status: crate::core::content::ContentStatus,
    truncated: bool,
) {
    if status == crate::core::content::ContentStatus::Indexed {
        state.content_indexed.fetch_add(1, Ordering::SeqCst);
        if truncated {
            state.content_truncated.fetch_add(1, Ordering::SeqCst);
        }
    } else {
        state.content_failed.fetch_add(1, Ordering::SeqCst);
    }
}

/// New roots are canonicalized once, and lexical relative/traversal/control
/// forms are refused before they reach the watcher or database.  Stored roots
/// are still rechecked by the watcher before each content read.
pub(crate) fn validate_root(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_ROOT_BYTES
        || trimmed.chars().any(|character| character.is_control())
    {
        return Err(ROOT_ERROR.to_string());
    }
    let raw = Path::new(trimmed);
    let unified = trimmed.replace('\\', "/");
    if unified.starts_with("//?/") || unified.starts_with("//./") {
        return Err(ROOT_ERROR.to_string());
    }
    if raw.is_relative()
        || raw
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ROOT_ERROR.to_string());
    }
    let metadata = std::fs::symlink_metadata(raw).map_err(|_| ROOT_ERROR.to_string())?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(ROOT_ERROR.to_string());
    }
    let canonical = std::fs::canonicalize(raw).map_err(|_| ROOT_ERROR.to_string())?;
    if !canonical.is_dir() {
        return Err(ROOT_ERROR.to_string());
    }
    Ok(crate::core::db::normalize_path(
        &canonical.to_string_lossy(),
    ))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::content::MAX_FILE_BYTES;
    use crate::core::db::{add_root as db_add_root, search, search_content};
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};
    use std::fs::{self, File};
    use std::sync::atomic::AtomicUsize;

    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    fn state(conn: Connection) -> Arc<AppState> {
        Arc::new(AppState {
            db: Mutex::new(conn),
            lifecycle: Mutex::new(()),
            indexing: AtomicBool::new(false),
            cancel_requested: AtomicBool::new(false),
            restart_requested: AtomicBool::new(false),
            indexed: AtomicI64::new(0),
            total: AtomicI64::new(0),
            content_indexed: AtomicI64::new(0),
            content_truncated: AtomicI64::new(0),
            content_failed: AtomicI64::new(0),
            last_indexed_at: AtomicI64::new(0),
            last_error: Mutex::new(None),
        })
    }

    fn pdf_fixture(text: &str) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = document.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 12.into()]),
                Operation::new("Td", vec![100.into(), 600.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = document.add_object(Stream::new(
            lopdf::Dictionary::new(),
            content.encode().unwrap(),
        ));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Resources" => resources_id,
            "Contents" => content_id,
        });
        document.objects.insert(
            pages_id,
            lopdf::Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn root_validation_rejects_relative_and_traversal_without_echoing_input() {
        for input in [
            "relative",
            "../private",
            "C:\\..\\secret",
            "\\\\?\\C:\\device-path",
            "\\\\.\\pipe\\device-path",
        ] {
            let error = validate_root(input).unwrap_err();
            assert_eq!(error, ROOT_ERROR);
            assert!(!error.contains(input));
        }
    }

    #[test]
    fn queued_format_reindex_escalates_to_a_full_rebuild() {
        assert_eq!(IndexFilter::Pdf.queued_restart(), IndexFilter::All);
        assert_eq!(IndexFilter::All.queued_restart(), IndexFilter::All);
    }

    #[test]
    fn root_validation_accepts_a_real_temp_directory() {
        let path =
            std::env::temp_dir().join(format!("everything-plus-indexing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let validated = validate_root(path.to_str().unwrap()).unwrap();
        assert!(Path::new(&validated).is_absolute());
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn indexes_utf8_utf16_empty_and_large_fixtures_then_updates_incrementally() {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "everything-plus-content-{id}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let english = root.join("english.txt");
        let korean = root.join("korean.md");
        let empty = root.join("empty.txt");
        let large = root.join("large.txt");
        let secret = root.join(".env");
        fs::write(&english, "offline content fixture").unwrap();
        fs::write(&korean, utf16le("한글 내용 fixture")).unwrap();
        File::create(&empty).unwrap();
        File::create(&large)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        fs::write(&secret, "TOKEN=must-not-be-indexed").unwrap();

        let conn = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        let stored_root = db_add_root(&conn, root.to_str().unwrap(), true).unwrap();
        let app_state = state(conn);
        run_index_with_filter(&app_state, &[], IndexFilter::All).unwrap();

        let conn = app_state.db.lock().unwrap();
        assert_eq!(search_content(&conn, "offline", 10).unwrap().len(), 1);
        assert_eq!(search_content(&conn, "한글", 10).unwrap().len(), 1);
        assert!(search_content(&conn, "fixture-that-is-not-present", 10)
            .unwrap()
            .is_empty());
        assert!(search_content(&conn, "must-not-be-indexed", 10)
            .unwrap()
            .is_empty());
        let large_status: String = conn
            .query_row(
                "SELECT content_status FROM file_content WHERE file_id =
                    (SELECT id FROM files WHERE path = ?1)",
                [crate::core::db::normalize_path(large.to_str().unwrap())],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(large_status, "too_large");
        let secret_status: String = conn
            .query_row(
                "SELECT content_status FROM file_content WHERE file_id =
                    (SELECT id FROM files WHERE path = ?1)",
                [crate::core::db::normalize_path(secret.to_str().unwrap())],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(secret_status, "skipped_sensitive");
        let summary = content_status_summary(&conn).unwrap();
        assert_eq!(summary.indexed_files, 3);
        assert_eq!(summary.failed_files, 2);
        drop(conn);

        fs::write(&english, "incremental update fixture").unwrap();
        crate::commands::watcher::apply_incremental(&app_state, english.to_str().unwrap()).unwrap();
        let conn = app_state.db.lock().unwrap();
        assert!(search_content(&conn, "incremental", 10).unwrap().len() == 1);
        assert!(search_content(&conn, "offline", 10).unwrap().is_empty());
        drop(conn);
        fs::remove_dir_all(&root).unwrap();
        assert!(!stored_root.is_empty());
    }

    #[test]
    fn pdf_reindex_replaces_only_pdf_content_and_preserves_text_content() {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "everything-plus-pdf-reindex-{id}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let text = root.join("notes.md");
        let pdf = root.join("report.pdf");
        let corrupt = root.join("broken.pdf");
        fs::write(&text, "text content remains").unwrap();
        fs::write(&pdf, pdf_fixture("old PDF content")).unwrap();
        fs::write(&corrupt, b"%PDF-1.7\nnot a valid fixture").unwrap();

        let conn = Connection::open_in_memory().unwrap();
        crate::core::db::migrate(&conn).unwrap();
        db_add_root(&conn, root.to_str().unwrap(), true).unwrap();
        let app_state = state(conn);
        run_index_with_filter(&app_state, &[], IndexFilter::All).unwrap();
        {
            let conn = app_state.db.lock().unwrap();
            assert_eq!(search_content(&conn, "remains", 10).unwrap().len(), 1);
            assert_eq!(search_content(&conn, "old PDF", 10).unwrap().len(), 1);
            assert_eq!(search(&conn, "broken", 10).unwrap().len(), 1);
            let status: String = conn
                .query_row(
                    "SELECT content_status FROM file_content WHERE file_id =
                        (SELECT id FROM files WHERE path = ?1)",
                    [crate::core::db::normalize_path(corrupt.to_str().unwrap())],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(status, "extract_error");
        }

        fs::write(&pdf, pdf_fixture("new PDF content")).unwrap();
        run_index_with_filter(&app_state, &[], IndexFilter::Pdf).unwrap();
        let conn = app_state.db.lock().unwrap();
        assert_eq!(search_content(&conn, "remains", 10).unwrap().len(), 1);
        assert!(search_content(&conn, "old PDF", 10).unwrap().is_empty());
        assert_eq!(search_content(&conn, "new PDF", 10).unwrap().len(), 1);
        drop(conn);
        fs::remove_dir_all(&root).unwrap();
    }

    fn utf16le(value: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in value.encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
    }
}
