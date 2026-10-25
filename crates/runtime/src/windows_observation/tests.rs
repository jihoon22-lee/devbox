use super::*;
use crate::windows_copy_test_support::*;

async fn intent_fixture(o: &Owned) -> (store::CopyRecord, Attempt) {
    o.copy
        .identity
        .ensure_owned()
        .expect("private owned metadata");
    let identity = o.copy.identity.clone();
    let r = reader(o, "logical", "revision");
    let a = attempt(o, &r, "attempt", 1);
    let result = a.clone();
    let (copy, _) =
        o.f.db
            .write(move |tx| {
                store::reserve(tx, &identity, &r)?.expect("reserve");
                store::publish(tx, &r.copy)?.expect("publish");
                store::submit(tx, &r.copy)?.expect("dispatch");
                store::intent(tx, &a)?.expect("new wrapper proof");
                Ok(store::copy(tx, &identity.key)?.expect("copy"))
            })
            .await
            .expect("intent DB");
    (copy, result)
}
#[tokio::test]
async fn grant_is_published_only_after_valid_assignment_and_durable_commit_and_is_never_reissued() {
    let o = owned().await;
    let (copy, a) = intent_fixture(&o).await;
    let svc = service(&o);
    let private = paths(&copy, &a.control).expect("private paths");
    let receipt = assigned(&a);
    assert!(!private.grant.exists());
    publish_once(&private.assigned, &receipt).expect("bootstrap assigned receipt");
    svc.monitor_attempt(copy.clone(), a.clone(), CancellationToken::new())
        .await
        .expect("first commit");
    let nonce = a.control.nonce.clone();
    let saved =
        o.f.db
            .read(move |tx| store::attempt(tx, &nonce))
            .await
            .expect("committed DB")
            .expect("attempt");
    assert_eq!(saved.phase, AttemptPhase::Committed);
    assert_eq!(saved.assigned, Some(receipt.clone()));
    assert_eq!(
        serde_json::from_slice::<ObservationAssigned>(&fs::read(&private.grant).expect("grant"))
            .expect("typed grant"),
        receipt
    );
    // A crash after DB commit but before grant publication must not get a new grant.
    fs::remove_file(&private.grant).expect("owned missing-grant crash fixture");
    svc.monitor_attempt(copy, a, CancellationToken::new())
        .await
        .expect("duplicate monitor");
    assert!(!private.grant.exists());
    svc.shutdown().await;
}
#[tokio::test]
async fn cancellation_before_assignment_closes_the_grant_and_delayed_receipt_cannot_revive_it() {
    let o = owned().await;
    let (copy, a) = intent_fixture(&o).await;
    let svc = service(&o);
    let private = paths(&copy, &a.control).expect("private paths");
    let cancel = CancellationToken::new();
    cancel.cancel();
    svc.monitor_attempt(copy.clone(), a.clone(), cancel)
        .await
        .expect("revoke pending launch");
    assert!(private.closed.exists());
    assert!(!private.grant.exists());
    publish_once(&private.assigned, &assigned(&a)).expect("late assigned receipt");
    svc.monitor_attempt(copy, a.clone(), CancellationToken::new())
        .await
        .expect("late monitor cannot grant");
    assert!(!private.grant.exists());
    let nonce = a.control.nonce.clone();
    assert_eq!(
        o.f.db
            .read(move |tx| Ok(store::attempt(tx, &nonce)?.expect("attempt").phase))
            .await
            .expect("phase"),
        AttemptPhase::Revoked
    );
    svc.shutdown().await;
}
#[tokio::test]
async fn queued_commit_rechecks_cancel_inside_the_same_serialized_writer_transaction() {
    let o = owned().await;
    let (_, a) = intent_fixture(&o).await;
    let receipt = assigned(&a);
    let cancel = CancellationToken::new();
    let stop = CancellationToken::new();
    // Models cancellation after receipt read and initial outer ownership check.
    cancel.cancel();
    let nonce = a.control.nonce.clone();
    let saved_nonce = nonce.clone();
    let (granted, _) =
        o.f.db
            .write_checked(move |tx| {
                commit_if_live(
                    tx,
                    &nonce,
                    &receipt,
                    &cancel,
                    &stop,
                    std::time::Instant::now() + Duration::from_secs(10),
                )
            })
            .await
            .expect("writer")
            .expect("valid cancellation");
    assert!(!granted);
    assert_eq!(
        o.f.db
            .read(move |tx| Ok(store::attempt(tx, &saved_nonce)?.expect("attempt").phase))
            .await
            .expect("revoked"),
        AttemptPhase::Revoked
    );
}
#[tokio::test]
async fn invalid_assigned_identity_is_revoked_without_a_grant_or_plaintext_command_receipt() {
    for field in ["nonce", "sid", "creation", "run"] {
        let o = owned().await;
        let (copy, a) = intent_fixture(&o).await;
        let svc = service(&o);
        let private = paths(&copy, &a.control).expect("private paths");
        let mut receipt = assigned(&a);
        match field {
            "nonce" => receipt.nonce = "f".repeat(32),
            "sid" => receipt.sid = "not-a-SID".into(),
            "creation" => receipt.creation_time = "0".into(),
            "run" => receipt.run_id = "other-run".into(),
            _ => unreachable!(),
        }
        publish_once(&private.assigned, &receipt).expect("invalid fixture receipt");
        assert!(
            svc.monitor_attempt(copy, a, CancellationToken::new())
                .await
                .is_err()
        );
        assert!(!private.grant.exists());
        assert!(private.closed.exists());
        let closed = fs::read_to_string(&private.closed).expect("closed identity only");
        assert!(!closed.contains("command"));
        assert!(!closed.contains("env"));
        svc.shutdown().await;
    }
}
#[tokio::test]
async fn immutable_receipt_and_grant_paths_refuse_replacement_and_source_directory_proof() {
    let o = owned().await;
    let (copy, a) = intent_fixture(&o).await;
    let private = paths(&copy, &a.control).expect("private paths");
    let receipt = assigned(&a);
    publish_once(&private.assigned, &receipt).expect("first assigned");
    assert!(
        publish_once(&private.assigned, &receipt).is_err(),
        "duplicate bootstrap must not reuse assignment"
    );
    let mut wrong = a.control.clone();
    wrong.assigned_path = format!("{}\\forged.json", copy.identity.directory());
    assert!(paths(&copy, &wrong).is_err());
    assert!(!private.assigned.starts_with(&o.copy.target));
    use std::os::unix::fs::symlink;
    let outside = o.f.root.path().join("outside");
    fs::write(&outside, b"owned sentinel").expect("sentinel");
    symlink(&outside, &private.grant).expect("owned link");
    assert!(publish_once(&private.grant, &receipt).is_err());
    assert_eq!(
        fs::read_to_string(outside).expect("unchanged"),
        "owned sentinel"
    );
}
#[tokio::test]
async fn restart_revokes_dead_precommit_owner_but_never_reissues_a_committed_grant() {
    let o = owned().await;
    o.copy.identity.ensure_owned().expect("metadata");
    let identity = o.copy.identity.clone();
    let r = reader(&o, "old-daemon", "revision");
    let mut a = attempt(&o, &r, "pending", 1);
    a.owner.pid = u32::MAX;
    let pending = a.clone();
    let receipt = assigned(&a);
    let (copy, _) =
        o.f.db
            .write(move |tx| {
                store::reserve(tx, &identity, &r)?.expect("reserve");
                store::publish(tx, &r.copy)?.expect("publish");
                store::submit(tx, &r.copy)?.expect("dispatch");
                store::intent(tx, &a)?.expect("attempt");
                Ok(store::copy(tx, &identity.key)?.expect("copy"))
            })
            .await
            .expect("persist dead owner");
    let private = paths(&copy, &pending.control).expect("private paths");
    publish_once(&private.assigned, &receipt).expect("assigned before crash");
    let restored = service(&o);
    restored
        .reconcile_windows_copies()
        .await
        .expect("restart fence");
    assert!(private.closed.exists());
    assert!(!private.grant.exists());
    let nonce = pending.control.nonce.clone();
    assert_eq!(
        o.f.db
            .read(move |tx| Ok(store::attempt(tx, &nonce)?.expect("attempt").phase))
            .await
            .expect("old pending"),
        AttemptPhase::Revoked
    );
    restored.shutdown().await;
}
#[tokio::test]
async fn missing_or_inaccessible_observation_does_not_release_a_committed_reader() {
    for state in [
        ObservationState::Unavailable,
        ObservationState::IdentityMismatch,
        ObservationState::Active { count: 2 },
    ] {
        let mut o = owned().await;
        let (copy, a) = intent_fixture(&o).await;
        let receipt = assigned(&a);
        let nonce = a.control.nonce.clone();
        let launch = a.control.launch_id.clone();
        o.f.db
            .write(move |tx| {
                store::commit(tx, &nonce, &receipt)?.expect("commit");
                store::seal(tx, &launch)?;
                Ok(())
            })
            .await
            .expect("sealed with unknown descendant");
        o.config.windows_observation = Arc::new(move |_| {
            let state = state.clone();
            Box::pin(async move { state })
        });
        let svc = service(&o);
        svc.reconcile_windows_copies()
            .await
            .expect("conservative observation");
        assert!(
            o.f.db
                .read(store::needs_reconcile)
                .await
                .expect("still held")
        );
        assert!(!paths(&copy, &a.control).expect("private").grant.exists());
        svc.shutdown().await;
    }
}
#[tokio::test]
async fn exact_not_found_releases_only_a_positively_assigned_committed_attempt() {
    let mut o = owned().await;
    let (_, a) = intent_fixture(&o).await;
    let receipt = assigned(&a);
    let nonce = a.control.nonce.clone();
    let launch = a.control.launch_id.clone();
    o.f.db
        .write(move |tx| {
            store::commit(tx, &nonce, &receipt)?.expect("commit before grant");
            store::seal(tx, &launch)?;
            Ok(())
        })
        .await
        .expect("positive assignment");
    o.config.windows_observation = Arc::new(|_| Box::pin(async { ObservationState::NotFound }));
    let svc = service(&o);
    svc.reconcile_windows_copies()
        .await
        .expect("same-session exact query fixture");
    assert!(!o.f.db.read(store::needs_reconcile).await.expect("released"));
    svc.shutdown().await;
}

#[tokio::test]
async fn commit_deadline_is_checked_when_the_db_writer_runs_not_only_before_await() {
    let o = owned().await;
    let (_, a) = intent_fixture(&o).await;
    let receipt = assigned(&a);
    let nonce = a.control.nonce.clone();
    let (grant, _) =
        o.f.db
            .write_checked(move |tx| {
                commit_if_live(
                    tx,
                    &nonce,
                    &receipt,
                    &CancellationToken::new(),
                    &CancellationToken::new(),
                    std::time::Instant::now() - Duration::from_millis(1),
                )
            })
            .await
            .expect("writer")
            .expect("expired admission");
    assert!(!grant);
    let nonce = a.control.nonce;
    assert_eq!(
        o.f.db
            .read(move |tx| Ok(store::attempt(tx, &nonce)?.expect("attempt").phase))
            .await
            .expect("revoked"),
        AttemptPhase::Revoked
    );
}
