use super::*;
use devbox_activity::model::sessionizer::Sessionizer;

fn foreground(app: &str, title: &str, at_ms: i64) -> Observation {
    Observation {
        seq: 0,
        at_ms,
        kind: ObservationKind::Foreground,
        app: Some(app.into()),
        title: Some(title.into()),
        last_input_ms: at_ms,
    }
}
fn sessions(events: Vec<Observation>) -> Vec<(i64, i64)> {
    let mut engine = Sessionizer::new();
    let mut closed = Vec::new();
    for e in events {
        closed.extend(match e.kind {
            ObservationKind::Foreground | ObservationKind::Input => engine.observe_with_input(
                e.app.unwrap_or_default(),
                e.title.unwrap_or_default(),
                e.at_ms,
                e.last_input_ms,
            ),
            ObservationKind::Idle => engine.close_at_last_input(e.last_input_ms),
            ObservationKind::Pause => engine.finish_all(e.at_ms),
        });
    }
    closed.into_iter().map(|s| (s.start_ts, s.end_ts)).collect()
}
fn enabled() -> Collector {
    let mut c = Collector::restore(true, false).0;
    c.configure(Some(&PrivacyRules::default()));
    c
}

#[test]
fn consent_off_interval_is_never_sessionized_after_same_or_different_foreground() {
    for next in ["owned.exe", "different.exe"] {
        for lost_ack in [false, true] {
            let mut c = enabled();
            c.accept(foreground("owned.exe", "before", 1_000));
            c.accept(foreground("owned.exe", "before", 31_000));
            let before = c.drain(100);
            if !lost_ack {
                c.ack(before.upto);
            }
            c.accept(foreground("unsent.exe", "discard this title", 32_000));
            c.set_consent(false);
            c.set_consent(true);
            c.accept(foreground(next, "after", 201_000));
            c.set_paused(true, 211_000);
            let later = c.drain(100);
            assert!(!format!("{:?}", later.events).contains("discard this title"));
            let stored = sessions(before.events.into_iter().chain(later.events).collect());
            let duration: i64 = stored.iter().map(|(start, end)| end - start).sum();
            assert!(
                duration <= 41_000,
                "consent gap included: {duration} ms, {stored:?}"
            );
            assert!(
                stored
                    .iter()
                    .all(|(start, end)| *end <= 32_000 || *start >= 201_000)
            );
        }
    }
}

#[test]
fn withdrawal_boundary_survives_old_ack_and_offline_overflow_without_titles() {
    let mut c = enabled();
    c.accept(foreground("owned.exe", "before", 1_000));
    c.accept(foreground("owned.exe", "before", 31_000));
    let before = c.drain(100);
    c.set_consent(false);
    let withdrawn = c.drain(100);
    assert_eq!(
        withdrawn.events.len(),
        1,
        "only a nameless boundary remains"
    );
    let boundary = &withdrawn.events[0];
    assert_eq!(
        (
            boundary.kind,
            boundary.app.as_ref(),
            boundary.title.as_ref()
        ),
        (ObservationKind::Idle, None, None)
    );
    assert!(boundary.seq > before.upto);
    c.ack(before.upto); // An in-flight old batch finished after the withdrawal.
    c.set_consent(true);
    for i in 0..1100 {
        c.accept(foreground("after.exe", "after", 201_000 + i));
    }
    let over = c.drain(1000);
    assert_eq!(over.events.len(), 1000);
    assert_eq!(
        &over.events[0], boundary,
        "overflow cannot evict the stop boundary"
    );
    let stored = sessions(before.events.into_iter().chain(over.events).collect());
    assert!(
        stored
            .iter()
            .all(|(start, end)| *end <= 31_000 || *start >= 201_000)
    );
    c.ack(over.upto);
    assert!(c.drain(1000).events.iter().all(|e| e.seq > over.upto));
}

#[test]
fn repeated_withdrawal_keeps_earliest_unconfirmed_cutoff_and_invalidates_held_batch() {
    let mut c = enabled();
    c.accept(foreground("owned.exe", "before", 1_000));
    c.accept(foreground("owned.exe", "before", 31_000));
    let before = c.drain(100);
    c.ack(before.upto);
    c.set_consent(false);
    let first_stop = c.drain(100);
    c.set_consent(true);
    c.accept(foreground("after.exe", "unconfirmed title", 201_000));
    let held = c.drain(100);
    c.set_consent(false);
    let second_stop = c.drain(100);
    assert_eq!(second_stop.events.len(), 1);
    assert_eq!(second_stop.events[0].last_input_ms, 31_000);
    assert!(
        second_stop.events[0].seq > held.upto,
        "old uncertain payload cannot be resent"
    );
    c.ack(first_stop.upto);
    c.ack(held.upto);
    assert_eq!(
        c.drain(100),
        second_stop,
        "late ACKs cannot clear the new boundary"
    );
    let stored = sessions(
        before
            .events
            .into_iter()
            .chain(second_stop.events.clone())
            .collect(),
    );
    assert_eq!(stored, vec![(1_000, 31_000)]);
    c.ack(second_stop.upto);
    assert!(c.drain(100).events.is_empty());
    c.set_consent(false);
    assert!(
        c.drain(100).events.is_empty(),
        "already withdrawn does not create more boundaries"
    );
}
