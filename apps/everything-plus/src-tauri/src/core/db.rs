use crate::core::models::{ContentResult, FileEntry, RootInfo};
use rusqlite::{params, Connection};

/// 현재 스키마/정규화 규칙의 버전. 값을 올리면 다음 `migrate()` 호출 시
/// 기존 인덱스(파생 데이터)를 지우고 재인덱싱을 유도한다. 인덱스는 언제든
/// 다시 만들 수 있으므로 별도 마이그레이션 코드를 쓰지 않는다.
const SCHEMA_VERSION: i64 = 1;

/// DB를 열고 스키마(FTS5 외부 콘텐츠 테이블 + 트리거)를 준비한다.
pub fn init(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    Ok(conn)
}

/// 경로 구분자를 `/`로 통일하고 끝의 구분자를 제거한다.
/// Windows(`\`)와 유닉스(`/`) 구분자를 동일하게 취급하기 위해 저장 진입점
/// (`add_root`, `upsert_file`)에서 공통으로 사용한다.
pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_string()
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
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
            content TEXT NOT NULL
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
    // 기존 DB에 roots.content 컬럼 추가 (마이그레이션)
    let has_content = conn
        .prepare("SELECT 1 FROM pragma_table_info('roots') WHERE name='content'")?
        .exists([])?;
    if !has_content {
        conn.execute_batch("ALTER TABLE roots ADD COLUMN content INTEGER NOT NULL DEFAULT 0")?;
    }
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
    if current_version < SCHEMA_VERSION {
        clear_all(conn)?;
        conn.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SCHEMA_VERSION.to_string()],
        )?;
    }
    Ok(())
}

pub fn add_root(conn: &Connection, path: &str, index_content: bool) -> rusqlite::Result<()> {
    let path = normalize_path(path);
    conn.execute(
        "INSERT OR IGNORE INTO roots (path, content) VALUES (?1, ?2)",
        params![path, index_content],
    )?;
    Ok(())
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
    let pattern = format!("{}/%", normalize_path(root_path));
    conn.execute(
        "DELETE FROM file_content WHERE file_id IN (SELECT id FROM files WHERE path LIKE ?1)",
        params![pattern],
    )?;
    conn.execute("DELETE FROM files WHERE path LIKE ?1", params![pattern])?;
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

pub fn upsert_content(conn: &Connection, file_id: i64, content: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO file_content (file_id, content) VALUES (?1, ?2)
         ON CONFLICT(file_id) DO UPDATE SET content = excluded.content",
        params![file_id, content],
    )?;
    Ok(())
}

pub fn total_files(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
}

/// FTS5 파일명 검색. 쿼리는 토큰 단위 prefix 매치로 안전하게 이스케이프한다.
pub fn search(conn: &Connection, query: &str, limit: i64) -> rusqlite::Result<Vec<FileEntry>> {
    let q = build_fts_query(query);
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
    let q = build_fts_query(query);
    let mut stmt = conn.prepare(
        "SELECT f.path, f.name, snippet(file_content_fts, 0, '[', ']', '…', 20) AS snip
         FROM file_content_fts
         JOIN file_content fc ON fc.id = file_content_fts.rowid
         JOIN files f ON f.id = fc.file_id
         WHERE file_content_fts MATCH ?1
         ORDER BY f.name
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![q, limit], |r| {
        Ok(ContentResult {
            path: r.get(0)?,
            name: r.get(1)?,
            snippet: r.get(2)?,
        })
    })?;
    rows.collect()
}

/// 사용자 입력 → FTS5 MATCH 쿼리. 각 토큰을 `"토큰"*` (prefix) 형태로 만든다.
fn build_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|tok| {
            let escaped = tok.replace('"', "\"\"");
            format!("\"{escaped}\"*")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
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
    fn build_fts_query_escapes_quotes() {
        assert_eq!(build_fts_query("my \"file\""), "\"my\"* \"\"\"file\"\"\"*");
    }

    #[test]
    fn content_search_matches_body() {
        let conn = mem();
        let id = upsert_file(&conn, "C:/notes/meeting.md", 10, 0, 1).unwrap();
        upsert_content(&conn, id, "quarterly review with the team").unwrap();
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
    fn normalize_path_unifies_separators_and_trims_trailing_slash() {
        assert_eq!(normalize_path("C:\\projects\\foo\\"), "C:/projects/foo");
        assert_eq!(normalize_path("C:/projects/foo/"), "C:/projects/foo");
        assert_eq!(normalize_path("C:\\projects\\foo"), "C:/projects/foo");
        assert_eq!(normalize_path("C:/projects/foo"), "C:/projects/foo");
    }

    #[test]
    fn upsert_file_preserves_id_on_reindex() {
        let conn = mem();
        let id1 = upsert_file(&conn, "C:/projects/foo/bar.rs", 10, 100, 1).unwrap();
        upsert_content(&conn, id1, "fn main() {}").unwrap();
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
        upsert_content(&conn, id, "buy milk").unwrap();
        remove_root(&conn, "C:/notes").unwrap();
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM file_content", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0, "file_content가 고아로 남으면 안 된다");
    }

    #[test]
    fn migrate_is_idempotent_and_does_not_wipe_current_schema() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        upsert_file(&conn, "C:/a/b.rs", 1, 0, 1).unwrap();
        // 같은 schema_version으로 다시 migrate를 호출해도 인덱스가 지워지지 않는다.
        migrate(&conn).unwrap();
        assert_eq!(total_files(&conn).unwrap(), 1);
    }
}
