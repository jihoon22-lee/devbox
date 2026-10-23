//! The `index.db` `search` domain: note full text. Rebuildable from the vault;
//! holds no user data of its own (saved searches live in `devbox.db`).
use crate::model::NoteHit;
use crate::snippet::{MAX_RANGES, excerpt, find_all};
use devbox_core::{CoreError, IndexDb};
use grep_regex::RegexMatcher;
use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

/// `search_docs` maps a vault path to its FTS row and content hash (R8: a
/// same-content reindex changes nothing). `grams` holds Hangul syllable bigrams.
pub const SEARCH_SCHEMA: &str = "
CREATE TABLE search_docs(id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, hash TEXT NOT NULL);
CREATE VIRTUAL TABLE search_notes USING fts5(title, body, grams, tokenize='unicode61');";
pub(crate) const SCHEMA_VERSION: u32 = 1;
/// Note excerpt characters, and characters shown before the first match.
const NOTE_SNIPPET: usize = 160;
const NOTE_LEAD: usize = 40;
/// Title characters in a hit.
const MAX_TITLE_CHARS: usize = 256;

/// One note as the notes index read it (vault-relative path, index title, text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedNote {
    pub path: String,
    pub title: String,
    pub body: String,
}

fn hangul(c: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&c)
}

/// Bigrams of each Hangul syllable run plus the run's last syllable, which also
/// separates runs: `데몬재시작` → `데몬 몬재 재시 시작 작`. A phrase of bigrams then
/// matches exactly a substring of one run.
pub fn grams(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut run: Vec<char> = Vec::new();
    let mut flush = |run: &mut Vec<char>| {
        out.extend(run.windows(2).map(|w| w.iter().collect::<String>()));
        if let Some(last) = run.last() {
            out.push(last.to_string());
        }
        run.clear();
    };
    for c in text.chars() {
        if hangul(c) {
            run.push(c);
        } else {
            flush(&mut run);
        }
    }
    flush(&mut run);
    out.join(" ")
}

/// v0.9.0 `crates/search` `build_fts_query`: every whitespace token quoted
/// (`"` doubled) and prefix-matched.
pub(crate) fn build_fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .map(|tok| format!("\"{}\"*", tok.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A MATCH expression and the highlight pattern for its terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FtsQuery {
    pub expr: String,
    pub highlight: String,
}
/// Each whitespace term is split into Hangul and other runs. A Hangul run of two
/// or more syllables is a `grams` phrase (substring match), one syllable a
/// `grams` prefix; other runs are quoted prefix terms of `title`/`body`
/// ([`build_fts_query`]). User text never reaches MATCH unquoted. `None` when no
/// run has a letter or digit (the caller scans instead).
pub(crate) fn fts_query(text: &str) -> Option<FtsQuery> {
    let (mut exprs, mut highlights) = (Vec::new(), Vec::new());
    for term in text.split_whitespace() {
        let mut runs: Vec<(bool, String)> = Vec::new();
        for c in term.chars() {
            match runs.last_mut() {
                Some((h, run)) if *h == hangul(c) => run.push(c),
                _ => runs.push((hangul(c), c.to_string())),
            }
        }
        for (is_hangul, run) in runs {
            if is_hangul {
                let g = grams(&run);
                let g = g
                    .rsplit_once(' ')
                    .map_or(g.as_str(), |(bigrams, _)| bigrams);
                exprs.push(if run.chars().count() == 1 {
                    format!("grams : \"{run}\"*")
                } else {
                    format!("grams : \"{g}\"")
                });
                highlights.push(regex_syntax::escape(&run));
            } else if run.chars().any(char::is_alphanumeric) {
                exprs.push(format!("{{title body}} : {}", build_fts_query(&run)));
                let word = if run.starts_with(char::is_alphanumeric) {
                    "\\b"
                } else {
                    ""
                };
                highlights.push(format!("{word}{}", regex_syntax::escape(&run)));
            }
        }
    }
    if exprs.is_empty() {
        return None;
    }
    highlights.sort_by_key(|h| std::cmp::Reverse(h.len()));
    Some(FtsQuery {
        expr: exprs.join(" AND "),
        highlight: highlights.join("|"),
    })
}

pub(crate) enum Mode {
    /// MATCH expression and the matcher that highlights its terms.
    Fts(String, RegexMatcher),
    /// Stored titles and bodies run through the matcher (regex or case-sensitive).
    Scan(RegexMatcher),
}

/// Hidden (dot) components, `.trash`, `.devbox-tmp-*` and `.git` are never indexed (CV-F06).
fn indexable(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\0')
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != ".." && !c.starts_with('.'))
        && !devbox_documents::is_internal(path)
}

fn hash(title: &str, body: &str) -> String {
    let mut h = Sha256::new();
    h.update(title.as_bytes());
    h.update([0]);
    h.update(body.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn upsert_one(tx: &Transaction<'_>, n: &IndexedNote) -> rusqlite::Result<bool> {
    let body = n.body.strip_prefix('\u{feff}').unwrap_or(&n.body);
    let digest = hash(&n.title, body);
    let known: Option<(i64, String)> = tx
        .query_row(
            "SELECT id, hash FROM search_docs WHERE path = ?1",
            [&n.path],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let id = match known {
        Some((_, old)) if old == digest => return Ok(false),
        Some((id, _)) => {
            tx.execute(
                "UPDATE search_docs SET hash = ?2 WHERE id = ?1",
                params![id, digest],
            )?;
            tx.execute("DELETE FROM search_notes WHERE rowid = ?1", [id])?;
            id
        }
        None => {
            tx.execute(
                "INSERT INTO search_docs(path, hash) VALUES (?1, ?2)",
                params![n.path, digest],
            )?;
            tx.last_insert_rowid()
        }
    };
    let grams = format!("{} {}", grams(&n.title), grams(body));
    tx.execute(
        "INSERT INTO search_notes(rowid, title, body, grams) VALUES (?1, ?2, ?3, ?4)",
        params![id, n.title, body, grams],
    )?;
    Ok(true)
}

fn hit(path: String, title: String, body: &str, marks: &RegexMatcher) -> NoteHit {
    let found = find_all(marks, body, MAX_RANGES);
    let (snippet, ranges_utf16) = excerpt(body, &found, NOTE_SNIPPET, NOTE_LEAD);
    NoteHit {
        path,
        title: title.chars().take(MAX_TITLE_CHARS).collect(),
        snippet,
        ranges_utf16,
    }
}

#[derive(Clone)]
pub(crate) struct NoteIndex {
    db: IndexDb,
}
impl NoteIndex {
    /// Synchronous; `true` when the domain was (re)created and needs every note.
    pub fn open(db: IndexDb, version: u32) -> Result<(Self, bool), CoreError> {
        let rebuilt = db.ensure_domain("search", version, SEARCH_SCHEMA)?;
        Ok((Self { db }, rebuilt))
    }
    /// `true` when any stored content changed. A leading BOM is not indexed.
    pub async fn upsert(&self, notes: Vec<IndexedNote>) -> Result<bool, CoreError> {
        self.db
            .write(move |tx| {
                let mut changed = false;
                for n in notes.iter().filter(|n| indexable(&n.path)) {
                    changed |= upsert_one(tx, n)?;
                }
                Ok(changed)
            })
            .await
    }
    /// Removes the notes at `paths` and below them; `true` when any existed.
    pub async fn remove(&self, paths: Vec<String>) -> Result<bool, CoreError> {
        self.db
            .write(move |tx| {
                let mut changed = false;
                for path in &paths {
                    let ids: Vec<i64> = tx
                        .prepare(
                            "SELECT id FROM search_docs WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/'",
                        )?
                        .query_map([path], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?;
                    for id in ids {
                        tx.execute("DELETE FROM search_notes WHERE rowid = ?1", [id])?;
                        tx.execute("DELETE FROM search_docs WHERE id = ?1", [id])?;
                        changed = true;
                    }
                }
                Ok(changed)
            })
            .await
    }
    /// Removes every note not in `paths`; `true` when any existed.
    pub async fn retain(&self, paths: Vec<String>) -> Result<bool, CoreError> {
        self.db
            .write(move |tx| {
                let keep: std::collections::HashSet<String> = paths.into_iter().collect();
                let all: Vec<(i64, String)> = tx
                    .prepare("SELECT id, path FROM search_docs")?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()?;
                let mut changed = false;
                for (id, _) in all.into_iter().filter(|(_, p)| !keep.contains(p)) {
                    tx.execute("DELETE FROM search_notes WHERE rowid = ?1", [id])?;
                    tx.execute("DELETE FROM search_docs WHERE id = ?1", [id])?;
                    changed = true;
                }
                Ok(changed)
            })
            .await
    }
    /// At most `limit` hits (best first for FTS, by path for a scan) and whether more matched.
    pub async fn search(
        &self,
        mode: Mode,
        limit: usize,
        cancel: CancellationToken,
    ) -> Result<(Vec<NoteHit>, bool), CoreError> {
        self.db
            .read(move |tx| {
                let mut out = Vec::new();
                match mode {
                    Mode::Fts(expr, marks) => {
                        let mut st = tx.prepare(
                            "SELECT d.path, n.title, n.body FROM (SELECT rowid AS id, bm25(search_notes, 4.0, 1.0, 1.0) AS score
                               FROM search_notes WHERE search_notes MATCH ?1 ORDER BY score LIMIT ?2) m
                             JOIN search_notes n ON n.rowid = m.id JOIN search_docs d ON d.id = m.id
                             ORDER BY m.score, d.path",
                        )?;
                        let mut rows = st.query(params![expr, limit as i64 + 1])?;
                        while let Some(r) = rows.next()? {
                            if cancel.is_cancelled() || out.len() > limit {
                                break;
                            }
                            let body: String = r.get(2)?;
                            out.push(hit(r.get(0)?, r.get(1)?, &body, &marks));
                        }
                    }
                    Mode::Scan(m) => {
                        let mut st = tx.prepare(
                            "SELECT d.path, n.title, n.body FROM search_docs d JOIN search_notes n ON n.rowid = d.id ORDER BY d.path",
                        )?;
                        let mut rows = st.query([])?;
                        while let Some(r) = rows.next()? {
                            if cancel.is_cancelled() || out.len() > limit {
                                break;
                            }
                            let (title, body): (String, String) = (r.get(1)?, r.get(2)?);
                            if find_all(&m, &body, 1).is_empty() && find_all(&m, &title, 1).is_empty() {
                                continue;
                            }
                            out.push(hit(r.get(0)?, title, &body, &m));
                        }
                    }
                }
                let truncated = out.len() > limit;
                out.truncate(limit);
                Ok((out, truncated))
            })
            .await
    }
    /// Every indexed vault path.
    pub async fn paths(&self) -> Result<Vec<String>, CoreError> {
        self.db
            .read(|tx| {
                let mut st = tx.prepare("SELECT path FROM search_docs ORDER BY path")?;
                st.query_map([], |r| r.get(0))?.collect()
            })
            .await
    }
}
