//! M-3: an attempt held after its run ended (its reader sealed, or a newer attempt replaced
//! it; no receipt yet) is probed on Windows at once, then at most once per window, while its
//! receipt is still read on every 10 s pass. Each test keeps one service across passes, as the
//! daemon does; the window lives in that service.
use super::*;
use crate::{
    windows_copy_store::ReaderState,
    windows_probe_cadence::{AttemptRun, HELD_PROBE_INTERVAL, ProbeCadence},
};
use std::{sync::atomic::AtomicBool, time::Duration};

/// Every Windows probe so far: job observations and holder liveness checks.
fn probes(observations: &AtomicUsize, checks: &Asked) -> usize {
    observations.load(Ordering::SeqCst) + asked(checks).len()
}
/// One reconcile pass of the long-lived service, as the 10 s poll runs it.
async fn pass(svc: &RuntimeService) {
    svc.reconcile_windows_copies()
        .await
        .expect("reconcile Windows copies");
}
/// The unit of the live run below. A pass asks the user's systemd about a live reader; where it
/// answers, this unit is not found and the pass seals the reader at its end, which every test
/// with a live reader allows for.
const UNIT: &str = "devbox-test-copy-run-live.service";
/// A live run's reader, reserved and launched as a run's start does it, with these attempts
/// committed in this order (each start of a service's wrapper appends one).
async fn live(o: &Owned, svc: &RuntimeService, ids: &[(&str, u8)]) -> Vec<Attempt> {
    let permit = svc
        .reserve_windows_copy(o.copy.clone(), "run-live".into(), UNIT.into())
        .await
        .expect("reserve");
    permit.submitted().await.expect("dispatch");
    permit.launched().await.expect("submission ACK");
    let mut r = reader(o, &permit.reference.launch_id, &permit.reference.revision);
    r.run_id = "run-live".into();
    let attempts: Vec<_> = ids.iter().map(|(id, n)| attempt(o, &r, id, *n)).collect();
    let stored = attempts.clone();
    o.f.db
        .write(move |tx| {
            for a in &stored {
                store::intent(tx, a)?.expect("attempt");
                store::commit(tx, &a.control.nonce, &assigned(a))?.expect("commit");
            }
            Ok(())
        })
        .await
        .expect("committed attempts");
    attempts
}
#[tokio::test]
async fn a_stop_then_an_edited_restart_is_not_refused_as_busy() {
    // A stop seals the reader and reconciles; a restart seals it and reserves at once.
    for stop_reconciles in [true, false] {
        let mut o = owned().await;
        let running = Arc::new(AtomicBool::new(true));
        let job = running.clone();
        let observations = Arc::new(AtomicUsize::new(0));
        let count = observations.clone();
        o.config.windows_observation = Arc::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            let state = if job.load(Ordering::SeqCst) {
                ObservationState::Active { count: 1 }
            } else {
                ObservationState::Empty
            };
            Box::pin(async move { state })
        });
        let svc = service(&o);
        let [a] = live(&o, &svc, &[("one", 1)])
            .await
            .try_into()
            .expect("one attempt");
        // Each 10 s pass of the live run probes it.
        pass(&svc).await;
        assert_eq!(observations.load(Ordering::SeqCst), 1);
        // Stopped with no child outliving it; the holder's receipt is still up to 500 ms away.
        running.store(false, Ordering::SeqCst);
        svc.seal_windows_copy(&a.control.launch_id)
            .await
            .expect("stop");
        if stop_reconciles {
            svc.reconcile_windows_target(&o.copy.identity.key)
                .await
                .expect("the stop's reconcile");
        }
        // The restart of an edited source reserves the copy at once.
        std::fs::write(o.copy.source.join("input.txt"), "source v2").expect("edited source");
        let restarted = svc
            .reserve_windows_copy(o.copy.clone(), "run-live".into(), UNIT.into())
            .await;
        assert!(
            restarted.is_ok(),
            "stop reconciles: {stop_reconciles}: {:?}",
            restarted.err()
        );
        assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
        // The sealed reader was probed once, at once: the job had emptied.
        assert_eq!(observations.load(Ordering::SeqCst), 2);
        drop(restarted);
        svc.shutdown().await;
    }
}
#[tokio::test]
async fn an_attempt_a_service_restart_replaced_is_held_while_the_service_runs() {
    let mut o = owned().await;
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let record = asked.clone();
    o.config.windows_observation = Arc::new(move |assigned| {
        record.lock().expect("observations").push(assigned.nonce);
        Box::pin(async { ObservationState::Active { count: 1 } })
    });
    let svc = service(&o);
    // The service restarted while orphans of its first attempt kept that job alive. The ids sort
    // against their order: the newest attempt is the last one started, not the largest id.
    let [old, new] = live(&o, &svc, &[("z-first", 1), ("a-second", 2)])
        .await
        .try_into()
        .expect("two attempts");
    let probes = |a: &Attempt| {
        let asked = asked.lock().expect("observations");
        asked.iter().filter(|n| **n == a.control.nonce).count()
    };
    // Two passes: the replaced attempt is probed once, the running one on both. (Where the pass
    // seals the reader, the running attempt's second probe is the first after its run ended.)
    pass(&svc).await;
    pass(&svc).await;
    assert_eq!((probes(&old), probes(&new)), (1, 2));
    svc.probe_cadence.backdate(HELD_PROBE_INTERVAL);
    pass(&svc).await;
    assert_eq!((probes(&old), probes(&new)), (2, 3));
    assert_eq!(phase(&o.f.db, &old).await, AttemptPhase::Committed);
    svc.shutdown().await;
}
#[tokio::test]
async fn a_held_attempt_is_probed_at_most_once_per_window() {
    // Leftover build servers keep the job active; a vanished job, or one only another session
    // sees, is checked against a holder that is still alive. One or two probes per probing pass.
    for (job, per_probe) in [
        (ObservationState::Active { count: 1 }, 1),
        (ObservationState::NotFound, 2),
        (ObservationState::IdentityMismatch, 2),
    ] {
        let mut o = owned().await;
        let a = one(&o).await;
        write_holder(&o, &a, &identity(&a));
        let observations = observed(&mut o, job.clone());
        let checks = holder_check(&mut o, ObservationState::Active { count: 1 });
        let svc = service(&o);
        pass(&svc).await;
        assert_eq!(
            probes(&observations, &checks),
            per_probe,
            "{job:?}: first pass"
        );
        for _ in 0..5 {
            pass(&svc).await;
        }
        svc.probe_cadence
            .backdate(HELD_PROBE_INTERVAL - Duration::from_secs(1));
        pass(&svc).await;
        assert_eq!(
            probes(&observations, &checks),
            per_probe,
            "{job:?}: in window"
        );
        svc.probe_cadence.backdate(Duration::from_secs(1));
        pass(&svc).await;
        pass(&svc).await;
        assert_eq!(
            probes(&observations, &checks),
            2 * per_probe,
            "{job:?}: once more per window"
        );
        assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed, "{job:?}");
        assert!(held(&o.f.db).await);
        svc.shutdown().await;
    }
}
#[tokio::test]
async fn a_receipt_between_probes_still_releases_on_the_next_pass() {
    let mut o = owned().await;
    let a = one(&o).await;
    write_holder(&o, &a, &identity(&a));
    let observations = observed(&mut o, ObservationState::Active { count: 1 });
    let svc = service(&o);
    pass(&svc).await;
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed);
    // The last build server ends; the live holder writes the receipt within 500 ms.
    write_receipt(&o, &a, &matching(&a));
    pass(&svc).await;
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
    assert!(!held(&o.f.db).await);
    assert_eq!(observations.load(Ordering::SeqCst), 1, "no probe needed");
    svc.shutdown().await;
}
#[tokio::test]
async fn a_vanished_holder_still_releases_at_most_one_window_late() {
    let mut o = owned().await;
    let a = one(&o).await;
    write_holder(&o, &a, &identity(&a));
    // Only the holder kept the job name after the bootstrap exited.
    observed(&mut o, ObservationState::NotFound);
    let alive = Arc::new(AtomicBool::new(true));
    let holder = alive.clone();
    liveness(&mut o, move |_| {
        if holder.load(Ordering::SeqCst) {
            ObservationState::Active { count: 1 }
        } else {
            ObservationState::NotFound
        }
    });
    let svc = service(&o);
    pass(&svc).await;
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed);
    // Ended in Task Manager together with the build servers: no receipt is ever written.
    alive.store(false, Ordering::SeqCst);
    pass(&svc).await;
    assert_eq!(
        phase(&o.f.db, &a).await,
        AttemptPhase::Committed,
        "in window"
    );
    svc.probe_cadence.backdate(HELD_PROBE_INTERVAL);
    pass(&svc).await;
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
    assert!(!held(&o.f.db).await);
    svc.shutdown().await;
}
#[tokio::test]
async fn attempts_that_are_not_held_keep_the_pass_cadence() {
    // An rc.1 row has no holder and no receipt, so a probe is its only release: every pass asks.
    let mut o = owned().await;
    let a = one(&o).await;
    stored_without(&o, &a, &["holderReadyPath", "emptiedPath"]).await;
    let observations = observed(&mut o, ObservationState::Active { count: 1 });
    let svc = service(&o);
    for _ in 0..3 {
        pass(&svc).await;
    }
    assert_eq!(observations.load(Ordering::SeqCst), 3);
    svc.shutdown().await;
    // A live run's attempt is probed on every pass; the reconcile pass of a live reader also
    // asks the user's systemd, so the cadence is checked here directly. The first claim once the
    // run ended probes at once (the job may have emptied with it), then once per window; a
    // closed attempt is forgotten.
    let (live, ended) = (AttemptRun::Live, AttemptRun::Ended);
    let cadence = ProbeCadence::default();
    assert!((0..3).all(|_| cadence.claim("one", live)));
    assert!(cadence.claim("one", ended));
    assert!(!cadence.claim("one", ended));
    cadence.backdate(HELD_PROBE_INTERVAL);
    assert!(cadence.claim("one", ended));
    assert!(!cadence.claim("one", ended));
    cadence.forget("one");
    assert!(cadence.claim("one", ended));
    // A run is live only while its reader is not sealed and it is the reader's newest attempt.
    use ReaderState::{Sealed, Submitted};
    assert_eq!(AttemptRun::of(Submitted, Some("one"), "one"), live);
    for (reader, newest) in [
        (Submitted, Some("two")),
        (Sealed, Some("one")),
        (Submitted, None),
    ] {
        assert_eq!(
            AttemptRun::of(reader, newest, "one"),
            ended,
            "{reader:?} {newest:?}"
        );
    }
}
#[tokio::test]
async fn a_restarted_daemon_probes_a_held_attempt_at_once() {
    let mut o = owned().await;
    let a = one(&o).await;
    write_holder(&o, &a, &identity(&a));
    let observations = observed(&mut o, ObservationState::Active { count: 1 });
    let first = service(&o);
    pass(&first).await;
    first.shutdown().await;
    // The window lives in one daemon process only.
    let second = service(&o);
    pass(&second).await;
    assert_eq!(observations.load(Ordering::SeqCst), 2);
    pass(&second).await;
    assert_eq!(observations.load(Ordering::SeqCst), 2);
    second.shutdown().await;
}
