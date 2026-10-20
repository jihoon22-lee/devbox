use crate::{exit_record::now_ms, run_store::StoredRun, *};
use devbox_core::{Paths, systemd::Systemd};
use devbox_protocol::{Fail, Internal};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedMutexGuard};

pub struct StartedRun {
    pub id: String,
    pub kind: RunKind,
}
pub struct ReadyCheck {
    pub unit: String,
    pub pid: Option<u32>,
    pub ports: Vec<u16>,
}
pub type Readiness =
    Arc<dyn Fn(ReadyCheck) -> Pin<Box<dyn Future<Output = bool> + Send>> + Send + Sync>;
pub type WorktreeResolver = Arc<
    dyn Fn(String, String) -> Pin<Box<dyn Future<Output = R<WorktreeLocation>> + Send>>
        + Send
        + Sync,
>;
pub struct WorktreeLocation {
    pub path: String,
    pub port: u16,
}
pub struct ExecutionConfig {
    pub paths: Paths,
    pub executable: std::path::PathBuf,
    pub slice: Option<String>,
    pub readiness: Readiness,
    pub worktree: Option<WorktreeResolver>,
    pub windows_root: Option<String>,
    pub windows_interop: std::path::PathBuf,
}
impl ExecutionConfig {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            executable: std::env::current_exe().expect("running executable"),
            slice: None,
            readiness: Arc::new(|check| Box::pin(async move { check.ports.is_empty() })),
            worktree: None,
            windows_root: None,
            windows_interop: devbox_secrets::keystore::POWERSHELL.into(),
        }
    }
}
impl RuntimeService {
    pub fn with_execution(mut self, execution: ExecutionConfig) -> Self {
        self.execution = Some(execution);
        self
    }
    pub(crate) fn execution(&self) -> R<&ExecutionConfig> {
        self.execution
            .as_ref()
            .ok_or_else(|| Fail::Internal("execution runtime is not configured".into()))
    }
    pub(crate) async fn run_lock(&self, id: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.run_locks.lock().expect("run locks");
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks.get(id).and_then(std::sync::Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(id.into(), Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }
    /// Resolve only a recorded run; callers cannot turn an arbitrary ID into a path.
    pub async fn log_path(&self, id: &str) -> R<std::path::PathBuf> {
        let run = self.stored(id).await?;
        Ok(crate::exit_record::ExitStore::new(
            &self.execution()?.paths.data_dir.join("runs"),
            &run.run.id,
        )
        .internal("resolve recorded run log")?
        .log_path())
    }
    pub(crate) async fn stored(&self, id: &str) -> R<StoredRun> {
        let id = id.to_owned();
        let missing = id.clone();
        self.db
            .read(move |tx| run_store::get(tx, &id))
            .await
            .internal("read execution")?
            .ok_or_else(|| RuntimeError::RunNotFound { id: missing }.into())
    }
    pub(crate) async fn save_run(&self, value: &StoredRun) -> R<u64> {
        let value = value.clone();
        let project = value.run.project_id.clone();
        let hub = self.hub.clone();
        let notification = value
            .schedule
            .clone()
            .map(|claim| (claim, value.run.clone()));
        let (_, rev) = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    run_store::put(tx, &value)?;
                    let alert = if let Some(claim) = &value.schedule {
                        let outcome = ScheduleOutcome::from_run_state(value.run.state);
                        schedule_store::finish(tx, claim, outcome, Some(&value.run.id))?.1
                    } else {
                        false
                    };
                    Ok(Ok::<_, RuntimeError>(alert))
                },
                move |alert, rev| {
                    if *alert {
                        if let Some((claim, run)) = notification {
                            crate::schedule_service::schedule_alert(
                                &hub,
                                rev,
                                &claim,
                                ScheduleOutcome::Failed,
                                Some(run.id),
                            );
                        }
                    }
                    hub.publish::<RuntimeChanged>(
                        rev,
                        &RuntimeChange {
                            rev: rev.to_string(),
                            project_id: project,
                        },
                    );
                },
            )
            .await
            .internal("save execution state")??;
        Ok(rev)
    }
    pub async fn runs(&self, p: RunsQuery) -> R<RunsResult> {
        let limit = p.limit.clamp(1, 256);
        let (items, rev) = self
            .db
            .read(move |tx| {
                Ok((
                    run_store::list(tx, p.project_id.as_deref(), limit)?,
                    devbox_core::current_rev(tx)?,
                ))
            })
            .await
            .internal("list executions")?;
        Ok(RunsResult {
            rev: rev.to_string(),
            items: items.into_iter().map(|s| s.run).collect(),
        })
    }
    pub async fn start(&self, p: StartRun) -> R<Run> {
        let prepared = self.authorize_run_graph(&p).await?;
        self.start_prepared_graph(prepared).await
    }
    pub async fn start_prepared_graph(&self, prepared: PreparedGraph) -> R<Run> {
        self.start_graph(prepared, None, &mut Vec::new()).await
    }
    pub async fn start_prepared_graph_reporting(
        &self,
        prepared: PreparedGraph,
        started: &mut Vec<StartedRun>,
    ) -> R<Run> {
        self.start_graph(prepared, None, started).await
    }
    async fn start_graph(
        &self,
        prepared: PreparedGraph,
        replacement: Option<StoredRun>,
        started: &mut Vec<StartedRun>,
    ) -> R<Run> {
        let execution = self.execution()?;
        // EnvironmentFile must exist before any DB row or unit is created.
        if !execution.paths.instance_config_dir.join("env").is_file() {
            return Err(RuntimeError::DefInvalid {
                id: prepared
                    .nodes
                    .last()
                    .expect("nonempty prepared graph")
                    .id()
                    .into(),
                reason: "captured instance environment is missing; run setup".into(),
            }
            .into());
        }
        let mut last = None;
        let count = prepared.nodes.len();
        for (index, def) in prepared.nodes.iter().enumerate() {
            if let Some(copy) = prepared.windows.get(def.id()) {
                copy.sync()
                    .await
                    .map_err(|_| Fail::Internal("Windows copy failed".into()))?;
            }
            let final_node = index + 1 == count;
            let existing = replacement.as_ref().filter(|_| final_node);
            let id = existing
                .map(|v| v.run.id.clone())
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
            let guard = self.run_lock(&id).await;
            let (kind, restart) = match def {
                RunDef::Task(_) => (
                    if final_node && prepared.schedule.is_some() {
                        RunKind::Scheduled
                    } else {
                        RunKind::Task
                    },
                    RestartPolicy::No,
                ),
                RunDef::Service(d) => (RunKind::Service, d.restart),
                RunDef::Schedule(_) => unreachable!("preflight rejects schedules"),
            };
            let mut value = StoredRun {
                secret_definition: crate::secret_input::requires(def).then(|| def.clone()),
                windows: prepared.windows.get(def.id()).map(|w| {
                    crate::windows::WindowsDefinition {
                        cwd: w.cwd.clone(),
                        def: def.clone(),
                    }
                }),
                schedule: prepared.schedule.clone().filter(|_| final_node),
                run: Run {
                    id: id.clone(),
                    project_id: prepared.project_id.clone(),
                    def_id: def.id().into(),
                    kind,
                    state: RunState::Starting,
                    exit: None,
                    started_ms: now_ms(),
                    ended_ms: None,
                    restarts: 0,
                    unit: launch::unit_name(&id, &execution.paths),
                    ready: false,
                    pid: None,
                    port: match def {
                        RunDef::Task(d) => d.env.get("DEVBOX_PORT"),
                        RunDef::Service(d) => d.env.get("DEVBOX_PORT"),
                        _ => None,
                    }
                    .and_then(|s| s.parse().ok()),
                    attempt_id: None,
                    windows: prepared.windows.get(def.id()).map(|w| WindowsRun {
                        directory: w.cwd.clone(),
                        notice: crate::windows::EXECUTION_NOTICE.into(),
                    }),
                },
                restart,
                start_limit: if let RunDef::Service(d) = def {
                    Some(d.start_limit.clone())
                } else {
                    None
                },
                worktree: prepared.worktree.clone(),
                start_limit_notified: false,
            };
            if let Some(existing) = existing {
                self.stop_unit(&existing.run.unit).await?;
                // --collect services preserve failed state until this manual reset.
                let state = Systemd
                    .show(&existing.run.unit)
                    .await
                    .internal("inspect execution before restart")?;
                if state.load_state != "not-found" {
                    Systemd
                        .reset_failed(&existing.run.unit)
                        .await
                        .internal("reset execution start limit")?;
                }
            }
            self.save_run(&value).await?;
            let args = launch::unit_args_with_executable(
                def,
                &id,
                &execution.paths,
                execution.slice.as_deref(),
                &execution.executable,
            );
            if Systemd.run_transient(&args).await.is_err() {
                value.run.state = RunState::Failed;
                value.run.ended_ms = Some(now_ms());
                self.save_run(&value).await?;
                return Err(Fail::Internal("start transient execution failed".into()));
            }
            started.push(StartedRun {
                id: id.clone(),
                kind,
            });
            drop(guard);
            let mut run = self.reconcile_one(&id).await?;
            if let RunDef::Service(service) = def {
                run = self.wait_ready(run, service).await?;
                if !final_node && (!run.state.is_live() || !run.ready) {
                    return Err(RuntimeError::DependencyFailed {
                        id: def.id().into(),
                    }
                    .into());
                }
            } else if !final_node {
                loop {
                    if !run.state.is_live() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    run = self.reconcile_one(&id).await?;
                }
                if run.state != RunState::Exited
                    || run.exit.as_ref().and_then(|e| e.code) != Some(0)
                {
                    return Err(RuntimeError::DependencyFailed {
                        id: def.id().into(),
                    }
                    .into());
                }
            }
            last = Some(run);
        }
        Ok(last.expect("preflight requires requested definition"))
    }
    async fn wait_ready(&self, mut run: Run, service: &ServiceDef) -> R<Run> {
        let execution = self.execution()?;
        let until =
            tokio::time::Instant::now() + Duration::from_secs(service.ready_timeout_sec.into());
        loop {
            if !run.state.is_live() {
                // A directly requested short-lived service still returns its observable result.
                return Ok(run);
            }
            if tokio::time::Instant::now() >= until {
                return Err(RuntimeError::NotReady {
                    id: service.id.clone(),
                }
                .into());
            }
            let ready = if service.target == RunTarget::Windows {
                service.ports_hint.is_empty() || run.ready
            } else {
                tokio::time::timeout_at(
                    until,
                    (execution.readiness)(ReadyCheck {
                        unit: run.unit.clone(),
                        pid: run.pid,
                        ports: service.ports_hint.clone(),
                    }),
                )
                .await
                .map_err(|_| RuntimeError::NotReady {
                    id: service.id.clone(),
                })?
            };
            // timeout_at may poll an immediately-ready future before its expired timer.
            if tokio::time::Instant::now() >= until {
                return Err(RuntimeError::NotReady {
                    id: service.id.clone(),
                }
                .into());
            }
            if ready {
                let _guard = self.run_lock(&run.id).await;
                let mut stored = self.stored(&run.id).await?;
                if !stored.run.state.is_live() {
                    return Ok(stored.run);
                }
                if stored.run.state == RunState::Running
                    && stored.run.attempt_id == run.attempt_id
                    && stored.run.started_ms == run.started_ms
                {
                    stored.run.ready = true;
                    if !run.ready {
                        self.save_run(&stored).await?;
                    }
                    return Ok(stored.run);
                }
                // An old attempt's successful check cannot authorize its replacement.
                run = stored.run;
            }
            if tokio::time::Instant::now() >= until {
                return Err(RuntimeError::NotReady {
                    id: service.id.clone(),
                }
                .into());
            }
            let next_poll = tokio::time::Instant::now() + Duration::from_secs(1);
            tokio::time::sleep_until(next_poll.min(until)).await;
            if tokio::time::Instant::now() >= until {
                return Err(RuntimeError::NotReady {
                    id: service.id.clone(),
                }
                .into());
            }
            run = self.reconcile_one(&run.id).await?;
        }
    }
    async fn stop_unit(&self, unit: &str) -> R<()> {
        let state = Systemd
            .show(unit)
            .await
            .internal("inspect execution before stop")?;
        if state.load_state == "not-found" {
            return Ok(());
        }
        if state.active_state == "failed" {
            Systemd
                .reset_failed(unit)
                .await
                .internal("reset failed execution")?;
        } else {
            Systemd.stop(unit).await.internal("stop execution")?;
        }
        Ok(())
    }
    pub async fn stop(&self, p: RunId) -> R<Run> {
        let _guard = self.run_lock(&p.run_id).await;
        let mut value = self.stored(&p.run_id).await?;
        self.stop_unit(&value.run.unit).await?;
        value.run.state = RunState::Stopped;
        value.run.ready = false;
        value.run.pid = None;
        value.run.ended_ms = Some(now_ms());
        self.save_run(&value).await?;
        Ok(value.run)
    }
    pub async fn restart(&self, p: RunId) -> R<Run> {
        let value = self.stored(&p.run_id).await?;
        let mut prepared = self
            .authorize_run_graph(&StartRun {
                project_id: value.run.project_id.clone(),
                id: value.run.def_id.clone(),
                worktree: value.worktree.clone(),
            })
            .await?;
        if prepared.automatic_ports.contains(&value.run.def_id)
            && let Some(port) = value.run.port
        {
            match prepared.nodes.last_mut().expect("requested definition") {
                RunDef::Task(d) => {
                    d.env.insert("DEVBOX_PORT".into(), port.to_string());
                }
                RunDef::Service(d) => {
                    d.env.insert("DEVBOX_PORT".into(), port.to_string());
                }
                _ => (),
            }
        }
        self.start_graph(prepared, Some(value), &mut Vec::new())
            .await
    }
}
