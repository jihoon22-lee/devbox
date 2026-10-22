//! The `index.db` `notes` domain: titles, link keys, links and tags per note.
//! Rebuildable from the vault; holds no user data of its own.
use crate::frontmatter::{self, DocMeta};
use crate::links::{note_link_keys, parse_markdown_links, parse_wikilinks};
use crate::vault::file_stem;
use devbox_core::{CoreError, IndexDb};
use rusqlite::{OptionalExtension, params};
use std::collections::HashMap;

pub const NOTES_SCHEMA: &str = "
CREATE TABLE notes_files(path TEXT PRIMARY KEY, title TEXT NOT NULL, hash TEXT NOT NULL);
CREATE TABLE notes_keys(key TEXT NOT NULL, path TEXT NOT NULL, PRIMARY KEY(key, path));
CREATE INDEX notes_keys_path ON notes_keys(path);
CREATE TABLE notes_links(src TEXT NOT NULL, raw TEXT NOT NULL, key TEXT, heading TEXT,
  kind TEXT NOT NULL, line INTEGER NOT NULL, from_utf16 INTEGER NOT NULL, to_utf16 INTEGER NOT NULL);
CREATE INDEX notes_links_key ON notes_links(key);
CREATE INDEX notes_links_src ON notes_links(src);
CREATE TABLE notes_tags(path TEXT NOT NULL, tag TEXT NOT NULL, PRIMARY KEY(path, tag));";
/// 2: links in code blocks and HTML comments are no longer indexed (rebuilds older indexes).
const SCHEMA_VERSION: u32 = 2;

/// Receives each indexed note so search can index note bodies (Task 7 connects it).
pub trait NoteSink: Send + Sync {
    fn indexed(&self, path: &str, title: &str, body: &str);
    fn removed(&self, path: &str);
}
pub struct NoSink;
impl NoteSink for NoSink {
    fn indexed(&self, _: &str, _: &str, _: &str) {}
    fn removed(&self, _: &str) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow {
    pub raw: String,
    pub key: Option<String>,
    pub heading: Option<String>,
    pub kind: &'static str,
    pub line: usize,
    pub from_utf16: usize,
    pub to_utf16: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analyzed {
    pub path: String,
    pub hash: String,
    pub title: String,
    pub keys: Vec<String>,
    pub links: Vec<LinkRow>,
    pub tags: Vec<String>,
}

/// AR-F10: a blank or missing frontmatter title falls back to the file name; the
/// note itself is never rewritten.
pub fn title_of(meta: &DocMeta, path: &str) -> String {
    meta.title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| file_stem(path))
}

pub fn analyze(path: &str, hash: String, text: Option<&str>) -> Analyzed {
    let text = text.unwrap_or("");
    let (meta, _) = frontmatter::parse(text);
    let title = title_of(&meta, path);
    let mut links: Vec<LinkRow> = parse_wikilinks(text)
        .into_iter()
        .map(|l| LinkRow {
            raw: l.target,
            key: l.target_key,
            heading: l.heading,
            kind: "wiki",
            line: l.line,
            from_utf16: l.from_utf16,
            to_utf16: l.to_utf16,
        })
        .collect();
    links.extend(
        parse_markdown_links(text, Some(path))
            .into_iter()
            .map(|l| LinkRow {
                raw: l.raw,
                key: l.target_key,
                heading: l.heading,
                kind: "md",
                line: l.line,
                from_utf16: l.from_utf16,
                to_utf16: l.to_utf16,
            }),
    );
    let mut tags = meta.tags;
    tags.sort();
    tags.dedup();
    Analyzed {
        keys: note_link_keys(path, &title),
        path: path.into(),
        hash,
        title,
        links,
        tags,
    }
}

/// A link that may point at a note: `(src, kind, key, line, from, to)`.
pub type LinkHit = (String, String, String, u32, u32, u32);

#[derive(Clone)]
pub struct NoteIndex {
    db: IndexDb,
}
impl NoteIndex {
    /// Synchronous; call once at service creation (a version change empties the domain).
    pub fn open(db: IndexDb) -> Result<Self, CoreError> {
        db.ensure_domain("notes", SCHEMA_VERSION, NOTES_SCHEMA)?;
        Ok(Self { db })
    }
    pub async fn hashes(&self) -> Result<HashMap<String, String>, CoreError> {
        self.db
            .read(|tx| {
                let mut st = tx.prepare("SELECT path, hash FROM notes_files")?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
            })
            .await
    }
    pub async fn upsert(&self, notes: Vec<Analyzed>) -> Result<(), CoreError> {
        self.db
            .write(move |tx| {
                for n in &notes {
                    delete(tx, &n.path)?;
                    tx.execute(
                        "INSERT INTO notes_files(path,title,hash) VALUES (?1,?2,?3)",
                        params![n.path, n.title, n.hash],
                    )?;
                    for key in &n.keys {
                        tx.execute(
                            "INSERT OR IGNORE INTO notes_keys(key,path) VALUES (?1,?2)",
                            params![key, n.path],
                        )?;
                    }
                    for l in &n.links {
                        tx.execute(
                            "INSERT INTO notes_links(src,raw,key,heading,kind,line,from_utf16,to_utf16) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                            params![n.path, l.raw, l.key, l.heading, l.kind, l.line as i64, l.from_utf16 as i64, l.to_utf16 as i64],
                        )?;
                    }
                    for tag in &n.tags {
                        tx.execute(
                            "INSERT OR IGNORE INTO notes_tags(path,tag) VALUES (?1,?2)",
                            params![n.path, tag],
                        )?;
                    }
                }
                Ok(())
            })
            .await
    }
    /// Removes the notes at `paths` and every note below them (a removed folder).
    pub async fn remove(&self, paths: Vec<String>) -> Result<Vec<String>, CoreError> {
        self.db
            .write(move |tx| {
                let mut gone = Vec::new();
                for path in &paths {
                    let mut st = tx.prepare(
                        "SELECT path FROM notes_files WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/'",
                    )?;
                    let found: Vec<String> = st
                        .query_map([path], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?;
                    for p in found {
                        delete(tx, &p)?;
                        gone.push(p);
                    }
                }
                Ok(gone)
            })
            .await
    }
    pub async fn hash_of(&self, path: String) -> Result<Option<String>, CoreError> {
        self.db
            .read(move |tx| {
                tx.query_row(
                    "SELECT hash FROM notes_files WHERE path = ?1",
                    [path],
                    |r| r.get(0),
                )
                .optional()
            })
            .await
    }
    pub async fn candidates(&self, key: String) -> Result<Vec<String>, CoreError> {
        self.db
            .read(move |tx| {
                let mut st = tx.prepare("SELECT path FROM notes_keys WHERE key = ?1")?;
                st.query_map([key], |r| r.get(0))?.collect()
            })
            .await
    }
    pub async fn title(&self, path: String) -> Result<Option<String>, CoreError> {
        self.db
            .read(move |tx| {
                tx.query_row(
                    "SELECT title FROM notes_files WHERE path = ?1",
                    [path],
                    |r| r.get(0),
                )
                .optional()
            })
            .await
    }
    /// Links whose key is one of `path`'s keys, with the source title.
    pub async fn links_to(&self, path: String) -> Result<Vec<(LinkHit, String)>, CoreError> {
        self.db
            .read(move |tx| {
                let mut st = tx.prepare(
                    "SELECT l.src, l.kind, l.key, l.line, l.from_utf16, l.to_utf16, f.title
                     FROM notes_links l JOIN notes_keys k ON k.key = l.key
                     JOIN notes_files f ON f.path = l.src
                     WHERE k.path = ?1 ORDER BY l.src, l.from_utf16",
                )?;
                st.query_map([path], |r| {
                    Ok((
                        (
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                        ),
                        r.get(6)?,
                    ))
                })?
                .collect()
            })
            .await
    }
    /// Notes holding a link whose key is one of `keys`, sorted.
    pub async fn sources(&self, keys: Vec<String>) -> Result<Vec<String>, CoreError> {
        self.db
            .read(move |tx| {
                let mut st = tx.prepare("SELECT DISTINCT src FROM notes_links WHERE key = ?1")?;
                let mut out = std::collections::BTreeSet::new();
                for key in &keys {
                    for src in st.query_map([key], |r| r.get::<_, String>(0))? {
                        out.insert(src?);
                    }
                }
                Ok(out.into_iter().collect())
            })
            .await
    }
    pub async fn tags(&self) -> Result<Vec<(String, u32)>, CoreError> {
        self.db
            .read(|tx| {
                let mut st = tx.prepare(
                    "SELECT tag, COUNT(*) FROM notes_tags GROUP BY tag ORDER BY COUNT(*) DESC, tag",
                )?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
            })
            .await
    }
    /// Every note `(path, title)` sorted by path.
    pub async fn list(&self) -> Result<Vec<(String, String)>, CoreError> {
        self.db
            .read(|tx| {
                let mut st = tx.prepare("SELECT path, title FROM notes_files ORDER BY path")?;
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
            })
            .await
    }
}

fn delete(tx: &rusqlite::Transaction<'_>, path: &str) -> rusqlite::Result<()> {
    for table in ["notes_files", "notes_keys", "notes_tags"] {
        tx.execute(&format!("DELETE FROM {table} WHERE path = ?1"), [path])?;
    }
    tx.execute("DELETE FROM notes_links WHERE src = ?1", [path])?;
    Ok(())
}
