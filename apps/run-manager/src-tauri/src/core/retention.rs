//! Pure retention planning for terminal run metadata and log references.
//!
//! Filesystem deletion and SQLite updates are deliberately two separate
//! phases. A plan may request log deletion, but it never requests deletion of
//! the same metadata row until a later snapshot confirms that `log_dir` has
//! been cleared. This prevents a failed file deletion from orphaning logs.

use super::models::RunStatus;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

pub const DEFAULT_TERMINAL_ROWS_PER_JOB: usize = 50;
pub const DEFAULT_TOTAL_LOG_BYTES: u64 = 200 * 1024 * 1024;

/// Storage and filesystem facts required for one planning pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionRun {
    pub run_id: String,
    pub job_id: String,
    pub status: RunStatus,
    pub ended_at: Option<i64>,
    pub created_at: i64,
    /// Whether SQLite still contains a `log_dir` reference.
    pub has_log_reference: bool,
    /// Whether the referenced directory was observed on disk.
    pub log_files_exist: bool,
    /// Total bytes below the validated run log directory. Callers use zero
    /// when no directory exists; this planner never follows a path itself.
    pub log_bytes: u64,
}

impl RetentionRun {
    fn terminal(&self) -> bool {
        matches!(
            self.status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::Cancelled | RunStatus::Skipped
        )
    }

    fn recency_key(&self) -> (i64, i64, &str) {
        (
            self.ended_at.unwrap_or(self.created_at),
            self.created_at,
            &self.run_id,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogDeletionReason {
    /// The row is older than the per-job terminal metadata window.
    ExcessTerminalHistory,
    /// Terminal log files exceed the repository-wide byte cap.
    GlobalByteCap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogDeletion {
    pub run_id: String,
    pub reasons: Vec<LogDeletionReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionPlan {
    /// Delete these app-owned log directories, then clear `log_dir` and set
    /// `logs_deleted_at` only after successful deletion.
    pub log_deletions: Vec<LogDeletion>,
    /// These references point to no directory. Clear the reference and mark
    /// the deletion timestamp without attempting another filesystem delete.
    pub missing_log_references: Vec<String>,
    /// Delete these old terminal rows now. Every listed row already has no log
    /// reference, so this action cannot orphan files.
    pub row_deletions: Vec<String>,
    /// Projected terminal log bytes after every requested log deletion.
    pub projected_log_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionLimits {
    pub terminal_rows_per_job: usize,
    pub total_log_bytes: u64,
}

impl Default for RetentionLimits {
    fn default() -> Self {
        Self {
            terminal_rows_per_job: DEFAULT_TERMINAL_ROWS_PER_JOB,
            total_log_bytes: DEFAULT_TOTAL_LOG_BYTES,
        }
    }
}

/// Build a deterministic, retry-safe retention plan.
pub fn plan_retention(runs: &[RetentionRun], limits: RetentionLimits) -> RetentionPlan {
    let terminal: Vec<&RetentionRun> = runs.iter().filter(|run| run.terminal()).collect();
    let mut by_job = BTreeMap::<&str, Vec<&RetentionRun>>::new();
    for run in &terminal {
        by_job.entry(&run.job_id).or_default().push(run);
    }

    let mut reasons = BTreeMap::<&str, BTreeSet<LogDeletionReason>>::new();
    let mut row_deletions = BTreeSet::<&str>::new();
    let mut missing_log_references = BTreeSet::<&str>::new();

    for job_runs in by_job.values_mut() {
        job_runs.sort_by_key(|run| Reverse(run.recency_key()));
        for run in job_runs.iter().skip(limits.terminal_rows_per_job) {
            if run.has_log_reference {
                if run.log_files_exist {
                    reasons
                        .entry(&run.run_id)
                        .or_default()
                        .insert(LogDeletionReason::ExcessTerminalHistory);
                } else {
                    // The command layer clears the stale reference first. A
                    // following pass can then safely delete the metadata row.
                    missing_log_references.insert(&run.run_id);
                }
            } else {
                row_deletions.insert(&run.run_id);
            }
        }
    }

    let total_bytes = terminal
        .iter()
        .filter(|run| run.has_log_reference && run.log_files_exist)
        .fold(0_u64, |total, run| total.saturating_add(run.log_bytes));
    let already_selected_bytes = terminal
        .iter()
        .filter(|run| reasons.contains_key(run.run_id.as_str()))
        .fold(0_u64, |total, run| total.saturating_add(run.log_bytes));
    let mut projected_log_bytes = total_bytes.saturating_sub(already_selected_bytes);

    if projected_log_bytes > limits.total_log_bytes {
        let mut oldest_first: Vec<&RetentionRun> = terminal
            .iter()
            .copied()
            .filter(|run| run.has_log_reference && run.log_files_exist)
            .collect();
        oldest_first.sort_by_key(|run| run.recency_key());
        for run in oldest_first {
            if projected_log_bytes <= limits.total_log_bytes {
                break;
            }
            let entry = reasons.entry(&run.run_id).or_default();
            if entry.insert(LogDeletionReason::GlobalByteCap)
                && !entry.contains(&LogDeletionReason::ExcessTerminalHistory)
            {
                projected_log_bytes = projected_log_bytes.saturating_sub(run.log_bytes);
            }
        }
    }

    let log_deletions = reasons
        .into_iter()
        .map(|(run_id, reasons)| LogDeletion {
            run_id: run_id.to_string(),
            reasons: reasons.into_iter().collect(),
        })
        .collect();

    RetentionPlan {
        log_deletions,
        missing_log_references: missing_log_references
            .into_iter()
            .map(str::to_string)
            .collect(),
        row_deletions: row_deletions.into_iter().map(str::to_string).collect(),
        projected_log_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        run_id: impl Into<String>,
        job_id: impl Into<String>,
        status: RunStatus,
        ended_at: i64,
        has_log_reference: bool,
        log_files_exist: bool,
        log_bytes: u64,
    ) -> RetentionRun {
        RetentionRun {
            run_id: run_id.into(),
            job_id: job_id.into(),
            status,
            ended_at: Some(ended_at),
            created_at: ended_at - 1,
            has_log_reference,
            log_files_exist,
            log_bytes,
        }
    }

    #[test]
    fn excess_history_deletes_logs_before_metadata() {
        let runs: Vec<_> = (0..=50)
            .map(|index| {
                run(
                    format!("run-{index:02}"),
                    "frequent",
                    RunStatus::Succeeded,
                    index,
                    true,
                    true,
                    10,
                )
            })
            .collect();
        let first = plan_retention(&runs, RetentionLimits::default());
        assert_eq!(first.log_deletions.len(), 1);
        assert_eq!(first.log_deletions[0].run_id, "run-00");
        assert_eq!(
            first.log_deletions[0].reasons,
            vec![LogDeletionReason::ExcessTerminalHistory]
        );
        assert!(first.row_deletions.is_empty());

        let mut after_cleanup = runs;
        after_cleanup[0].has_log_reference = false;
        after_cleanup[0].log_files_exist = false;
        after_cleanup[0].log_bytes = 0;
        let second = plan_retention(&after_cleanup, RetentionLimits::default());
        assert_eq!(second.row_deletions, vec!["run-00"]);
        assert!(second.log_deletions.is_empty());
    }

    #[test]
    fn global_cap_removes_oldest_logs_but_keeps_recent_rows() {
        let mib = 1024 * 1024;
        let runs = vec![
            run(
                "old",
                "daily",
                RunStatus::Succeeded,
                1,
                true,
                true,
                80 * mib,
            ),
            run(
                "middle",
                "daily",
                RunStatus::Failed,
                2,
                true,
                true,
                80 * mib,
            ),
            run(
                "new",
                "daily",
                RunStatus::Cancelled,
                3,
                true,
                true,
                80 * mib,
            ),
        ];
        let plan = plan_retention(&runs, RetentionLimits::default());
        assert_eq!(plan.projected_log_bytes, 160 * mib);
        assert_eq!(plan.log_deletions.len(), 1);
        assert_eq!(plan.log_deletions[0].run_id, "old");
        assert_eq!(
            plan.log_deletions[0].reasons,
            vec![LogDeletionReason::GlobalByteCap]
        );
        assert!(plan.row_deletions.is_empty());
    }

    #[test]
    fn active_rows_are_never_retention_candidates_or_byte_inputs() {
        let gib = 1024 * 1024 * 1024;
        let statuses = [
            RunStatus::Queued,
            RunStatus::Starting,
            RunStatus::Running,
            RunStatus::Stopping,
        ];
        let runs: Vec<_> = statuses
            .into_iter()
            .enumerate()
            .map(|(index, status)| {
                run(format!("active-{index}"), "job", status, 0, true, true, gib)
            })
            .collect();
        let plan = plan_retention(&runs, RetentionLimits::default());
        assert!(plan.log_deletions.is_empty());
        assert!(plan.row_deletions.is_empty());
        assert_eq!(plan.projected_log_bytes, 0);
    }

    #[test]
    fn missing_old_log_reference_is_cleared_before_row_deletion() {
        let runs = vec![
            run("missing", "job", RunStatus::Skipped, 1, true, false, 0),
            run("recent", "job", RunStatus::Succeeded, 2, false, false, 0),
        ];
        let plan = plan_retention(
            &runs,
            RetentionLimits {
                terminal_rows_per_job: 1,
                total_log_bytes: DEFAULT_TOTAL_LOG_BYTES,
            },
        );
        assert_eq!(plan.missing_log_references, vec!["missing"]);
        assert!(plan.row_deletions.is_empty());
    }

    #[test]
    fn per_job_windows_are_independent_and_terminal_only() {
        let mut runs = Vec::new();
        for job in ["minute", "daily"] {
            for index in 0..3 {
                runs.push(run(
                    format!("{job}-{index}"),
                    job,
                    RunStatus::Succeeded,
                    index,
                    false,
                    false,
                    0,
                ));
            }
        }
        runs.push(run(
            "daily-running",
            "daily",
            RunStatus::Running,
            -100,
            false,
            false,
            0,
        ));
        let plan = plan_retention(
            &runs,
            RetentionLimits {
                terminal_rows_per_job: 2,
                total_log_bytes: DEFAULT_TOTAL_LOG_BYTES,
            },
        );
        assert_eq!(plan.row_deletions, vec!["daily-0", "minute-0"]);
    }

    #[test]
    fn one_deletion_can_satisfy_both_limits_without_double_subtraction() {
        let runs = vec![
            run("old", "job", RunStatus::Succeeded, 1, true, true, 70),
            run("middle", "job", RunStatus::Succeeded, 2, true, true, 70),
            run("new", "job", RunStatus::Succeeded, 3, true, true, 70),
        ];
        let plan = plan_retention(
            &runs,
            RetentionLimits {
                terminal_rows_per_job: 2,
                total_log_bytes: 100,
            },
        );
        assert_eq!(plan.projected_log_bytes, 70);
        assert_eq!(plan.log_deletions.len(), 2);
        let old = plan
            .log_deletions
            .iter()
            .find(|deletion| deletion.run_id == "old")
            .unwrap();
        assert_eq!(
            old.reasons,
            vec![
                LogDeletionReason::ExcessTerminalHistory,
                LogDeletionReason::GlobalByteCap
            ]
        );
        let middle = plan
            .log_deletions
            .iter()
            .find(|deletion| deletion.run_id == "middle")
            .unwrap();
        assert_eq!(middle.reasons, vec![LogDeletionReason::GlobalByteCap]);
    }
}
