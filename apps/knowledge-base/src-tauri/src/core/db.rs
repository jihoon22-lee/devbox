use crate::core::frontmatter::parse;
use rusqlite::{params, Connection};

/// DB를 열고 스키마를 준비한다.
pub fn init(path: &std::path::Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS docs (
            id INTEGER PRIMARY KEY,
            path TEXT UNIQUE NOT NULL,
            title TEXT,
            body TEXT,
            tags TEXT,
            modified_ts INTEGER NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS docs_fts USING fts5(title, body, content='docs', content_rowid='id');
        CREATE TRIGGER IF NOT EXISTS docs_ai AFTER INSERT ON docs BEGIN
            INSERT INTO docs_fts(rowid, title, body) VALUES (new.id, coalesce(new.title,''), coalesce(new.body,''));
        END;
        CREATE TRIGGER IF NOT EXISTS docs_ad AFTER DELETE ON docs BEGIN
            INSERT INTO docs_fts(docs_fts, rowid, title, body) VALUES ('delete', old.id, coalesce(old.title,''), coalesce(old.body,''));
        END;
        CREATE TRIGGER IF NOT EXISTS docs_au AFTER UPDATE ON docs BEGIN
            INSERT INTO docs_fts(docs_fts, rowid, title, body) VALUES ('delete', old.id, coalesce(old.title,''), coalesce(old.body,''));
            INSERT INTO docs_fts(rowid, title, body) VALUES (new.id, coalesce(new.title,''), coalesce(new.body,''));
        END;
        ",
    )
}

pub fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )
    .optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// 문서를 인덱스한다. FTS 트리거가 title/body를 동기화한다.
pub fn index_doc(conn: &Connection, path: &str, content: &str) -> rusqlite::Result<()> {
    let (meta, body) = parse(content);
    let title = meta.title.unwrap_or_else(|| default_title(path));
    let tags_json = serde_json::to_string(&meta.tags).unwrap_or_else(|_| "[]".into());
    let modified_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO docs (path, title, body, tags, modified_ts) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(path) DO UPDATE SET
            title=excluded.title, body=excluded.body, tags=excluded.tags, modified_ts=excluded.modified_ts",
        params![path, title, body, tags_json, modified_ts],
    )?;
    Ok(())
}

pub fn remove_doc(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM docs WHERE path = ?1", params![path])?;
    Ok(())
}

/// 파일 하나 또는 폴더 아래의 모든 문서를 검색 인덱스에서 제거한다.
/// `LIKE`를 쓰지 않아 `%`와 `_`가 들어간 실제 폴더명도 wildcard로 해석되지 않는다.
pub fn remove_docs_under(conn: &Connection, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM docs
         WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/'",
        params![path],
    )?;
    Ok(())
}

/// FTS5 검색. title+body 대상, prefix 매치.
pub fn search(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> rusqlite::Result<Vec<(String, String)>> {
    let q = search::build_fts_query(query);
    let mut stmt = conn.prepare(
        "SELECT d.path, d.title
         FROM docs_fts JOIN docs d ON d.id = docs_fts.rowid
         WHERE docs_fts MATCH ?1
         ORDER BY d.modified_ts DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![q, limit], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    rows.collect()
}

pub fn list_tags(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT tags FROM docs")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut tags = std::collections::BTreeSet::new();
    for json in rows.flatten() {
        if let Ok(list) = serde_json::from_str::<Vec<String>>(&json) {
            tags.extend(list);
        }
    }
    Ok(tags.into_iter().collect())
}

pub fn default_title(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md")
        .to_string()
}

trait OptionalRow {
    fn optional(self) -> rusqlite::Result<Option<String>>;
}
impl OptionalRow for rusqlite::Result<String> {
    fn optional(self) -> rusqlite::Result<Option<String>> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn indexes_and_searches_docs() {
        let conn = mem();
        index_doc(
            &conn,
            "Notes/rust.md",
            "---\ntitle: Rust study\ntags: [rust]\n---\nLearn borrow checker",
        )
        .unwrap();
        index_doc(
            &conn,
            "Journal/2026-01-01.md",
            "---\ntags: [daily]\n---\nToday notes",
        )
        .unwrap();

        let results = search(&conn, "borrow", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "Rust study");

        let results = search(&conn, "rust", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "Rust study");
    }

    #[test]
    fn title_falls_back_to_filename() {
        let conn = mem();
        index_doc(&conn, "Notes/my-note.md", "plain content").unwrap();
        let results = search(&conn, "plain", 10).unwrap();
        assert_eq!(results[0].1, "my-note");
    }

    #[test]
    fn removes_docs_from_index() {
        let conn = mem();
        index_doc(&conn, "Notes/a.md", "unique text").unwrap();
        assert_eq!(search(&conn, "unique", 10).unwrap().len(), 1);
        remove_doc(&conn, "Notes/a.md").unwrap();
        assert_eq!(search(&conn, "unique", 10).unwrap().len(), 0);
    }

    #[test]
    fn removes_folder_docs_without_treating_names_as_wildcards() {
        let conn = mem();
        index_doc(&conn, "Notes_100/a.md", "first unique").unwrap();
        index_doc(&conn, "Notes_100/nested/b.md", "second unique").unwrap();
        index_doc(&conn, "NotesX100/keep.md", "keep unique").unwrap();

        remove_docs_under(&conn, "Notes_100").unwrap();

        assert!(search(&conn, "first", 10).unwrap().is_empty());
        assert!(search(&conn, "second", 10).unwrap().is_empty());
        assert_eq!(search(&conn, "keep", 10).unwrap().len(), 1);
    }

    #[test]
    fn collects_tags() {
        let conn = mem();
        index_doc(&conn, "a.md", "---\ntags: [rust, tauri]\n---\nx").unwrap();
        index_doc(&conn, "b.md", "---\ntags: [rust, wsl]\n---\ny").unwrap();
        let tags = list_tags(&conn).unwrap();
        assert_eq!(tags, vec!["rust", "tauri", "wsl"]);
    }

    #[test]
    fn settings_roundtrip() {
        let conn = mem();
        set_setting(&conn, "root", "C:/kb").unwrap();
        assert_eq!(
            get_setting(&conn, "root").unwrap().as_deref(),
            Some("C:/kb")
        );
    }
}
