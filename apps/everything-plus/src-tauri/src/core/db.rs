use crate::core::content::{
    ContentRecord, DOCX_EXTRACTOR_VERSION, ODS_EXTRACTOR_VERSION, PDF_EXTRACTOR_VERSION,
    XLSX_EXTRACTOR_VERSION, XLS_EXTRACTOR_VERSION,
};
use crate::core::models::{ContentResult, FileEntry, RootInfo};
use rusqlite::{params, Connection, OptionalExtension};

const PDF_EXTRACTOR_META_KEY: &str = "pdf_extractor_version";
const DOCX_EXTRACTOR_META_KEY: &str = "docx_extractor_version";
const XLS_EXTRACTOR_META_KEY: &str = "xls_extractor_version";
const XLSX_EXTRACTOR_META_KEY: &str = "xlsx_extractor_version";
const ODS_EXTRACTOR_META_KEY: &str = "ods_extractor_version";

/// 현재 스키마/정규화 규칙의 버전. 값을 올리면 다음 `migrate()` 호출 시
/// 기존 인덱스(파생 데이터)를 지우고 재인덱싱을 유도한다. 인덱스는 언제든
/// 다시 만들 수 있으므로 별도 마이그레이션 코드를 쓰지 않는다.
const SCHEMA_VERSION: i64 = 2;

/// DB를 열고 스키마(FTS5 외부 콘텐츠 테이블 + 트리거)를 준비한다.
/// 반환하는 `bool`은 `migrate()`가 스키마 버전 상승으로 파생 인덱스를
/// 비웠는지 여부다. 호출자(`lib.rs`의 setup)는 이 값이 `true`이고 등록된
/// 루트가 있으면 전체 재인덱싱을 걸어야 한다 — 그렇지 않으면 사용자가 빈
/// 검색 결과만 보고 앱이 고장났다고 오해한다.
pub fn init(path: &std::path::Path) -> rusqlite::Result<(Connection, bool)> {
    let conn = Connection::open(path)?;
    let cleared = migrate(&conn)?;
    Ok((conn, cleared))
}

/// 경로 구분자를 `/`로 통일하고 끝의 구분자를 제거한다.
/// Windows(`\`)와 유닉스(`/`) 구분자를 동일하게 취급하기 위해 저장 진입점
/// (`add_root`, `upsert_file`)에서 공통으로 사용한다.
///
/// 드라이브 루트(`C:\` → `C:/`)와 유닉스 루트(`/`)는 예외로, 구분자를
/// 제거하지 않는다. Windows에서 `C:`(구분자 없음)는 "그 드라이브의 현재
/// 디렉터리 기준 상대 경로"를 뜻하는 별개의 의미(드라이브 상대 경로)라
/// `C:/`(그 드라이브의 루트)와 다르다 — 구분자를 지우면 걷는 대상 디렉터리
/// 자체가 바뀐다.
pub fn normalize_path(path: &str) -> String {
    let mut unified = path.replace('\\', "/");
    // Windows canonicalize() may return an extended-length spelling. Keep the
    // stored/event spelling stable so a watcher callback using `C:/...` still
    // matches a root that was canonicalized as `\\\\?\\C:\\...`.
    if let Some(rest) = unified.strip_prefix("//?/UNC/") {
        unified = format!("//{rest}");
    } else if let Some(rest) = unified.strip_prefix("//?/") {
        unified = rest.to_string();
    }
    let trimmed = unified.trim_end_matches('/');
    if trimmed.is_empty() {
        // 입력이 "/"류(유닉스 루트)뿐이었던 경우
        return "/".to_string();
    }
    if is_drive_letter(trimmed) && unified.len() > trimmed.len() {
        // "C:" + 구분자가 하나 이상 있었다면 드라이브 루트다: 구분자를 보존한다.
        return format!("{trimmed}/");
    }
    trimmed.to_string()
}

/// `s`가 드라이브 문자 형태("C:", "d:" 등 정확히 2글자)인지 확인한다.
fn is_drive_letter(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// 반환값은 `clear_all()`이 실행되어 파생 인덱스(`files`/`file_content`)가
/// 비워졌는지 여부다. `init()`을 거쳐 호출자가 전체 재인덱싱을 걸어야 하는지
/// 판단하는 데 쓰인다.
pub fn migrate(conn: &Connection) -> rusqlite::Result<bool> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS roots (
            id INTEGER PRIMARY KEY,
            path TEXT UNIQUE NOT NULL,
            content INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS files (
            id INTEGER PRIMARY KEY,
            path TEXT UNIQUE NOT NULL,
            name TEXT NOT NULL,
            ext TEXT,
            size INTEGER NOT NULL,
            modified_ts INTEGER NOT NULL,
            root_id INTEGER
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(name, content='files', content_rowid='id');
        CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
            INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
        END;
        CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
            INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
        END;
        CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN
            INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
            INSERT INTO files_fts(rowid, name) VALUES (new.id, new.name);
        END;
        CREATE TABLE IF NOT EXISTS file_content (
            id INTEGER PRIMARY KEY,
            file_id INTEGER UNIQUE NOT NULL,
            content TEXT NOT NULL,
            content_status TEXT NOT NULL DEFAULT 'indexed',
            extractor_version TEXT NOT NULL DEFAULT 'text-v1',
            truncated INTEGER NOT NULL DEFAULT 0,
            indexed_at INTEGER,
            error_code TEXT,
            encoding TEXT,
            text_chars INTEGER NOT NULL DEFAULT 0
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS file_content_fts USING fts5(content, content='file_content', content_rowid='id');
        CREATE TRIGGER IF NOT EXISTS file_content_ai AFTER INSERT ON file_content BEGIN
            INSERT INTO file_content_fts(rowid, content) VALUES (new.id, new.content);
        END;
        CREATE TRIGGER IF NOT EXISTS file_content_ad AFTER DELETE ON file_content BEGIN
            INSERT INTO file_content_fts(file_content_fts, rowid, content) VALUES ('delete', old.id, old.content);
        END;
        CREATE TRIGGER IF NOT EXISTS file_content_au AFTER UPDATE ON file_content BEGIN
            INSERT INTO file_content_fts(file_content_fts, rowid, content) VALUES ('delete', old.id, old.content);
            INSERT INTO file_content_fts(rowid, content) VALUES (new.id, new.content);
        END;
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    // 기존 v0.4.x DB는 roots.content와 content metadata가 없을 수 있다.  먼저
    // 컬럼을 보강한 뒤 schema_version을 올리면서 파생 index를 재생성한다.
    ensure_column(conn, "roots", "content", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(
        conn,
        "file_content",
        "content_status",
        "TEXT NOT NULL DEFAULT 'indexed'",
    )?;
    ensure_column(
        conn,
        "file_content",
        "extractor_version",
        "TEXT NOT NULL DEFAULT 'text-v1'",
    )?;
    ensure_column(
        conn,
        "file_content",
        "truncated",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(conn, "file_content", "indexed_at", "INTEGER")?;
    ensure_column(conn, "file_content", "error_code", "TEXT")?;
    ensure_column(conn, "file_content", "encoding", "TEXT")?;
    ensure_column(
        conn,
        "file_content",
        "text_chars",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    // schema_version이 낮으면(신규 DB 포함) 파생 데이터(files/file_content)만 지워
    // 재인덱싱을 유도한다. roots(사용자가 등록한 경로 목록)는 그대로 둔다.
    let current_version: i64 = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let cleared = current_version < SCHEMA_VERSION;
    if cleared {
        clear_all(conn)?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        )?;
    }
    Ok(cleared)
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    // Table and column names are compile-time literals at every call site; do
    // not accept user input here because SQLite cannot bind identifiers.
    let present = conn
        .prepare(&format!(
            "SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1"
        ))?
        .exists([column])?;
    if !present {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))?;
    }
    Ok(())
}

/// 루트를 등록하고, 실제로 저장된(정규화된) 경로를 반환한다.
/// 호출자(커맨드 계층)는 이 반환값을 그대로 인덱싱 대상으로 써야 한다 —
/// 원본 입력 문자열을 다시 쓰면 정규화 전후 값이 어긋나 `run_index`의
/// 루트 필터가 아무 것도 매치하지 못할 수 있다.
pub fn add_root(conn: &Connection, path: &str, index_content: bool) -> rusqlite::Result<String> {
    let path = normalize_path(path);
    conn.execute(
        "INSERT INTO roots (path, content) VALUES (?1, ?2)
         ON CONFLICT(path) DO UPDATE SET content = excluded.content",
        params![path, index_content],
    )?;
    Ok(path)
}

pub fn list_roots(conn: &Connection) -> rusqlite::Result<Vec<RootInfo>> {
    let mut stmt = conn.prepare("SELECT path, content FROM roots ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(RootInfo {
            path: r.get(0)?,
            content: r.get::<_, i64>(1)? != 0,
        })
    })?;
    rows.collect()
}

/// 루트 하나를 등록 해제하고, 그 아래 인덱스된 데이터를 모두 지운다.
pub fn remove_root(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    let path = normalize_path(path);
    clear_root(conn, &path)?;
    conn.execute("DELETE FROM roots WHERE path = ?1", params![path])?;
    Ok(())
}

/// 특정 루트 아래의 인덱스 데이터를 지운다 (`file_content` → `files` 순, FK
/// CASCADE가 없으므로 순서가 중요하다). `remove_root`과 부분 재인덱싱이 공유한다.
pub fn clear_root(conn: &Connection, root_path: &str) -> rusqlite::Result<()> {
    let normalized = normalize_path(root_path);
    // 드라이브 루트("C:/")와 유닉스 루트("/")는 normalize_path가 이미 끝에
    // 구분자를 남겨두므로, 여기서 또 붙이면 "C://%"가 되어 매치가 0건이 된다.
    let prefix = if normalized.ends_with('/') {
        normalized
    } else {
        format!("{normalized}/")
    };
    let escaped = prefix.replace('%', "\\%").replace('_', "\\_");
    let pattern = format!("{escaped}%");
    conn.execute(
        "DELETE FROM file_content WHERE file_id IN
             (SELECT id FROM files WHERE path LIKE ?1 ESCAPE '\\')",
        params![pattern],
    )?;
    conn.execute(
        "DELETE FROM files WHERE path LIKE ?1 ESCAPE '\\'",
        params![pattern],
    )?;
    Ok(())
}

pub fn clear_all(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM file_content", [])?;
    conn.execute("DELETE FROM files", [])?;
    Ok(())
}

/// 파일을 upsert하고 파일 id를 반환한다. 같은 경로를 재인덱싱해도 `files.id`가
/// 유지되도록 `ON CONFLICT ... RETURNING`을 쓴다 (`INSERT OR REPLACE`는 충돌 시
/// 행을 삭제 후 재삽입해 id가 바뀌고, 그 결과 `file_content.file_id`가 고아가 된다).
pub fn upsert_file(
    conn: &Connection,
    path: &str,
    size: i64,
    modified_ts: i64,
    root_id: i64,
) -> rusqlite::Result<i64> {
    let path = normalize_path(path);
    let name = path.rsplit('/').next().unwrap_or(&path).to_string();
    let ext = path
        .rsplit('.')
        .next()
        .filter(|e| *e != path && e.len() <= 10)
        .unwrap_or("")
        .to_lowercase();
    conn.query_row(
        "INSERT INTO files (path, name, ext, size, modified_ts, root_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path) DO UPDATE SET
            name = excluded.name,
            ext = excluded.ext,
            size = excluded.size,
            modified_ts = excluded.modified_ts,
            root_id = excluded.root_id
         RETURNING id",
        params![path, name, ext, size, modified_ts, root_id],
        |r| r.get(0),
    )
}

/// Store a bounded extraction result.  Failed records contain an empty FTS
/// body but retain fixed status metadata so the UI can explain why a filename
/// exists without making a content hit.
pub fn upsert_content_record(
    conn: &Connection,
    file_id: i64,
    record: &ContentRecord,
    indexed_at: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO file_content
            (file_id, content, content_status, extractor_version, truncated,
             indexed_at, error_code, encoding, text_chars)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(file_id) DO UPDATE SET
            content = excluded.content,
            content_status = excluded.content_status,
            extractor_version = excluded.extractor_version,
            truncated = excluded.truncated,
            indexed_at = excluded.indexed_at,
            error_code = excluded.error_code,
            encoding = excluded.encoding,
            text_chars = excluded.text_chars",
        params![
            file_id,
            record.text.as_str(),
            record.status.as_str(),
            record.extractor_version,
            record.truncated,
            indexed_at,
            record.error_code,
            record.encoding,
            record.text_chars as i64,
        ],
    )?;
    Ok(())
}

/// Remove only PDF-derived rows below one root.  Format-specific reindexing
/// must leave text/source/Markdown rows untouched when the PDF extractor
/// version changes.
pub fn clear_pdf(conn: &Connection, root_path: &str) -> rusqlite::Result<()> {
    clear_format(conn, root_path, "pdf")
}

/// Remove only DOCX-derived rows below one root. DOCX reindexing must leave
/// text/source/Markdown and other document-format rows untouched.
pub fn clear_docx(conn: &Connection, root_path: &str) -> rusqlite::Result<()> {
    clear_format(conn, root_path, "docx")
}

fn clear_format(conn: &Connection, root_path: &str, extension: &str) -> rusqlite::Result<()> {
    let normalized = normalize_path(root_path);
    let prefix = if normalized.ends_with('/') {
        normalized
    } else {
        format!("{normalized}/")
    };
    let escaped = prefix.replace('%', "\\%").replace('_', "\\_");
    let pattern = format!("{escaped}%");
    conn.execute(
        "DELETE FROM file_content WHERE file_id IN
             (SELECT id FROM files WHERE ext = ?1 AND path LIKE ?2 ESCAPE '\\')",
        params![extension, pattern],
    )?;
    conn.execute(
        "DELETE FROM files WHERE ext = ?1 AND path LIKE ?2 ESCAPE '\\'",
        params![extension, pattern],
    )?;
    Ok(())
}

/// Whether PDF rows need a format-specific rebuild.  The metadata key is
/// required even when no PDF row exists: an upgraded v0.4 database has no PDF
/// rows at all, which must still trigger the first PDF scan.
pub fn pdf_reindex_required(conn: &Connection) -> rusqlite::Result<bool> {
    format_reindex_required(conn, PDF_EXTRACTOR_META_KEY, "pdf", PDF_EXTRACTOR_VERSION)
}

/// Whether DOCX rows need a format-specific rebuild. The marker is required
/// even when no DOCX row exists so an upgraded database receives its first
/// bounded WordprocessingML scan.
pub fn docx_reindex_required(conn: &Connection) -> rusqlite::Result<bool> {
    format_reindex_required(
        conn,
        DOCX_EXTRACTOR_META_KEY,
        "docx",
        DOCX_EXTRACTOR_VERSION,
    )
}

fn format_reindex_required(
    conn: &Connection,
    meta_key: &str,
    extension: &str,
    extractor_version: &str,
) -> rusqlite::Result<bool> {
    let recorded: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [meta_key], |row| {
            row.get(0)
        })
        .optional()?;
    if recorded.as_deref() != Some(extractor_version) {
        return Ok(true);
    }

    conn.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM file_content fc
            JOIN files f ON f.id = fc.file_id
            WHERE f.ext = ?1 AND fc.extractor_version <> ?2
        )",
        params![extension, extractor_version],
        |row| row.get(0),
    )
}

/// Remove only legacy XLS-derived rows below one root. Format-specific
/// reindexing must leave text/source/Markdown/PDF rows untouched when the XLS
/// extractor version changes.
pub fn clear_xls(conn: &Connection, root_path: &str) -> rusqlite::Result<()> {
    clear_format(conn, root_path, "xls")
}

pub fn clear_xlsx(conn: &Connection, root_path: &str) -> rusqlite::Result<()> {
    clear_format(conn, root_path, "xlsx")
}

pub fn clear_ods(conn: &Connection, root_path: &str) -> rusqlite::Result<()> {
    clear_format(conn, root_path, "ods")
}

/// Record a successfully completed full/PDF-only scan.  Callers must not set
/// this marker after cancellation or a partial-root scan.
pub fn record_pdf_extractor_version(conn: &Connection) -> rusqlite::Result<()> {
    record_format_version(conn, PDF_EXTRACTOR_META_KEY, PDF_EXTRACTOR_VERSION)
}

/// Record a successfully completed full/DOCX-only scan. Callers must not set
/// this marker after cancellation or a partial-root scan.
pub fn record_docx_extractor_version(conn: &Connection) -> rusqlite::Result<()> {
    record_format_version(conn, DOCX_EXTRACTOR_META_KEY, DOCX_EXTRACTOR_VERSION)
}

fn record_format_version(
    conn: &Connection,
    meta_key: &str,
    extractor_version: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![meta_key, extractor_version],
    )?;
    Ok(())
}

/// Whether XLS rows need a format-specific rebuild. The metadata key is
/// required even when no XLS row exists so a pre-XLS database still receives
/// its first bounded workbook scan.
pub fn xls_reindex_required(conn: &Connection) -> rusqlite::Result<bool> {
    format_reindex_required(conn, XLS_EXTRACTOR_META_KEY, "xls", XLS_EXTRACTOR_VERSION)
}

pub fn xlsx_reindex_required(conn: &Connection) -> rusqlite::Result<bool> {
    format_reindex_required(
        conn,
        XLSX_EXTRACTOR_META_KEY,
        "xlsx",
        XLSX_EXTRACTOR_VERSION,
    )
}

pub fn ods_reindex_required(conn: &Connection) -> rusqlite::Result<bool> {
    format_reindex_required(conn, ODS_EXTRACTOR_META_KEY, "ods", ODS_EXTRACTOR_VERSION)
}

/// Record a successfully completed full/XLS-only scan. Callers must not set
/// this marker after cancellation or a partial-root scan.
pub fn record_xls_extractor_version(conn: &Connection) -> rusqlite::Result<()> {
    record_format_version(conn, XLS_EXTRACTOR_META_KEY, XLS_EXTRACTOR_VERSION)
}

pub fn record_xlsx_extractor_version(conn: &Connection) -> rusqlite::Result<()> {
    record_format_version(conn, XLSX_EXTRACTOR_META_KEY, XLSX_EXTRACTOR_VERSION)
}

pub fn record_ods_extractor_version(conn: &Connection) -> rusqlite::Result<()> {
    record_format_version(conn, ODS_EXTRACTOR_META_KEY, ODS_EXTRACTOR_VERSION)
}

/// Remove content metadata when a file becomes non-text, is no longer covered
/// by a content-enabled root, or is replaced by a directory/symlink.
pub fn delete_content(conn: &Connection, file_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM file_content WHERE file_id = ?1", [file_id])? > 0)
}

pub fn content_status_summary(conn: &Connection) -> rusqlite::Result<ContentStatusSummary> {
    let mut stmt = conn.prepare(
        "SELECT
            COALESCE(SUM(CASE WHEN content_status = 'indexed' THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN truncated != 0 THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN content_status != 'indexed' THEN 1 ELSE 0 END), 0),
            MAX(indexed_at)
         FROM file_content",
    )?;
    stmt.query_row([], |row| {
        Ok(ContentStatusSummary {
            indexed_files: row.get(0)?,
            truncated_files: row.get(1)?,
            failed_files: row.get(2)?,
            last_indexed_at: row.get(3)?,
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentStatusSummary {
    pub indexed_files: i64,
    pub truncated_files: i64,
    pub failed_files: i64,
    pub last_indexed_at: Option<i64>,
}

/// 파일과 그 내용 인덱스를 삭제한다. 삭제된 행이 있었으면 true.
pub fn delete_file(conn: &Connection, path: &str) -> rusqlite::Result<bool> {
    let path = normalize_path(path);
    let deleted = conn.execute(
        "DELETE FROM file_content WHERE file_id IN (SELECT id FROM files WHERE path = ?1)",
        params![path],
    )?;
    let removed = conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
    Ok(deleted > 0 || removed > 0)
}

/// 경로를 포함하는 루트를 찾는다 (가장 긴 prefix 우선).
pub fn find_root_for(conn: &Connection, path: &str) -> rusqlite::Result<Option<RootInfo>> {
    let path = normalize_path(path);
    let roots = list_roots(conn)?;
    let mut best: Option<(usize, RootInfo)> = None;
    for root in roots {
        if crate::core::watcher::is_within_root(&root.path, &path) {
            let matched = root.path.len();
            if best.as_ref().map(|(len, _)| matched > *len).unwrap_or(true) {
                best = Some((matched, root));
            }
        }
    }
    Ok(best.map(|(_, r)| r))
}

/// 경로를 포함하는 루트의 (id, content)를 찾는다.
pub fn root_row_for(conn: &Connection, path: &str) -> rusqlite::Result<Option<(i64, bool)>> {
    let Some(root) = find_root_for(conn, path)? else {
        return Ok(None);
    };
    let id: i64 = conn.query_row(
        "SELECT id FROM roots WHERE path = ?1",
        params![root.path],
        |r| r.get(0),
    )?;
    Ok(Some((id, root.content)))
}

pub fn total_files(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
}

/// FTS5 파일명 검색. 쿼리는 토큰 단위 prefix 매치로 안전하게 이스케이프한다.
pub fn search(conn: &Connection, query: &str, limit: i64) -> rusqlite::Result<Vec<FileEntry>> {
    // Regex filename mode asks for a larger bounded FTS candidate set and then
    // performs the regular-expression match in the frontend.
    let limit = limit.clamp(0, 2_000);
    let q = search::build_fts_query(query);
    let mut stmt = conn.prepare(
        "SELECT f.id, f.path, f.name, f.ext, f.size, f.modified_ts
         FROM files_fts JOIN files f ON f.id = files_fts.rowid
         WHERE files_fts MATCH ?1
         ORDER BY f.name
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![q, limit], |r| {
        Ok(FileEntry {
            id: r.get(0)?,
            path: r.get(1)?,
            name: r.get(2)?,
            ext: r.get(3)?,
            size: r.get(4)?,
            modified_ts: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// FTS5 내용 검색. 스니펫을 함께 반환한다.
pub fn search_content(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<ContentResult>> {
    let limit = limit.clamp(0, 200);
    let q = search::build_fts_query(query);
    let mut stmt = conn.prepare(
        "SELECT f.path, f.name, snippet(file_content_fts, 0, '[', ']', '…', 20) AS snip
         FROM file_content_fts
         JOIN file_content fc ON fc.id = file_content_fts.rowid
         JOIN files f ON f.id = fc.file_id
         WHERE file_content_fts MATCH ?1 AND fc.content_status = 'indexed'
         ORDER BY f.name
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![q, limit], |r| {
        Ok(ContentResult {
            path: r.get(0)?,
            name: r.get(1)?,
            snippet: crate::core::content::redact_snippet(&r.get::<_, String>(2)?),
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::content::{
        ContentStatus, DOCX_EXTRACTOR_VERSION, EXTRACTOR_VERSION, ODS_EXTRACTOR_VERSION,
        XLSX_EXTRACTOR_VERSION, XLS_EXTRACTOR_VERSION,
    };

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    fn indexed_record(content: &str) -> ContentRecord {
        ContentRecord {
            text: content.to_string(),
            status: ContentStatus::Indexed,
            extractor_version: EXTRACTOR_VERSION,
            encoding: Some("utf8"),
            truncated: false,
            error_code: None,
            text_chars: content.chars().count(),
        }
    }

    fn indexed_pdf_record(content: &str, extractor_version: &'static str) -> ContentRecord {
        ContentRecord {
            text: content.to_string(),
            status: ContentStatus::Indexed,
            extractor_version,
            encoding: Some("pdf"),
            truncated: false,
            error_code: None,
            text_chars: content.chars().count(),
        }
    }

    fn indexed_docx_record(content: &str, extractor_version: &'static str) -> ContentRecord {
        ContentRecord {
            text: content.to_string(),
            status: ContentStatus::Indexed,
            extractor_version,
            encoding: Some("docx"),
            truncated: false,
            error_code: None,
            text_chars: content.chars().count(),
        }
    }

    fn indexed_xls_record(content: &str, extractor_version: &'static str) -> ContentRecord {
        ContentRecord {
            text: content.to_string(),
            status: ContentStatus::Indexed,
            extractor_version,
            encoding: Some("xls"),
            truncated: false,
            error_code: None,
            text_chars: content.chars().count(),
        }
    }

    fn indexed_xlsx_record(content: &str, extractor_version: &'static str) -> ContentRecord {
        ContentRecord {
            text: content.to_string(),
            status: ContentStatus::Indexed,
            extractor_version,
            encoding: Some("xlsx"),
            truncated: false,
            error_code: None,
            text_chars: content.chars().count(),
        }
    }

    fn indexed_ods_record(content: &str, extractor_version: &'static str) -> ContentRecord {
        ContentRecord {
            text: content.to_string(),
            status: ContentStatus::Indexed,
            extractor_version,
            encoding: Some("ods"),
            truncated: false,
            error_code: None,
            text_chars: content.chars().count(),
        }
    }

    fn seed(conn: &Connection) {
        for (path, size) in [
            ("C:/projects/PortManager/src/lib.rs", 10),
            ("C:/projects/PortManager/PLAN.md", 20),
            ("C:/projects/DevBox/target/release/a.exe", 99),
        ] {
            upsert_file(conn, path, size, 0, 1).unwrap();
        }
    }

    #[test]
    fn fts_matches_prefix() {
        let conn = mem();
        seed(&conn);
        let results = search(&conn, "plan", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].path.contains("PLAN.md"));
    }

    #[test]
    fn fts_matches_partial_via_prefix() {
        let conn = mem();
        seed(&conn);
        let results = search(&conn, "li", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].name.ends_with(".rs"));
    }

    #[test]
    fn fts_matches_multiple_tokens() {
        let conn = mem();
        seed(&conn);
        // 이름 "lib.rs"는 토큰 lib, rs 로 분리된다
        let results = search(&conn, "lib rs", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn filename_search_preserves_regex_candidate_limit_above_two_hundred() {
        let conn = mem();
        for index in 0..205 {
            upsert_file(
                &conn,
                &format!("C:/projects/shared/candidate-{index:03}.txt"),
                1,
                0,
                1,
            )
            .unwrap();
        }
        assert_eq!(search(&conn, "candidate", 500).unwrap().len(), 205);
    }

    #[test]
    fn upsert_replace_keeps_fts_in_sync() {
        let conn = mem();
        seed(&conn);
        // 같은 경로를 다른 이름으로 재삽입하면 FTS에서 이전 이름이 사라져야 한다
        upsert_file(&conn, "C:/projects/PortManager/PLAN.md", 30, 0, 1).unwrap();
        let by_old = search(&conn, "plan", 10).unwrap();
        let by_new = search(&conn, "plan", 10).unwrap();
        assert_eq!(by_old.len(), by_new.len());
        assert_eq!(by_new[0].size, 30);
    }

    #[test]
    fn content_search_matches_body() {
        let conn = mem();
        let id = upsert_file(&conn, "C:/notes/meeting.md", 10, 0, 1).unwrap();
        let record = indexed_record("quarterly review with the team");
        upsert_content_record(&conn, id, &record, 1).unwrap();
        let res = search_content(&conn, "quarterly", 10).unwrap();
        assert_eq!(res.len(), 1);
        assert!(res[0].snippet.contains("quarterly"));
        // 미인덱스 파일은 내용 검색에서 제외
        let id2 = upsert_file(&conn, "C:/notes/other.md", 10, 0, 1).unwrap();
        let _ = id2;
        let res = search_content(&conn, "quarterly", 10).unwrap();
        assert_eq!(res.len(), 1);
    }

    #[test]
    fn content_metadata_filters_failures_and_redacts_snippets() {
        let conn = mem();
        let indexed = upsert_file(&conn, "C:/notes/meeting.md", 10, 0, 1).unwrap();
        let mut record = indexed_record("Authorization: Bearer abc123 quarterly review");
        record.text_chars = 46;
        upsert_content_record(&conn, indexed, &record, 123).unwrap();
        let failed = upsert_file(&conn, "C:/notes/large.txt", 20, 0, 1).unwrap();
        let failed_record = ContentRecord {
            text: String::new(),
            status: ContentStatus::TooLarge,
            extractor_version: EXTRACTOR_VERSION,
            encoding: None,
            truncated: false,
            error_code: Some("file_too_large"),
            text_chars: 0,
        };
        upsert_content_record(&conn, failed, &failed_record, 124).unwrap();

        let results = search_content(&conn, "quarterly", 500).unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].snippet.contains("abc123"));
        let summary = content_status_summary(&conn).unwrap();
        assert_eq!(summary.indexed_files, 1);
        assert_eq!(summary.failed_files, 1);
        assert_eq!(summary.last_indexed_at, Some(124));
        let extractor_version: String = conn
            .query_row(
                "SELECT extractor_version FROM file_content WHERE file_id = ?1",
                [indexed],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(extractor_version, EXTRACTOR_VERSION);
    }

    #[test]
    fn clear_root_escapes_like_wildcards_and_keeps_sibling_paths() {
        let conn = mem();
        upsert_file(&conn, "C:/a%/inside.md", 1, 0, 1).unwrap();
        upsert_file(&conn, "C:/aX/sibling.md", 1, 0, 1).unwrap();
        clear_root(&conn, "C:/a%").unwrap();
        assert!(search(&conn, "inside", 10).unwrap().is_empty());
        assert_eq!(search(&conn, "sibling", 10).unwrap().len(), 1);
    }

    #[test]
    fn normalize_path_unifies_separators_and_trims_trailing_slash() {
        assert_eq!(normalize_path("C:\\projects\\foo\\"), "C:/projects/foo");
        assert_eq!(normalize_path("C:/projects/foo/"), "C:/projects/foo");
        assert_eq!(normalize_path("C:\\projects\\foo"), "C:/projects/foo");
        assert_eq!(normalize_path("C:/projects/foo"), "C:/projects/foo");
    }

    #[test]
    fn normalize_path_preserves_drive_and_unix_root_separator() {
        // 드라이브 루트: 구분자를 지우면 "C:"(드라이브 상대 경로, 다른 의미)가
        // 되어버리므로 반드시 유지해야 한다.
        assert_eq!(normalize_path("C:\\"), "C:/");
        assert_eq!(normalize_path("C:/"), "C:/");
        assert_eq!(normalize_path("d:\\"), "d:/");
        // 구분자가 아예 없는 "C:"는 드라이브 상대 경로이므로 그대로 둔다
        // (없던 구분자를 새로 붙이지 않는다).
        assert_eq!(normalize_path("C:"), "C:");
        // 유닉스 루트
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(
            normalize_path("\\\\?\\C:\\projects\\foo"),
            "C:/projects/foo"
        );
        assert_eq!(
            normalize_path("\\\\?\\UNC\\server\\share\\project"),
            "//server/share/project"
        );
    }

    #[test]
    fn upsert_file_preserves_id_on_reindex() {
        let conn = mem();
        let id1 = upsert_file(&conn, "C:/projects/foo/bar.rs", 10, 100, 1).unwrap();
        let record = indexed_record("fn main() {}");
        upsert_content_record(&conn, id1, &record, 1).unwrap();
        // 같은 경로를 다시 인덱싱해도(크기/수정시각 변경) id는 그대로여야
        // file_content가 고아가 되지 않는다.
        let id2 = upsert_file(&conn, "C:/projects/foo/bar.rs", 20, 200, 1).unwrap();
        assert_eq!(id1, id2);
        let res = search_content(&conn, "main", 10).unwrap();
        assert_eq!(res.len(), 1);
    }

    #[test]
    fn partial_reindex_preserves_other_roots() {
        // 부분 재인덱싱(대상 루트만 clear_root)이 clear_all을 대체했는지 확인한다.
        let conn = mem();
        add_root(&conn, "C:/A", false).unwrap();
        add_root(&conn, "C:/B", false).unwrap();
        upsert_file(&conn, "C:/A/one.rs", 1, 0, 1).unwrap();
        upsert_file(&conn, "C:/B/two.rs", 2, 0, 2).unwrap();

        // B만 다시 인덱싱: clear_root(B) 후 B의 파일을 재삽입한다.
        clear_root(&conn, "C:/B").unwrap();
        upsert_file(&conn, "C:/B/two.rs", 2, 0, 2).unwrap();

        assert_eq!(total_files(&conn).unwrap(), 2);
        let results = search(&conn, "one", 10).unwrap();
        assert_eq!(
            results.len(),
            1,
            "루트 A의 인덱스가 그대로 남아 있어야 한다"
        );
    }

    #[test]
    fn clear_pdf_only_removes_pdf_rows_below_the_requested_root() {
        let conn = mem();
        add_root(&conn, "C:/A", true).unwrap();
        add_root(&conn, "C:/B", true).unwrap();
        let text_id = upsert_file(&conn, "C:/A/notes.md", 1, 0, 1).unwrap();
        upsert_content_record(&conn, text_id, &indexed_record("ordinary source"), 1).unwrap();
        let pdf_a = upsert_file(&conn, "C:/A/report.pdf", 1, 0, 1).unwrap();
        upsert_content_record(&conn, pdf_a, &indexed_pdf_record("old pdf A", "pdf-old"), 2)
            .unwrap();
        let pdf_b = upsert_file(&conn, "C:/B/report.pdf", 1, 0, 2).unwrap();
        upsert_content_record(
            &conn,
            pdf_b,
            &indexed_pdf_record("old pdf B", PDF_EXTRACTOR_VERSION),
            3,
        )
        .unwrap();

        clear_pdf(&conn, "C:/A").unwrap();

        assert_eq!(search_content(&conn, "ordinary", 10).unwrap().len(), 1);
        assert!(search_content(&conn, "old pdf A", 10).unwrap().is_empty());
        assert_eq!(search_content(&conn, "old pdf B", 10).unwrap().len(), 1);
        record_pdf_extractor_version(&conn).unwrap();
        assert!(!pdf_reindex_required(&conn).unwrap());
    }

    #[test]
    fn pdf_reindex_metadata_detects_first_install_and_stale_rows() {
        let conn = mem();
        assert!(pdf_reindex_required(&conn).unwrap());
        record_pdf_extractor_version(&conn).unwrap();
        assert!(!pdf_reindex_required(&conn).unwrap());

        let text_id = upsert_file(&conn, "C:/notes/readme.md", 1, 0, 1).unwrap();
        upsert_content_record(&conn, text_id, &indexed_record("text-v1 row"), 1).unwrap();
        let pdf_id = upsert_file(&conn, "C:/notes/report.pdf", 1, 0, 1).unwrap();
        upsert_content_record(&conn, pdf_id, &indexed_pdf_record("old", "pdf-old"), 2).unwrap();
        assert!(pdf_reindex_required(&conn).unwrap());

        upsert_content_record(
            &conn,
            pdf_id,
            &indexed_pdf_record("current", PDF_EXTRACTOR_VERSION),
            3,
        )
        .unwrap();
        assert!(!pdf_reindex_required(&conn).unwrap());
    }

    #[test]
    fn clear_docx_only_removes_docx_rows_below_the_requested_root() {
        let conn = mem();
        add_root(&conn, "C:/A", true).unwrap();
        add_root(&conn, "C:/B", true).unwrap();
        let text_a = upsert_file(&conn, "C:/A/notes.md", 1, 0, 1).unwrap();
        let xlsx_a = upsert_file(&conn, "C:/A/report.xlsx", 1, 0, 1).unwrap();
        let docx_a = upsert_file(&conn, "C:/A/report.docx", 1, 0, 1).unwrap();
        let docx_b = upsert_file(&conn, "C:/B/report.docx", 1, 0, 2).unwrap();
        upsert_content_record(&conn, text_a, &indexed_record("ordinary source"), 1).unwrap();
        upsert_content_record(
            &conn,
            xlsx_a,
            &indexed_xlsx_record("xlsx stays", XLSX_EXTRACTOR_VERSION),
            1,
        )
        .unwrap();
        upsert_content_record(&conn, docx_a, &indexed_docx_record("docx A", "docx-old"), 2)
            .unwrap();
        upsert_content_record(
            &conn,
            docx_b,
            &indexed_docx_record("docx B", DOCX_EXTRACTOR_VERSION),
            3,
        )
        .unwrap();

        clear_docx(&conn, "C:/A").unwrap();

        assert_eq!(search_content(&conn, "ordinary", 10).unwrap().len(), 1);
        assert_eq!(search_content(&conn, "xlsx stays", 10).unwrap().len(), 1);
        assert!(search_content(&conn, "docx A", 10).unwrap().is_empty());
        assert_eq!(search_content(&conn, "docx B", 10).unwrap().len(), 1);
    }

    #[test]
    fn docx_reindex_metadata_detects_first_install_and_stale_rows_independently() {
        let conn = mem();
        assert!(docx_reindex_required(&conn).unwrap());
        record_docx_extractor_version(&conn).unwrap();
        assert!(!docx_reindex_required(&conn).unwrap());
        assert!(xlsx_reindex_required(&conn).unwrap());

        let text_id = upsert_file(&conn, "C:/notes/readme.md", 1, 0, 1).unwrap();
        upsert_content_record(&conn, text_id, &indexed_record("text-v1 row"), 1).unwrap();
        let docx_id = upsert_file(&conn, "C:/notes/report.docx", 1, 0, 1).unwrap();
        upsert_content_record(&conn, docx_id, &indexed_docx_record("old", "docx-old"), 2).unwrap();
        assert!(docx_reindex_required(&conn).unwrap());

        upsert_content_record(
            &conn,
            docx_id,
            &indexed_docx_record("current", DOCX_EXTRACTOR_VERSION),
            3,
        )
        .unwrap();
        assert!(!docx_reindex_required(&conn).unwrap());
        assert!(xlsx_reindex_required(&conn).unwrap());
    }

    #[test]
    fn clear_xls_only_removes_xls_rows_below_the_requested_root() {
        let conn = mem();
        add_root(&conn, "C:/A", true).unwrap();
        add_root(&conn, "C:/B", true).unwrap();
        let text_id = upsert_file(&conn, "C:/A/notes.md", 1, 0, 1).unwrap();
        upsert_content_record(&conn, text_id, &indexed_record("ordinary source"), 1).unwrap();
        let xls_a = upsert_file(&conn, "C:/A/report.xls", 1, 0, 1).unwrap();
        upsert_content_record(&conn, xls_a, &indexed_xls_record("old xls A", "xls-old"), 2)
            .unwrap();
        let xls_b = upsert_file(&conn, "C:/B/report.xls", 1, 0, 2).unwrap();
        upsert_content_record(
            &conn,
            xls_b,
            &indexed_xls_record("old xls B", XLS_EXTRACTOR_VERSION),
            3,
        )
        .unwrap();

        clear_xls(&conn, "C:/A").unwrap();

        assert_eq!(search_content(&conn, "ordinary", 10).unwrap().len(), 1);
        assert!(search_content(&conn, "old xls A", 10).unwrap().is_empty());
        assert_eq!(search_content(&conn, "old xls B", 10).unwrap().len(), 1);
        record_xls_extractor_version(&conn).unwrap();
        assert!(!xls_reindex_required(&conn).unwrap());
    }

    #[test]
    fn xls_reindex_metadata_detects_first_install_and_stale_rows() {
        let conn = mem();
        assert!(xls_reindex_required(&conn).unwrap());
        record_xls_extractor_version(&conn).unwrap();
        assert!(!xls_reindex_required(&conn).unwrap());

        let text_id = upsert_file(&conn, "C:/notes/readme.md", 1, 0, 1).unwrap();
        upsert_content_record(&conn, text_id, &indexed_record("text-v1 row"), 1).unwrap();
        let xls_id = upsert_file(&conn, "C:/notes/report.xls", 1, 0, 1).unwrap();
        upsert_content_record(&conn, xls_id, &indexed_xls_record("old", "xls-old"), 2).unwrap();
        assert!(xls_reindex_required(&conn).unwrap());

        upsert_content_record(
            &conn,
            xls_id,
            &indexed_xls_record("current", XLS_EXTRACTOR_VERSION),
            3,
        )
        .unwrap();
        assert!(!xls_reindex_required(&conn).unwrap());
    }

    #[test]
    fn modern_spreadsheet_clear_is_scoped_by_format_and_root() {
        let conn = mem();
        let text = upsert_file(&conn, "C:/A/notes.md", 1, 0, 1).unwrap();
        let xls = upsert_file(&conn, "C:/A/legacy.xls", 1, 0, 1).unwrap();
        let xlsx_a = upsert_file(&conn, "C:/A/modern.xlsx", 1, 0, 1).unwrap();
        let xlsx_b = upsert_file(&conn, "C:/B/modern.xlsx", 1, 0, 1).unwrap();
        let ods_a = upsert_file(&conn, "C:/A/open.ods", 1, 0, 1).unwrap();
        let ods_b = upsert_file(&conn, "C:/B/open.ods", 1, 0, 1).unwrap();
        upsert_content_record(&conn, text, &indexed_record("ordinary"), 1).unwrap();
        upsert_content_record(
            &conn,
            xls,
            &indexed_xls_record("legacy", XLS_EXTRACTOR_VERSION),
            1,
        )
        .unwrap();
        upsert_content_record(
            &conn,
            xlsx_a,
            &indexed_xlsx_record("xlsx A", XLSX_EXTRACTOR_VERSION),
            1,
        )
        .unwrap();
        upsert_content_record(
            &conn,
            xlsx_b,
            &indexed_xlsx_record("xlsx B", XLSX_EXTRACTOR_VERSION),
            1,
        )
        .unwrap();
        upsert_content_record(
            &conn,
            ods_a,
            &indexed_ods_record("ods A", ODS_EXTRACTOR_VERSION),
            1,
        )
        .unwrap();
        upsert_content_record(
            &conn,
            ods_b,
            &indexed_ods_record("ods B", ODS_EXTRACTOR_VERSION),
            1,
        )
        .unwrap();

        clear_xlsx(&conn, "C:/A").unwrap();
        assert!(search_content(&conn, "xlsx A", 10).unwrap().is_empty());
        assert_eq!(search_content(&conn, "xlsx B", 10).unwrap().len(), 1);
        assert_eq!(search_content(&conn, "ods A", 10).unwrap().len(), 1);
        assert_eq!(search_content(&conn, "ordinary", 10).unwrap().len(), 1);
        assert_eq!(search_content(&conn, "legacy", 10).unwrap().len(), 1);

        clear_ods(&conn, "C:/A").unwrap();
        assert!(search_content(&conn, "ods A", 10).unwrap().is_empty());
        assert_eq!(search_content(&conn, "ods B", 10).unwrap().len(), 1);
        assert_eq!(search_content(&conn, "xlsx B", 10).unwrap().len(), 1);
    }

    #[test]
    fn modern_spreadsheet_metadata_detects_first_install_and_stale_rows_independently() {
        let conn = mem();
        assert!(xlsx_reindex_required(&conn).unwrap());
        assert!(ods_reindex_required(&conn).unwrap());
        record_xlsx_extractor_version(&conn).unwrap();
        assert!(!xlsx_reindex_required(&conn).unwrap());
        assert!(ods_reindex_required(&conn).unwrap());
        record_ods_extractor_version(&conn).unwrap();
        assert!(!ods_reindex_required(&conn).unwrap());

        let xlsx = upsert_file(&conn, "C:/notes/report.xlsx", 1, 0, 1).unwrap();
        let ods = upsert_file(&conn, "C:/notes/report.ods", 1, 0, 1).unwrap();
        upsert_content_record(&conn, xlsx, &indexed_xlsx_record("old", "xlsx-old"), 1).unwrap();
        upsert_content_record(&conn, ods, &indexed_ods_record("old", "ods-old"), 1).unwrap();
        assert!(xlsx_reindex_required(&conn).unwrap());
        assert!(ods_reindex_required(&conn).unwrap());

        upsert_content_record(
            &conn,
            xlsx,
            &indexed_xlsx_record("current", XLSX_EXTRACTOR_VERSION),
            2,
        )
        .unwrap();
        assert!(!xlsx_reindex_required(&conn).unwrap());
        assert!(ods_reindex_required(&conn).unwrap());

        upsert_content_record(
            &conn,
            ods,
            &indexed_ods_record("current", ODS_EXTRACTOR_VERSION),
            2,
        )
        .unwrap();
        assert!(!ods_reindex_required(&conn).unwrap());
    }

    #[test]
    fn remove_root_matches_backslash_input() {
        let conn = mem();
        // 저장은 정규화되어 슬래시(`/`)로 이뤄진다.
        upsert_file(&conn, "C:\\projects\\foo\\bar.rs", 1, 0, 1).unwrap();
        remove_root(&conn, "C:\\projects\\foo").unwrap();
        assert!(search(&conn, "bar", 10).unwrap().is_empty());
    }

    #[test]
    fn remove_root_matches_forward_slash_input() {
        let conn = mem();
        upsert_file(&conn, "C:\\projects\\foo\\bar.rs", 1, 0, 1).unwrap();
        remove_root(&conn, "C:/projects/foo").unwrap();
        assert!(search(&conn, "bar", 10).unwrap().is_empty());
    }

    #[test]
    fn remove_root_deletes_file_content_rows() {
        let conn = mem();
        let id = upsert_file(&conn, "C:/notes/todo.md", 1, 0, 1).unwrap();
        let record = indexed_record("buy milk");
        upsert_content_record(&conn, id, &record, 1).unwrap();
        remove_root(&conn, "C:/notes").unwrap();
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM file_content", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0, "file_content가 고아로 남으면 안 된다");
    }

    #[test]
    fn migrate_is_idempotent_and_does_not_wipe_current_schema() {
        let conn = Connection::open_in_memory().unwrap();
        let first = migrate(&conn).unwrap();
        assert!(
            first,
            "최초 migrate는 schema_version을 0→SCHEMA_VERSION으로 올리며 clear_all을 실행해야 한다"
        );
        upsert_file(&conn, "C:/a/b.rs", 1, 0, 1).unwrap();
        // 같은 schema_version으로 다시 migrate를 호출해도 인덱스가 지워지지 않는다.
        let second = migrate(&conn).unwrap();
        assert!(
            !second,
            "이미 최신 버전이면 clear_all을 다시 실행하면 안 된다"
        );
        assert_eq!(total_files(&conn).unwrap(), 1);
    }

    #[test]
    fn migrate_preserves_roots_and_upgrades_legacy_content_schema() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE roots (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL);
             CREATE TABLE files (id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL, ext TEXT, size INTEGER NOT NULL,
                modified_ts INTEGER NOT NULL, root_id INTEGER);
             CREATE VIRTUAL TABLE files_fts USING fts5(name, content='files', content_rowid='id');
             CREATE TRIGGER files_ad AFTER DELETE ON files BEGIN
                INSERT INTO files_fts(files_fts, rowid, name) VALUES ('delete', old.id, old.name);
             END;
             CREATE TABLE file_content (id INTEGER PRIMARY KEY, file_id INTEGER UNIQUE NOT NULL,
                content TEXT NOT NULL);
             CREATE VIRTUAL TABLE file_content_fts USING fts5(content, content='file_content', content_rowid='id');
             CREATE TRIGGER file_content_ad AFTER DELETE ON file_content BEGIN
                INSERT INTO file_content_fts(file_content_fts, rowid, content)
                    VALUES ('delete', old.id, old.content);
             END;
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO roots(path) VALUES ('C:/legacy');
             INSERT INTO meta(key, value) VALUES ('schema_version', '1');
             INSERT INTO files(path, name, ext, size, modified_ts, root_id)
                VALUES ('C:/legacy/a.md', 'a.md', 'md', 1, 0, 1);
             INSERT INTO file_content(file_id, content) VALUES (1, 'old text');",
        )
        .unwrap();
        conn.execute("INSERT INTO files_fts(rowid, name) VALUES (1, 'a.md')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO file_content_fts(rowid, content) VALUES (1, 'old text')",
            [],
        )
        .unwrap();

        assert!(migrate(&conn).unwrap());
        assert_eq!(list_roots(&conn).unwrap()[0].path, "C:/legacy");
        assert_eq!(total_files(&conn).unwrap(), 0);
        for column in [
            "content_status",
            "extractor_version",
            "truncated",
            "indexed_at",
            "error_code",
            "encoding",
            "text_chars",
        ] {
            assert!(conn
                .prepare("SELECT 1 FROM pragma_table_info('file_content') WHERE name = ?1")
                .unwrap()
                .exists([column])
                .unwrap());
        }
    }

    #[test]
    fn add_root_round_trips_backslash_path_through_list_and_clear() {
        // R1 회귀 재현: add_root에 역슬래시 경로를 넣고, DB에 실제로 저장된
        // (list_roots가 돌려주는) 값으로 clear_root를 호출해야 매치된다.
        // add_root의 반환값도 list_roots가 돌려주는 값과 같아야 한다 —
        // 커맨드 계층이 원본 대신 이 반환값을 재인덱싱에 써야 하는 이유다.
        let conn = mem();
        let stored = add_root(&conn, "C:\\projects\\foo", false).unwrap();
        assert_eq!(stored, "C:/projects/foo");

        let roots = list_roots(&conn).unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, stored);

        upsert_file(&conn, "C:\\projects\\foo\\bar.rs", 1, 0, 1).unwrap();
        clear_root(&conn, &roots[0].path).unwrap();
        assert!(search(&conn, "bar", 10).unwrap().is_empty());
    }

    #[test]
    fn clear_root_matches_files_under_drive_root() {
        // R2 회귀 재현: 루트가 "C:/"(드라이브 루트)일 때 clear_root의 LIKE
        // 패턴이 "C://%"가 되어 매치 0건이 되지 않는지 확인한다.
        let conn = mem();
        let stored = add_root(&conn, "C:\\", false).unwrap();
        assert_eq!(stored, "C:/");

        upsert_file(&conn, "C:\\readme.txt", 1, 0, 1).unwrap();
        upsert_file(&conn, "C:\\projects\\foo\\bar.rs", 1, 0, 1).unwrap();

        clear_root(&conn, &stored).unwrap();
        assert!(search(&conn, "readme", 10).unwrap().is_empty());
        assert!(search(&conn, "bar", 10).unwrap().is_empty());
        assert_eq!(total_files(&conn).unwrap(), 0);
    }

    #[test]
    fn delete_file_removes_file_and_content() {
        let conn = mem();
        let id = upsert_file(&conn, "C:/notes/todo.md", 10, 0, 1).unwrap();
        let record = indexed_record("buy milk");
        upsert_content_record(&conn, id, &record, 1).unwrap();
        assert!(delete_file(&conn, "C:\\notes\\todo.md").unwrap());
        assert!(search(&conn, "todo", 10).unwrap().is_empty());
        assert!(search_content(&conn, "milk", 10).unwrap().is_empty());
        // 두 번째 삭제는 idempotent
        assert!(!delete_file(&conn, "C:/notes/todo.md").unwrap());
    }

    #[test]
    fn root_row_for_finds_deepest_root() {
        let conn = mem();
        add_root(&conn, "C:/proj", false).unwrap();
        let (root_id, content) = root_row_for(&conn, "C:/proj/sub/a.rs").unwrap().unwrap();
        assert_eq!(root_id, 1);
        assert!(!content);
        // 루트 밖 경로는 None
        assert!(root_row_for(&conn, "C:/other/x.rs").unwrap().is_none());
    }

    #[test]
    fn find_root_for_prefers_longest_prefix() {
        let conn = mem();
        add_root(&conn, "C:/a", false).unwrap();
        add_root(&conn, "C:/a/b", true).unwrap();
        let root = find_root_for(&conn, "C:/a/b/c.rs").unwrap().unwrap();
        assert_eq!(root.path, "C:/a/b");
        assert!(root.content);
    }
}
