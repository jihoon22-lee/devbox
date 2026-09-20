//! A read-only log lease issued by the Runtime database owner. Paths and
//! filesystem handles are native-only; the wire reference is an opaque run
//! revision. Logs never opens the Runtime database itself.
use super::{ProductFile, ProductRoot};
use crate::{core::models::Run, storage::DatabaseState};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

pub struct OwnedRunLog {
    database: Arc<DatabaseState>,
    data: ProductRoot,
    database_file: ProductFile,
    directory: ProductRoot,
    run_id: String,
    revision: String,
}
impl OwnedRunLog {
    pub(super) fn open(
        database: Arc<DatabaseState>,
        root: &Path,
        run_id: &str,
    ) -> Result<Self, String> {
        if run_id.is_empty()
            || run_id.len() > 128
            || run_id
                .bytes()
                .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
        {
            return Err("runtime_log_identity_invalid".into());
        }
        let data = ProductRoot::open(root)?;
        let database_file = ProductFile::open(&root.join("data.db"))?;
        let run = current_run(&database, run_id)?;
        let directory = ProductRoot::open(
            &crate::logs::resolve_run_directory(
                root,
                run.log_dir.as_deref().ok_or("runtime_log_unavailable")?,
                run_id,
            )
            .map_err(|_| "runtime_log_unavailable")?,
        )?;
        let revision = revision(&run, &data, &directory)?;
        let lease = Self {
            database,
            database_file,
            data,
            directory,
            run_id: run_id.into(),
            revision,
        };
        lease.revalidate()?;
        Ok(lease)
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
    /// The provider passes this only to the native fixed rotation reader.
    /// It is intentionally not serializable or constructible by a renderer.
    pub fn data_root(&self) -> &Path {
        &self.data.path
    }
    pub fn revalidate(&self) -> Result<(), String> {
        self.database_file.check()?;
        self.data.checked()?;
        self.directory.checked()?;
        let run = current_run(&self.database, &self.run_id)?;
        let path = crate::logs::resolve_run_directory(
            &self.data.path,
            run.log_dir.as_deref().ok_or("runtime_log_unavailable")?,
            &self.run_id,
        )
        .map_err(|_| "runtime_log_unavailable")?;
        if path != self.directory.path
            || revision(&run, &self.data, &self.directory)? != self.revision
        {
            return Err("runtime_log_changed".into());
        }
        Ok(())
    }
}
fn current_run(database: &DatabaseState, run_id: &str) -> Result<Run, String> {
    let run = database
        .get_run(run_id)
        .map_err(|_| "runtime_log_unavailable")?
        .ok_or("runtime_log_unavailable")?;
    if run.logs_deleted_at.is_some() || run.log_dir.is_none() {
        return Err("runtime_log_unavailable".into());
    }
    Ok(run)
}
fn revision(run: &Run, data: &ProductRoot, directory: &ProductRoot) -> Result<String, String> {
    // Terminal status/end time may change during follow. Occurrence and owner
    // identity may not; a replacement attempt or directory requires reconnect.
    let bytes = serde_json::to_vec(&(
        &run.id,
        &run.job_id,
        run.created_at,
        run.queue_sequence,
        &run.owner_instance_id,
        &run.attempt_token,
        &run.log_dir,
        data.identity.components(),
        directory.identity.components(),
    ))
    .map_err(|_| "runtime_log_unavailable")?;
    let mut digest = Sha256::new();
    digest.update(b"workspace-runtime-log-v1\0");
    digest.update(bytes);
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{EnvironmentUpdate, JobInput, OverlapPolicy, RunStatus, TargetKind};
    fn fixture() -> (tempfile::TempDir, Arc<DatabaseState>, String) {
        let root = tempfile::tempdir().unwrap();
        let database = Arc::new(DatabaseState::open(&root.path().join("data.db")).unwrap());
        let job = database
            .create_job_at(
                JobInput {
                    name: "synthetic job".into(),
                    command: "echo synthetic".into(),
                    cwd: None,
                    target_kind: TargetKind::Windows,
                    target_distro: None,
                    environment: EnvironmentUpdate::Keep,
                    cron_expr: "0 * * * *".into(),
                    enabled: false,
                    overlap_policy: OverlapPolicy::Skip,
                    catch_up: false,
                },
                100,
            )
            .unwrap();
        let run = database.create_manual_run_at(&job.id, 200).unwrap();
        assert!(database
            .claim_run_starting(&run.id, "fixture-owner", "fixture-attempt")
            .unwrap());
        let log_dir = format!("logs/runs/{}", run.id);
        std::fs::create_dir_all(root.path().join(&log_dir)).unwrap();
        assert!(database
            .mark_run_log_dir(&run.id, "fixture-owner", "fixture-attempt", &log_dir)
            .unwrap());
        (root, database, run.id)
    }
    #[test]
    fn terminal_transition_preserves_revision_but_retention_revokes_a_held_reader() {
        let (root, database, id) = fixture();
        let lease = OwnedRunLog::open(database.clone(), root.path(), &id).unwrap();
        assert_eq!(lease.revision().len(), 64);
        assert!(database
            .finish_run(
                &id,
                "fixture-owner",
                "fixture-attempt",
                RunStatus::Succeeded,
                Some(0),
                None,
                300
            )
            .unwrap());
        lease.revalidate().unwrap();
        assert_eq!(
            OwnedRunLog::open(database.clone(), root.path(), &id)
                .unwrap()
                .revision(),
            lease.revision()
        );
        assert!(database.mark_run_logs_deleted(&id, 400).unwrap());
        assert!(lease.revalidate().is_err());
        assert!(OwnedRunLog::open(database, root.path(), &id).is_err());
    }
    #[test]
    fn wrong_run_or_traversal_never_issues_a_descriptor() {
        let (root, database, _) = fixture();
        for id in ["", "../other", "missing-run", "run:stdout"] {
            assert!(OwnedRunLog::open(database.clone(), root.path(), id).is_err());
        }
    }
    #[test]
    #[cfg(unix)]
    fn replaced_directory_revokes_the_lease_and_changes_the_next_revision() {
        let (root, database, id) = fixture();
        let lease = OwnedRunLog::open(database.clone(), root.path(), &id).unwrap();
        let directory = root.path().join("logs/runs").join(&id);
        std::fs::rename(&directory, root.path().join("previous-log-object")).unwrap();
        std::fs::create_dir(&directory).unwrap();
        assert!(lease.revalidate().is_err());
        let next = OwnedRunLog::open(database, root.path(), &id).unwrap();
        assert_ne!(next.revision(), lease.revision());
    }
}
