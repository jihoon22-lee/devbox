use crate::{
    scheduler::{Scheduler, test_scheduler},
    service_tests::Fixture,
    *,
};
use devbox_core::{
    HubMessage,
    notify::{Notification, NotificationReason},
};
#[path = "scheduler_retry_tests.rs"]
mod retry;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, Ordering},
};
const BASE: i64 = 1_800_000_000_000; // exact minute boundary
struct Harness {
    f: Fixture,
    now: Arc<AtomicI64>,
    started: Arc<Mutex<Vec<Vec<RunDef>>>>,
    scheduler: Scheduler,
}
impl Harness {
    async fn new(policy: MissedPolicy) -> Self {
        let f = Fixture::new().await;
        f.define("task", "true", SaveTo::ThisPc).await;
        define_schedule(&f, "* * * * *", policy).await;
        let now = Arc::new(AtomicI64::new(BASE));
        let started = Arc::new(Mutex::new(vec![]));
        let scheduler = make_scheduler(&f, now.clone(), started.clone());
        Self {
            f,
            now,
            started,
            scheduler,
        }
    }
    fn advance(&self, minutes: i64) {
        self.now.store(BASE + minutes * 60000, Ordering::SeqCst);
    }
    fn count(&self) -> usize {
        self.started.lock().expect("fixture result").len()
    }
    async fn outcome(&self) -> Option<ScheduleOutcome> {
        self.f
            .runs
            .schedules(SchedulesQuery {
                project_id: self.f.project.clone(),
            })
            .await
            .expect("fixture result")
            .items[0]
            .last
            .as_ref()
            .map(|l| l.result)
    }
}
fn make_scheduler(
    f: &Fixture,
    now: Arc<AtomicI64>,
    started: Arc<Mutex<Vec<Vec<RunDef>>>>,
) -> Scheduler {
    test_scheduler(
        f.runs.clone(),
        Arc::new(move || now.load(Ordering::SeqCst)),
        Arc::new(move |graph| {
            started
                .lock()
                .expect("fixture result")
                .push(graph.nodes.clone());
            Box::pin(async move { Ok(fake_run(&graph, RunState::Running)) })
        }),
    )
}
fn fake_run(graph: &PreparedGraph, state: RunState) -> Run {
    Run {
        id: uuid::Uuid::new_v4().simple().to_string(),
        project_id: graph.project_id.clone(),
        def_id: graph.nodes.last().expect("fixture value").id().into(),
        kind: RunKind::Scheduled,
        state,
        exit: None,
        started_ms: BASE as u64,
        ended_ms: None,
        restarts: 0,
        unit: "owned-fake".into(),
        ready: false,
        pid: None,
        port: None,
        attempt_id: None,
        windows: None,
    }
}
async fn define_schedule(f: &Fixture, cron: &str, missed: MissedPolicy) {
    f.runs
        .define(DefineRun {
            project_id: f.project.clone(),
            save_to: SaveTo::ThisPc,
            def: RunDef::Schedule(ScheduleDef {
                id: "daily".into(),
                task: "task".into(),
                cron: cron.into(),
                tz: Some("UTC".into()),
                missed,
            }),
        })
        .await
        .expect("fixture result");
}
async fn approve(f: &Fixture) {
    use devbox_projects::*;
    let preview = f
        .projects
        .trust_preview(
            TrustPreviewParams {
                id: f.project.clone(),
                operation: TrustOperation::Run,
                agent_id: None,
                worktrees: vec![],
                worktree: None,
            },
            true,
            vec![],
        )
        .await
        .expect("fixture result");
    f.projects
        .trust_approve(TrustApprove {
            id: f.project.clone(),
            expected_canonical_path: preview.canonical_path,
            expected_digest: preview.digest.expect("fixture value"),
            expected_scope_version: preview.scope_version,
        })
        .await
        .expect("fixture result");
}
async fn notification(events: &mut devbox_core::HubReceiver) -> Notification {
    let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .expect("fixture result")
        .expect("fixture value");
    let HubMessage::Event { payload, .. } = event else {
        panic!("resync")
    };
    serde_json::from_str(&payload).expect("fixture result")
}
#[tokio::test]
async fn scheduler_new_definitions_begin_at_current_time() {
    let h = Harness::new(MissedPolicy::RunOnce).await;
    assert_eq!(
        h.scheduler.tick(true).await.expect("fixture result"),
        Some(BASE + 60000)
    );
    assert_eq!(h.count(), 0);
    assert_eq!(h.outcome().await, None);
}
#[tokio::test]
async fn scheduler_missed_run_once_fires_exactly_once_after_downtime() {
    let h = Harness::new(MissedPolicy::RunOnce).await;
    h.scheduler.tick(true).await.expect("fixture result");
    h.advance(100);
    h.scheduler.tick(true).await.expect("fixture result");
    h.scheduler.drain().await;
    h.scheduler.tick(false).await.expect("fixture result");
    let restarted = make_scheduler(&h.f, h.now.clone(), h.started.clone());
    restarted.tick(true).await.expect("fixture result");
    restarted.drain().await;
    assert_eq!(h.count(), 1);
    assert_eq!(h.outcome().await, Some(ScheduleOutcome::Running));
}
#[tokio::test]
async fn scheduler_missed_skip_records_without_running() {
    let h = Harness::new(MissedPolicy::Skip).await;
    h.scheduler.tick(true).await.expect("fixture result");
    h.advance(100);
    h.scheduler.tick(true).await.expect("fixture result");
    h.scheduler.drain().await;
    assert_eq!(h.count(), 0);
    assert_eq!(h.outcome().await, Some(ScheduleOutcome::SkippedMissed));
    h.advance(101);
    h.scheduler.tick(false).await.expect("fixture result");
    h.scheduler.drain().await;
    assert_eq!(h.count(), 1);
}
#[tokio::test]
async fn scheduler_recomputes_when_definitions_change() {
    let h = Harness::new(MissedPolicy::Skip).await;
    assert_eq!(
        h.scheduler.tick(true).await.expect("fixture result"),
        Some(BASE + 60000)
    );
    define_schedule(&h.f, "*/5 * * * *", MissedPolicy::Skip).await;
    assert_eq!(
        h.scheduler.tick(false).await.expect("fixture result"),
        Some(BASE + 300000)
    );
    h.advance(1);
    h.scheduler.tick(false).await.expect("fixture result");
    assert_eq!(h.count(), 0);
}
#[tokio::test]
async fn scheduler_stale_trust_skips_two_ticks_notifies_once_and_reapproval_runs_next() {
    let h = Harness::new(MissedPolicy::RunOnce).await;
    h.f.write("[[task]]\nid='task'\ncommand='echo repository'\n");
    let mut events = h.f.hub.subscribe("notify.raised");
    h.scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    h.scheduler.tick(false).await.expect("fixture result");
    h.advance(2);
    h.scheduler.tick(false).await.expect("fixture result");
    assert_eq!(h.count(), 0);
    assert_eq!(h.outcome().await, Some(ScheduleOutcome::SkippedTrust));
    let alert = notification(&mut events).await;
    assert!(matches!(
        alert.reason,
        NotificationReason::ScheduleTrustRequired
    ));
    assert!(alert.run_id.is_none());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), events.recv())
            .await
            .is_err()
    );
    approve(&h.f).await;
    h.scheduler.tick(false).await.expect("fixture result");
    h.advance(3);
    h.scheduler.tick(false).await.expect("fixture result");
    h.scheduler.drain().await;
    assert_eq!(h.count(), 1);
    h.f.write("[[task]]\nid='task'\ncommand='echo changed'\n");
    h.advance(4);
    h.scheduler.tick(false).await.expect("fixture result");
    assert!(matches!(
        notification(&mut events).await.reason,
        NotificationReason::ScheduleTrustRequired
    ));
}
#[tokio::test]
async fn scheduler_project_schedule_requires_trust_even_for_this_pc_task() {
    let h = Harness::new(MissedPolicy::Skip).await;
    h.f.write("[[schedule]]\nid='daily'\ntask='task'\ncron='* * * * *'\ntz='UTC'\n");
    h.scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    h.scheduler.tick(false).await.expect("fixture result");
    assert_eq!(h.outcome().await, Some(ScheduleOutcome::SkippedTrust));
    assert_eq!(h.count(), 0);
    approve(&h.f).await;
    h.advance(2);
    h.scheduler.tick(false).await.expect("fixture result");
    h.scheduler.drain().await;
    assert_eq!(h.count(), 1);
}
#[tokio::test]
async fn scheduler_all_this_pc_graph_does_not_require_unrelated_project_trust() {
    let h = Harness::new(MissedPolicy::Skip).await;
    h.f.write("[[task]]\nid='unrelated'\ncommand='echo unapproved'\n");
    h.scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    h.scheduler.tick(false).await.expect("fixture result");
    h.scheduler.drain().await;
    assert_eq!(h.count(), 1);
}
#[tokio::test]
async fn scheduler_authorized_graph_keeps_exact_bytes_when_file_changes() {
    let h = Harness::new(MissedPolicy::Skip).await;
    h.f.write("[[task]]\nid='task'\ncommand='echo approved'\n");
    approve(&h.f).await;
    let definition = Definition {
        source: DefinitionSource::ThisPc,
        def: RunDef::Schedule(ScheduleDef {
            id: "daily".into(),
            task: "task".into(),
            cron: "* * * * *".into(),
            tz: Some("UTC".into()),
            missed: MissedPolicy::Skip,
        }),
    };
    let graph =
        h.f.runs
            .authorize_schedule(&h.f.project, &definition)
            .await
            .expect("fixture result");
    h.f.write("[[task]]\nid='task'\ncommand='echo replaced'\n");
    assert_eq!(graph.nodes[0].command(), Some("echo approved"));
}
#[tokio::test]
async fn scheduler_terminal_result_survives_late_running_worker_and_notifies_once() {
    let h = Harness::new(MissedPolicy::Skip).await;
    h.scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    h.scheduler.tick(false).await.expect("fixture result");
    h.scheduler.drain().await;
    let claim = crate::schedule_model::ScheduleClaim {
        project_id: h.f.project.clone(),
        schedule_id: "daily".into(),
        task: "task".into(),
        at: BASE + 60000,
    };
    let mut alerts = h.f.hub.subscribe("notify.raised");
    h.f.runs
        .finish_schedule(
            claim.clone(),
            ScheduleOutcome::Failed,
            Some("actual-run".into()),
        )
        .await
        .expect("fixture result");
    h.f.runs
        .finish_schedule(
            claim.clone(),
            ScheduleOutcome::Running,
            Some("actual-run".into()),
        )
        .await
        .expect("fixture result");
    h.f.runs
        .finish_schedule(claim, ScheduleOutcome::Failed, None)
        .await
        .expect("fixture result");
    assert_eq!(h.outcome().await, Some(ScheduleOutcome::Failed));
    let alert = notification(&mut alerts).await;
    assert!(matches!(alert.reason, NotificationReason::ScheduleFailed));
    assert_eq!(alert.run_id.as_deref(), Some("actual-run"));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), alerts.recv())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn scheduler_start_error_raises_failure_without_fabricated_run_id() {
    let h = Harness::new(MissedPolicy::Skip).await;
    let now = h.now.clone();
    let scheduler = test_scheduler(
        h.f.runs.clone(),
        Arc::new(move || now.load(Ordering::SeqCst)),
        Arc::new(|_| {
            Box::pin(async { Err(devbox_protocol::Fail::Internal("fake failure".into())) })
        }),
    );
    let mut alerts = h.f.hub.subscribe("notify.raised");
    scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    scheduler.tick(false).await.expect("fixture result");
    scheduler.drain().await;
    let alert = notification(&mut alerts).await;
    assert!(matches!(alert.reason, NotificationReason::ScheduleFailed));
    assert!(alert.run_id.is_none());
}

#[tokio::test]
async fn scheduler_readded_definition_gets_new_baseline() {
    let h = Harness::new(MissedPolicy::RunOnce).await;
    h.scheduler.tick(true).await.expect("fixture result");
    h.f.runs
        .undefine(UndefineRun {
            project_id: h.f.project.clone(),
            id: "daily".into(),
        })
        .await
        .expect("fixture result");
    h.advance(10);
    define_schedule(&h.f, "* * * * *", MissedPolicy::RunOnce).await;
    assert_eq!(
        h.scheduler.tick(true).await.expect("fixture result"),
        Some(BASE + 660000)
    );
    h.scheduler.drain().await;
    assert_eq!(
        h.count(),
        0,
        "readded schedule must not replay the removed definition's missed occurrences"
    );
}

#[tokio::test]
async fn scheduler_slow_graph_does_not_block_other_schedule_or_shutdown() {
    use std::sync::atomic::AtomicUsize;
    let h = Harness::new(MissedPolicy::Skip).await;
    h.f.runs
        .define(DefineRun {
            project_id: h.f.project.clone(),
            save_to: SaveTo::ThisPc,
            def: RunDef::Schedule(ScheduleDef {
                id: "second".into(),
                task: "task".into(),
                cron: "* * * * *".into(),
                tz: Some("UTC".into()),
                missed: MissedPolicy::Skip,
            }),
        })
        .await
        .expect("fixture result");
    let now = h.now.clone();
    let pending = Arc::new(AtomicUsize::new(0));
    let complete = Arc::new(AtomicUsize::new(0));
    let active = pending.clone();
    let done = complete.clone();
    struct Pending(Arc<AtomicUsize>);
    impl Drop for Pending {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let scheduler = test_scheduler(
        h.f.runs.clone(),
        Arc::new(move || now.load(Ordering::SeqCst)),
        Arc::new(move |graph| {
            let active = active.clone();
            let done = done.clone();
            Box::pin(async move {
                if graph.schedule.as_ref().expect("fixture value").schedule_id == "daily" {
                    active.fetch_add(1, Ordering::SeqCst);
                    let _pending = Pending(active);
                    std::future::pending::<()>().await;
                }
                done.fetch_add(1, Ordering::SeqCst);
                Ok(fake_run(&graph, RunState::Running))
            })
        }),
    );
    scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    tokio::time::timeout(std::time::Duration::from_secs(1), scheduler.tick(false))
        .await
        .expect("fixture result")
        .expect("fixture result");
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while pending.load(Ordering::SeqCst) != 1 || complete.load(Ordering::SeqCst) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fixture result");
    scheduler.stop().await;
    assert_eq!(pending.load(Ordering::SeqCst), 0);
    assert_eq!(
        complete.load(Ordering::SeqCst),
        1,
        "pending graph must not start remaining nodes after shutdown"
    );
}

#[tokio::test]
async fn scheduler_reapproval_then_edit_before_event_consumption_resets_alert() {
    let h = Harness::new(MissedPolicy::Skip).await;
    h.f.write("[[task]]\nid='task'\ncommand='true'\n");
    let mut alerts = h.f.hub.subscribe("notify.raised");
    h.scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    h.scheduler.tick(false).await.expect("fixture result");
    notification(&mut alerts).await;
    approve(&h.f).await;
    // Both project events are coalesced before the scheduler observes status.
    h.f.write("[[task]]\nid='task'\ncommand='echo changed again'\n");
    h.advance(2);
    h.scheduler.tick(false).await.expect("fixture result");
    assert!(matches!(
        notification(&mut alerts).await.reason,
        NotificationReason::ScheduleTrustRequired
    ));
    assert_eq!(h.count(), 0);
}

#[tokio::test]
async fn scheduler_stopped_worker_result_is_terminal_without_failure_alert() {
    let h = Harness::new(MissedPolicy::Skip).await;
    let now = h.now.clone();
    let scheduler = test_scheduler(
        h.f.runs.clone(),
        Arc::new(move || now.load(Ordering::SeqCst)),
        Arc::new(|graph| Box::pin(async move { Ok(fake_run(&graph, RunState::Stopped)) })),
    );
    let mut alerts = h.f.hub.subscribe("notify.raised");
    scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    scheduler.tick(false).await.expect("fixture result");
    scheduler.drain().await;
    assert_eq!(h.outcome().await, Some(ScheduleOutcome::Stopped));
    let claim = crate::schedule_model::ScheduleClaim {
        project_id: h.f.project.clone(),
        schedule_id: "daily".into(),
        task: "task".into(),
        at: BASE + 60000,
    };
    for late in [ScheduleOutcome::Running, ScheduleOutcome::Failed] {
        h.f.runs
            .finish_schedule(claim.clone(), late, None)
            .await
            .expect("fixture result");
    }
    assert_eq!(h.outcome().await, Some(ScheduleOutcome::Stopped));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), alerts.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn scheduler_cron_edit_preserves_trust_alert_until_approval_or_new_identity() {
    let h = Harness::new(MissedPolicy::Skip).await;
    h.f.write("[[task]]\nid='task'\ncommand='true'\n");
    let mut alerts = h.f.hub.subscribe("notify.raised");
    h.scheduler.tick(true).await.expect("fixture result");
    h.advance(1);
    h.scheduler.tick(false).await.expect("fixture result");
    notification(&mut alerts).await;
    define_schedule(&h.f, "*/1 * * * *", MissedPolicy::Skip).await;
    h.scheduler.tick(false).await.expect("fixture result");
    h.advance(2);
    h.scheduler.tick(false).await.expect("fixture result");
    assert_eq!(h.outcome().await, Some(ScheduleOutcome::SkippedTrust));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), alerts.recv())
            .await
            .is_err(),
        "cron edit does not change approval identity"
    );
    h.f.runs
        .undefine(UndefineRun {
            project_id: h.f.project.clone(),
            id: "daily".into(),
        })
        .await
        .expect("fixture result");
    define_schedule(&h.f, "*/1 * * * *", MissedPolicy::Skip).await;
    h.scheduler.tick(false).await.expect("fixture result");
    h.advance(3);
    h.scheduler.tick(false).await.expect("fixture result");
    assert!(matches!(
        notification(&mut alerts).await.reason,
        NotificationReason::ScheduleTrustRequired
    ));
    assert_eq!(h.count(), 0);
}
