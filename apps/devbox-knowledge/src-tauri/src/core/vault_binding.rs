//! A next-start folder choice changes only the Notes binding and derived index.
//! A SQLite receipt makes a crash between commit and schedule cleanup resumable.
use super::stores::Manifest;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};

pub const PROOF_KEY: &str = "devbox_knowledge_vault_binding_v1";
const MAX_SCHEDULE: u64 = 96 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Schedule {
    pub schema_version: u32,
    pub id: String,
    pub target: String,
    pub base: Manifest,
    pub previous_root: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Proof {
    schema_version: u32,
    id: String,
    root: String,
    target: String,
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub fn target(raw: &str) -> Result<String, String> {
    devbox_filesystem::parse_safe_project_path(raw)
        .map(|path| path.into_string())
        .ok_or_else(|| "vault_change_invalid".into())
}
fn validate(schedule: &Schedule) -> Result<(), String> {
    if schedule.schema_version != 1 || schedule.base.schema_version != 1 {
        return Err("vault_change_future".into());
    }
    if !uuid(&schedule.id)
        || !uuid(&schedule.base.generation)
        || target(&schedule.target)? != schedule.target
        || schedule.previous_root.is_empty()
        || schedule.previous_root.len() > 32768
        || schedule.previous_root.chars().any(char::is_control)
    {
        return Err("vault_change_invalid".into());
    }
    Ok(())
}
pub fn read(root: &Path) -> Result<Option<Schedule>, String> {
    let path = root.join("vault-on-next-start.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("vault_change_invalid".into()),
        Ok(metadata) if !metadata.is_file() || metadata.len() > MAX_SCHEDULE => {
            return Err("vault_change_invalid".into())
        }
        _ => {}
    }
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "vault_change_invalid")?;
    let (mut file, identity) = devbox_filesystem::open_filesystem_object(&path, false)
        .map_err(|_| "vault_change_invalid")?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_SCHEDULE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "vault_change_invalid")?;
    if bytes.len() as u64 > MAX_SCHEDULE
        || devbox_filesystem::filesystem_identity(&path, false)
            .map_err(|_| "vault_change_invalid")?
            != identity
    {
        return Err("vault_change_invalid".into());
    }
    let schedule: Option<Schedule> =
        serde_json::from_slice(&bytes).map_err(|_| "vault_change_invalid")?;
    if let Some(schedule) = &schedule {
        validate(schedule)?;
    }
    Ok(schedule)
}
pub fn write(root: &Path, schedule: Option<&Schedule>) -> Result<(), String> {
    if let Some(schedule) = schedule {
        validate(schedule)?;
    }
    let path = root.join("vault-on-next-start.json");
    devbox_filesystem::ensure_no_links(root).map_err(|_| "vault_change_invalid")?;
    if path.exists() {
        devbox_filesystem::ensure_no_links(&path).map_err(|_| "vault_change_invalid")?;
    }
    devbox_filesystem::atomic_write(
        path,
        &serde_json::to_vec(&schedule).map_err(|_| "vault_change_invalid")?,
    )
    .map_err(|_| "vault_change_save_failed".into())
}
pub fn current_root(conn: &Connection) -> Result<String, String> {
    conn.query_row("SELECT value FROM settings WHERE key='root' AND length(CAST(value AS BLOB)) BETWEEN 1 AND 32768", [], |row| row.get(0)).map_err(|_| "vault_change_invalid".into())
}
fn proof(conn: &Connection, root: &str) -> Result<Option<Proof>, String> {
    let value: Option<Option<String>> = conn.query_row(
        "SELECT CASE WHEN length(CAST(value AS BLOB))<=65536 THEN value ELSE NULL END FROM settings WHERE key=?1",
        [PROOF_KEY], |row| row.get(0)
    ).optional().map_err(|_| "vault_change_invalid")?;
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.ok_or("vault_change_invalid")?;
    let proof: Proof = serde_json::from_str(&value).map_err(|_| "vault_change_invalid")?;
    if proof.schema_version != 1 {
        return Err("vault_change_future".into());
    }
    if !uuid(&proof.id) || proof.root != root || target(&proof.target)? != proof.target {
        return Err("vault_change_invalid".into());
    }
    Ok(Some(proof))
}
pub fn approval_id(conn: &Connection, root: &str) -> Result<Option<String>, String> {
    Ok(proof(conn, root)?.map(|proof| proof.id))
}
pub fn committed(conn: &Connection, schedule: &Schedule) -> Result<bool, String> {
    let root = current_root(conn)?;
    Ok(proof(conn, &root)?
        .is_some_and(|proof| proof.id == schedule.id && proof.target == schedule.target))
}
/// The host has quiesced all engines and verified its exact preview/owner.
/// No Markdown/assets or templates are deleted by this transaction.
pub fn apply(conn: &Connection, schedule: &Schedule, root: &str) -> Result<(), String> {
    validate(schedule)?;
    super::import_rows::validate_owned_store(conn, super::import_rows::Source::Notes)?;
    if root.is_empty() || root.len() > 32768 || root.chars().any(char::is_control) {
        return Err("vault_change_invalid".into());
    }
    let transaction = conn
        .unchecked_transaction()
        .map_err(|_| "vault_change_save_failed")?;
    if committed(&transaction, schedule)? {
        return if current_root(&transaction)? == root {
            Ok(())
        } else {
            Err("vault_change_stale".into())
        };
    }
    if current_root(&transaction)? != schedule.previous_root {
        return Err("vault_change_stale".into());
    }
    let proof = serde_json::to_string(&Proof {
        schema_version: 1,
        id: schedule.id.clone(),
        root: root.into(),
        target: schedule.target.clone(),
    })
    .map_err(|_| "vault_change_invalid")?;
    transaction
        .execute("UPDATE settings SET value=?1 WHERE key='root'", [root])
        .map_err(|_| "vault_change_save_failed")?;
    transaction.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![PROOF_KEY,proof]).map_err(|_| "vault_change_save_failed")?;
    transaction
        .execute_batch("DELETE FROM docs; DELETE FROM doc_link_keys; DELETE FROM wikilinks;")
        .map_err(|_| "vault_change_save_failed")?;
    transaction
        .commit()
        .map_err(|_| "vault_change_save_failed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn committed_binding_recovers_without_touching_templates_or_vault_bytes() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir(&old).unwrap();
        fs::create_dir(&new).unwrap();
        fs::write(old.join("original.md"), "original").unwrap();
        fs::write(new.join("existing.md"), "existing").unwrap();
        let database = root.path().join("notes.db");
        knowledge_base_lib::component::create_empty_store(&database, &old).unwrap();
        let conn = Connection::open(&database).unwrap();
        conn.execute("INSERT INTO note_templates(name,content,created_ts,updated_ts) VALUES('keep','body',1,1)", []).unwrap();
        conn.execute("INSERT INTO docs(path,title,body,tags,modified_ts) VALUES('original.md','original','body','[]',1)", []).unwrap();
        let schedule = Schedule {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            target: new.to_string_lossy().into_owned(),
            previous_root: current_root(&conn).unwrap(),
            base: Manifest {
                schema_version: 1,
                generation: uuid::Uuid::new_v4().to_string(),
            },
        };
        write(root.path(), Some(&schedule)).unwrap();
        apply(&conn, &schedule, new.to_str().unwrap()).unwrap();
        assert!(committed(&conn, &read(root.path()).unwrap().unwrap()).unwrap());
        assert_eq!(
            conn.query_row("SELECT content FROM note_templates", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "body"
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM docs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            fs::read_to_string(old.join("original.md")).unwrap(),
            "original"
        );
        assert_eq!(
            fs::read_to_string(new.join("existing.md")).unwrap(),
            "existing"
        );
        apply(&conn, &schedule, new.to_str().unwrap()).unwrap();
        assert!(apply(&conn, &schedule, old.to_str().unwrap()).is_err());
        let mut substituted = schedule.clone();
        substituted.target = old.to_string_lossy().into_owned();
        assert!(!committed(&conn, &substituted).unwrap());
        assert!(apply(&conn, &substituted, old.to_str().unwrap()).is_err());
        write(root.path(), None).unwrap();
        assert!(read(root.path()).unwrap().is_none());
    }
    #[test]
    fn invalid_or_future_schedule_is_preserved_and_cannot_select_a_root() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("vault-on-next-start.json");
        for value in ["broken", r#"{"schemaVersion":99}"#] {
            fs::write(&path, value).unwrap();
            assert!(read(root.path()).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), value);
        }
        assert!(target("../other").is_err());
        assert!(target("C:/").is_err());
    }
    #[test]
    fn failed_index_cleanup_rolls_back_binding_and_approval_together() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("notes.db");
        knowledge_base_lib::component::create_empty_store(&database, root.path()).unwrap();
        let conn = Connection::open(&database).unwrap();
        conn.execute("INSERT INTO docs(path,title,body,tags,modified_ts) VALUES('keep.md','keep','body','[]',1)", []).unwrap();
        conn.execute_batch("CREATE TRIGGER fail_rebind BEFORE DELETE ON docs BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        let previous_root = current_root(&conn).unwrap();
        let schedule = Schedule {
            schema_version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            target: root.path().join("new").to_string_lossy().into_owned(),
            previous_root: previous_root.clone(),
            base: Manifest {
                schema_version: 1,
                generation: uuid::Uuid::new_v4().to_string(),
            },
        };
        assert!(apply(&conn, &schedule, &schedule.target).is_err());
        assert_eq!(current_root(&conn).unwrap(), previous_root);
        assert!(approval_id(&conn, &previous_root).unwrap().is_none());
        assert_eq!(
            conn.query_row("SELECT count(*) FROM docs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        conn.execute_batch("DROP TRIGGER fail_rebind; PRAGMA user_version=99;")
            .unwrap();
        assert_eq!(
            apply(&conn, &schedule, &schedule.target).unwrap_err(),
            "import_schema_unsupported"
        );
        assert_eq!(current_root(&conn).unwrap(), previous_root);
        assert_eq!(
            conn.query_row("SELECT count(*) FROM docs", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
