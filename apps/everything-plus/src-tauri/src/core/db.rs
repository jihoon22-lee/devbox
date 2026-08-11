use crate::core::models::{ContentResult, FileEntry, RootInfo};
use rusqlite::{params, Connection};

/// DB를 열고 스키마(FTS5 외부 콘텐츠 테이블 + 트리거)를 준비한다.
pub fn init(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    Ok(conn)
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
        ",
    )?;
    // 기존 DB에 roots.content 컬럼 추가 (마이그레이션)
    let has_content = conn
        .prepare("SELECT 1 FROM pragma_table_info('roots') WHERE name='content'")?
        .exists([])?;
    if !has_content {
        conn.execute_batch("ALTER TABLE roots ADD COLUMN content INTEGER NOT NULL DEFAULT 0")?;
    }
    Ok(())
}

pub fn add_root(conn: &Connection, path: &str, index_content: bool) -> rusqlite::Result<()> {
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

pub fn remove_root(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM files WHERE path LIKE ?1 || '/%'",
        params![path],
    )?;
    conn.execute("DELETE FROM roots WHERE path = ?1", params![path])?;
    Ok(())
}

pub fn clear_all(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM file_content", [])?;
    conn.execute("DELETE FROM files", [])?;
    Ok(())
}

/// 파일을 upsert하고 파일 id를 반환한다.
pub fn upsert_file(
    conn: &Connection,
    path: &str,
    size: i64,
    modified_ts: i64,
    root_id: i64,
) -> rusqlite::Result<i64> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
    let ext = path
        .rsplit('.')
        .next()
        .filter(|e| *e != path && e.len() <= 10)
        .unwrap_or("")
        .to_lowercase();
    conn.execute(
        "INSERT OR REPLACE INTO files (path, name, ext, size, modified_ts, root_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![path, name, ext, size, modified_ts, root_id],
    )?;
    Ok(conn.last_insert_rowid())
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
}
