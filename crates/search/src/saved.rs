//! Saved searches: user data, so they live in `devbox.db` (backed up), not index.db.
use crate::R;
use crate::model::*;
use devbox_core::{Db, Migration};
use devbox_protocol::{Done, Internal};

pub const MIGRATIONS: &[Migration] = &[Migration {
    domain: "search",
    version: 1,
    sql: "CREATE TABLE search_saved(id TEXT PRIMARY KEY, name TEXT NOT NULL, query TEXT NOT NULL, created_ms INTEGER NOT NULL);",
}];

pub(crate) async fn list(db: &Db) -> R<SavedSearches> {
    let rows: Vec<(String, String, String)> = db
        .read(|tx| {
            let mut st = tx.prepare(
                "SELECT id, name, query FROM search_saved ORDER BY created_ms, id LIMIT ?1",
            )?;
            st.query_map([MAX_SAVED as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect()
        })
        .await
        .internal("read saved searches")?;
    let items = rows
        .into_iter()
        .filter_map(|(id, name, query)| {
            let query = serde_json::from_str(&query).ok()?;
            Some(SavedSearch { id, name, query })
        })
        .collect();
    Ok(SavedSearches {
        rev: db.rev().to_string(),
        items,
    })
}

/// A name of 1–100 characters without control characters; the query as `search.query` checks it.
pub(crate) async fn save(db: &Db, p: SaveSearch, now_ms: i64) -> R<SavedRef> {
    let name = p.name.trim().to_owned();
    let count = name.chars().count();
    if count == 0 || count > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(SearchError::InvalidInput.into());
    }
    crate::service::validate(&p.query)?;
    let query = serde_json::to_string(&p.query).internal("encode saved search")?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let row = id.clone();
    let saved = db
        .write_checked(move |tx| {
            let have: i64 = tx.query_row("SELECT COUNT(*) FROM search_saved", [], |r| r.get(0))?;
            if have >= MAX_SAVED as i64 {
                return Ok(Err(SearchError::InvalidInput));
            }
            tx.execute(
                "INSERT INTO search_saved(id, name, query, created_ms) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![row, name, query, now_ms],
            )?;
            Ok(Ok(()))
        })
        .await
        .internal("write saved search")?;
    let ((), rev) = saved?;
    Ok(SavedRef {
        id,
        rev: rev.to_string(),
    })
}

pub(crate) async fn unsave(db: &Db, p: SavedId) -> R<Done> {
    let removed = db
        .write_checked(move |tx| {
            match tx.execute("DELETE FROM search_saved WHERE id = ?1", [&p.id])? {
                0 => Ok(Err(SearchError::NotFound)),
                _ => Ok(Ok(())),
            }
        })
        .await
        .internal("write saved search")?;
    let ((), rev) = removed?;
    Ok(Done {
        rev: rev.to_string(),
    })
}
