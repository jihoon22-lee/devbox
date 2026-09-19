//! Original SQLite backup selected by the accepted Runtime import digest.
//! Reading this inventory never opens an execution owner or initializes a DB.
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, String>;
pub fn accepted(root: &Path) -> Result<Option<String>> {
    Ok(receipt(root)?.map(|(digest, _)| digest))
}
pub fn receipt(root: &Path) -> Result<Option<(String, Option<String>)>> {
    devbox_filesystem::ensure_no_links(root).map_err(|_| "runtime_backup_unavailable")?;
    let path = root.join("data.db");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("runtime_backup_unavailable".into()),
        Ok(_) => {}
    }
    devbox_filesystem::ensure_no_links(&path).map_err(|_| "runtime_backup_unavailable")?;
    let (_file, identity) = devbox_filesystem::open_filesystem_object(&path, false)
        .map_err(|_| "runtime_backup_unavailable")?;
    let mut connection = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "runtime_backup_unavailable")?;
    connection
        .busy_timeout(Duration::from_millis(100))
        .map_err(|_| "runtime_backup_unavailable")?;
    let transaction = connection
        .transaction()
        .map_err(|_| "runtime_backup_unavailable")?;
    let version: String = transaction
        .query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |row| row.get(0),
        )
        .map_err(|_| "runtime_backup_unavailable")?;
    if version != crate::storage::SCHEMA_VERSION.to_string() {
        return Err("runtime_backup_schema_unsupported".into());
    }
    let digest: Option<String> = transaction.query_row(
        "SELECT CASE WHEN length(CAST(digest AS BLOB))=64 THEN digest ELSE '' END FROM workspace_runtime_imports WHERE origin='legacy-run-manager-v1'",
        [], |row| row.get(0)).optional().map_err(|_| "runtime_backup_unavailable")?;
    if digest.as_ref().is_some_and(|digest| !valid_digest(digest)) {
        return Err("runtime_backup_invalid".into());
    }
    if devbox_filesystem::filesystem_identity(&path, false).map_err(|_| "runtime_backup_changed")?
        != identity
    {
        return Err("runtime_backup_changed".into());
    }
    let binding = if digest.is_some() && transaction.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='workspace_runtime_import_backups')", [], |row| row.get::<_, bool>(0)).map_err(|_| "runtime_backup_unavailable")? {
        transaction.query_row("SELECT CASE WHEN length(CAST(manifest_digest AS BLOB))=64 THEN manifest_digest ELSE '' END FROM workspace_runtime_import_backups WHERE origin='legacy-run-manager-v1'", [], |row| row.get::<_, String>(0)).optional().map_err(|_| "runtime_backup_unavailable")?
    } else { None };
    if binding.as_ref().is_some_and(|value| !valid_digest(value)) {
        return Err("runtime_backup_invalid".into());
    }
    Ok(digest.map(|digest| (digest, binding)))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
pub fn verify(root: &Path, expected: &str) -> Result<(u64, u32, String, bool)> {
    let accepted = receipt(root)?.ok_or("runtime_backup_missing")?;
    if !valid_digest(expected) || accepted.0 != expected {
        return Err("runtime_backup_missing".into());
    }
    let stages = root.join("legacy-imports");
    devbox_filesystem::ensure_no_links(&stages).map_err(|_| "runtime_backup_unavailable")?;
    let (_directory, identity) = devbox_filesystem::open_filesystem_object(&stages, true)
        .map_err(|_| "runtime_backup_unavailable")?;
    let started = Instant::now();
    for (index, entry) in std::fs::read_dir(&stages)
        .map_err(|_| "runtime_backup_unavailable")?
        .enumerate()
    {
        if index >= 1000 || started.elapsed() > Duration::from_secs(5) {
            return Err("runtime_backup_limit".into());
        }
        let entry = entry.map_err(|_| "runtime_backup_unavailable")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "runtime_backup_invalid")?;
        if name.len() != 32
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("runtime_backup_invalid".into());
        }
        let stage = entry.path();
        let metadata = stage.join("snapshot.json");
        devbox_filesystem::ensure_no_links(&metadata).map_err(|_| "runtime_backup_invalid")?;
        let (file, _) = match devbox_filesystem::open_filesystem_object(&metadata, false) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("runtime_backup_unavailable".into()),
        };
        let mut bytes = Vec::new();
        file.take(16_385)
            .read_to_end(&mut bytes)
            .map_err(|_| "runtime_backup_unavailable")?;
        if bytes.len() > 16_384 {
            return Err("runtime_backup_limit".into());
        }
        let snapshot: devbox_data_migration::core::migration::Snapshot =
            serde_json::from_slice(&bytes).map_err(|_| "runtime_backup_invalid")?;
        if snapshot.sha256 != expected {
            continue;
        }
        if let Some(binding) = &accepted.1 {
            let prepared = super::runtime_import::PreparedImport::reopen(
                &stage,
                &std::sync::atomic::AtomicBool::new(false),
            )?;
            if prepared.backup_digest()? != *binding {
                continue;
            }
            if receipt(root)?.as_ref() != Some(&accepted)
                || devbox_filesystem::filesystem_identity(&stages, true)
                    .map_err(|_| "runtime_backup_changed")?
                    != identity
            {
                return Err("runtime_backup_changed".into());
            }
            return Ok((prepared.backup_bytes()?, 1, binding.clone(), true));
        }
        let verified = devbox_data_migration::core::migration::resume_snapshot(&stage)?;
        if verified.sha256 != expected
            || receipt(root)?.as_ref() != Some(&accepted)
            || devbox_filesystem::filesystem_identity(&stages, true)
                .map_err(|_| "runtime_backup_changed")?
                != identity
        {
            return Err("runtime_backup_changed".into());
        }
        return Ok((
            verified.bytes,
            u32::try_from(verified.schema_version).map_err(|_| "runtime_backup_invalid")?,
            verified.sha256,
            false,
        ));
    }
    Err("runtime_backup_missing".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_accepted_snapshot_digest_selects_backup_and_missing_bytes_stay_visible() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("product");
        std::fs::create_dir(&root).unwrap();
        assert_eq!(accepted(&root).unwrap(), None);
        assert!(!root.join("data.db").exists());
        drop(crate::storage::DatabaseState::open_product(&root.join("data.db")).unwrap());
        let source = base.path().join("source.db");
        Connection::open(&source)
            .unwrap()
            .execute_batch(
                "CREATE TABLE fixture(value TEXT); INSERT INTO fixture VALUES('preserved');",
            )
            .unwrap();
        let stages = root.join("legacy-imports");
        std::fs::create_dir(&stages).unwrap();
        let stage = stages.join("a".repeat(32));
        let snapshot = devbox_data_migration::core::migration::acquire_snapshot(
            &source,
            &stage,
            &[0],
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        assert!(verify(&root, &snapshot.sha256).is_err());
        let db = Connection::open(root.join("data.db")).unwrap();
        db.execute(
            "INSERT INTO workspace_runtime_imports VALUES('legacy-run-manager-v1',?1,0,'{}')",
            [&snapshot.sha256],
        )
        .unwrap();
        assert_eq!(verify(&root, &snapshot.sha256).unwrap().2, snapshot.sha256);
        std::fs::write(stage.join("snapshot.db"), b"changed backup").unwrap();
        assert_eq!(accepted(&root).unwrap(), Some(snapshot.sha256.clone()));
        assert!(verify(&root, &snapshot.sha256).is_err());
        db.execute_batch("UPDATE meta SET value='999' WHERE key='schema_version';")
            .unwrap();
        assert!(accepted(&root).is_err());
    }
}
