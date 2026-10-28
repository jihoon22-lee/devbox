//! I1: a vanished job name proves nothing while orphaned descendants may still run.
//! Only Empty, or NotFound with this attempt's own holder receipt, releases the reader.
use crate::{
    RuntimeService,
    windows_copy_store::{self as store, Attempt, AttemptPhase},
    windows_copy_test_support::*,
};
use devbox_protocol::windows::ObservationState;
use std::sync::Arc;

/// One sealed reader whose attempts were committed with exact assigned identities.
async fn committed(o: &Owned, ids: &[(&str, u8)]) -> Vec<Attempt> {
    o.copy
        .identity
        .ensure_owned()
        .expect("private control directory");
    let identity = o.copy.identity.clone();
    let r = reader(o, "orphans", "rev1");
    let attempts: Vec<_> = ids.iter().map(|(id, n)| attempt(o, &r, id, *n)).collect();
    let stored = attempts.clone();
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("reserve");
            store::publish(tx, &r.copy)?.expect("publish");
            store::submit(tx, &r.copy)?.expect("dispatch");
            for a in &stored {
                store::intent(tx, a)?.expect("attempt");
                store::commit(tx, &a.control.nonce, &assigned(a))?.expect("commit");
            }
            store::seal(tx, &r.copy.launch_id)?;
            Ok(())
        })
        .await
        .expect("sealed committed consumer");
    attempts
}
/// Every query of an attempt answers `state(nonce)`, as the direct and native probes would.
fn observe(o: &mut Owned, state: impl Fn(&str) -> ObservationState + Send + Sync + 'static) {
    let state = Arc::new(state);
    o.config.windows_observation = Arc::new(move |assigned| {
        let answer = state(&assigned.nonce);
        Box::pin(async move { answer })
    });
}
async fn phase(db: &devbox_core::Db, a: &Attempt) -> AttemptPhase {
    let nonce = a.control.nonce.clone();
    db.read(move |tx| Ok(store::attempt(tx, &nonce)?.expect("attempt").phase))
        .await
        .expect("attempt phase")
}
async fn held(db: &devbox_core::Db) -> bool {
    db.read(store::needs_reconcile).await.expect("readers")
}
fn matching(a: &Attempt) -> String {
    format!(r#"{{"nonce":"{}"}}"#, a.control.nonce)
}
fn write_receipt(o: &Owned, a: &Attempt, body: &str) {
    std::fs::write(emptied_receipt(o, a), body).expect("owned holder receipt fixture");
}
#[tokio::test]
async fn not_found_without_receipt_keeps_the_reader_open() {
    let mut o = owned().await;
    let [a] = committed(&o, &[("one", 1)])
        .await
        .try_into()
        .expect("one attempt");
    observe(&mut o, |_| ObservationState::NotFound);
    let svc = service(&o);
    for _ in 0..2 {
        svc.reconcile_windows_copies()
            .await
            .expect("an unconfirmed exit is retained");
        assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed);
        assert!(
            held(&o.f.db).await,
            "orphans may still run in the vanished job"
        );
    }
    assert!(
        !emptied_receipt(&o, &a).exists(),
        "only the Windows holder writes a receipt"
    );
    svc.shutdown().await;
}
#[tokio::test]
async fn not_found_with_a_matching_receipt_closes() {
    let mut o = owned().await;
    let [a] = committed(&o, &[("one", 1)])
        .await
        .try_into()
        .expect("one attempt");
    observe(&mut o, |_| ObservationState::NotFound);
    let svc = service(&o);
    svc.reconcile_windows_copies()
        .await
        .expect("no receipt yet");
    assert!(held(&o.f.db).await);
    write_receipt(&o, &a, &matching(&a));
    svc.reconcile_windows_copies()
        .await
        .expect("the receipt proves the job emptied");
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
    assert!(!held(&o.f.db).await);
    svc.shutdown().await;
}
#[tokio::test]
async fn receipt_with_another_nonce_is_ignored() {
    let mut o = owned().await;
    let [a, b] = committed(&o, &[("one", 1), ("two", 2)])
        .await
        .try_into()
        .expect("two attempts");
    observe(&mut o, |_| ObservationState::NotFound);
    // b's receipt names a, and a's own nonce elsewhere is not at a's derived path.
    write_receipt(&o, &b, &matching(&a));
    let control = o.copy.identity.mapped_base.join("control");
    std::fs::write(control.join("emptied.json"), matching(&a)).expect("owned stray receipt");
    std::fs::write(o.copy.target.join("emptied.json"), matching(&a)).expect("owned copy file");
    let svc = service(&o);
    svc.reconcile_windows_copies()
        .await
        .expect("foreign receipts prove nothing");
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed);
    assert_eq!(phase(&o.f.db, &b).await, AttemptPhase::Committed);
    assert!(held(&o.f.db).await);
    svc.shutdown().await;
}
#[tokio::test]
async fn empty_still_closes() {
    let mut o = owned().await;
    let [a, b] = committed(&o, &[("one", 1), ("two", 2)])
        .await
        .try_into()
        .expect("two attempts");
    let empty = a.control.nonce.clone();
    observe(&mut o, move |nonce| {
        if nonce == empty {
            ObservationState::Empty
        } else {
            ObservationState::NotFound
        }
    });
    let svc = service(&o);
    svc.reconcile_windows_copies()
        .await
        .expect("an existing empty job is proof without a receipt");
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
    assert_eq!(
        phase(&o.f.db, &b).await,
        AttemptPhase::Committed,
        "unlike a vanished job name"
    );
    assert!(!emptied_receipt(&o, &a).exists());
    svc.shutdown().await;
}
#[tokio::test]
async fn receipt_decision_survives_a_daemon_restart() {
    let mut o = owned().await;
    let [a] = committed(&o, &[("one", 1)])
        .await
        .try_into()
        .expect("one attempt");
    observe(&mut o, |_| ObservationState::NotFound);
    let first = service(&o);
    first.reconcile_windows_copies().await.expect("no receipt");
    first.shutdown().await;
    // A fresh store and service over the same database and private control directory.
    let restart = || {
        let db = devbox_core::Db::open(
            &o.f.root.path().join("index.db"),
            &o.f.root.path().join("backups"),
            "1.0.0",
            &[devbox_projects::MIGRATIONS, crate::MIGRATIONS].concat(),
        )
        .expect("reopen durable DB");
        let svc = RuntimeService::new(db.clone(), o.f.hub.clone(), o.f.projects.clone())
            .with_execution(o.config.clone());
        (db, svc)
    };
    let (db, second) = restart();
    second
        .reconcile_windows_copies()
        .await
        .expect("still no receipt after restart");
    assert_eq!(phase(&db, &a).await, AttemptPhase::Committed);
    assert!(held(&db).await);
    second.shutdown().await;
    write_receipt(&o, &a, &matching(&a));
    let (db, third) = restart();
    third
        .reconcile_windows_copies()
        .await
        .expect("the durable receipt is read after restart");
    assert_eq!(phase(&db, &a).await, AttemptPhase::Closed);
    assert!(!held(&db).await);
    third.shutdown().await;
}
#[tokio::test]
async fn malformed_oversized_or_linked_receipts_keep_the_reader_open() {
    let mut o = owned().await;
    let [a] = committed(&o, &[("one", 1)])
        .await
        .try_into()
        .expect("one attempt");
    observe(&mut o, |_| ObservationState::NotFound);
    let path = emptied_receipt(&o, &a);
    let good = matching(&a);
    let outside = o.f.root.path().join("outside-receipt.json");
    std::fs::write(&outside, &good).expect("owned link target");
    let svc = service(&o);
    for case in [
        "empty",
        "malformed",
        "unknown field",
        "wrong type",
        "oversized",
        "directory",
        "link",
    ] {
        let nonce = &a.control.nonce;
        match case {
            "empty" => std::fs::write(&path, ""),
            "malformed" => std::fs::write(&path, r#"{"nonce":"#),
            "unknown field" => std::fs::write(&path, format!(r#"{{"nonce":"{nonce}","pid":1}}"#)),
            "wrong type" => std::fs::write(&path, r#"{"nonce":1}"#),
            "oversized" => std::fs::write(&path, format!("{good}{}", " ".repeat(2048))),
            "directory" => std::fs::create_dir(&path),
            "link" => std::os::unix::fs::symlink(&outside, &path),
            _ => unreachable!(),
        }
        .expect("owned receipt fixture");
        svc.reconcile_windows_copies().await.expect(case);
        assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Committed, "{case}");
        if case == "directory" {
            std::fs::remove_dir(&path)
        } else {
            std::fs::remove_file(&path)
        }
        .expect("remove owned fixture");
    }
    assert_eq!(
        std::fs::read_to_string(&outside).expect("link target"),
        good
    );
    // The same derived path with valid content is the proof.
    write_receipt(&o, &a, &good);
    svc.reconcile_windows_copies().await.expect("valid receipt");
    assert_eq!(phase(&o.f.db, &a).await, AttemptPhase::Closed);
    svc.shutdown().await;
}
#[path = "windows_copy_lifecycle_holder_tests.rs"]
mod holder;
