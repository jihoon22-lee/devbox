//! One JSON document per API Studio store, replacing WebView localStorage.
//! Writes are compare-and-swap on a revision so a stale writer cannot
//! overwrite newer data.
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_REVISION: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Collections,
    History,
    Environments,
    GrpcHistory,
    Workflows,
}

impl DocumentKind {
    fn key(self) -> &'static str {
        match self {
            Self::Collections => "collections",
            Self::History => "history",
            Self::Environments => "environments",
            Self::GrpcHistory => "grpc_history",
            Self::Workflows => "workflows",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Stored {
    pub revision: u64,
    pub body: String,
}

pub struct DocumentStore(Mutex<Connection>);

impl DocumentStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|_| "store_unavailable")?;
        connection
            .busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| "store_unavailable")?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS documents (
                   kind TEXT PRIMARY KEY,
                   revision INTEGER NOT NULL,
                   body TEXT NOT NULL,
                   updated_ms INTEGER NOT NULL
                 );",
            )
            .map_err(|_| "store_unavailable")?;
        Ok(Self(Mutex::new(connection)))
    }

    /// Production workflows already used a native metadata file before document storage.
    /// Preserve that authority (including an explicitly empty file) and retain the source.
    pub fn import_legacy_workflows(&self, root: &Path) -> Result<(), String> {
        if self.load(DocumentKind::Workflows)?.is_some() {
            return Ok(());
        }
        let directory = root.join("transforms");
        let path = directory.join(transforms_core::core::workflows::WORKFLOW_FILE_NAME);
        let identity = match devbox_filesystem::filesystem_identity(&path, false) {
            Ok(identity) => identity,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("store_document_invalid".into()),
        };
        let loaded = transforms_core::core::workflows::load_from_dir_with_status(&directory);
        if !loaded.writable
            || devbox_filesystem::filesystem_identity(&path, false)
                .ok()
                .as_ref()
                != Some(&identity)
        {
            return Err("store_document_invalid".into());
        }
        let body = serde_json::to_string(&loaded.metadata).map_err(|_| "store_document_invalid")?;
        match self.save(DocumentKind::Workflows, &body, None) {
            Ok(revision) => {
                if self.load(DocumentKind::Workflows)? != Some(Stored { revision, body }) {
                    return Err("store_revision_conflict".into());
                }
                Ok(())
            }
            Err(issue)
                if issue == "store_revision_conflict"
                    && self.load(DocumentKind::Workflows)?.is_some() =>
            {
                Ok(())
            }
            Err(issue) => Err(issue),
        }
    }

    pub fn load(&self, kind: DocumentKind) -> Result<Option<Stored>, String> {
        let connection = self.0.lock().map_err(|_| "store_unavailable")?;
        let row: Option<(i64, Option<String>)> = connection
            .query_row(
                "SELECT revision, CASE WHEN length(CAST(body AS BLOB)) <= ?2 THEN body END FROM documents WHERE kind = ?1",
                params![kind.key(), MAX_DOCUMENT_BYTES as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional().map_err(|_| "store_unavailable")?;
        row.map(|(revision, body)| {
            let revision = u64::try_from(revision).map_err(|_| "store_unavailable")?;
            if revision == 0 || revision > MAX_REVISION {
                return Err("store_unavailable".into());
            }
            let body = body.ok_or("store_document_too_large")?;
            serde_json::from_str::<serde_json::Value>(&body)
                .map_err(|_| "store_document_invalid")?;
            Ok(Stored { revision, body })
        })
        .transpose()
    }

    pub fn save(
        &self,
        kind: DocumentKind,
        body: &str,
        expected: Option<u64>,
    ) -> Result<u64, String> {
        if body.len() > MAX_DOCUMENT_BYTES {
            return Err("store_document_too_large".into());
        }
        serde_json::from_str::<serde_json::Value>(body).map_err(|_| "store_document_invalid")?;
        let mut connection = self.0.lock().map_err(|_| "store_unavailable")?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| "store_unavailable")?;
        let current: Option<i64> = transaction
            .query_row(
                "SELECT revision FROM documents WHERE kind = ?1",
                params![kind.key()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| "store_unavailable")?;
        let current = current
            .map(|value| u64::try_from(value).map_err(|_| "store_unavailable"))
            .transpose()?;
        if current.is_some_and(|revision| revision == 0 || revision > MAX_REVISION) {
            return Err("store_unavailable".into());
        }
        if current != expected {
            return Err("store_revision_conflict".into());
        }
        let next = expected
            .unwrap_or(0)
            .checked_add(1)
            .filter(|value| *value <= MAX_REVISION)
            .ok_or("store_unavailable")?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis() as i64);
        transaction
            .execute(
                "INSERT INTO documents(kind, revision, body, updated_ms) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(kind) DO UPDATE SET revision = ?2, body = ?3, updated_ms = ?4",
                params![kind.key(), next as i64, body, now],
            )
            .map_err(|_| "store_unavailable")?;
        transaction.commit().map_err(|_| "store_unavailable")?;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, DocumentStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = DocumentStore::open(&dir.path().join("api-store.db")).unwrap();
        (dir, store)
    }

    #[test]
    fn documents_are_created_then_updated_with_revisions() {
        let (_dir, store) = store();
        assert!(store.load(DocumentKind::Collections).unwrap().is_none());
        let first = store
            .save(
                DocumentKind::Collections,
                r#"{"version":2,"entries":[]}"#,
                None,
            )
            .unwrap();
        assert_eq!(first, 1);
        let second = store
            .save(
                DocumentKind::Collections,
                r#"{"version":2,"entries":[1]}"#,
                Some(first),
            )
            .unwrap();
        assert_eq!(second, 2);
        assert_eq!(
            store.load(DocumentKind::Collections).unwrap().unwrap().body,
            r#"{"version":2,"entries":[1]}"#
        );
    }

    #[test]
    fn stale_writers_and_oversized_documents_are_refused() {
        let (_dir, store) = store();
        store.save(DocumentKind::History, "{}", None).unwrap();
        assert_eq!(
            store
                .save(DocumentKind::History, "{\"a\":1}", Some(7))
                .unwrap_err(),
            "store_revision_conflict"
        );
        assert_eq!(
            store.save(DocumentKind::History, "{}", None).unwrap_err(),
            "store_revision_conflict"
        );
        let huge = "x".repeat(MAX_DOCUMENT_BYTES + 1);
        assert_eq!(
            store
                .save(DocumentKind::History, &huge, Some(1))
                .unwrap_err(),
            "store_document_too_large"
        );
        assert_eq!(
            store.load(DocumentKind::History).unwrap().unwrap().body,
            "{}"
        );
    }

    #[test]
    fn bodies_must_be_json() {
        let (_dir, store) = store();
        assert_eq!(
            store
                .save(DocumentKind::Workflows, "not json", None)
                .unwrap_err(),
            "store_document_invalid"
        );
    }
}

#[cfg(test)]
mod boundaries {
    use super::*;
    #[test]
    fn independent_connections_cannot_overwrite_the_same_revision() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("api-store.db");
        let first = DocumentStore::open(&path).unwrap();
        let second = DocumentStore::open(&path).unwrap();
        first.save(DocumentKind::Collections, "{}", None).unwrap();
        let gate = std::sync::Barrier::new(2);
        let results = std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                gate.wait();
                first.save(DocumentKind::Collections, "{\"writer\":1}", Some(1))
            });
            let two = scope.spawn(|| {
                gate.wait();
                second.save(DocumentKind::Collections, "{\"writer\":2}", Some(1))
            });
            [one.join().unwrap(), two.join().unwrap()]
        });
        assert_eq!(results.iter().filter(|r| **r == Ok(2)).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| **r == Err("store_revision_conflict".into()))
                .count(),
            1
        );
        let retained = first.load(DocumentKind::Collections).unwrap().unwrap();
        drop(first);
        drop(second);
        assert_eq!(
            DocumentStore::open(&path)
                .unwrap()
                .load(DocumentKind::Collections)
                .unwrap(),
            Some(retained)
        );
    }
    #[test]
    fn all_kinds_are_independent_and_json_size_is_measured_in_utf8_bytes() {
        let root = tempfile::tempdir().unwrap();
        let store = DocumentStore::open(&root.path().join("api-store.db")).unwrap();
        for kind in [
            DocumentKind::Collections,
            DocumentKind::History,
            DocumentKind::Environments,
            DocumentKind::GrpcHistory,
            DocumentKind::Workflows,
        ] {
            assert_eq!(store.save(kind, "{}", None).unwrap(), 1);
        }
        let huge = format!("\"{}\"", "한".repeat(MAX_DOCUMENT_BYTES / 3 + 1));
        assert_eq!(
            store
                .save(DocumentKind::History, &huge, Some(1))
                .unwrap_err(),
            "store_document_too_large"
        );
        assert_eq!(
            store.load(DocumentKind::History).unwrap().unwrap().revision,
            1
        );
    }
    #[test]
    fn exhausted_revisions_never_wrap_or_become_unsafe_javascript_numbers() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("api-store.db");
        let store = DocumentStore::open(&path).unwrap();
        store.save(DocumentKind::History, "{}", None).unwrap();
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute("UPDATE documents SET revision = 9007199254740991", [])
            .unwrap();
        assert_eq!(
            store
                .save(DocumentKind::History, "[]", Some(9007199254740991))
                .unwrap_err(),
            "store_unavailable"
        );
        assert_eq!(
            store.load(DocumentKind::History).unwrap().unwrap().body,
            "{}"
        );
    }
}

#[cfg(test)]
mod legacy_workflows {
    use super::*;
    #[test]
    fn existing_native_workflows_are_copied_once_without_removing_the_source() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("transforms");
        std::fs::create_dir(&directory).unwrap();
        let metadata = transforms_core::core::workflows::WorkflowMetadata::default();
        transforms_core::core::workflows::save_to_dir(&directory, &metadata).unwrap();
        let path = directory.join("smart-workflows.json");
        let original = std::fs::read(&path).unwrap();
        let store = DocumentStore::open(&root.path().join("api-store.db")).unwrap();
        store.import_legacy_workflows(root.path()).unwrap();
        assert_eq!(
            store.load(DocumentKind::Workflows).unwrap().unwrap().body,
            serde_json::to_string(&metadata).unwrap()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        store.save(DocumentKind::Workflows, "{}", Some(1)).unwrap();
        store.import_legacy_workflows(root.path()).unwrap();
        assert_eq!(
            store.load(DocumentKind::Workflows).unwrap().unwrap().body,
            "{}"
        );
    }
    #[test]
    fn absent_files_do_not_create_empty_documents_and_invalid_sources_are_retained() {
        let root = tempfile::tempdir().unwrap();
        let store = DocumentStore::open(&root.path().join("api-store.db")).unwrap();
        store.import_legacy_workflows(root.path()).unwrap();
        assert!(store.load(DocumentKind::Workflows).unwrap().is_none());
        std::fs::create_dir(root.path().join("transforms")).unwrap();
        let path = root.path().join("transforms/smart-workflows.json");
        std::fs::write(&path, "invalid").unwrap();
        assert!(store.import_legacy_workflows(root.path()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid");
        assert!(store.load(DocumentKind::Workflows).unwrap().is_none());
        store.save(DocumentKind::History, "{}", None).unwrap();
    }
}
