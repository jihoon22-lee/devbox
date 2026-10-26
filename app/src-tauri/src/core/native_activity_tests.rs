use super::*;
#[test]
fn direct_exit_does_not_release_live_descendants() {
    assert_eq!(observe(Some(true), Some(1)), Observation::Active);
}
#[test]
fn only_exact_child_exit_and_retained_job_empty_complete() {
    assert_eq!(observe(Some(true), Some(0)), Observation::Complete);
    assert_eq!(observe(Some(false), Some(0)), Observation::Active);
    for (child, job) in [(None, Some(0)), (Some(true), None), (None, None)] {
        assert_eq!(observe(child, job), Observation::Unknown);
    }
}
#[test]
fn unknown_job_query_retains_the_attempt_until_proven_quiescence() {
    let mut ledger = Ledger::default();
    let id = ledger.reserve(false).unwrap();
    ledger.observe(id, observe(Some(false), Some(1)));
    let snapshot = ledger.snapshot();
    ledger.observe(id, observe(Some(true), Some(2)));
    assert_eq!(ledger.snapshot(), snapshot);
    ledger.observe(id, observe(Some(true), None));
    assert_eq!(ledger.snapshot().0, None);
    ledger.observe(id, observe(Some(true), Some(0)));
    assert_eq!(ledger.snapshot().0, Some(0));
}
#[test]
fn epoch_changes_only_when_admission_or_observed_state_changes() {
    let mut ledger = Ledger::default();
    let id = ledger.reserve(false).unwrap();
    let first = ledger.snapshot();
    ledger.observe(id, Observation::Unknown);
    assert_eq!(ledger.snapshot(), first);
    ledger.observe(id, Observation::Active);
    let active = ledger.snapshot();
    assert_ne!(active.1, first.1);
    ledger.observe(id, Observation::Active);
    assert_eq!(ledger.snapshot(), active);
    ledger.observe(id, Observation::Complete);
    let finished = ledger.snapshot();
    assert_eq!(finished.0, Some(0));
    assert_ne!(finished.1, active.1);
    ledger.observe(id, Observation::Active);
    assert_eq!(ledger.snapshot(), finished);
}
#[test]
fn final_seal_refuses_native_admission_and_summary_epoch_detects_new_attempt() {
    let mut ledger = Ledger::default();
    let before = ledger.snapshot();
    assert!(ledger.reserve(true).is_none());
    assert_eq!(ledger.snapshot(), before);
    let id = ledger.reserve(false).unwrap();
    assert_ne!(ledger.snapshot().1, before.1);
    ledger.not_spawned(id);
    assert_eq!(ledger.snapshot().0, Some(0));
    assert_ne!(ledger.snapshot().1, before.1);
}
#[test]
fn unknown_lifetimes_are_bounded_and_never_expire_to_zero() {
    let mut ledger = Ledger::default();
    for _ in 0..64 {
        assert!(ledger.reserve(false).is_some());
    }
    assert!(ledger.reserve(false).is_none());
    assert_eq!(ledger.snapshot().0, None);
    ledger.not_spawned(1);
    assert!(ledger.reserve(false).is_some());
    assert_eq!(ledger.snapshot().0, None);
}
