use super::*;
use crate::{
    exit_record::ExitStore,
    run_store::{self, StoredRun},
    windows::WindowsDefinition,
    windows_copy_store::{self as store, Attempt},
    windows_copy_test_support::*,
    *,
};
use std::sync::Mutex;
use tokio::sync::Notify;

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Intent,
    Committed,
    Revoked,
    Closed,
}
type Calls = Arc<Mutex<Vec<(ObservationAssigned, Vec<u16>)>>>;
struct Case {
    _owned: Owned,
    svc: Arc<RuntimeService>,
    id: RunIdentity,
    attempt: Attempt,
    calls: Calls,
}
fn answering(state: ListenerObservation, calls: &Calls) -> WindowsListeners {
    let calls = calls.clone();
    Arc::new(move |assigned, ports| {
        calls.lock().expect("calls").push((assigned, ports));
        Box::pin(async move { state })
    })
}
async fn case(phase: Phase, listeners: impl FnOnce(&Calls) -> WindowsListeners) -> Case {
    let mut o = owned().await;
    let calls = Calls::default();
    o.config.windows_listeners = listeners(&calls);
    o.copy.identity.ensure_owned().expect("owned metadata");
    let identity = o.copy.identity.clone();
    let r = reader(&o, "logical", "revision");
    let a = attempt(&o, &r, "attempt", 1);
    let receipt = assigned(&a);
    let (reference, nonce, intent) = (r.copy.clone(), a.control.nonce.clone(), a.clone());
    o.f.db
        .write(move |tx| {
            store::reserve(tx, &identity, &r)?.expect("reserve");
            store::publish(tx, &r.copy)?.expect("publish");
            store::submit(tx, &r.copy)?.expect("submit");
            store::intent(tx, &intent)?.expect("intent");
            match phase {
                Phase::Intent => {}
                Phase::Revoked => assert!(store::revoke(tx, &nonce)?),
                Phase::Committed | Phase::Closed => {
                    assert!(store::commit(tx, &nonce, &receipt)?.expect("commit"));
                    if phase == Phase::Closed {
                        store::observed_closed(tx, &nonce, &receipt)?.expect("closed");
                    }
                }
            }
            Ok(())
        })
        .await
        .expect("attempt DB");
    let id = RunIdentity {
        run_id: a.run_id.clone(),
        attempt_id: a.attempt_id.clone(),
    };
    let value = StoredRun {
        source_cwd: None,
        secret_definition: None,
        windows: Some(WindowsDefinition {
            cwd: o.copy.cwd.clone(),
            def: RunDef::Service(ServiceDef {
                id: "web".into(),
                command: "npm start".into(),
                target: RunTarget::Windows,
                ports_hint: vec![3000, 3001, 3000],
                ..Default::default()
            }),
            copy: Some(reference),
        }),
        run: Run {
            id: id.run_id.clone(),
            project_id: o.f.project.clone(),
            def_id: "web".into(),
            kind: RunKind::Service,
            state: RunState::Running,
            exit: None,
            started_ms: 1,
            ended_ms: None,
            restarts: 0,
            unit: "devbox-test-copy-run-logical.service".into(),
            ready: false,
            pid: None,
            port: None,
            attempt_id: Some(id.attempt_id.clone()),
            windows: None,
        },
        schedule: None,
        restart: RestartPolicy::No,
        start_limit: None,
        worktree: None,
        start_limit_notified: false,
    };
    o.f.db
        .write(move |tx| run_store::put(tx, &value))
        .await
        .expect("stored run");
    ExitStore::new(&o.config.paths.data_dir.join("runs"), &id.run_id)
        .expect("exit store")
        .begin(&id.attempt_id)
        .expect("current attempt");
    let svc = service(&o);
    Case {
        _owned: o,
        svc,
        id,
        attempt: a,
        calls,
    }
}
async fn ready(c: &Case) -> bool {
    c.svc.stored(&c.id.run_id).await.expect("run").run.ready
}

#[tokio::test]
async fn unrelated_listener_on_the_port_never_marks_the_windows_service_ready() {
    let c = case(Phase::Committed, |calls| {
        answering(ListenerObservation::NotOwned, calls)
    })
    .await;
    let result = c.svc.windows_ready_gate(&c.id).await;
    assert!(
        matches!(result, Ok(WindowsReadiness::NotReady)),
        "an unrelated listener is a typed not-ready result"
    );
    assert!(
        !ready(&c).await,
        "an unrelated listener must not mark ready"
    );
    c.svc.shutdown().await;
}

#[tokio::test]
async fn owned_listener_marks_ready_from_the_durable_receipt_and_stored_ports() {
    let c = case(Phase::Committed, |calls| {
        answering(ListenerObservation::Owned, calls)
    })
    .await;
    assert_eq!(
        c.svc.windows_ready_gate(&c.id).await.expect("gate"),
        WindowsReadiness::Ready
    );
    assert!(ready(&c).await);
    assert_eq!(
        *c.calls.lock().expect("calls"),
        vec![(assigned(&c.attempt), vec![3000, 3001])],
        "receipt comes from the store and ports from the stored definition"
    );
    // Ready is sticky: a later call short-circuits without another probe.
    assert_eq!(
        c.svc.windows_ready_gate(&c.id).await.expect("sticky"),
        WindowsReadiness::Ready
    );
    assert_eq!(c.calls.lock().expect("calls").len(), 1);
    c.svc.shutdown().await;
}

#[tokio::test]
async fn unknown_or_absent_owned_listeners_remain_not_ready() {
    for state in [
        ListenerObservation::Unavailable,
        ListenerObservation::IdentityMismatch,
        ListenerObservation::Empty,
        ListenerObservation::NotFound,
    ] {
        let c = case(Phase::Committed, |calls| answering(state, calls)).await;
        assert_eq!(
            c.svc.windows_ready_gate(&c.id).await.expect("typed"),
            WindowsReadiness::NotReady,
            "{state:?}"
        );
        assert!(!ready(&c).await, "{state:?}");
        c.svc.shutdown().await;
    }
}

#[tokio::test]
async fn missing_revoked_closed_or_replaced_attempt_never_probes_or_marks_ready() {
    for phase in [Phase::Intent, Phase::Revoked, Phase::Closed] {
        let c = case(phase, |calls| answering(ListenerObservation::Owned, calls)).await;
        let result = c.svc.windows_ready_gate(&c.id).await;
        assert!(
            !matches!(result, Ok(WindowsReadiness::Ready)),
            "uncommitted or finished attempt"
        );
        if phase == Phase::Intent {
            assert_eq!(result.expect("typed"), WindowsReadiness::NotReady);
        }
        assert!(!ready(&c).await);
        assert!(c.calls.lock().expect("calls").is_empty());
        c.svc.shutdown().await;
    }
    let c = case(Phase::Committed, |calls| {
        answering(ListenerObservation::Owned, calls)
    })
    .await;
    let mut other = c.id.clone();
    other.attempt_id = "replacement".into();
    assert!(c.svc.windows_ready_gate(&other).await.is_err());
    assert!(!ready(&c).await);
    assert!(c.calls.lock().expect("calls").is_empty());
    c.svc.shutdown().await;
}

#[tokio::test]
async fn probe_runs_without_the_run_lock_and_a_restart_during_it_wins() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (inside, gate) = (entered.clone(), release.clone());
    let c = case(Phase::Committed, move |_| {
        Arc::new(move |_, _| {
            let (inside, gate) = (inside.clone(), gate.clone());
            Box::pin(async move {
                inside.notify_one();
                gate.notified().await;
                ListenerObservation::Owned
            })
        })
    })
    .await;
    let (svc, id) = (c.svc.clone(), c.id.clone());
    let task = tokio::spawn(async move { svc.windows_ready_gate(&id).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .expect("gate probes listeners");
    {
        let _guard = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            c.svc.run_lock(&c.id.run_id),
        )
        .await
        .expect("stop/restart can take the run lock while the probe blocks");
        let mut value = c.svc.stored(&c.id.run_id).await.expect("run");
        value.run.attempt_id = Some("replacement".into());
        c.svc.save_run(&value).await.expect("restart");
    }
    release.notify_one();
    let result = task.await.expect("gate task");
    assert!(!matches!(result, Ok(WindowsReadiness::Ready)));
    assert!(!ready(&c).await, "stale probe cannot mark the replacement");
    c.svc.shutdown().await;
}

#[tokio::test]
async fn no_port_windows_service_is_never_probed_or_authorized_by_the_gate() {
    let c = case(Phase::Committed, |calls| {
        answering(ListenerObservation::Owned, calls)
    })
    .await;
    let mut value = c.svc.stored(&c.id.run_id).await.expect("run");
    if let Some(RunDef::Service(def)) = value.windows.as_mut().map(|w| &mut w.def) {
        def.ports_hint.clear();
    }
    c.svc.save_run(&value).await.expect("no-port definition");
    assert_eq!(
        c.svc.windows_ready_gate(&c.id).await.expect("typed"),
        WindowsReadiness::NotReady
    );
    assert!(!ready(&c).await);
    assert!(c.calls.lock().expect("calls").is_empty());
    c.svc.shutdown().await;
}
