use devbox_core::Migration;
use rusqlite::{OptionalExtension, Transaction, params};
pub const MIGRATIONS: &[Migration] = &[Migration {
    domain: "secrets",
    version: 1,
    sql: "
CREATE TABLE secrets_key(id INTEGER PRIMARY KEY CHECK(id=1),key_id TEXT NOT NULL);
CREATE TABLE secrets_values(name TEXT PRIMARY KEY,key_id TEXT NOT NULL,sealed BLOB NOT NULL);
",
}];
pub fn key_id(tx: &Transaction<'_>) -> rusqlite::Result<Option<String>> {
    tx.query_row("SELECT key_id FROM secrets_key WHERE id=1", [], |r| {
        r.get(0)
    })
    .optional()
}
pub fn init(tx: &Transaction<'_>, id: &str) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO secrets_key(id,key_id) VALUES(1,?1)", [id])?;
    Ok(())
}
pub fn names(tx: &Transaction<'_>) -> rusqlite::Result<Vec<String>> {
    tx.prepare("SELECT name FROM secrets_values ORDER BY name")?
        .query_map([], |r| r.get(0))?
        .collect()
}
pub fn get(tx: &Transaction<'_>, name: &str) -> rusqlite::Result<Option<(String, Vec<u8>)>> {
    tx.query_row(
        "SELECT key_id,sealed FROM secrets_values WHERE name=?1",
        [name],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}
pub fn set(tx: &Transaction<'_>, name: &str, id: &str, blob: &[u8]) -> rusqlite::Result<()> {
    tx.execute("INSERT INTO secrets_values(name,key_id,sealed) VALUES(?1,?2,?3) ON CONFLICT(name) DO UPDATE SET key_id=excluded.key_id,sealed=excluded.sealed",params![name,id,blob])?;
    Ok(())
}
pub fn delete(tx: &Transaction<'_>, name: &str) -> rusqlite::Result<()> {
    tx.execute("DELETE FROM secrets_values WHERE name=?1", [name])?;
    Ok(())
}
