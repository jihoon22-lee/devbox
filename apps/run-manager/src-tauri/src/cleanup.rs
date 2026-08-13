//! Filesystem and SQLite application of the pure retention plan.
//!
//! Directory removal always precedes clearing its database reference. A crash
//! between those phases leaves a missing directory that the next pass can
//! reconcile; it never leaves an unreferenced directory because of DB-first
//! deletion.

use crate::core::retention::{plan_retention, RetentionLimits, RetentionRun};
use crate::logs::{relative_log_dir, resolve_run_directory, LogError, LOG_RELATIVE_ROOT};
use crate::storage::{DatabaseState, StorageError};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub deleted_log_directories: usize,
    pub cleared_missing_references: usize,
    pub deleted_rows: usize,
    pub deleted_orphan_directories: usize,
    pub projected_log_bytes: u64,
}

#[derive(Debug)]
pub enum CleanupError {
    Storage(StorageError),
    Log(LogError),
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
    UnsafeLogTree,
    SizeOverflow,
}

impl CleanupError {
    fn io(operation: &'static str, error: io::Error) -> Self {
        Self::Io {
            operation,
            kind: error.kind(),
        }
    }
}

impl fmt::Display for CleanupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "retention database update failed: {error}"),
            Self::Log(error) => write!(formatter, "retention log validation failed: {error}"),
            Self::Io { operation, kind } => {
                write!(formatter, "retention I/O failed while {operation}: {kind}")
            }
            Self::UnsafeLogTree => formatter.write_str("retention found an unsafe log tree"),
            Self::SizeOverflow => formatter.write_str("retention log size overflowed"),
        }
    }
}

impl std::error::Error for CleanupError {}

impl From<StorageError> for CleanupError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<LogError> for CleanupError {
    fn from(error: LogError) -> Self {
        Self::Log(error)
    }
}

pub struct RetentionCleaner {
    app_data_root: PathBuf,
    limits: RetentionLimits,
}

impl RetentionCleaner {
    pub fn new(app_data_root: impl AsRef<Path>) -> Self {
        Self::with_limits(app_data_root, RetentionLimits::default())
    }

    pub fn with_limits(app_data_root: impl AsRef<Path>, limits: RetentionLimits) -> Self {
        Self {
            app_data_root: app_data_root.as_ref().to_path_buf(),
            limits,
        }
    }

    pub fn run(&self, database: &DatabaseState, now: i64) -> Result<CleanupReport, CleanupError> {
        let app_data_root = fs::canonicalize(&self.app_data_root)
            .map_err(|error| CleanupError::io("resolving app data root", error))?;
        let runs = database.list_runs_for_retention()?;
        let known_run_ids = runs
            .iter()
            .map(|run| run.id.as_str())
            .collect::<BTreeSet<_>>();
        let mut snapshot = Vec::with_capacity(runs.len());
        for run in &runs {
            let (log_files_exist, log_bytes) = match run.log_dir.as_deref() {
                None => (false, 0),
                Some(log_dir) => match resolve_run_directory(&app_data_root, log_dir, &run.id) {
                    Ok(directory) => (true, directory_size(&directory)?),
                    Err(LogError::RunDirectoryMissing) => (false, 0),
                    Err(error) => return Err(error.into()),
                },
            };
            snapshot.push(RetentionRun {
                run_id: run.id.clone(),
                job_id: run.job_id.clone(),
                status: run.status,
                ended_at: run.ended_at,
                created_at: run.created_at,
                has_log_reference: run.log_dir.is_some(),
                log_files_exist,
                log_bytes,
            });
        }

        let plan = plan_retention(&snapshot, self.limits);
        let mut report = CleanupReport {
            projected_log_bytes: plan.projected_log_bytes,
            ..CleanupReport::default()
        };
        for deletion in &plan.log_deletions {
            let relative = relative_log_dir(&deletion.run_id)?;
            match resolve_run_directory(&app_data_root, &relative, &deletion.run_id) {
                Ok(directory) => {
                    fs::remove_dir_all(&directory)
                        .map_err(|error| CleanupError::io("deleting run log directory", error))?;
                    database.mark_run_logs_deleted(&deletion.run_id, now)?;
                    report.deleted_log_directories += 1;
                }
                Err(LogError::RunDirectoryMissing) => {
                    if database.mark_run_logs_deleted(&deletion.run_id, now)? {
                        report.cleared_missing_references += 1;
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        for run_id in &plan.missing_log_references {
            if database.mark_run_logs_deleted(run_id, now)? {
                report.cleared_missing_references += 1;
            }
        }
        for run_id in &plan.row_deletions {
            if database.delete_terminal_run_without_logs(run_id)? {
                report.deleted_rows += 1;
            }
        }
        report.deleted_orphan_directories =
            remove_orphan_directories(&app_data_root, &known_run_ids)?;
        Ok(report)
    }
}

fn directory_size(root: &Path) -> Result<u64, CleanupError> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| CleanupError::io("reading log directory metadata", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CleanupError::UnsafeLogTree);
    }
    let mut total = 0_u64;
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| CleanupError::io("reading log directory", error))?;
        for entry in entries {
            let entry = entry.map_err(|error| CleanupError::io("reading log entry", error))?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| CleanupError::io("reading log entry metadata", error))?;
            if metadata.file_type().is_symlink() {
                return Err(CleanupError::UnsafeLogTree);
            }
            if metadata.is_dir() {
                directories.push(entry.path());
            } else if metadata.is_file() {
                total = total
                    .checked_add(metadata.len())
                    .ok_or(CleanupError::SizeOverflow)?;
            } else {
                return Err(CleanupError::UnsafeLogTree);
            }
        }
    }
    Ok(total)
}

fn remove_orphan_directories(
    app_data_root: &Path,
    known_run_ids: &BTreeSet<&str>,
) -> Result<usize, CleanupError> {
    let log_root = app_data_root.join(LOG_RELATIVE_ROOT);
    let entries = match fs::read_dir(&log_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(CleanupError::io("reading run log root", error)),
    };
    let mut deleted = 0;
    for entry in entries {
        let entry = entry.map_err(|error| CleanupError::io("reading orphan entry", error))?;
        let Some(run_id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if known_run_ids.contains(run_id.as_str()) || uuid::Uuid::parse_str(&run_id).is_err() {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| CleanupError::io("reading orphan metadata", error))?;
        if metadata.file_type().is_symlink() {
            fs::remove_file(entry.path())
                .map_err(|error| CleanupError::io("deleting orphan link", error))?;
            deleted += 1;
        } else if metadata.is_dir() {
            // Reuse the same canonical containment and symlink-component
            // checks before recursively deleting an app-generated UUID path.
            let relative = relative_log_dir(&run_id)?;
            let directory = resolve_run_directory(app_data_root, relative, &run_id)?;
            fs::remove_dir_all(directory)
                .map_err(|error| CleanupError::io("deleting orphan directory", error))?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::{JobInput, OverlapPolicy, TargetKind};
    use crate::logs::{LogStream, LogStreams};
    use rusqlite::params;
    use tempfile::tempdir;

    fn job_input() -> JobInput {
        JobInput {
            name: "cleanup".into(),
            command: "echo cleanup".into(),
            cwd: None,
            target_kind: TargetKind::Windows,
            target_distro: None,
            env_ciphertext: None,
            cron_expr: "0 * * * *".into(),
            enabled: false,
            overlap_policy: OverlapPolicy::Skip,
            catch_up: false,
        }
    }

    fn terminal_run(
        database: &DatabaseState,
        database_path: &Path,
        job_id: &str,
        app_data: &Path,
        ended_at: i64,
    ) -> String {
        let run = database
            .create_manual_run_at(job_id, ended_at - 10)
            .unwrap();
        let logs = LogStreams::open_default(app_data, &run.id).unwrap();
        tauri::async_runtime::block_on(logs.append(LogStream::Stdout, b"log bytes")).unwrap();
        drop(logs);
        let connection = rusqlite::Connection::open(database_path).unwrap();
        connection
            .execute(
                "UPDATE runs SET status = 'succeeded', ended_at = ?2, log_dir = ?3 WHERE id = ?1",
                params![run.id, ended_at, format!("logs/runs/{}", run.id)],
            )
            .unwrap();
        run.id
    }

    #[test]
    fn cleanup_deletes_files_before_reference_and_rows_on_a_later_pass() {
        let directory = tempdir().unwrap();
        let app_data = directory.path().join("data");
        fs::create_dir_all(&app_data).unwrap();
        let database_path = app_data.join("data.db");
        let database = DatabaseState::open(&database_path).unwrap();
        let job = database.create_job_at(job_input(), 1).unwrap();
        let old = terminal_run(&database, &database_path, &job.id, &app_data, 10);
        let recent = terminal_run(&database, &database_path, &job.id, &app_data, 20);
        let cleaner = RetentionCleaner::with_limits(
            &app_data,
            RetentionLimits {
                terminal_rows_per_job: 1,
                total_log_bytes: u64::MAX,
            },
        );

        let first = cleaner.run(&database, 30).unwrap();
        assert_eq!(first.deleted_log_directories, 1);
        assert!(database.get_run(&old).unwrap().unwrap().log_dir.is_none());
        assert!(database.get_run(&old).unwrap().is_some());
        assert!(database
            .get_run(&recent)
            .unwrap()
            .unwrap()
            .log_dir
            .is_some());

        let second = cleaner.run(&database, 31).unwrap();
        assert_eq!(second.deleted_rows, 1);
        assert!(database.get_run(&old).unwrap().is_none());
    }

    #[test]
    fn missing_reference_and_uuid_orphan_are_reconciled_idempotently() {
        let directory = tempdir().unwrap();
        let app_data = directory.path().join("data");
        fs::create_dir_all(app_data.join(LOG_RELATIVE_ROOT)).unwrap();
        let database_path = app_data.join("data.db");
        let database = DatabaseState::open(&database_path).unwrap();
        let job = database.create_job_at(job_input(), 1).unwrap();
        let run = database.create_manual_run_at(&job.id, 2).unwrap();
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        connection
            .execute(
                "UPDATE runs SET status = 'failed', ended_at = 3, log_dir = ?2 WHERE id = ?1",
                params![run.id, format!("logs/runs/{}", run.id)],
            )
            .unwrap();
        let orphan = uuid::Uuid::new_v4().to_string();
        fs::create_dir(app_data.join(LOG_RELATIVE_ROOT).join(&orphan)).unwrap();
        let cleaner = RetentionCleaner::with_limits(
            &app_data,
            RetentionLimits {
                terminal_rows_per_job: 0,
                total_log_bytes: 0,
            },
        );

        let report = cleaner.run(&database, 4).unwrap();
        assert_eq!(report.cleared_missing_references, 1);
        assert_eq!(report.deleted_orphan_directories, 1);
        assert!(database
            .get_run(&run.id)
            .unwrap()
            .unwrap()
            .log_dir
            .is_none());
        assert!(!app_data.join(LOG_RELATIVE_ROOT).join(orphan).exists());
    }

    #[cfg(unix)]
    #[test]
    fn referenced_nested_symlink_fails_without_deleting_or_clearing() {
        use std::os::unix::fs::symlink;
        let directory = tempdir().unwrap();
        let app_data = directory.path().join("data");
        fs::create_dir_all(&app_data).unwrap();
        let database_path = app_data.join("data.db");
        let database = DatabaseState::open(&database_path).unwrap();
        let job = database.create_job_at(job_input(), 1).unwrap();
        let run_id = terminal_run(&database, &database_path, &job.id, &app_data, 2);
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(
            &outside,
            app_data
                .join(LOG_RELATIVE_ROOT)
                .join(&run_id)
                .join("escape"),
        )
        .unwrap();
        let cleaner = RetentionCleaner::with_limits(
            &app_data,
            RetentionLimits {
                terminal_rows_per_job: 0,
                total_log_bytes: 0,
            },
        );
        assert!(matches!(
            cleaner.run(&database, 3),
            Err(CleanupError::UnsafeLogTree)
        ));
        assert!(database
            .get_run(&run_id)
            .unwrap()
            .unwrap()
            .log_dir
            .is_some());
        assert!(outside.exists());
    }
}
