//! `devbox.db`의 `api` 도메인 표. 프로젝트 컬렉션 파일의 비밀 재검토를 파일 digest에 묶는다.
use devbox_core::Migration;
use rusqlite::{OptionalExtension, Transaction, params};

pub const MIGRATIONS: &[Migration] = &[Migration {
    domain: "api",
    version: 1,
    sql: "
CREATE TABLE api_reviews(root TEXT NOT NULL,path TEXT NOT NULL,digest TEXT NOT NULL,reviewed_ms INTEGER NOT NULL,PRIMARY KEY(root,path));
",
}];

pub(crate) fn reviewed_digest(
    tx: &Transaction<'_>,
    root: &str,
    path: &str,
) -> rusqlite::Result<Option<String>> {
    tx.query_row(
        "SELECT digest FROM api_reviews WHERE root=?1 AND path=?2",
        params![root, path],
        |r| r.get(0),
    )
    .optional()
}

pub(crate) fn reviewed_all(
    tx: &Transaction<'_>,
    root: &str,
) -> rusqlite::Result<std::collections::BTreeMap<String, String>> {
    tx.prepare("SELECT path,digest FROM api_reviews WHERE root=?1")?
        .query_map([root], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

pub(crate) fn record(
    tx: &Transaction<'_>,
    root: &str,
    path: &str,
    digest: &str,
    now_ms: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO api_reviews(root,path,digest,reviewed_ms) VALUES(?1,?2,?3,?4)
         ON CONFLICT(root,path) DO UPDATE SET digest=excluded.digest,reviewed_ms=excluded.reviewed_ms",
        params![root, path, digest, now_ms],
    )?;
    Ok(())
}

pub(crate) fn forget(tx: &Transaction<'_>, root: &str, path: &str) -> rusqlite::Result<()> {
    tx.execute(
        "DELETE FROM api_reviews WHERE root=?1 AND path=?2",
        params![root, path],
    )?;
    Ok(())
}
