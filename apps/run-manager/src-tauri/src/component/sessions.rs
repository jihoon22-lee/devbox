//! Native-only Runtime leases for Development Sessions. None of these types
//! can be constructed from a renderer payload or a deserialized run/PID record.
use crate::{
    core::models::{Job, JobKind, ServiceInstanceState},
    lifecycle::RuntimeState,
    scheduler::SchedulerCoordinator,
    storage::{ControlReservation, DatabaseState},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::Manager;

#[derive(Clone)]
pub struct PreparedJob {
    job: Job,
    task: Option<crate::core::workspace_tasks::WorkspaceTaskExecution>,
    revision: String,
}
impl PreparedJob {
    pub fn job(&self) -> &Job {
        &self.job
    }
    pub fn task(&self) -> Option<&crate::core::workspace_tasks::WorkspaceTaskExecution> {
        self.task.as_ref()
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn revalidate(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let current = prepare_job(app, &self.job.id)?;
        if current.revision != self.revision || current.task != self.task {
            return Err("session_runtime_definition_changed".into());
        }
        if let Some(task) = &self.task {
            crate::core::workspace_tasks::revalidate_workspace_task_execution(task)
                .map_err(|_| "session_runtime_source_changed")?;
        }
        Ok(())
    }
}
fn digest(value: &impl Serialize) -> Result<String, String> {
    Ok(
        Sha256::digest(serde_json::to_vec(value).map_err(|_| "session_runtime_invalid")?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}
fn database(app: &tauri::AppHandle) -> Result<Arc<DatabaseState>, String> {
    super::data_root(app)?;
    Ok(app
        .try_state::<Arc<DatabaseState>>()
        .ok_or("component_state_unavailable")?
        .inner()
        .clone())
}
fn coordinator(app: &tauri::AppHandle) -> Result<SchedulerCoordinator, String> {
    super::data_root(app)?;
    Ok(app
        .try_state::<Arc<RuntimeState>>()
        .ok_or("component_state_unavailable")?
        .coordinator())
}
pub fn prepare_job(app: &tauri::AppHandle, id: &str) -> Result<PreparedJob, String> {
    let database = database(app)?;
    let job = database
        .get_run_job(id)
        .map_err(|_| "session_runtime_unavailable")?
        .ok_or("session_runtime_missing")?;
    let task = database
        .get_workspace_task_execution(id)
        .map_err(|_| "session_runtime_unavailable")?;
    // Keep executable authority nonserializable. The source revision identifies
    // its contents; revalidate additionally compares the complete native value.
    let source = task.as_ref().map(|task| {
        (
            &task.source_id,
            task.source_index,
            &task.source_root,
            &task.project_identity,
            &task.revision,
            task.trusted,
            task.shell_trusted,
            task.available,
        )
    });
    let revision = digest(&(&job.execution_definition(), source))?;
    Ok(PreparedJob {
        job,
        task,
        revision,
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceDescriptor {
    pub kind: String,
    pub owner_id: String,
    pub generation: String,
    pub created: bool,
    pub definition_revision: String,
}
#[derive(Clone)]
enum Identity {
    Service(i64),
    Run(String),
    TaskOperation(String),
}
#[derive(Clone)]
pub struct RuntimeLease {
    app: tauri::AppHandle,
    coordinator: SchedulerCoordinator,
    descriptor: ResourceDescriptor,
    identity: Identity,
}
/// Retained by the Session actor before it starts an async Runtime effect.
/// Publication/storage failure may return an error after process creation; this
/// native witness still retains the exact creator lease for reconciliation.
#[derive(Default)]
pub struct RuntimeStartWitness {
    started: AtomicBool,
    lease: Mutex<Option<RuntimeLease>>,
}
impl RuntimeStartWitness {
    pub fn lease(&self) -> Option<RuntimeLease> {
        self.lease.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    fn begin(&self) -> Result<(), String> {
        if self.started.swap(true, Ordering::AcqRel) {
            return Err("session_runtime_pending".into());
        }
        Ok(())
    }
    fn retain(
        &self,
        app: &tauri::AppHandle,
        coordinator: &SchedulerCoordinator,
        prepared: &PreparedJob,
        identity: Identity,
        created: bool,
    ) {
        let (kind, generation) = match &identity {
            Identity::Service(generation) => {
                let mut hash = Sha256::new();
                hash.update(prepared.job.id.as_bytes());
                hash.update([0]);
                hash.update(generation.to_le_bytes());
                hash.update(coordinator.native_owner_id().as_bytes());
                (
                    "service",
                    hash.finalize()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                )
            }
            Identity::Run(id) => ("job", id.clone()),
            Identity::TaskOperation(id) => ("taskOperation", id.clone()),
        };
        *self.lease.lock().unwrap_or_else(|p| p.into_inner()) = Some(RuntimeLease {
            app: app.clone(),
            coordinator: coordinator.clone(),
            identity,
            descriptor: ResourceDescriptor {
                kind: kind.into(),
                owner_id: prepared.job.id.clone(),
                generation,
                created,
                definition_revision: prepared.revision.clone(),
            },
        });
    }
}

impl RuntimeLease {
    pub fn descriptor(&self) -> &ResourceDescriptor {
        &self.descriptor
    }
    pub fn status(&self) -> Result<Value, String> {
        let database = database(&self.app)?;
        match &self.identity {
            Identity::Service(generation) => {
                let instance = database
                    .get_service_instance(&self.descriptor.owner_id)
                    .map_err(|_| "session_runtime_unavailable")?
                    .ok_or("session_runtime_changed")?;
                if instance.generation != *generation {
                    return Err("session_runtime_changed".into());
                }
                Ok(json!({"state":instance.state,"runId":instance.active_run_id}))
            }
            Identity::Run(id) => {
                let run = database
                    .get_run(id)
                    .map_err(|_| "session_runtime_unavailable")?
                    .ok_or("session_runtime_changed")?;
                if run.job_id != self.descriptor.owner_id {
                    return Err("session_runtime_changed".into());
                }
                Ok(json!({"state":run.status,"runId":run.id,"exitCode":run.exit_code}))
            }
            Identity::TaskOperation(id) => {
                let operation = database
                    .get_workspace_task_operation(id)
                    .map_err(|_| "session_runtime_unavailable")?
                    .ok_or("session_runtime_changed")?;
                if operation.root_job_id != self.descriptor.owner_id {
                    return Err("session_runtime_changed".into());
                }
                Ok(
                    json!({"state":operation.status,"operationId":operation.id,"runs":operation.runs}),
                )
            }
        }
    }
    /// Only the retained creator lease can stop a resource. A last shared holder
    /// uses the creator's deferred lease; a borrowed/external lease cannot kill.
    pub async fn stop(&self, operation_id: &str) -> Result<(), String> {
        if !self.descriptor.created {
            return Err("session_runtime_borrowed".into());
        }
        let database = database(&self.app)?;
        let method = match self.identity {
            Identity::Service(_) => "session_stop_service_generation",
            _ => "session_stop_exact_run",
        };
        let fingerprint = digest(&self.descriptor)?;
        if existing(
            &database,
            operation_id,
            method,
            &self.descriptor.owner_id,
            &fingerprint,
        )?
        .is_some()
        {
            return Ok(());
        }
        let now = crate::storage::current_epoch_millis();
        let result = match &self.identity {
            Identity::Service(generation) => self
                .coordinator
                .stop_service_generation_at(&self.descriptor.owner_id, *generation, now)
                .await
                .map(|_| ())
                .map_err(|_| "session_runtime_stop_failed".to_owned()),
            Identity::Run(id) => self
                .coordinator
                .stop_session_run_at(&self.descriptor.owner_id, id, now)
                .await
                .map(|_| ())
                .map_err(|_| "session_runtime_stop_failed".to_owned()),
            Identity::TaskOperation(id) => {
                crate::workspace_orchestration::stop_workspace_task_operation_owned(
                    &database,
                    &self.coordinator,
                    id,
                )
                .await
                .map(|_| ())
            }
        };
        database
            .finish_runtime_control(operation_id, result.as_ref().ok().map(|_| &Value::Null))
            .map_err(|_| "session_runtime_recovery_required")?;
        result.map_err(|_| "session_runtime_stop_failed".into())
    }
}

fn existing(
    database: &DatabaseState,
    id: &str,
    method: &str,
    target: &str,
    fingerprint: &str,
) -> Result<Option<Value>, String> {
    match database
        .reserve_runtime_control(
            id,
            method,
            target,
            fingerprint,
            crate::storage::current_epoch_millis(),
        )
        .map_err(|_| "session_runtime_receipt_unavailable")?
    {
        ControlReservation::New => Ok(None),
        ControlReservation::Existing(receipt) => match receipt.state.as_str() {
            "completed" => receipt
                .result
                .map(Some)
                .ok_or("session_runtime_recovery_required".into()),
            "failed" => Err("session_runtime_failed".into()),
            "pending" => Err("session_runtime_pending".into()),
            _ => Err("session_runtime_recovery_required".into()),
        },
    }
}

pub async fn acquire_service(
    app: &tauri::AppHandle,
    prepared: &PreparedJob,
    operation_id: &str,
    allow_borrow: bool,
    witness: &RuntimeStartWitness,
) -> Result<RuntimeLease, String> {
    witness.begin()?;
    prepared.revalidate(app)?;
    if prepared.job.kind != JobKind::Service || prepared.task.is_some() {
        return Err("session_runtime_invalid".into());
    }
    let database = database(app)?;
    let coordinator = coordinator(app)?;
    let fingerprint = digest(&(&prepared.revision, allow_borrow))?;
    if existing(
        &database,
        operation_id,
        "session_acquire_service",
        &prepared.job.id,
        &fingerprint,
    )?
    .is_some()
    {
        // The owning Session worker retains its lease through renderer loss.
        // A disk receipt by itself cannot reconstruct native authority.
        return Err("session_runtime_replay_review".into());
    }
    let observed = |instance: &crate::core::models::ServiceInstance, created| {
        witness.retain(
            app,
            &coordinator,
            prepared,
            Identity::Service(instance.generation),
            created,
        );
    };
    let result = coordinator
        .acquire_service_observed_at(
            &prepared.job.id,
            Some(&prepared.job),
            allow_borrow,
            Some(&observed),
            crate::storage::current_epoch_millis(),
        )
        .await;
    let record = result
        .as_ref()
        .ok()
        .map(|(instance, created)| json!({"generation":instance.generation,"created":created}));
    database
        .finish_runtime_control(operation_id, record.as_ref())
        .map_err(|_| "session_runtime_recovery_required")?;
    result.map_err(|_| "session_runtime_start_failed")?;
    witness
        .lease()
        .ok_or("session_runtime_recovery_required".into())
}

pub async fn start_job(
    app: &tauri::AppHandle,
    prepared: &PreparedJob,
    operation_id: &str,
    witness: &RuntimeStartWitness,
) -> Result<RuntimeLease, String> {
    witness.begin()?;
    prepared.revalidate(app)?;
    if prepared.job.kind != JobKind::Job {
        return Err("session_runtime_invalid".into());
    }
    let coordinator = coordinator(app)?;
    let database = database(app)?;
    if existing(
        &database,
        operation_id,
        "session_start_job",
        &prepared.job.id,
        &prepared.revision,
    )?
    .is_some()
    {
        return Err("session_runtime_replay_review".into());
    }
    let result = if prepared.task.is_some() {
        crate::workspace_orchestration::start_workspace_task_operation_reviewed(
            database.clone(),
            coordinator.clone(),
            &prepared.job.id,
            true,
            prepared.task.as_ref().map(|task| task.revision.as_str()),
        )
        .and_then(|operation| {
            witness.retain(
                app,
                &coordinator,
                prepared,
                Identity::TaskOperation(operation.id.clone()),
                true,
            );
            serde_json::to_value(operation).map_err(|_| "session_runtime_response_invalid".into())
        })
    } else {
        let observed = |run: &crate::core::models::Run| {
            witness.retain(
                app,
                &coordinator,
                prepared,
                Identity::Run(run.id.clone()),
                true,
            );
            Ok(())
        };
        coordinator
            .trigger_manual_observed_at(
                &prepared.job.id,
                Some(&prepared.job),
                Some(&observed),
                crate::storage::current_epoch_millis(),
            )
            .await
            .map_err(|_| "session_runtime_start_failed".to_owned())
            .and_then(|run| {
                serde_json::to_value(crate::core::models::RunView::from_run(&run))
                    .map_err(|_| "session_runtime_response_invalid".into())
            })
    };
    database
        .finish_runtime_control(operation_id, result.as_ref().ok())
        .map_err(|_| "session_runtime_recovery_required")?;
    result?;
    witness
        .lease()
        .ok_or("session_runtime_recovery_required".into())
}

pub fn service_available_to_borrow(app: &tauri::AppHandle, id: &str) -> Result<bool, String> {
    let database = database(app)?;
    let coordinator = coordinator(app)?;
    Ok(database
        .get_service_instance(id)
        .map_err(|_| "session_runtime_unavailable")?
        .is_some_and(|instance| {
            instance.owner_instance_id.as_deref() == Some(coordinator.native_owner_id())
                && matches!(
                    instance.state,
                    ServiceInstanceState::Running | ServiceInstanceState::RetryWaiting
                )
        }))
}
