//! Native Development Session owner. Durable intent precedes effects; retained
//! Runtime leases, never deserialized history, authorize resource cleanup.
use crate::{
    core::development_sessions::{Mode, Phase, ResourceIdentity, ResourceKind, Store},
    host::Host,
    private_metadata::MetadataRoot,
};
use product_contract::{ProjectContext, RouteRequest};
use run_manager_lib::component::sessions::{
    self as runtime, PreparedJob, RuntimeLease, RuntimeStartWitness,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{Arc, Mutex},
};
use tauri::{Manager, WebviewWindow};

type Result<T> = std::result::Result<T, &'static str>;
const FILE: &str = "development-sessions.json";

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Intent {
    jobs: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Document {
    store: Store,
    intents: BTreeMap<String, Intent>,
}
#[derive(Clone)]
struct Plan {
    jobs: Vec<PreparedJob>,
    revision: String,
}
struct Inner {
    root: MetadataRoot,
    document: Document,
    plans: HashMap<String, Plan>,
    leases: HashMap<String, RuntimeLease>,
    witnesses: HashMap<String, Arc<RuntimeStartWitness>>,
    starting: HashSet<String>,
    stopping: HashSet<String>,
}
#[derive(Default)]
pub(crate) struct Sessions {
    inner: Mutex<Option<Inner>>,
    stop_gate: tokio::sync::Mutex<()>,
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_millis() as u64)
        .unwrap_or(0)
}
fn parse<T: serde::de::DeserializeOwned>(args: Value) -> Result<T> {
    if !args.is_object() {
        return Err("session_args_invalid");
    }
    serde_json::from_value(args).map_err(|_| "session_args_invalid")
}
fn write(inner: &mut Inner, change: impl FnOnce(&mut Document) -> Result<()>) -> Result<()> {
    let mut next = inner.document.clone();
    change(&mut next)?;
    next.store.validate()?;
    inner.root.write(
        FILE,
        &serde_json::to_vec(&next).map_err(|_| "session_store_invalid")?,
    )?;
    inner.document = next;
    Ok(())
}
fn identity(lease: &RuntimeLease) -> Result<ResourceIdentity> {
    let descriptor = lease.descriptor();
    Ok(ResourceIdentity {
        kind: match descriptor.kind.as_str() {
            "service" => ResourceKind::Service,
            "job" => ResourceKind::Job,
            "taskOperation" => ResourceKind::TaskOperation,
            _ => return Err("session_resource_invalid"),
        },
        owner_id: descriptor.owner_id.clone(),
        generation: descriptor.generation.clone(),
    })
}
fn resource_key(identity: &ResourceIdentity) -> String {
    Sha256::digest(serde_json::to_vec(identity).expect("typed resource identity"))
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Sessions {
    fn access<T>(&self, f: impl FnOnce(&mut Inner) -> Result<T>) -> Result<T> {
        let mut inner = self.inner.lock().map_err(|_| "session_owner_busy")?;
        f(inner.as_mut().ok_or("session_owner_unavailable")?)
    }
    fn initialize(&self, host: &Host) -> Result<()> {
        let mut selected = self.inner.lock().map_err(|_| "session_owner_busy")?;
        let path = host.component("terminal")?;
        if let Some(inner) = selected.as_ref() {
            if inner.root.path() != path {
                return Err("session_store_changed");
            }
            return inner.root.revalidate();
        }
        let root = MetadataRoot::open(&path)?;
        let mut document: Document = match root.read(FILE)? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|_| "session_store_invalid")?,
            None => Document {
                store: Store::default(),
                intents: BTreeMap::new(),
            },
        };
        document.store.validate()?;
        if document.intents.len() != document.store.sessions.len()
            || document.intents.iter().any(|(id, intent)| {
                !document.store.sessions.contains_key(id)
                    || intent.jobs.len() > 16
                    || intent
                        .jobs
                        .iter()
                        .any(|id| uuid::Uuid::parse_str(id).is_err())
            })
        {
            return Err("session_store_invalid");
        }
        document.store.recover()?;
        root.write(
            FILE,
            &serde_json::to_vec(&document).map_err(|_| "session_store_invalid")?,
        )?;
        *selected = Some(Inner {
            root,
            document,
            plans: HashMap::new(),
            leases: HashMap::new(),
            witnesses: HashMap::new(),
            starting: HashSet::new(),
            stopping: HashSet::new(),
        });
        Ok(())
    }
    pub(crate) fn handles(method: &str) -> bool {
        matches!(
            method,
            "development_candidates"
                | "development_sessions"
                | "prepare_development_session"
                | "start_development_session"
                | "stop_development_session"
        )
    }
    pub(crate) fn manage(
        self: &Arc<Self>,
        window: &WebviewWindow,
        host: &Arc<Host>,
        header: &RouteRequest,
        method: &str,
        args: Value,
    ) -> Result<Value> {
        self.initialize(host)?;
        match method {
            "development_candidates" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Empty {}
                parse::<Empty>(args)?;
                runtime::candidates(window.app_handle()).map_err(|_| "session_runtime_unavailable")
            }
            "development_sessions" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Empty {}
                parse::<Empty>(args)?;
                self.access(|inner| Ok(json!({"sessions":inner.document.store.sessions.values().collect::<Vec<_>>(), "resources":inner.document.store.resources, "intents":inner.document.intents})))
            }
            "prepare_development_session" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    operation_id: String,
                    jobs: Vec<String>,
                }
                let input: Input = parse(args)?;
                if input.jobs.len() > 16
                    || input.jobs.iter().collect::<BTreeSet<_>>().len() != input.jobs.len()
                {
                    return Err("session_plan_invalid");
                }
                let context = header.context.as_ref().ok_or("session_context_required")?;
                host.projects()?
                    .admit_selection(host.helper_directory()?, context)?;
                let jobs = input
                    .jobs
                    .iter()
                    .map(|id| {
                        runtime::prepare_job(window.app_handle(), id)
                            .map_err(|_| "session_runtime_unavailable")
                    })
                    .collect::<Result<Vec<_>>>()?;
                for job in &jobs {
                    job.revalidate(window.app_handle())
                        .map_err(|_| "session_plan_changed")?;
                }
                let revision: String = Sha256::digest(
                    serde_json::to_vec(&(
                        context,
                        jobs.iter()
                            .map(|job| (job.job().id.as_str(), job.revision()))
                            .collect::<Vec<_>>(),
                    ))
                    .map_err(|_| "session_plan_invalid")?,
                )
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
                crate::files_host::current_deadline(header.deadline_ms)?;
                let plan = Plan {
                    jobs,
                    revision: revision.clone(),
                };
                self.access(|inner| {
                    if let Some(existing) = inner.document.store.sessions.get(&input.operation_id) {
                        if existing.context != *context || existing.plan_revision != revision { return Err("session_identity_conflict"); }
                        return Ok(json!({"session":existing,"jobs":plan.jobs.iter().map(|job| job.job()).collect::<Vec<_>>()}));
                    }
                    write(inner, |document| {
                        let session = document.store.create(input.operation_id.clone(),context.clone(),revision.clone(),now())?;
                        document.store.reviewed(&session.id,session.revision,&revision,false,now())?;
                        document.intents.insert(session.id,Intent { jobs: input.jobs });
                        Ok(())
                    })?;
                    let response = json!({"session":inner.document.store.sessions[&input.operation_id],"jobs":plan.jobs.iter().map(|job| job.job()).collect::<Vec<_>>()});
                    inner.plans.insert(input.operation_id,plan);
                    Ok(response)
                })
            }
            "start_development_session" => {
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Input {
                    id: String,
                    revision: u64,
                    plan_revision: String,
                    mode: Mode,
                }
                let input: Input = parse(args)?;
                let (context, plan) = self.access(|inner| {
                    let session = inner
                        .document
                        .store
                        .sessions
                        .get(&input.id)
                        .ok_or("session_missing")?;
                    if header.context.as_ref() != Some(&session.context) {
                        return Err("session_context_changed");
                    }
                    Ok((
                        session.context.clone(),
                        inner
                            .plans
                            .get(&input.id)
                            .ok_or("session_native_owner_lost")?
                            .clone(),
                    ))
                })?;
                host.projects()?
                    .admit_selection(host.helper_directory()?, &context)?;
                for job in &plan.jobs {
                    job.revalidate(window.app_handle())
                        .map_err(|_| "session_plan_changed")?;
                }
                let response = self.access(|inner| {
                    if inner.starting.contains(&input.id) {
                        return Err("session_start_pending");
                    }
                    write(inner, |document| {
                        document.store.approve(
                            &input.id,
                            input.revision,
                            &input.plan_revision,
                            input.mode,
                            now(),
                        )
                    })?;
                    inner.starting.insert(input.id.clone());
                    Ok(json!(inner.document.store.sessions[&input.id]))
                })?;
                let owner = self.clone();
                let app = window.app_handle().clone();
                let host = host.clone();
                tauri::async_runtime::spawn(async move {
                    let result = owner.start(&app, &host, &input.id, &context, plan).await;
                    let _ = owner.access(|inner| {
                        inner.starting.remove(&input.id);
                        if let Err(issue) = result {
                            write(inner, |document| {
                                let session = document
                                    .store
                                    .sessions
                                    .get_mut(&input.id)
                                    .ok_or("session_missing")?;
                                if session.phase != Phase::Stopped
                                    && session.phase != Phase::Stopping
                                {
                                    session.phase = Phase::Degraded;
                                }
                                session.issue = Some(issue.into());
                                session.revision += 1;
                                Ok(())
                            })?;
                        }
                        Ok(())
                    });
                });
                Ok(response)
            }
            "stop_development_session" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    id: String,
                }
                let input: Input = parse(args)?;
                self.access(|inner| {
                    if inner
                        .document
                        .store
                        .sessions
                        .get(&input.id)
                        .ok_or("session_missing")?
                        .issue
                        .as_deref()
                        == Some("session_native_owner_lost")
                    {
                        return Err("session_native_owner_lost");
                    }
                    Ok(())
                })?;
                let response = self.access(|inner| {
                    write(inner, |document| document.store.begin_stop(&input.id))?;
                    Ok(json!(inner.document.store.sessions[&input.id]))
                })?;
                self.spawn_stop(input.id);
                Ok(response)
            }
            _ => Err("session_method_denied"),
        }
    }
    fn publish(&self, operation: &str, lease: RuntimeLease) -> Result<()> {
        let identity = identity(&lease)?;
        let key = resource_key(&identity);
        self.access(|inner| {
            // Retain creator authority even if metadata publication fails. A
            // borrowed reference must never replace the retained creator lease.
            if lease.descriptor().created || !inner.leases.contains_key(&key) {
                inner.leases.insert(key.clone(), lease.clone());
            }
            write(inner, |document| {
                document.store.acquired(
                    operation,
                    key,
                    identity.clone(),
                    lease.descriptor().created,
                    identity.kind == ResourceKind::Service,
                )
            })
        })
    }
    fn advance(&self, id: &str, from: Phase) -> Result<()> {
        self.access(|inner| {
            write(inner, |document| {
                let revision = document
                    .store
                    .sessions
                    .get(id)
                    .ok_or("session_missing")?
                    .revision;
                document.store.advance(id, revision, from, now())
            })
        })
    }
    fn cancelled(&self, id: &str) -> bool {
        self.access(|inner| {
            Ok(inner
                .document
                .store
                .sessions
                .get(id)
                .is_none_or(|session| session.cancel_requested))
        })
        .unwrap_or(true)
    }
    async fn start(
        self: &Arc<Self>,
        app: &tauri::AppHandle,
        host: &Host,
        id: &str,
        context: &ProjectContext,
        plan: Plan,
    ) -> Result<()> {
        // Files already persist per-worktree state. RestoreOnly restores that
        // context without submitting any saved task or service command.
        host.projects()?.binding(context)?;
        self.advance(id, Phase::Restoring)?;
        if self
            .access(|inner| Ok(inner.document.store.sessions[id].mode == Some(Mode::RestoreOnly)))?
        {
            return Ok(());
        }
        self.advance(id, Phase::Preparing)?;
        for job in &plan.jobs {
            if self.cancelled(id) {
                return Ok(());
            }
            host.projects()?.binding(context)?;
            job.revalidate(app).map_err(|_| "session_plan_changed")?;
            let operation = uuid::Uuid::new_v4().to_string();
            let kind = if job.task().is_some() {
                ResourceKind::TaskOperation
            } else if job.job().kind == run_manager_lib::core::models::JobKind::Service {
                ResourceKind::Service
            } else {
                ResourceKind::Job
            };
            self.access(|inner| {
                write(inner, |document| {
                    document
                        .store
                        .reserve(
                            id,
                            operation.clone(),
                            job.job().id.clone(),
                            kind,
                            &plan.revision,
                            now(),
                        )
                        .map(|_| ())
                })
            })?;
            let weak = Arc::downgrade(self);
            let operation_for_publish = operation.clone();
            let witness = Arc::new(RuntimeStartWitness::with_publisher(move |lease| {
                weak.upgrade()
                    .ok_or("session_owner_unavailable")?
                    .publish(&operation_for_publish, lease)
                    .map_err(str::to_owned)
            }));
            self.access(|inner| {
                inner.witnesses.insert(operation.clone(), witness.clone());
                Ok(())
            })?;
            let remaining = self.access(|inner| {
                Ok(inner.document.store.sessions[id]
                    .deadline_ms
                    .unwrap_or(now())
                    .saturating_sub(now()))
            })?;
            let effect = async {
                if kind == ResourceKind::Service {
                    runtime::acquire_service(app, job, &operation, true, &witness).await
                } else {
                    runtime::start_job(app, job, &operation, &witness).await
                }
            };
            tokio::pin!(effect);
            let result = tokio::select! {
                biased;
                _=tokio::time::sleep(std::time::Duration::from_millis(remaining)) => {
                    witness.cancel();
                    self.access(|inner|write(inner,|document|document.store.begin_stop(id)))?;
                    self.spawn_stop(id.into());
                    // Do not drop a future which may already be crossing the
                    // process adapter. Cancellation wakes its native admission.
                    let _=effect.await;
                    Err("session_phase_expired".into())
                }
                result=&mut effect => result,
            };
            if let Err(_issue) = result {
                if witness
                    .lease()
                    .is_none_or(|lease| !lease.descriptor().created)
                {
                    self.access(|inner| {
                        if inner
                            .document
                            .store
                            .operations
                            .get(&operation)
                            .is_some_and(|reservation| reservation.resource_key.is_none())
                        {
                            write(inner, |document| {
                                document.store.settle_without_resource(&operation)
                            })?;
                        }
                        Ok(())
                    })?;
                }
                return Err("session_resource_start_failed");
            }
        }
        if self.cancelled(id) {
            return Ok(());
        }
        self.advance(id, Phase::Starting)?;
        loop {
            if self.cancelled(id) {
                return Ok(());
            }
            let leases = self.access(|inner| {
                let session = inner
                    .document
                    .store
                    .sessions
                    .get(id)
                    .ok_or("session_missing")?;
                if session
                    .deadline_ms
                    .is_some_and(|deadline| now() >= deadline)
                {
                    return Err("session_readiness_expired");
                }
                session
                    .resources
                    .iter()
                    .map(|key| {
                        inner
                            .leases
                            .get(key)
                            .cloned()
                            .ok_or("session_native_owner_lost")
                    })
                    .collect::<Result<Vec<_>>>()
            })?;
            let mut ready = true;
            for lease in leases {
                ready &= lease
                    .ready()
                    .await
                    .map_err(|_| "session_readiness_failed")?;
            }
            if ready {
                return self.advance(id, Phase::Readiness);
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }
    fn spawn_stop(self: &Arc<Self>, id: String) {
        if !self
            .access(|inner| Ok(inner.stopping.insert(id.clone())))
            .unwrap_or(false)
        {
            return;
        }
        let owner = self.clone();
        tauri::async_runtime::spawn(async move {
            let result = owner.stop(&id).await;
            let _ = owner.access(|inner| {
                inner.stopping.remove(&id);
                if let Err(issue) = result {
                    write(inner, |document| {
                        let session = document
                            .store
                            .sessions
                            .get_mut(&id)
                            .ok_or("session_missing")?;
                        session.issue = Some(issue.into());
                        session.revision += 1;
                        Ok(())
                    })?;
                }
                Ok(())
            });
        });
    }
    async fn stop(&self, id: &str) -> Result<()> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            // A failed publication retains its native witness. Reconcile that
            // exact lease before deciding which references can be released.
            let pending = self.access(|inner| {
                for operation in &inner
                    .document
                    .store
                    .sessions
                    .get(id)
                    .ok_or("session_missing")?
                    .pending
                {
                    if let Some(witness) = inner.witnesses.get(operation) {
                        witness.cancel();
                    }
                }
                Ok(inner
                    .document
                    .store
                    .sessions
                    .get(id)
                    .ok_or("session_missing")?
                    .pending
                    .iter()
                    .filter_map(|operation| {
                        inner
                            .witnesses
                            .get(operation)
                            .and_then(|witness| witness.lease())
                            .map(|lease| (operation.clone(), lease))
                    })
                    .collect::<Vec<_>>())
            })?;
            for (operation, lease) in pending {
                let created = lease.descriptor().created;
                if let Err(issue) = self.publish(&operation, lease) {
                    if created {
                        return Err(issue);
                    }
                    self.access(|inner| {
                        if inner.document.store.operations[&operation]
                            .resource_key
                            .is_none()
                        {
                            write(inner, |document| {
                                document.store.settle_without_resource(&operation)
                            })?;
                        }
                        Ok(())
                    })?;
                }
            }
            let stop_guard = self.stop_gate.lock().await;
            let candidates = self.access(|inner| {
                if !inner
                    .document
                    .store
                    .sessions
                    .get(id)
                    .ok_or("session_missing")?
                    .pending
                    .is_empty()
                {
                    return Ok(None);
                }
                let mut candidates = Vec::new();
                write(inner, |document| {
                    candidates = document.store.release(id)?;
                    Ok(())
                })?;
                Ok(Some(candidates))
            })?;
            if let Some(candidates) = candidates {
                for (key, identity) in candidates {
                    let (operation, lease) = self.access(|inner| {
                        let lease = inner
                            .leases
                            .get(&key)
                            .cloned()
                            .ok_or("session_native_owner_lost")?;
                        let mut operation = String::new();
                        write(inner, |document| {
                            operation = document
                                .store
                                .reserve_stop(&key, uuid::Uuid::new_v4().to_string())?;
                            Ok(())
                        })?;
                        Ok((operation, lease))
                    })?;
                    match lease.stop(&operation).await {
                        Ok(()) => self.access(|inner| {
                            write(inner, |document| {
                                document.store.retired(&key, &identity, &operation)
                            })
                        })?,
                        Err(_) => {
                            self.access(|inner| {
                                write(inner, |document| {
                                    document.store.stop_failed(&key, &identity, &operation)
                                })
                            })?;
                            return Err("session_resource_stop_failed");
                        }
                    }
                }
                if self.access(|inner| Ok(!inner.starting.contains(id)))? {
                    return self
                        .access(|inner| write(inner, |document| document.store.finish_stop(id)));
                }
            }
            drop(stop_guard);
            if tokio::time::Instant::now() >= deadline {
                return Err("session_cleanup_pending");
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    pub(crate) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        let ids = {
            let inner = self.inner.lock().map_err(|_| "session_owner_busy")?;
            let Some(inner) = inner.as_ref() else {
                return Ok(());
            };
            inner
                .document
                .store
                .sessions
                .values()
                .filter(|session| {
                    session.phase != Phase::Stopped
                        && session.issue.as_deref() != Some("session_native_owner_lost")
                })
                .map(|session| session.id.clone())
                .collect::<Vec<_>>()
        };
        for id in &ids {
            self.access(|inner| write(inner, |document| document.store.begin_stop(id)))?;
            self.spawn_stop(id.clone());
        }
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let done = self.access(|inner| {
                Ok(inner.starting.is_empty()
                    && inner.stopping.is_empty()
                    && ids
                        .iter()
                        .all(|id| inner.document.store.sessions[id].phase == Phase::Stopped))
            })?;
            if done {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err("session_cleanup_pending");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}
