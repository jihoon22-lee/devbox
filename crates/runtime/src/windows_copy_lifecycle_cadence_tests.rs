//! M-3: an attempt held after its run ended (sealed reader, no receipt yet) is probed on
//! Windows at most once per window, while its receipt is still read on every 10 s pass. Each
//! test keeps one service across passes, as the daemon does; the window lives in that service.
use super::*;
use crate::{
    windows_copy_store::ReaderState,
    windows_probe_cadence::{HELD_PROBE_INTERVAL, ProbeCadence},
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
    // A live run's attempt (its reader submitted, not yet sealed) is probed on every pass; the
    // reconcile pass of a live reader also asks the user's systemd, so the cadence is checked
    // here directly. Its last live probe starts the window once the run ended, and a closed
    // attempt is forgotten.
    let (live, sealed) = (ReaderState::Submitted, ReaderState::Sealed);
    let cadence = ProbeCadence::default();
    assert!((0..3).all(|_| cadence.claim("one", live)));
    assert!(!cadence.claim("one", sealed));
    cadence.backdate(HELD_PROBE_INTERVAL);
    assert!(cadence.claim("one", sealed));
    assert!(!cadence.claim("one", sealed));
    cadence.forget("one");
    assert!(cadence.claim("one", sealed));
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
