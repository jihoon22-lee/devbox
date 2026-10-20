//! Event-driven scheduling. Each due occurrence is durably claimed before launching a worker.
use crate::{schedule_model::ScheduleClaim, schedule_service::next, service::project_error, *};
use chrono::Utc;
use devbox_projects::{ListProjects, TrustPreviewStatus};
use devbox_protocol::{Fail, Internal};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::{sync::Mutex, task::JoinSet};
use tokio_util::sync::CancellationToken;

type Runner =
    Arc<dyn Fn(PreparedGraph) -> Pin<Box<dyn Future<Output = R<Run>> + Send>> + Send + Sync>;
type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
pub struct Scheduler {
    service: Arc<RuntimeService>,
    clock: Clock,
    runner: Runner,
    workers: Mutex<JoinSet<()>>,
    tick_lock: Mutex<()>,
}
impl Scheduler {
    pub fn new(service: Arc<RuntimeService>) -> Self {
        let runs = service.clone();
        Self::with_boundaries(
            service,
            Arc::new(|| Utc::now().timestamp_millis()),
            Arc::new(move |graph| {
                let runs = runs.clone();
                Box::pin(async move {
                    let claim = graph.schedule.clone();
                    let mut started = Vec::new();
                    let result = runs
                        .start_prepared_graph_reporting(graph, &mut started)
                        .await;
                    if result.is_err() {
                        if let Some(claim) = claim {
                            runs.finish_schedule(
                                claim,
                                ScheduleOutcome::Failed,
                                started.last().map(|run| run.id.clone()),
                            )
                            .await?;
                        }
                    }
                    result
                })
            }),
        )
    }
    fn with_boundaries(service: Arc<RuntimeService>, clock: Clock, runner: Runner) -> Self {
        Self {
            service,
            clock,
            runner,
            workers: Mutex::new(JoinSet::new()),
            tick_lock: Mutex::new(()),
        }
    }
    /// `startup` applies the missed policy; regular wakeups execute a due tick even if late.
    pub async fn tick(&self, startup: bool) -> R<Option<i64>> {
        let _guard = self.tick_lock.lock().await;
        while self.workers.lock().await.try_join_next().is_some() {}
        let now = (self.clock)();
        let mut earliest = None;
        let projects = self
            .service
            .projects
            .list(ListProjects {})
            .await
            .map_err(project_error)?
            .items;
        for project in projects {
            let state = match self.service.projects.run_config_status(&project.id).await {
                Ok(state) => state,
                Err(_) => continue, // invalid project bytes never authorize or run cached definitions
            };
            let (local, _) = self.service.local(&project.id).await?;
            let merged = match merge(state.config.definitions(), local) {
                Ok(merged) => merged,
                Err(_) => continue,
            };
            let pid = project.id.clone();
            let known = self
                .service
                .db
                .read(move |tx| schedule_store::ids(tx, &pid))
                .await
                .internal("read schedule identities")?;
            for id in known {
                if !merged
                    .defs
                    .iter()
                    .any(|d| matches!(&d.def,RunDef::Schedule(s) if s.id==id))
                {
                    self.service.remove_schedule_state(&project.id, &id).await?;
                }
            }
            for definition in merged.defs {
                let RunDef::Schedule(def) = &definition.def else {
                    continue;
                };
                let encoded =
                    serde_json::to_string(&definition).internal("encode schedule definition")?;
                let pid = project.id.clone();
                let id = def.id.clone();
                let previous = self
                    .service
                    .db
                    .read(move |tx| schedule_store::state(tx, &pid, &id))
                    .await
                    .internal("read schedule cursor")?;
                let approval_stamp = state.approval_stamp.clone().unwrap_or_default();
                let cursor = match previous {
                    Some(previous) if previous.definition == encoded => {
                        if previous.approval_stamp != approval_stamp
                            || previous.trust_notified
                                && matches!(
                                    state.trust,
                                    TrustPreviewStatus::Approved | TrustPreviewStatus::NotRequired
                                )
                        {
                            let pid = project.id.clone();
                            let id = def.id.clone();
                            let approval_stamp = approval_stamp.clone();
                            self.service
                                .db
                                .write(move |tx| {
                                    schedule_store::approved(tx, &pid, &id, &approval_stamp)
                                })
                                .await
                                .internal("reset approved schedule notification")?;
                        }
                        previous.cursor
                    }
                    _ => {
                        let pid = project.id.clone();
                        let id = def.id.clone();
                        let encoded = encoded.clone();
                        self.service
                            .db
                            .write(move |tx| {
                                schedule_store::establish(
                                    tx,
                                    &pid,
                                    &id,
                                    &encoded,
                                    now,
                                    &approval_stamp,
                                )
                            })
                            .await
                            .internal("establish schedule baseline")?;
                        now
                    }
                };
                let mut future = next(def, cursor)?;
                if let Some(at) = future.filter(|at| *at <= now) {
                    let outcome = if startup && at < now && def.missed == MissedPolicy::Skip {
                        ScheduleOutcome::SkippedMissed
                    } else {
                        ScheduleOutcome::Claimed
                    };
                    let claim = ScheduleClaim {
                        project_id: project.id.clone(),
                        schedule_id: def.id.clone(),
                        task: def.task.clone(),
                        at,
                    };
                    let stored = claim.clone();
                    let (claimed, rev) = self
                        .service
                        .db
                        .write(move |tx| {
                            schedule_store::claim(tx, &stored, &encoded, cursor, now, outcome)
                        })
                        .await
                        .internal("claim schedule occurrence")?;
                    if claimed {
                        self.service.changed(&project.id, rev);
                        if outcome == ScheduleOutcome::Claimed {
                            // Authorization is repeated for both normal and missed occurrences, and
                            // its immutable graph is the only input handed to the runner.
                            match self
                                .service
                                .authorize_schedule(&project.id, &definition)
                                .await
                            {
                                Ok(mut graph) => {
                                    graph.schedule = Some(claim.clone());
                                    let service = self.service.clone();
                                    let runner = self.runner.clone();
                                    self.workers.lock().await.spawn(async move {
                                        let result = runner(graph).await;
                                        let (outcome, run) = match result {
                                            Ok(run) => (
                                                ScheduleOutcome::from_run_state(run.state),
                                                Some(run.id),
                                            ),
                                            Err(_) => (ScheduleOutcome::Failed, None),
                                        };
                                        if service
                                            .finish_schedule(claim, outcome, run)
                                            .await
                                            .is_err()
                                        {
                                            tracing::warn!("schedule completion record failed");
                                        }
                                    });
                                }
                                Err(Fail::Domain(
                                    RuntimeError::ProjectTrustRequired { .. }
                                    | RuntimeError::ProjectConfigChanged { .. },
                                )) => {
                                    self.service
                                        .finish_schedule(claim, ScheduleOutcome::SkippedTrust, None)
                                        .await?;
                                }
                                Err(_) => {
                                    self.service
                                        .finish_schedule(claim, ScheduleOutcome::Failed, None)
                                        .await?;
                                }
                            }
                        }
                    }
                    future = next(def, now)?;
                }
                if let Some(next) = future {
                    earliest = Some(earliest.map_or(next, |prior: i64| prior.min(next)));
                }
            }
        }
        Ok(earliest)
    }
    pub(crate) async fn stop(&self) {
        let mut workers = self.workers.lock().await;
        workers.abort_all();
        while workers.join_next().await.is_some() {}
    }
    #[cfg(test)]
    pub(crate) async fn drain(&self) {
        while self.workers.lock().await.join_next().await.is_some() {}
    }
    pub(crate) async fn launch(service: Arc<RuntimeService>) -> R<crate::watch::WatchHandle> {
        let scheduler = Self::new(service.clone());
        let mut changes = service.hub.subscribe("runtime.changed");
        let mut approvals = service.hub.subscribe("projects.changed");
        let cancel = CancellationToken::new();
        let stopped = cancel.clone();
        let mut next = scheduler.tick(true).await?;
        let join = tokio::spawn(async move {
            loop {
                let delay = next.map(|at| {
                    Duration::from_millis(at.saturating_sub((scheduler.clock)()).max(0) as u64)
                });
                let deadline =
                    tokio::time::Instant::now() + delay.unwrap_or(Duration::from_secs(86400));
                tokio::select! {
                    _=stopped.cancelled()=>break,
                    event=changes.recv()=>if event.is_none(){break},
                    event=approvals.recv()=>if event.is_none(){break},
                    _=tokio::time::sleep_until(deadline), if next.is_some()=>{},
                }
                match scheduler.tick(false).await {
                    Ok(value) => next = value,
                    Err(_) => {
                        tracing::warn!("schedule recomputation failed");
                        next = None;
                    }
                }
            }
            scheduler.stop().await;
        });
        Ok(crate::watch::WatchHandle { cancel, join })
    }
}
#[cfg(test)]
pub(crate) fn test_scheduler(
    service: Arc<RuntimeService>,
    clock: Clock,
    runner: Runner,
) -> Scheduler {
    Scheduler::with_boundaries(service, clock, runner)
}
