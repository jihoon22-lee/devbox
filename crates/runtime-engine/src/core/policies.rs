//! Pure scheduling and overlap policy decisions for Run Manager.
//!
//! This module deliberately has no clock, database, process, or async
//! dependencies. The scheduler supplies the already-generated canonical
//! occurrence timestamps and a snapshot of the run state. Keeping those
//! boundaries explicit makes the policy deterministic and keeps it usable by
//! both the future scheduler and preview/tests.

use super::models::{OverlapPolicy, RunStatus};
use serde::{Deserialize, Serialize};

/// How an occurrence relates to the daemon's fixed startup cutoff.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OccurrenceClass {
    /// An occurrence at or before `startup_cutoff`; this is part of the
    /// downtime gap and is the only class affected by `catch_up`.
    StartupGap,
    /// An occurrence strictly after `startup_cutoff`; this is steady-state
    /// due work and is selected regardless of `catch_up`.
    SteadyState,
}

/// A canonical occurrence selected by [`select_due_occurrences`]. Timestamps
/// are epoch milliseconds, matching the persistence model.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DueOccurrence {
    pub timestamp: i64,
    pub class: OccurrenceClass,
}

/// Inputs for the startup-gap/steady-state policy.
///
/// `occurrences` is the canonical, due occurrence set produced by the cron
/// layer for one evaluation window. The policy sorts it before making a
/// decision so callers cannot accidentally make catch-up depend on iteration
/// order. A caller that has no due occurrences can pass an empty vector.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SchedulePolicyInput {
    pub enabled: bool,
    pub catch_up: bool,
    pub startup_cutoff: i64,
    pub occurrences: Vec<i64>,
}

/// Classify one occurrence against the fixed daemon startup cutoff.
///
/// Equality belongs to the startup gap by design: `occurrence <=
/// startup_cutoff` is the contract used by the scheduler and the preview.
pub const fn classify_occurrence(timestamp: i64, startup_cutoff: i64) -> OccurrenceClass {
    if timestamp <= startup_cutoff {
        OccurrenceClass::StartupGap
    } else {
        OccurrenceClass::SteadyState
    }
}

/// Select due occurrences according to enabled/catch-up policy.
///
/// When a job is disabled, no occurrence is selected. For an enabled job, the
/// startup gap contributes at most its latest occurrence and only when
/// `catch_up` is true. Every steady-state occurrence is retained, including
/// when `catch_up` is false. The returned list is chronological.
pub fn select_due_occurrences(input: &SchedulePolicyInput) -> Vec<DueOccurrence> {
    if !input.enabled {
        return Vec::new();
    }

    let mut occurrences = input.occurrences.clone();
    occurrences.sort_unstable();

    let latest_gap = input
        .catch_up
        .then(|| {
            occurrences
                .iter()
                .copied()
                .filter(|timestamp| *timestamp <= input.startup_cutoff)
                .max()
        })
        .flatten();

    let mut selected = Vec::with_capacity(occurrences.len());
    let mut selected_gap = false;
    for timestamp in occurrences {
        if timestamp > input.startup_cutoff {
            selected.push(DueOccurrence {
                timestamp,
                class: OccurrenceClass::SteadyState,
            });
        } else if Some(timestamp) == latest_gap && !selected_gap {
            selected_gap = true;
            selected.push(DueOccurrence {
                timestamp,
                class: OccurrenceClass::StartupGap,
            });
        }
    }
    selected
}

/// Convenience form of [`select_due_occurrences`] for scheduler call sites
/// that already have the individual values.
pub fn select_occurrences(
    occurrences: &[i64],
    startup_cutoff: i64,
    catch_up: bool,
    enabled: bool,
) -> Vec<DueOccurrence> {
    select_due_occurrences(&SchedulePolicyInput {
        enabled,
        catch_up,
        startup_cutoff,
        occurrences: occurrences.to_vec(),
    })
}

/// A small run snapshot sufficient for overlap and queue decisions.
///
/// It intentionally contains no process handle, PID, database connection, or
/// adapter state. `queue_sequence` is the durable FIFO key allocated by the
/// storage layer; policy only compares the supplied value.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunPolicySnapshot {
    pub id: String,
    pub status: RunStatus,
    pub queue_sequence: i64,
    pub blocked_by_run_id: Option<String>,
}

impl RunPolicySnapshot {
    pub fn new(
        id: impl Into<String>,
        status: RunStatus,
        queue_sequence: i64,
        blocked_by_run_id: Option<impl Into<String>>,
    ) -> Self {
        Self {
            id: id.into(),
            status,
            queue_sequence,
            blocked_by_run_id: blocked_by_run_id.map(Into::into),
        }
    }
}

/// Whether a run status can still block a new occurrence.
pub const fn is_active_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Queued | RunStatus::Starting | RunStatus::Running | RunStatus::Stopping
    )
}

/// A durable run row intent. The scheduler/storage layer can persist this
/// intent and then perform its conditional state transition in a transaction.
/// A `Queued` intent is never permission to spawn by itself.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunIntent {
    pub status: RunStatus,
    pub scheduled_at: Option<i64>,
    pub queue_sequence: i64,
    pub blocked_by_run_id: Option<String>,
}

impl RunIntent {
    fn queued(
        scheduled_at: Option<i64>,
        queue_sequence: i64,
        blocked_by_run_id: Option<String>,
    ) -> Self {
        Self {
            status: RunStatus::Queued,
            scheduled_at,
            queue_sequence,
            blocked_by_run_id,
        }
    }

    fn skipped(scheduled_at: Option<i64>, queue_sequence: i64) -> Self {
        Self {
            status: RunStatus::Skipped,
            scheduled_at,
            queue_sequence,
            blocked_by_run_id: None,
        }
    }
}

/// What the policy wants done to an existing run for kill-previous.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StopAction {
    /// The existing run must be transitioned to `stopping` before cleanup.
    RequestStop,
    /// The existing run is already stopping; do not issue a second transition.
    AlreadyStopping,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StopIntent {
    pub run_id: String,
    pub from_status: RunStatus,
    pub to_status: RunStatus,
    pub action: StopAction,
}

/// The overlap action selected for one occurrence.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OverlapAction {
    /// The job is disabled, so this occurrence must not be claimed.
    Disabled,
    /// No active run blocks the occurrence. The accompanying intent is an
    /// unblocked durable queued row ready for the normal starting CAS.
    Start,
    /// Claim the occurrence as a terminal skipped row.
    Skip,
    /// Preserve the occurrence as an unblocked FIFO queued row.
    Queue,
    /// Stop the old run and preserve this occurrence as a queued row blocked
    /// by that old run until cleanup is confirmed.
    KillPrevious,
}

/// Pure overlap result. `run_intent` is present for every enabled occurrence,
/// including `Start` and `Skip`, so the storage layer can claim it durably.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlapDecision {
    pub action: OverlapAction,
    pub run_intent: Option<RunIntent>,
    pub stop_intent: Option<StopIntent>,
}

/// Inputs for one per-job overlap decision. The scheduler serializes this
/// evaluation under its per-job mutex; this function itself performs no I/O.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlapPolicyInput {
    pub enabled: bool,
    pub overlap_policy: OverlapPolicy,
    pub scheduled_at: Option<i64>,
    pub queue_sequence: i64,
    pub active_run: Option<RunPolicySnapshot>,
}

/// Decide what to do with one occurrence.
///
/// Terminal snapshots do not block a new occurrence. The four durable active
/// statuses (`queued`, `starting`, `running`, `stopping`) all block it. In
/// particular, kill-previous never returns `Start` while an active snapshot
/// exists: it returns a queued intent whose `blocked_by_run_id` points to the
/// old run and a stop intent that moves the old run to `stopping`.
pub fn decide_overlap(input: &OverlapPolicyInput) -> OverlapDecision {
    if !input.enabled {
        return OverlapDecision {
            action: OverlapAction::Disabled,
            run_intent: None,
            stop_intent: None,
        };
    }

    let active_run = input
        .active_run
        .as_ref()
        .filter(|run| is_active_status(run.status));

    let Some(active_run) = active_run else {
        let run_intent = match input.overlap_policy {
            OverlapPolicy::Skip => {
                RunIntent::queued(input.scheduled_at, input.queue_sequence, None)
            }
            OverlapPolicy::Queue | OverlapPolicy::KillPrevious => {
                RunIntent::queued(input.scheduled_at, input.queue_sequence, None)
            }
        };

        return OverlapDecision {
            action: OverlapAction::Start,
            run_intent: Some(run_intent),
            stop_intent: None,
        };
    };

    match input.overlap_policy {
        OverlapPolicy::Skip => OverlapDecision {
            action: OverlapAction::Skip,
            run_intent: Some(RunIntent::skipped(input.scheduled_at, input.queue_sequence)),
            stop_intent: None,
        },
        OverlapPolicy::Queue => OverlapDecision {
            action: OverlapAction::Queue,
            run_intent: Some(RunIntent::queued(
                input.scheduled_at,
                input.queue_sequence,
                None,
            )),
            stop_intent: None,
        },
        OverlapPolicy::KillPrevious => {
            let (action, to_status) = if active_run.status == RunStatus::Stopping {
                (StopAction::AlreadyStopping, RunStatus::Stopping)
            } else {
                (StopAction::RequestStop, RunStatus::Stopping)
            };
            OverlapDecision {
                action: OverlapAction::KillPrevious,
                run_intent: Some(RunIntent::queued(
                    input.scheduled_at,
                    input.queue_sequence,
                    Some(active_run.id.clone()),
                )),
                stop_intent: Some(StopIntent {
                    run_id: active_run.id.clone(),
                    from_status: active_run.status,
                    to_status,
                    action,
                }),
            }
        }
    }
}

/// A queued row is spawn-eligible only after the kill-previous blocker has
/// been cleared. Status is checked as well so terminal rows can never be
/// accidentally dequeued.
pub const fn can_dequeue(snapshot: &RunPolicySnapshot) -> bool {
    matches!(snapshot.status, RunStatus::Queued) && snapshot.blocked_by_run_id.is_none()
}

/// Pick the FIFO queued row that is currently unblocked.
///
/// The earliest queued row is selected before checking its blocker. A blocked
/// kill-previous row therefore holds the FIFO position and returns `None`
/// until cleanup clears its link; a later unblocked row cannot overtake it.
pub fn next_queued_run(runs: &[RunPolicySnapshot]) -> Option<&RunPolicySnapshot> {
    let earliest = runs
        .iter()
        .filter(|run| matches!(run.status, RunStatus::Queued))
        .min_by(|left, right| {
            left.queue_sequence
                .cmp(&right.queue_sequence)
                .then_with(|| left.id.cmp(&right.id))
        })?;

    can_dequeue(earliest).then_some(earliest)
}

/// Result of the old run's external cleanup. The adapter reports this state
/// after it has verified actual process/descendant termination; policy only
/// consumes the result.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CleanupStatus {
    Pending,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunStateUpdate {
    pub run_id: String,
    pub status: RunStatus,
    pub blocked_by_run_id: Option<String>,
}

/// Pure result for the kill-previous cleanup boundary.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", tag = "action")]
pub enum CleanupDecision {
    /// Keep the linked queued row blocked and do not start anything yet.
    Wait,
    /// Cleanup is confirmed: old becomes cancelled, and the linked row is
    /// unblocked but remains queued for a later starting CAS.
    Unblock {
        old_run: RunStateUpdate,
        queued_run: RunStateUpdate,
    },
    /// Cleanup failed: both rows become terminal failed rows and no spawn is
    /// permitted.
    Fail {
        old_run: RunStateUpdate,
        queued_run: RunStateUpdate,
    },
    /// The supplied snapshots do not describe the linked stopping/queued
    /// pair; fail safe without allowing a launch.
    Reject,
}

/// Resolve the durable link created by [`OverlapAction::KillPrevious`].
///
/// The linked row must remain `queued` and reference the old `stopping` row.
/// Confirmation changes only the policy result: the caller still performs
/// the short conditional database updates and then separately claims
/// `queued -> starting`. A failure never returns an executable queued row.
pub fn resolve_kill_previous_cleanup(
    old_run: &RunPolicySnapshot,
    queued_run: &RunPolicySnapshot,
    cleanup: CleanupStatus,
) -> CleanupDecision {
    if old_run.status != RunStatus::Stopping
        || queued_run.status != RunStatus::Queued
        || queued_run.blocked_by_run_id.as_deref() != Some(old_run.id.as_str())
    {
        return CleanupDecision::Reject;
    }

    match cleanup {
        CleanupStatus::Pending => CleanupDecision::Wait,
        CleanupStatus::Confirmed => CleanupDecision::Unblock {
            old_run: RunStateUpdate {
                run_id: old_run.id.clone(),
                status: RunStatus::Cancelled,
                blocked_by_run_id: None,
            },
            queued_run: RunStateUpdate {
                run_id: queued_run.id.clone(),
                status: RunStatus::Queued,
                blocked_by_run_id: None,
            },
        },
        CleanupStatus::Failed => CleanupDecision::Fail {
            old_run: RunStateUpdate {
                run_id: old_run.id.clone(),
                status: RunStatus::Failed,
                blocked_by_run_id: None,
            },
            queued_run: RunStateUpdate {
                run_id: queued_run.id.clone(),
                status: RunStatus::Failed,
                blocked_by_run_id: Some(old_run.id.clone()),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(
        occurrences: &[i64],
        startup_cutoff: i64,
        catch_up: bool,
        enabled: bool,
    ) -> Vec<DueOccurrence> {
        select_occurrences(occurrences, startup_cutoff, catch_up, enabled)
    }

    fn snapshot(id: &str, status: RunStatus, queue_sequence: i64) -> RunPolicySnapshot {
        RunPolicySnapshot::new(id, status, queue_sequence, None::<&str>)
    }

    fn overlap(
        policy: OverlapPolicy,
        enabled: bool,
        active_run: Option<RunPolicySnapshot>,
    ) -> OverlapDecision {
        decide_overlap(&OverlapPolicyInput {
            enabled,
            overlap_policy: policy,
            scheduled_at: Some(10_000),
            queue_sequence: 42,
            active_run,
        })
    }

    #[test]
    fn cutoff_equality_is_startup_gap() {
        assert_eq!(
            classify_occurrence(1_000, 1_000),
            OccurrenceClass::StartupGap
        );
        assert_eq!(
            schedule(&[999, 1_000, 1_001], 1_000, true, true),
            vec![
                DueOccurrence {
                    timestamp: 1_000,
                    class: OccurrenceClass::StartupGap,
                },
                DueOccurrence {
                    timestamp: 1_001,
                    class: OccurrenceClass::SteadyState,
                },
            ]
        );
    }

    #[test]
    fn empty_gap_has_no_catch_up_but_steady_due_is_kept() {
        assert_eq!(
            schedule(&[1_001, 1_002], 1_000, true, true),
            vec![
                DueOccurrence {
                    timestamp: 1_001,
                    class: OccurrenceClass::SteadyState,
                },
                DueOccurrence {
                    timestamp: 1_002,
                    class: OccurrenceClass::SteadyState,
                },
            ]
        );
    }

    #[test]
    fn many_gap_occurrences_select_only_the_latest_when_enabled() {
        assert_eq!(
            schedule(&[100, 300, 200, 300, 400], 300, true, true),
            vec![
                DueOccurrence {
                    timestamp: 300,
                    class: OccurrenceClass::StartupGap,
                },
                DueOccurrence {
                    timestamp: 400,
                    class: OccurrenceClass::SteadyState,
                },
            ]
        );
    }

    #[test]
    fn disabled_job_selects_nothing() {
        assert!(schedule(&[100, 200, 300], 200, true, false).is_empty());
        assert!(schedule(&[100, 200, 300], 200, false, false).is_empty());
    }

    #[test]
    fn catch_up_disabled_skips_only_the_gap() {
        assert_eq!(
            schedule(&[100, 200, 300, 400], 300, false, true),
            vec![DueOccurrence {
                timestamp: 400,
                class: OccurrenceClass::SteadyState,
            },]
        );
    }

    #[test]
    fn steady_due_is_independent_of_catch_up() {
        let with_catch_up = schedule(&[1_000, 1_001, 1_002], 999, true, true);
        let without_catch_up = schedule(&[1_000, 1_001, 1_002], 999, false, true);
        assert_eq!(with_catch_up, without_catch_up);
        assert!(without_catch_up
            .iter()
            .all(|occurrence| occurrence.class == OccurrenceClass::SteadyState));
    }

    #[test]
    fn no_active_run_starts_a_durable_unblocked_intent() {
        let decision = overlap(OverlapPolicy::Queue, true, None);
        assert_eq!(decision.action, OverlapAction::Start);
        assert_eq!(
            decision.run_intent,
            Some(RunIntent {
                status: RunStatus::Queued,
                scheduled_at: Some(10_000),
                queue_sequence: 42,
                blocked_by_run_id: None,
            })
        );
        assert!(decision.stop_intent.is_none());
    }

    #[test]
    fn disabled_job_is_gated_before_overlap_policy() {
        let decision = overlap(
            OverlapPolicy::KillPrevious,
            false,
            Some(snapshot("old", RunStatus::Running, 1)),
        );
        assert_eq!(
            decision,
            OverlapDecision {
                action: OverlapAction::Disabled,
                run_intent: None,
                stop_intent: None,
            }
        );
    }

    #[test]
    fn skip_policy_claims_terminal_skipped_intent_when_active() {
        let decision = overlap(
            OverlapPolicy::Skip,
            true,
            Some(snapshot("old", RunStatus::Running, 1)),
        );
        assert_eq!(decision.action, OverlapAction::Skip);
        assert_eq!(
            decision.run_intent.as_ref().map(|intent| intent.status),
            Some(RunStatus::Skipped)
        );
        assert!(decision.stop_intent.is_none());
    }

    #[test]
    fn queue_policy_claims_unblocked_fifo_intent_when_active() {
        let decision = overlap(
            OverlapPolicy::Queue,
            true,
            Some(snapshot("old", RunStatus::Running, 1)),
        );
        assert_eq!(decision.action, OverlapAction::Queue);
        let intent = decision.run_intent.expect("queue intent");
        assert_eq!(intent.status, RunStatus::Queued);
        assert_eq!(intent.blocked_by_run_id, None);
    }

    #[test]
    fn kill_previous_queues_behind_old_run_without_concurrent_start() {
        let decision = overlap(
            OverlapPolicy::KillPrevious,
            true,
            Some(snapshot("old", RunStatus::Running, 7)),
        );
        assert_eq!(decision.action, OverlapAction::KillPrevious);
        let intent = decision.run_intent.expect("durable queued intent");
        assert_eq!(intent.status, RunStatus::Queued);
        assert_eq!(intent.blocked_by_run_id.as_deref(), Some("old"));
        let stop = decision.stop_intent.expect("stop intent");
        assert_eq!(stop.run_id, "old");
        assert_eq!(stop.from_status, RunStatus::Running);
        assert_eq!(stop.to_status, RunStatus::Stopping);
        assert_eq!(stop.action, StopAction::RequestStop);
    }

    #[test]
    fn kill_previous_does_not_request_second_stop_for_stopping_run() {
        let decision = overlap(
            OverlapPolicy::KillPrevious,
            true,
            Some(snapshot("old", RunStatus::Stopping, 7)),
        );
        let stop = decision.stop_intent.expect("stop intent");
        assert_eq!(stop.action, StopAction::AlreadyStopping);
        assert_eq!(stop.to_status, RunStatus::Stopping);
        assert_eq!(
            decision.run_intent.unwrap().blocked_by_run_id.as_deref(),
            Some("old")
        );
    }

    #[test]
    fn every_active_status_blocks_kill_previous_and_terminal_statuses_do_not() {
        for status in [
            RunStatus::Queued,
            RunStatus::Starting,
            RunStatus::Running,
            RunStatus::Stopping,
        ] {
            let decision = overlap(
                OverlapPolicy::KillPrevious,
                true,
                Some(snapshot("old", status, 1)),
            );
            assert_eq!(decision.action, OverlapAction::KillPrevious);
            assert_eq!(
                decision.run_intent.unwrap().blocked_by_run_id.as_deref(),
                Some("old")
            );
        }
        for status in [
            RunStatus::Succeeded,
            RunStatus::Failed,
            RunStatus::Cancelled,
            RunStatus::Skipped,
        ] {
            let decision = overlap(
                OverlapPolicy::KillPrevious,
                true,
                Some(snapshot("old", status, 1)),
            );
            assert_eq!(decision.action, OverlapAction::Start);
            assert!(decision.stop_intent.is_none());
        }
    }

    #[test]
    fn fifo_holds_on_earliest_blocked_row_instead_of_overtaking_it() {
        let runs = vec![
            RunPolicySnapshot::new("blocked-first", RunStatus::Queued, 1, Some("old")),
            snapshot("later", RunStatus::Queued, 2),
            snapshot("terminal", RunStatus::Succeeded, 0),
        ];
        assert_eq!(next_queued_run(&runs), None);
        assert!(!can_dequeue(&runs[0]));

        let mut unblocked = runs;
        unblocked[0].blocked_by_run_id = None;
        assert_eq!(
            next_queued_run(&unblocked).map(|run| run.id.as_str()),
            Some("blocked-first")
        );
    }

    #[test]
    fn blocked_kill_previous_row_waits_until_cleanup_confirmation() {
        let old = snapshot("old", RunStatus::Stopping, 1);
        let queued = RunPolicySnapshot::new("next", RunStatus::Queued, 2, Some("old"));
        assert!(!can_dequeue(&queued));
        assert_eq!(
            resolve_kill_previous_cleanup(&old, &queued, CleanupStatus::Pending),
            CleanupDecision::Wait
        );

        let confirmed = resolve_kill_previous_cleanup(&old, &queued, CleanupStatus::Confirmed);
        assert_eq!(
            confirmed,
            CleanupDecision::Unblock {
                old_run: RunStateUpdate {
                    run_id: "old".to_string(),
                    status: RunStatus::Cancelled,
                    blocked_by_run_id: None,
                },
                queued_run: RunStateUpdate {
                    run_id: "next".to_string(),
                    status: RunStatus::Queued,
                    blocked_by_run_id: None,
                },
            }
        );
        assert!(can_dequeue(&RunPolicySnapshot::new(
            "next",
            RunStatus::Queued,
            2,
            None::<&str>
        )));
    }

    #[test]
    fn failed_kill_previous_cleanup_fails_both_rows_without_spawn() {
        let old = snapshot("old", RunStatus::Stopping, 1);
        let queued = RunPolicySnapshot::new("next", RunStatus::Queued, 2, Some("old"));
        assert_eq!(
            resolve_kill_previous_cleanup(&old, &queued, CleanupStatus::Failed),
            CleanupDecision::Fail {
                old_run: RunStateUpdate {
                    run_id: "old".to_string(),
                    status: RunStatus::Failed,
                    blocked_by_run_id: None,
                },
                queued_run: RunStateUpdate {
                    run_id: "next".to_string(),
                    status: RunStatus::Failed,
                    blocked_by_run_id: Some("old".to_string()),
                },
            }
        );
    }

    #[test]
    fn cleanup_rejects_unlinked_or_non_stopping_pair() {
        let old = snapshot("old", RunStatus::Running, 1);
        let queued = RunPolicySnapshot::new("next", RunStatus::Queued, 2, Some("other"));
        assert_eq!(
            resolve_kill_previous_cleanup(&old, &queued, CleanupStatus::Confirmed),
            CleanupDecision::Reject
        );
    }
}
