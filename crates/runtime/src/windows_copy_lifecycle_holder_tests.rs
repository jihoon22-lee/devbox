//! Rulings 34/35: without a receipt, a vanished (or session-hidden) job closes only once this
//! attempt's own holder is provably gone. rc.1-shaped controls keep rc.1's rule.
use super::*;
use devbox_protocol::windows::{ControlShape, HolderIdentity, ObservationControl, RunIdentity};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
type Asked = Arc<Mutex<Vec<(HolderIdentity, bool)>>>;
/// A ready-marker body for an attempt, or none at all.
type Marker = fn(&Attempt) -> Option<String>;
/// The holder this attempt's bootstrap started, as its ready marker records it.
fn identity(a: &Attempt) -> HolderIdentity {
    let receipt = assigned(a);
    HolderIdentity {
        nonce: a.control.nonce.clone(),
        pid: 202,
        creation_time: "132000000000000001".into(),
        sid: receipt.sid,
        session_id: receipt.session_id,
    }
}
/// The bootstrap's own process, as its validated assigned receipt records it.
fn bootstrap(a: &Attempt) -> HolderIdentity {
    let receipt = assigned(a);
    HolderIdentity {
        nonce: receipt.nonce,
        pid: receipt.pid,
        creation_time: receipt.creation_time,
        sid: receipt.sid,
        session_id: receipt.session_id,
    }
}
fn write_marker(o: &Owned, a: &Attempt, body: &str) {
    let path = o
        .copy
        .identity
        .mapped_base
        .join("control")
        .join(format!("{}.holder-ready", a.control.nonce));
    std::fs::write(path, body).expect("owned holder marker fixture");
}
fn write_holder(o: &Owned, a: &Attempt, holder: &HolderIdentity) {
    write_marker(o, a, &serde_json::to_string(holder).expect("marker"));
}
/// Every job observation answers `state`; the count shows whether any probe ran.
fn observed(o: &mut Owned, state: ObservationState) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    o.config.windows_observation = Arc::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        let state = state.clone();
        Box::pin(async move { state })
    });
    calls
}
/// Every liveness check answers `answer(process)` and records what it was asked.
fn liveness(
    o: &mut Owned,
    answer: impl Fn(&HolderIdentity) -> ObservationState + Send + Sync + 'static,
) -> Asked {
    let asked: Asked = Arc::default();
    let record = asked.clone();
    o.config.windows_holder = Arc::new(move |process, cross_session| {
        let state = answer(&process);
        record
            .lock()
            .expect("holder checks")
            .push((process, cross_session));
        Box::pin(async move { state })
    });
    asked
}
/// Every liveness check answers `state`.
fn holder_check(o: &mut Owned, state: ObservationState) -> Asked {
    liveness(o, move |_| state.clone())
}
fn asked(checks: &Asked) -> Vec<(HolderIdentity, bool)> {
    checks.lock().expect("holder checks").clone()
}
async fn one(o: &Owned) -> Attempt {
    let [a] = committed(o, &[("one", 1)])
        .await
        .try_into()
        .expect("one attempt");
    a
}
async fn reconcile(o: &Owned) {
    let svc = service(o);
    svc.reconcile_windows_copies()
        .await
        .expect("reconcile Windows copies");
    svc.shutdown().await;
}
#[tokio::test]
async fn holder_gone_without_receipt_closes() {
    let mut o = owned().await;
    let a = one(&o).await;
    write_holder(&o, &a, &identity(&a));
    observed(&mut o, ObservationState::NotFound);
    let checks = holder_check(&mut o, ObservationState::NotFound);
    reconcile(&o).await;
    assert_eq!(asked(&checks), vec![(identity(&a), false)]);
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
    assert!(!held(&o.f.db).await);
}
#[tokio::test]
async fn holder_alive_or_unknown_without_receipt_stays_open() {
    for state in [
        ObservationState::Active { count: 1 },
        ObservationState::Unavailable,
        ObservationState::IdentityMismatch,
    ] {
        let mut o = owned().await;
        let a = one(&o).await;
        write_holder(&o, &a, &identity(&a));
        observed(&mut o, ObservationState::NotFound);
        let checks = holder_check(&mut o, state.clone());
        reconcile(&o).await;
        assert_eq!(asked(&checks).len(), 1, "{state:?}");
        assert_eq!(
            phase(&o.f.db, &a).await,
            AttemptPhase::Committed,
            "{state:?}"
        );
        assert!(held(&o.f.db).await);
    }
}
#[tokio::test]
async fn holder_identity_from_another_nonce_session_or_user_is_ignored() {
    let cases: [(&str, Marker); 6] = [
        ("missing", |_| None),
        ("other nonce", |a| {
            Some(
                serde_json::to_string(&HolderIdentity {
                    nonce: "f".repeat(32),
                    ..identity(a)
                })
                .expect("marker"),
            )
        }),
        ("other session", |a| {
            Some(
                serde_json::to_string(&HolderIdentity {
                    session_id: 9,
                    ..identity(a)
                })
                .expect("marker"),
            )
        }),
        ("other user", |a| {
            Some(
                serde_json::to_string(&HolderIdentity {
                    sid: "S-1-5-21-999".into(),
                    ..identity(a)
                })
                .expect("marker"),
            )
        }),
        ("malformed", |_| Some(r#"{"nonce":"#.into())),
        ("unknown field", |a| {
            let body = serde_json::to_string(&identity(a)).expect("marker");
            Some(body.replace('}', r#","parent":1}"#))
        }),
    ];
    for (case, marker) in cases {
        let mut o = owned().await;
        let a = one(&o).await;
        if let Some(body) = marker(&a) {
            write_marker(&o, &a, &body);
        }
        observed(&mut o, ObservationState::NotFound);
        let checks = holder_check(&mut o, ObservationState::NotFound);
        reconcile(&o).await;
        assert!(
            asked(&checks).is_empty(),
            "{case}: never asked about another holder"
        );
        assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed, "{case}");
    }
}
#[tokio::test]
async fn a_matching_receipt_closes_without_any_probe() {
    let mut o = owned().await;
    let a = one(&o).await;
    write_holder(&o, &a, &identity(&a));
    write_receipt(&o, &a, &matching(&a));
    let observations = observed(&mut o, ObservationState::Active { count: 1 });
    let checks = holder_check(&mut o, ObservationState::Active { count: 1 });
    reconcile(&o).await;
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
    assert_eq!(observations.load(Ordering::SeqCst), 0, "no job probe");
    assert!(asked(&checks).is_empty(), "no holder probe");
}
/// Rewrite a committed row as rc.1 stored it: the control without these holder keys.
async fn stored_without(o: &Owned, a: &Attempt, keys: &'static [&'static str]) {
    rewritten(o, a, move |control| {
        for key in keys {
            control.remove(*key).expect("stored holder field");
        }
    })
    .await;
}
/// Rewrite the stored control JSON of a committed row.
async fn rewritten(
    o: &Owned,
    a: &Attempt,
    edit: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>) + Send + 'static,
) {
    let nonce = a.control.nonce.clone();
    o.f.db
        .write(move |tx| {
            let body: String = tx.query_row(
                "SELECT body FROM runtime_windows_attempts WHERE nonce=?1",
                [&nonce],
                |row| row.get(0),
            )?;
            let mut row: serde_json::Value = serde_json::from_str(&body).expect("attempt row");
            edit(row["control"].as_object_mut().expect("stored control"));
            tx.execute(
                "UPDATE runtime_windows_attempts SET body=?1 WHERE nonce=?2",
                [row.to_string(), nonce.clone()],
            )?;
            Ok(())
        })
        .await
        .expect("rewrite as an older row");
}
#[tokio::test]
async fn an_rc1_shaped_committed_attempt_closes_on_not_found() {
    let mut o = owned().await;
    let a = one(&o).await;
    stored_without(&o, &a, &["holderReadyPath", "emptiedPath"]).await;
    let observations = observed(&mut o, ObservationState::NotFound);
    let checks = holder_check(&mut o, ObservationState::Active { count: 1 });
    reconcile(&o).await;
    assert_eq!(observations.load(Ordering::SeqCst), 1);
    assert!(asked(&checks).is_empty(), "no holder ever ran for it");
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
    assert!(!held(&o.f.db).await);
}
#[tokio::test]
async fn partial_holder_fields_are_never_observed() {
    for keys in [&["holderReadyPath"][..], &["emptiedPath"][..]] {
        let mut o = owned().await;
        let a = one(&o).await;
        stored_without(&o, &a, keys).await;
        let observations = observed(&mut o, ObservationState::Empty);
        reconcile(&o).await;
        assert_eq!(observations.load(Ordering::SeqCst), 0, "{keys:?}");
        assert_eq!(
            phase(&o.f.db, &a).await,
            AttemptPhase::Committed,
            "{keys:?}"
        );
    }
}
#[tokio::test]
async fn same_user_in_another_session_closes_once_the_holder_is_gone() {
    let mut o = owned().await;
    let a = one(&o).await;
    write_holder(&o, &a, &identity(&a));
    // The job lookup cannot judge another session's Local\ name.
    observed(&mut o, ObservationState::IdentityMismatch);
    let checks = holder_check(&mut o, ObservationState::NotFound);
    reconcile(&o).await;
    // Another session's Observe never opened the bootstrap PID, so it is checked too.
    assert_eq!(
        asked(&checks),
        vec![(identity(&a), true), (bootstrap(&a), true)]
    );
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
}
#[tokio::test]
async fn a_cross_session_release_also_needs_the_bootstrap_gone() {
    for state in [
        ObservationState::Active { count: 1 },
        ObservationState::Unavailable,
        ObservationState::IdentityMismatch,
    ] {
        let mut o = owned().await;
        let a = one(&o).await;
        write_holder(&o, &a, &identity(&a));
        observed(&mut o, ObservationState::IdentityMismatch);
        // The holder alone died; the bootstrap and its command may still run elsewhere.
        let holder_pid = identity(&a).pid;
        let answer = state.clone();
        let checks = liveness(&mut o, move |process| {
            if process.pid == holder_pid {
                ObservationState::NotFound
            } else {
                answer.clone()
            }
        });
        reconcile(&o).await;
        assert_eq!(
            asked(&checks),
            vec![(identity(&a), true), (bootstrap(&a), true)],
            "{state:?}"
        );
        assert_eq!(
            phase(&o.f.db, &a).await,
            AttemptPhase::Committed,
            "{state:?}"
        );
    }
}
#[tokio::test]
async fn the_holder_is_asked_only_for_a_vanished_or_session_hidden_job() {
    for (job, closes) in [
        (ObservationState::Unavailable, false),
        (ObservationState::Active { count: 1 }, false),
        (ObservationState::Empty, true),
    ] {
        let mut o = owned().await;
        let a = one(&o).await;
        write_holder(&o, &a, &identity(&a));
        observed(&mut o, job.clone());
        let checks = holder_check(&mut o, ObservationState::NotFound);
        reconcile(&o).await;
        assert!(asked(&checks).is_empty(), "{job:?}: holder never asked");
        let expected = if closes {
            AttemptPhase::Closed
        } else {
            AttemptPhase::Committed
        };
        assert_eq!(phase(&o.f.db, &a).await, expected, "{job:?}");
    }
}
#[tokio::test]
async fn explicitly_empty_holder_fields_are_never_observed() {
    let mut o = owned().await;
    let a = one(&o).await;
    // Present but empty is not rc.1's absence: never the rc.1 rule.
    rewritten(&o, &a, |control| {
        control.insert("holderReadyPath".into(), "".into());
        control.insert("emptiedPath".into(), "".into());
    })
    .await;
    let observations = observed(&mut o, ObservationState::NotFound);
    reconcile(&o).await;
    assert_eq!(observations.load(Ordering::SeqCst), 0);
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed);
}
#[tokio::test]
async fn same_user_in_another_session_keeps_a_live_holder_open() {
    let mut o = owned().await;
    let a = one(&o).await;
    write_holder(&o, &a, &identity(&a));
    observed(&mut o, ObservationState::IdentityMismatch);
    let checks = holder_check(&mut o, ObservationState::Active { count: 1 });
    reconcile(&o).await;
    assert_eq!(asked(&checks), vec![(identity(&a), true)]);
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed);
}
#[tokio::test]
async fn another_user_keeps_the_reader_open() {
    let mut o = owned().await;
    let a = one(&o).await;
    write_holder(&o, &a, &identity(&a));
    observed(&mut o, ObservationState::IdentityMismatch);
    // The liveness check's own SID gate.
    let checks = holder_check(&mut o, ObservationState::IdentityMismatch);
    reconcile(&o).await;
    assert_eq!(asked(&checks).len(), 1);
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed);
}
#[tokio::test]
async fn every_new_control_carries_the_holder_fields() {
    let o = owned().await;
    o.copy
        .identity
        .ensure_owned()
        .expect("private control directory");
    let identity = o.copy.identity.clone();
    let r = reader(&o, "fresh", "rev1");
    let stored = r.clone();
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &stored)?.expect("reserve");
            store::publish(tx, &stored.copy)?.expect("publish");
            store::submit(tx, &stored.copy)?.expect("dispatch");
            Ok(())
        })
        .await
        .expect("submitted reader");
    let svc = service(&o);
    let directory = o.copy.identity.control_directory();
    for attempt in ["one", "two"] {
        let id = RunIdentity {
            run_id: r.run_id.clone(),
            attempt_id: attempt.into(),
        };
        let control = svc
            .observation_intent(&id, &r.copy)
            .await
            .expect("a new attempt's control");
        assert_eq!(control.shape(), Some(ControlShape::Holder), "{attempt}");
        assert_eq!(
            control,
            ObservationControl::new(r.copy.launch_id.clone(), control.nonce.clone(), &directory)
        );
    }
    svc.shutdown().await;
}
#[path = "windows_copy_lifecycle_cadence_tests.rs"]
mod cadence;
