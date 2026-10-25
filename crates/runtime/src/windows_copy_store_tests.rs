use crate::{RuntimeError, windows_copy_store as store, windows_copy_test_support::*};
use devbox_projects::{ProjectsError, RemoveProject};
use devbox_protocol::Fail;

#[tokio::test]
async fn compatible_service_and_dependent_readers_share_one_revision_until_last_confirmed_exit() {
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let a = reader(&o, "service", "rev1");
    let b = reader(&o, "dependent", "rev1");
    let fresh = reader(&o, "fresh", "rev2");
    let aa = attempt(&o, &a, "attempt-a", 1);
    let ab = attempt(&o, &b, "attempt-b", 2);
    let ra = assigned(&aa);
    let rb = assigned(&ab);
    o.f.db
        .write(move |tx| {
            assert_eq!(
                store::reserve(tx, &identity, &a)?.expect("admitted transition"),
                store::Admission::Writer
            );
            store::publish(tx, &a.copy)?.expect("admitted transition");
            store::submit(tx, &a.copy)?.expect("admitted transition");
            store::launched(tx, &a.copy)?.expect("admitted transition");
            store::intent(tx, &aa)?.expect("admitted transition");
            assert!(store::commit(tx, &aa.control.nonce, &ra)?.expect("admitted transition"));
            assert_eq!(
                store::reserve(tx, &identity, &b)?.expect("admitted transition"),
                store::Admission::Shared
            );
            store::submit(tx, &b.copy)?.expect("admitted transition");
            store::launched(tx, &b.copy)?.expect("admitted transition");
            store::intent(tx, &ab)?.expect("admitted transition");
            store::commit(tx, &ab.control.nonce, &rb)?.expect("admitted transition");
            assert!(matches!(
                store::reserve(tx, &identity, &fresh)?,
                Err(RuntimeError::WindowsCopyBusy)
            ));
            store::seal(tx, &a.copy.launch_id)?;
            store::observed_closed(tx, &aa.control.nonce, &ra)?.expect("admitted transition");
            assert_eq!(
                store::reader(tx, &a.copy.launch_id)?
                    .expect("service reader")
                    .state,
                store::ReaderState::Closed
            );
            assert!(
                matches!(
                    store::reserve(tx, &identity, &fresh)?,
                    Err(RuntimeError::WindowsCopyBusy)
                ),
                "one completion cannot release dependent"
            );
            store::seal(tx, &b.copy.launch_id)?;
            store::observed_closed(tx, &ab.control.nonce, &rb)?.expect("admitted transition");
            assert_eq!(
                store::reserve(tx, &identity, &fresh)?.expect("admitted transition"),
                store::Admission::Writer
            );
            Ok(())
        })
        .await
        .expect("ledger transitions");
}
#[tokio::test]
async fn service_restart_gap_and_older_unknown_attempt_keep_the_logical_reader() {
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let r = reader(&o, "service", "rev1");
    let other = reader(&o, "changed", "rev2");
    let first = attempt(&o, &r, "first", 1);
    let second = attempt(&o, &r, "second", 2);
    let r1 = assigned(&first);
    let r2 = assigned(&second);
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("admitted transition");
            store::publish(tx, &r.copy)?.expect("admitted transition");
            store::submit(tx, &r.copy)?.expect("admitted transition");
            store::launched(tx, &r.copy)?.expect("admitted transition");
            store::intent(tx, &first)?.expect("admitted transition");
            store::commit(tx, &first.control.nonce, &r1)?.expect("admitted transition");
            store::observed_closed(tx, &first.control.nonce, &r1)?.expect("admitted transition");
            assert_eq!(
                store::reader(tx, &r.copy.launch_id)?.expect("reader").state,
                store::ReaderState::Submitted,
                "no auto-restart write gap"
            );
            assert!(matches!(
                store::reserve(tx, &identity, &other)?,
                Err(RuntimeError::WindowsCopyBusy)
            ));
            store::intent(tx, &second)?.expect("admitted transition");
            store::commit(tx, &second.control.nonce, &r2)?.expect("admitted transition");
            store::seal(tx, &r.copy.launch_id)?;
            assert!(
                matches!(
                    store::reserve(tx, &identity, &other)?,
                    Err(RuntimeError::WindowsCopyBusy)
                ),
                "terminal presentation does not prove child gone"
            );
            store::observed_closed(tx, &second.control.nonce, &r2)?.expect("admitted transition");
            assert_eq!(
                store::reserve(tx, &identity, &other)?.expect("admitted transition"),
                store::Admission::Writer
            );
            Ok(())
        })
        .await
        .expect("service lifetime");
}
#[tokio::test]
async fn a_newer_attempt_exit_cannot_retire_an_older_unknown_job() {
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let r = reader(&o, "service", "rev1");
    let first = attempt(&o, &r, "first", 1);
    let second = attempt(&o, &r, "second", 2);
    let r1 = assigned(&first);
    let r2 = assigned(&second);
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("admitted transition");
            store::publish(tx, &r.copy)?.expect("admitted transition");
            store::submit(tx, &r.copy)?.expect("admitted transition");
            store::launched(tx, &r.copy)?.expect("admitted transition");
            for (a, receipt) in [(&first, &r1), (&second, &r2)] {
                store::intent(tx, a)?.expect("admitted transition");
                store::commit(tx, &a.control.nonce, receipt)?.expect("admitted transition");
            }
            store::seal(tx, &r.copy.launch_id)?;
            store::observed_closed(tx, &second.control.nonce, &r2)?.expect("admitted transition");
            assert_eq!(
                store::reader(tx, &r.copy.launch_id)?.expect("reader").state,
                store::ReaderState::Sealed
            );
            assert_eq!(
                store::attempt(tx, &first.control.nonce)?
                    .expect("older")
                    .phase,
                store::AttemptPhase::Committed
            );
            store::observed_closed(tx, &first.control.nonce, &r1)?.expect("admitted transition");
            assert_eq!(
                store::reader(tx, &r.copy.launch_id)?.expect("reader").state,
                store::ReaderState::Closed
            );
            Ok(())
        })
        .await
        .expect("separate attempts");
}
#[tokio::test]
async fn pending_submission_cannot_be_confused_with_a_reused_units_old_inactive_state() {
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let mut r = reader(&o, "restart", "rev1");
    r.previous_attempt = Some("old-attempt".into());
    let old = attempt(&o, &r, "old-attempt", 1);
    let new = attempt(&o, &r, "new-attempt", 2);
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("admitted transition");
            store::publish(tx, &r.copy)?.expect("admitted transition");
            store::submit(tx, &r.copy)?.expect("admitted transition");
            let pending = store::reader(tx, &r.copy.launch_id)?.expect("pending submission");
            assert_eq!(pending.state, store::ReaderState::Dispatching);
            // Exercise the production passive-seal predicate against reused old unit observations.
            let mut unit = devbox_core::systemd::UnitState {
                load_state: "loaded".into(),
                active_state: "inactive".into(),
                sub_state: "dead".into(),
                result: "success".into(),
                main_pid: None,
                invocation_id: Some("old-attempt".into()),
                n_restarts: 0,
            };
            for active in ["inactive", "failed"] {
                unit.active_state = active.into();
                assert!(!crate::windows_observation::launch_sealed(
                    pending.state,
                    &unit,
                    false
                ));
                assert!(crate::windows_observation::launch_sealed(
                    store::ReaderState::Submitted,
                    &unit,
                    true
                ));
            }
            unit.load_state = "not-found".into();
            assert!(!crate::windows_observation::launch_sealed(
                pending.state,
                &unit,
                false
            ));
            assert!(!crate::windows_observation::launch_sealed(
                store::ReaderState::Submitted,
                &unit,
                false
            ));
            unit.load_state = "loaded".into();
            unit.active_state = "activating".into();
            unit.sub_state = "auto-restart".into();
            assert!(!crate::windows_observation::launch_sealed(
                store::ReaderState::Submitted,
                &unit,
                true
            ));
            assert!(
                store::intent(tx, &old)?.is_err(),
                "old wrapper cannot bind new logical launch"
            );
            store::intent(tx, &new)?.expect("admitted transition");
            assert_eq!(
                store::reader(tx, &r.copy.launch_id)?
                    .expect("new wrapper")
                    .state,
                store::ReaderState::Submitted
            );
            Ok(())
        })
        .await
        .expect("dispatch fence");
}
#[tokio::test]
async fn committed_grant_is_one_shot_and_cancellation_cannot_release_its_consumer() {
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let r = reader(&o, "run", "rev1");
    let a = attempt(&o, &r, "attempt", 1);
    let receipt = assigned(&a);
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("admitted transition");
            store::publish(tx, &r.copy)?.expect("admitted transition");
            store::submit(tx, &r.copy)?.expect("admitted transition");
            store::intent(tx, &a)?.expect("admitted transition");
            assert!(store::commit(tx, &a.control.nonce, &receipt)?.expect("admitted transition"));
            assert!(!store::commit(tx, &a.control.nonce, &receipt)?.expect("admitted transition"));
            assert!(!store::revoke(tx, &a.control.nonce)?);
            store::seal(tx, &r.copy.launch_id)?;
            assert_eq!(
                store::reader(tx, &r.copy.launch_id)?.expect("held").state,
                store::ReaderState::Sealed
            );
            let mut wrong = receipt.clone();
            wrong.creation_time.push('1');
            assert!(store::observed_closed(tx, &a.control.nonce, &wrong)?.is_err());
            store::observed_closed(tx, &a.control.nonce, &receipt)?.expect("admitted transition");
            Ok(())
        })
        .await
        .expect("one-shot grant");
}
#[tokio::test]
async fn precommit_cancel_fences_delayed_assignment_and_explicit_stop_seals_new_attempt_admission()
{
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let r = reader(&o, "run", "rev1");
    let a = attempt(&o, &r, "attempt", 1);
    let next = attempt(&o, &r, "later", 2);
    let receipt = assigned(&a);
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("admitted transition");
            store::publish(tx, &r.copy)?.expect("admitted transition");
            store::submit(tx, &r.copy)?.expect("admitted transition");
            store::intent(tx, &a)?.expect("admitted transition");
            assert!(store::revoke(tx, &a.control.nonce)?);
            assert!(!store::commit(tx, &a.control.nonce, &receipt)?.expect("admitted transition"));
            store::seal(tx, &r.copy.launch_id)?;
            assert!(store::intent(tx, &next)?.is_err());
            assert_eq!(
                store::reader(tx, &r.copy.launch_id)?.expect("closed").state,
                store::ReaderState::Closed
            );
            Ok(())
        })
        .await
        .expect("closed grant");
}
#[tokio::test]
async fn terminal_presentation_cannot_delete_a_project_until_every_copy_consumer_is_closed() {
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let r = reader(&o, "run", "rev1");
    let a = attempt(&o, &r, "attempt", 1);
    let receipt = assigned(&a);
    let launch = r.copy.launch_id.clone();
    let nonce = a.control.nonce.clone();
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("admitted transition");
            store::publish(tx, &r.copy)?.expect("admitted transition");
            store::submit(tx, &r.copy)?.expect("admitted transition");
            store::intent(tx, &a)?.expect("admitted transition");
            store::commit(tx, &a.control.nonce, &receipt)?.expect("admitted transition");
            store::seal(tx, &r.copy.launch_id)?;
            Ok(())
        })
        .await
        .expect("owned live child");
    assert!(matches!(
        o.f.projects
            .remove(RemoveProject {
                id: o.f.project.clone()
            })
            .await,
        Err(Fail::Domain(ProjectsError::InUse { .. }))
    ));
    // There is deliberately no live runtime_runs presentation row.
    o.f.db
        .write(move |tx| {
            let a = store::attempt(tx, &nonce)?.expect("persistent attempt");
            store::observed_closed(tx, &nonce, a.assigned.as_ref().expect("assigned"))?
                .expect("admitted transition");
            assert_eq!(
                store::reader(tx, &launch)?.expect("reader").state,
                store::ReaderState::Closed
            );
            Ok(())
        })
        .await
        .expect("late confirmed exit");
    o.f.projects
        .remove(RemoveProject {
            id: o.f.project.clone(),
        })
        .await
        .expect("deletion after proof");
}
#[tokio::test]
async fn reader_reservation_and_project_delete_are_fenced_by_the_same_writer() {
    let o = owned().await;
    let identity = o.copy.identity.clone();
    let r = reader(&o, "race", "rev1");
    let db = o.f.db.clone();
    let project = o.f.project.clone();
    let (reserve, remove) = tokio::join!(
        db.write_checked(move |tx| store::reserve(tx, &identity, &r)),
        o.f.projects.remove(RemoveProject { id: project })
    );
    match (reserve.expect("writer transaction"), remove) {
        (Ok(_), Err(Fail::Domain(ProjectsError::InUse { .. }))) => (),
        (Err(RuntimeError::ProjectNotFound { .. }), Ok(_)) => (),
        other => panic!("reservation/deletion must have one safe winner: {other:?}"),
    }
}
