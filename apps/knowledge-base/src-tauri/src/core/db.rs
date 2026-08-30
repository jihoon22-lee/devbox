use crate::core::frontmatter::parse;
use crate::core::wikilink::{note_link_keys, note_link_target, parse_wikilinks, ParsedWikilink};
use rusqlite::{params, Connection};
use std::collections::HashMap;

const WIKILINK_SCHEMA_KEY: &str = "wikilink-schema";
const WIKILINK_SCHEMA_VERSION: &str = "1";
pub const MAX_WIKILINK_CANDIDATES: i64 = 100;
pub const MAX_BACKLINKS: i64 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkResolution {
    Resolved(String),
    Missing,
    Ambiguous,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedWikilink {
    pub occurrence: ParsedWikilink,
    pub resolution: LinkResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikilinkCandidate {
    pub path: String,
    pub title: String,
    pub link_target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    pub source_path: String,
    pub target: String,
    pub line: usize,
    pub column: usize,
}

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
        CREATE TABLE IF NOT EXISTS note_templates (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL COLLATE NOCASE UNIQUE
                CHECK(length(CAST(name AS BLOB)) BETWEEN 1 AND 128),
            content TEXT NOT NULL
                CHECK(length(CAST(content AS BLOB)) <= 65536),
            created_ts INTEGER NOT NULL CHECK(created_ts > 0),
            updated_ts INTEGER NOT NULL CHECK(updated_ts >= created_ts)
        );
        CREATE INDEX IF NOT EXISTS note_templates_updated_idx
            ON note_templates(updated_ts DESC, id DESC);
        CREATE TABLE IF NOT EXISTS docs (
            id INTEGER PRIMARY KEY,
            path TEXT UNIQUE NOT NULL,
            title TEXT,
            body TEXT,
            tags TEXT,
            modified_ts INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS docs_modified_ts_idx
            ON docs(modified_ts);
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
        CREATE TABLE IF NOT EXISTS doc_link_keys (
            path TEXT NOT NULL,
            key TEXT NOT NULL,
            PRIMARY KEY(path, key)
        );
        CREATE INDEX IF NOT EXISTS doc_link_keys_key_idx ON doc_link_keys(key);
        CREATE TABLE IF NOT EXISTS wikilinks (
            source_path TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            target TEXT NOT NULL,
            target_key TEXT NOT NULL,
            line INTEGER NOT NULL,
            column INTEGER NOT NULL,
            PRIMARY KEY(source_path, ordinal)
        );
        CREATE INDEX IF NOT EXISTS wikilinks_target_key_idx ON wikilinks(target_key);
        CREATE TRIGGER IF NOT EXISTS docs_link_ad AFTER DELETE ON docs BEGIN
            DELETE FROM doc_link_keys WHERE path = old.path;
            DELETE FROM wikilinks WHERE source_path = old.path;
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

pub fn wikilink_index_needs_rebuild(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(get_setting(conn, WIKILINK_SCHEMA_KEY)?.as_deref() != Some(WIKILINK_SCHEMA_VERSION))
}

/// 기존 설치에서는 DB에 frontmatter가 제거된 body만 있어 source line을 복구할 수 없다.
/// 따라서 schema v1 최초 실행에 filesystem 원문을 한 번 주입받아 metadata만 재구축한다.
pub fn rebuild_wikilink_index(
    conn: &Connection,
    docs: &[(String, String)],
) -> rusqlite::Result<()> {
    let transaction = conn.unchecked_transaction()?;
    transaction.execute("DELETE FROM doc_link_keys", [])?;
    transaction.execute("DELETE FROM wikilinks", [])?;
    for (path, content) in docs {
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM docs WHERE path = ?1)",
            params![path],
            |row| row.get::<_, bool>(0),
        )?;
        if exists {
            refresh_link_metadata_for_content(&transaction, path, content)?;
        } else {
            index_doc_in_transaction(&transaction, path, content)?;
        }
    }
    set_setting(&transaction, WIKILINK_SCHEMA_KEY, WIKILINK_SCHEMA_VERSION)?;
    transaction.commit()
}

/// 문서를 인덱스한다. FTS 트리거가 title/body를 동기화한다.
pub fn index_doc(conn: &Connection, path: &str, content: &str) -> rusqlite::Result<()> {
    let transaction = conn.unchecked_transaction()?;
    index_doc_in_transaction(&transaction, path, content)?;
    transaction.commit()
}

pub fn index_doc_in_transaction(
    conn: &Connection,
    path: &str,
    content: &str,
) -> rusqlite::Result<()> {
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
    refresh_link_metadata(conn, path, &title, content)
}

pub fn refresh_link_metadata_for_content(
    conn: &Connection,
    path: &str,
    content: &str,
) -> rusqlite::Result<()> {
    let (meta, _) = parse(content);
    let title = meta.title.unwrap_or_else(|| default_title(path));
    refresh_link_metadata(conn, path, &title, content)
}

fn refresh_link_metadata(
    conn: &Connection,
    path: &str,
    title: &str,
    content: &str,
) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM doc_link_keys WHERE path = ?1", params![path])?;
    conn.execute(
        "DELETE FROM wikilinks WHERE source_path = ?1",
        params![path],
    )?;
    if !std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        return Ok(());
    }
    for key in note_link_keys(path, title) {
        conn.execute(
            "INSERT INTO doc_link_keys(path, key) VALUES (?1, ?2)",
            params![path, key],
        )?;
    }
    for (ordinal, link) in parse_wikilinks(content).into_iter().enumerate() {
        conn.execute(
            "INSERT INTO wikilinks(source_path, ordinal, target, target_key, line, column)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                path,
                ordinal as i64,
                link.target,
                link.target_key.unwrap_or_default(),
                link.line as i64,
                link.column as i64,
            ],
        )?;
    }
    Ok(())
}

pub fn analyze_wikilinks(
    conn: &Connection,
    content: &str,
) -> rusqlite::Result<Vec<AnalyzedWikilink>> {
    let mut cached = HashMap::<String, LinkResolution>::new();
    parse_wikilinks(content)
        .into_iter()
        .map(|occurrence| {
            let resolution = match &occurrence.target_key {
                Some(key) => match cached.get(key) {
                    Some(resolution) => resolution.clone(),
                    None => {
                        let resolution = resolve_link_key(conn, key)?;
                        cached.insert(key.clone(), resolution.clone());
                        resolution
                    }
                },
                None => LinkResolution::Invalid,
            };
            Ok(AnalyzedWikilink {
                occurrence,
                resolution,
            })
        })
        .collect()
}

fn resolve_link_key(conn: &Connection, key: &str) -> rusqlite::Result<LinkResolution> {
    let mut statement = conn
        .prepare("SELECT DISTINCT path FROM doc_link_keys WHERE key = ?1 ORDER BY path LIMIT 2")?;
    let paths = statement
        .query_map(params![key], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(match paths.as_slice() {
        [] => LinkResolution::Missing,
        [path] => LinkResolution::Resolved(path.clone()),
        _ => LinkResolution::Ambiguous,
    })
}

pub fn wikilink_candidates(
    conn: &Connection,
    query: &str,
) -> rusqlite::Result<Vec<WikilinkCandidate>> {
    let normalized = query.trim().to_lowercase();
    let pattern = format!("%{}%", escape_like(&normalized));
    let mut statement = conn.prepare(
        "SELECT DISTINCT d.path, coalesce(d.title, '')
         FROM doc_link_keys k JOIN docs d ON d.path = k.path
         WHERE ?1 = '' OR k.key LIKE ?2 ESCAPE '\\'
         ORDER BY coalesce(d.title, '') COLLATE NOCASE, d.path COLLATE NOCASE
         LIMIT ?3",
    )?;
    let candidates = statement
        .query_map(
            params![normalized, pattern, MAX_WIKILINK_CANDIDATES],
            |row| {
                let path = row.get::<_, String>(0)?;
                Ok(WikilinkCandidate {
                    link_target: note_link_target(&path),
                    path,
                    title: row.get(1)?,
                })
            },
        )?
        .collect();
    candidates
}

pub fn backlinks(conn: &Connection, target_path: &str) -> rusqlite::Result<Vec<Backlink>> {
    let mut statement = conn.prepare(
        "SELECT w.source_path, w.target, w.line, w.column
         FROM wikilinks w
         JOIN doc_link_keys target ON target.key = w.target_key AND target.path = ?1
         WHERE (SELECT count(DISTINCT candidate.path)
                FROM doc_link_keys candidate
                WHERE candidate.key = w.target_key) = 1
         ORDER BY w.source_path COLLATE NOCASE, w.line, w.column
         LIMIT ?2",
    )?;
    let links = statement
        .query_map(params![target_path, MAX_BACKLINKS], |row| {
            Ok(Backlink {
                source_path: row.get(0)?,
                target: row.get(1)?,
                line: row.get::<_, i64>(2)?.max(1) as usize,
                column: row.get::<_, i64>(3)?.max(1) as usize,
            })
        })?
        .collect();
    links
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
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

    #[test]
    fn wikilinks_resolve_dynamically_and_backlinks_keep_source_position() {
        let conn = mem();
        index_doc(
            &conn,
            "Notes/rust.md",
            "---\ntitle: Rust Study\n---\n# Rust",
        )
        .unwrap();
        let source = "---\ntitle: Index\n---\nSee [[Rust Study]] and [[Future]].";
        index_doc(&conn, "Index.md", source).unwrap();

        let analyzed = analyze_wikilinks(&conn, source).unwrap();
        assert_eq!(
            analyzed[0].resolution,
            LinkResolution::Resolved("Notes/rust.md".into())
        );
        assert_eq!(analyzed[0].occurrence.line, 4);
        assert_eq!(analyzed[0].occurrence.column, 5);
        assert_eq!(analyzed[1].resolution, LinkResolution::Missing);

        let links = backlinks(&conn, "Notes/rust.md").unwrap();
        assert_eq!(
            links,
            vec![Backlink {
                source_path: "Index.md".into(),
                target: "Rust Study".into(),
                line: 4,
                column: 5,
            }]
        );
        let candidates = wikilink_candidates(&conn, "rust").unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].link_target, "Notes/rust");

        index_doc(&conn, "Future.md", "# now exists").unwrap();
        let analyzed = analyze_wikilinks(&conn, source).unwrap();
        assert_eq!(
            analyzed[1].resolution,
            LinkResolution::Resolved("Future.md".into())
        );
        assert_eq!(
            backlinks(&conn, "Future.md").unwrap()[0].source_path,
            "Index.md"
        );
    }

    #[test]
    fn duplicate_note_keys_are_ambiguous_and_not_reported_as_backlinks() {
        let conn = mem();
        index_doc(&conn, "A/Rust.md", "# first").unwrap();
        index_doc(&conn, "B/Rust.md", "# second").unwrap();
        index_doc(&conn, "Index.md", "[[Rust]]").unwrap();

        assert_eq!(
            analyze_wikilinks(&conn, "[[Rust]]").unwrap()[0].resolution,
            LinkResolution::Ambiguous
        );
        assert!(backlinks(&conn, "A/Rust.md").unwrap().is_empty());
        assert!(backlinks(&conn, "B/Rust.md").unwrap().is_empty());
    }

    #[test]
    fn non_markdown_docs_never_resolve_or_appear_as_candidates() {
        let conn = mem();
        index_doc(&conn, "Rust.txt", "plain text remains searchable").unwrap();

        assert_eq!(
            analyze_wikilinks(&conn, "[[Rust]]").unwrap()[0].resolution,
            LinkResolution::Missing
        );
        assert!(wikilink_candidates(&conn, "rust").unwrap().is_empty());
    }

    #[test]
    fn one_time_rebuild_uses_full_markdown_and_marks_schema_only_after_success() {
        let conn = mem();
        conn.execute(
            "INSERT INTO docs(path, title, body, tags, modified_ts)
             VALUES ('Existing.md', 'Existing', '[[Target]]', '[]', 1)",
            [],
        )
        .unwrap();
        assert!(wikilink_index_needs_rebuild(&conn).unwrap());

        rebuild_wikilink_index(
            &conn,
            &[
                (
                    "Existing.md".into(),
                    "---\ntitle: Existing\n---\n[[Target]]".into(),
                ),
                ("Target.md".into(), "# target".into()),
            ],
        )
        .unwrap();

        assert!(!wikilink_index_needs_rebuild(&conn).unwrap());
        let links = backlinks(&conn, "Target.md").unwrap();
        assert_eq!((links[0].line, links[0].column), (4, 1));
        let modified: i64 = conn
            .query_row(
                "SELECT modified_ts FROM docs WHERE path = 'Existing.md'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(modified, 1);
    }
}
