//! Three independent SQLite stores are selected through one native-owned
//! generation pointer. Vault bytes live outside generations and are never moved.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[derive(ts_rs::TS)]
pub struct Manifest {
    pub schema_version: u32,
    pub generation: String,
}
fn valid_generation(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub fn read(root: &Path) -> Result<Option<Manifest>, String> {
    if let Err(error) = devbox_filesystem::ensure_no_links(root) {
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err("store_path_invalid".into())
        };
    }
    let path = root.join("active-stores.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("store_unavailable".into()),
        Ok(metadata) if !metadata.is_file() || metadata.len() > 4096 => {
            return Err("store_manifest_invalid".into())
        }
        _ => {}
    }
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "store_path_invalid")?;
    let (mut file, identity) =
        devbox_filesystem::open_filesystem_object(&path, false).map_err(|_| "store_unavailable")?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| "store_unavailable")?;
    if bytes.len() > 4096
        || devbox_filesystem::filesystem_identity(&path, false).map_err(|_| "store_unavailable")?
            != identity
    {
        return Err("store_manifest_invalid".into());
    }
    let value: Manifest = serde_json::from_slice(&bytes).map_err(|_| "store_manifest_invalid")?;
    if value.schema_version != 1 {
        return Err("store_future_schema".into());
    }
    if !valid_generation(&value.generation) {
        return Err("store_manifest_invalid".into());
    }
    Ok(Some(value))
}
pub fn directory(root: &Path, manifest: &Manifest, component: &str) -> Result<PathBuf, String> {
    if manifest.schema_version != 1
        || !valid_generation(&manifest.generation)
        || !matches!(component, "notes" | "activity" | "search")
    {
        return Err("store_manifest_invalid".into());
    }
    let directory = root
        .join("stores")
        .join(&manifest.generation)
        .join(component);
    devbox_filesystem::ensure_no_links(&directory).map_err(|_| "store_path_invalid")?;
    if !directory.is_dir() {
        return Err("store_unavailable".into());
    }
    devbox_filesystem::ensure_no_links(directory.join("data.db"))
        .map_err(|_| "store_path_invalid")?;
    if !directory.join("data.db").is_file() {
        return Err("store_unavailable".into());
    }
    Ok(directory)
}
/// A cancelled/failed preparation remains unselected. Existing active stores
/// are never replaced by the new-user action.
pub fn create_empty(root: &Path) -> Result<Manifest, String> {
    devbox_filesystem::ensure_no_links(root).map_err(|_| "store_path_invalid")?;
    let lock_path = root.join("store-activation.lock");
    if lock_path.exists() {
        devbox_filesystem::ensure_no_links(&lock_path).map_err(|_| "store_path_invalid")?;
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|_| "store_unavailable")?;
    lock.try_lock().map_err(|_| "store_busy")?;
    if let Some(active) = read(root)? {
        return Ok(active);
    }
    let stores = root.join("stores");
    fs::create_dir_all(&stores).map_err(|_| "store_unavailable")?;
    devbox_filesystem::ensure_no_links(&stores).map_err(|_| "store_path_invalid")?;
    let manifest = Manifest {
        schema_version: 1,
        generation: uuid::Uuid::new_v4().to_string(),
    };
    let generation = stores.join(&manifest.generation);
    fs::create_dir(&generation).map_err(|_| "store_unavailable")?;
    for component in ["notes", "activity", "search"] {
        fs::create_dir(generation.join(component)).map_err(|_| "store_unavailable")?;
    }
    knowledge_vault_engine::component::create_empty_store(
        &generation.join("notes/data.db"),
        &root.join("notes-vault"),
    )?;
    activity_engine::component::create_empty_store(&generation.join("activity/data.db"))?;
    content_index_engine::component::create_empty_store(&generation.join("search/data.db"))?;
    for component in ["notes", "activity", "search"] {
        directory(root, &manifest, component)?;
    }
    if read(root)?.is_some() {
        return Err("store_busy".into());
    }
    devbox_filesystem::atomic_write(
        root.join("active-stores.json"),
        &serde_json::to_vec(&manifest).map_err(|_| "store_manifest_invalid")?,
    )
    .map_err(|_| "store_unavailable")?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_activation_is_repeatable_and_selects_three_independent_stores() {
        let root = tempfile::tempdir().unwrap();
        let first = create_empty(root.path()).unwrap();
        assert_eq!(create_empty(root.path()).unwrap(), first);
        assert_eq!(read(root.path()).unwrap(), Some(first.clone()));
        let paths: Vec<_> = ["notes", "activity", "search"]
            .into_iter()
            .map(|c| directory(root.path(), &first, c).unwrap())
            .collect();
        assert_ne!(paths[0], paths[1]);
        assert_ne!(paths[1], paths[2]);
        assert!(
            !root.path().join("notes-vault").exists(),
            "preparation must not initialize a vault"
        );
    }
    #[test]
    fn future_or_corrupt_manifest_is_preserved_and_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("active-stores.json");
        for value in [
            r#"{"schemaVersion":2,"generation":"future"}"#,
            r#"{"schemaVersion":1,"generation":"../source"}"#,
            "broken",
        ] {
            fs::write(&path, value).unwrap();
            assert!(create_empty(root.path()).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), value);
            assert!(!root.path().join("stores").exists());
        }
    }
}

use rusqlite::Connection;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StoreKind {
    Notes,
    Activity,
    Search,
}
impl StoreKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Activity => "activity",
            Self::Search => "search",
        }
    }
}

fn sql<T>(value: rusqlite::Result<T>) -> Result<T, String> {
    value.map_err(|_| "import_database_invalid".into())
}
fn inventory(connection: &Connection, source: StoreKind) -> Result<(), String> {
    let allowed: &[&str] = match source {
        StoreKind::Notes => &[
            "settings",
            "note_templates",
            "docs",
            "docs_fts",
            "docs_fts_data",
            "docs_fts_idx",
            "docs_fts_docsize",
            "docs_fts_config",
            "doc_link_keys",
            "wikilinks",
        ],
        StoreKind::Activity => &["settings", "sessions", "knowledge_draft_history"],
        StoreKind::Search => &[
            "roots",
            "files",
            "files_fts",
            "files_fts_data",
            "files_fts_idx",
            "files_fts_docsize",
            "files_fts_config",
            "file_content",
            "file_content_fts",
            "file_content_fts_data",
            "file_content_fts_idx",
            "file_content_fts_docsize",
            "file_content_fts_config",
            "meta",
            "saved_queries",
        ],
    };
    let mut statement = sql(connection.prepare(
        "SELECT name FROM sqlite_master WHERE type IN ('table','view') ORDER BY name LIMIT 65",
    ))?;
    let rows = sql(statement.query_map([], |r| r.get::<_, String>(0)))?;
    for (index, name) in rows.enumerate() {
        let name = sql(name)?;
        if index >= 64
            || (!allowed.contains(&name.as_str())
                && !name.starts_with("sqlite_")
                && name != "knowledge_import_rows_v1")
        {
            return Err("import_schema_unsupported".into());
        }
    }
    Ok(())
}
pub fn validate_store(connection: &Connection, source: StoreKind) -> Result<(), String> {
    inventory(connection, source)?;
    let version: i64 = sql(connection.query_row("PRAGMA user_version", [], |row| row.get(0)))?;
    if version != 0 {
        return Err("import_schema_unsupported".into());
    }
    Ok(())
}

#[cfg(test)]
mod readiness_validation_tests {
    use super::*;
    #[test]
    fn validation_preserves_receipts_but_rejects_unknown_tables_and_versions() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE settings(key TEXT,value TEXT); CREATE TABLE knowledge_import_rows_v1(source TEXT);").unwrap();
        validate_store(&connection, StoreKind::Notes).unwrap();
        connection.execute_batch("CREATE TABLE future_documents(body TEXT); INSERT INTO future_documents VALUES('preserve');").unwrap();
        assert!(validate_store(&connection, StoreKind::Notes).is_err());
        assert_eq!(
            connection
                .query_row("SELECT body FROM future_documents", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "preserve"
        );
        connection
            .execute_batch("DROP TABLE future_documents; PRAGMA user_version=999;")
            .unwrap();
        assert!(validate_store(&connection, StoreKind::Notes).is_err());
    }
}

/// Agent startup reads the selected activity DB; it never creates or grants consent.
pub fn collection_consent(root: &Path) -> Result<bool, String> {
    use rusqlite::{OpenFlags, OptionalExtension};
    let Some(manifest) = read(root)? else {
        return Ok(false);
    };
    let path = directory(root, &manifest, "activity")?.join("data.db");
    let (_witness, identity) =
        devbox_filesystem::open_filesystem_object(&path, false).map_err(|_| "store_unavailable")?;
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "store_unavailable")?;
    connection
        .busy_timeout(std::time::Duration::from_millis(100))
        .map_err(|_| "store_unavailable")?;
    connection
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(|_| "store_unavailable")?;
    validate_store(&connection, StoreKind::Activity)?;
    let value: Option<Option<String>> = connection.query_row(
        "SELECT CASE WHEN length(CAST(value AS BLOB)) <= 8 THEN value ELSE NULL END FROM settings WHERE key='product_activity_collection_v1'",
        [], |row| row.get(0)).optional().map_err(|_| "store_unavailable")?;
    if devbox_filesystem::filesystem_identity(&path, false).map_err(|_| "store_unavailable")?
        != identity
        || read(root)? != Some(manifest)
    {
        return Err("store_changed".into());
    }
    Ok(value.flatten().as_deref() == Some("enabled"))
}
#[cfg(test)]
mod consent_tests {
    use super::*;
    #[test]
    fn consent_defaults_off_and_reads_only_the_current_selected_store() {
        let root = tempfile::tempdir().unwrap();
        assert!(!collection_consent(root.path()).unwrap());
        let manifest = create_empty(root.path()).unwrap();
        assert!(!collection_consent(root.path()).unwrap());
        let db = directory(root.path(), &manifest, "activity")
            .unwrap()
            .join("data.db");
        let writer = Connection::open(&db).unwrap();
        writer.execute("INSERT INTO settings(key,value) VALUES('product_activity_collection_v1','enabled')", []).unwrap();
        assert!(collection_consent(root.path()).unwrap());
        writer
            .execute(
                "UPDATE settings SET value='disabled' WHERE key='product_activity_collection_v1'",
                [],
            )
            .unwrap();
        assert!(!collection_consent(root.path()).unwrap());
        writer
            .execute(
                "UPDATE settings SET value='true' WHERE key='product_activity_collection_v1'",
                [],
            )
            .unwrap();
        assert!(!collection_consent(root.path()).unwrap());
        drop(writer);
        fs::write(&db, b"invalid SQLite fixture").unwrap();
        assert!(collection_consent(root.path()).is_err());
        assert_eq!(fs::read(&db).unwrap(), b"invalid SQLite fixture");
    }
    #[test]
    fn a_missing_product_root_is_not_created_by_an_agent_read() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("not-created");
        assert!(read(&missing).unwrap().is_none());
        assert!(!collection_consent(&missing).unwrap());
        assert!(!missing.exists());
    }
}

/// Resolve the configured vault from the selected notes database without creating a store.
pub fn vault_root(root: &Path) -> Result<Option<PathBuf>, String> {
    use rusqlite::{OpenFlags, OptionalExtension};
    let Some(manifest) = read(root)? else {
        return Ok(None);
    };
    let path = directory(root, &manifest, "notes")?.join("data.db");
    let (_witness, identity) =
        devbox_filesystem::open_filesystem_object(&path, false).map_err(|_| "store_unavailable")?;
    let connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "store_unavailable")?;
    connection
        .busy_timeout(std::time::Duration::from_millis(100))
        .map_err(|_| "store_unavailable")?;
    connection
        .execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON;")
        .map_err(|_| "store_unavailable")?;
    validate_store(&connection, StoreKind::Notes)?;
    let value: Option<Option<String>> = connection.query_row("SELECT CASE WHEN length(CAST(value AS BLOB)) <= 32768 THEN value ELSE NULL END FROM settings WHERE key='root'", [], |row| row.get(0)).optional().map_err(|_| "store_unavailable")?;
    if devbox_filesystem::filesystem_identity(&path, false).map_err(|_| "store_unavailable")?
        != identity
        || read(root)? != Some(manifest)
    {
        return Err("store_changed".into());
    }
    Ok(value
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from))
}

#[cfg(test)]
mod vault_root_tests {
    use super::*;
    #[test]
    fn vault_root_reads_only_the_selected_notes_store_without_creating_data() {
        let root = tempfile::tempdir().unwrap();
        assert!(vault_root(root.path()).unwrap().is_none());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        let manifest = create_empty(root.path()).unwrap();
        assert!(vault_root(root.path()).unwrap().is_some());
        let notes = directory(root.path(), &manifest, "notes").unwrap();
        let connection = Connection::open(notes.join("data.db")).unwrap();
        connection
            .execute("DELETE FROM settings WHERE key='root'", [])
            .unwrap();
        assert!(vault_root(root.path()).unwrap().is_none());
        connection
            .execute(
                "INSERT OR REPLACE INTO settings(key,value) VALUES('root',?1)",
                ["C:/fixture/notes"],
            )
            .unwrap();
        assert_eq!(
            vault_root(root.path()).unwrap(),
            Some(PathBuf::from("C:/fixture/notes"))
        );
        connection
            .execute("UPDATE settings SET value='' WHERE key='root'", [])
            .unwrap();
        assert!(vault_root(root.path()).unwrap().is_none());
    }
}
