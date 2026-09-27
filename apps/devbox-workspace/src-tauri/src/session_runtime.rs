//! Native Sessions adapter. Only the agent can turn remote references into leases.
use crate::runtime_owner;
use runtime_engine::component::sessions as native;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use workspace_core::{
    session_registry::Scope,
    session_rpc::{self as rpc, Call},
    Host,
};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone)]
pub(crate) struct PreparedJob {
    native: Option<native::PreparedJob>,
    scope: Scope,
    summary: rpc::Prepared,
}
impl PreparedJob {
    pub(crate) fn job(&self) -> &rpc::SessionJob {
        &self.summary.job
    }
    pub(crate) fn task(&self) -> Option<&rpc::TaskSummary> {
        self.summary.task.as_ref()
    }
    pub(crate) fn revision(&self) -> &str {
        &self.summary.revision
    }
    pub(crate) fn matches_context(
        &self,
        host: &Host,
        context: &product_contract::ProjectContext,
    ) -> bool {
        if &self.scope.context != context {
            return false;
        }
        match &self.native {
            Some(prepared) => prepared.task().is_none_or(|task| {
                workspace_core::task_sources::matches_context(
                    host,
                    context,
                    &task.source_root,
                    &task.project_identity,
                )
            }),
            None => self
                .summary
                .task
                .as_ref()
                .is_none_or(|task| task.same_context),
        }
    }
    pub(crate) async fn revalidate(&self, app: &tauri::AppHandle) -> Result<()> {
        if let Some(prepared) = &self.native {
            return prepared.revalidate(app);
        }
        let _: bool = runtime_owner::session(
            app,
            Call::Revalidate {
                scope: self.scope.clone(),
                reference: self.summary.reference.clone(),
            },
        )
        .await
        .map_err(str::to_owned)?;
        Ok(())
    }
    pub(crate) fn witness(
        &self,
        app: &tauri::AppHandle,
        operation: &str,
        publisher: impl Fn(RuntimeLease) -> Result<()> + Send + Sync + 'static,
    ) -> RuntimeStartWitness {
        if self.native.is_some() {
            return RuntimeStartWitness::Local(native::RuntimeStartWitness::with_publisher(
                move |lease| publisher(RuntimeLease::Local(lease)),
            ));
        }
        RuntimeStartWitness::Remote(Arc::new(RemoteWitness {
            app: app.clone(),
            scope: self.scope.clone(),
            operation: operation.into(),
            cancelled: AtomicBool::new(false),
            uncertain: AtomicBool::new(false),
            lease: Mutex::new(None),
            publisher: Box::new(publisher),
        }))
    }
}
pub(crate) fn candidates(app: &tauri::AppHandle) -> Result<Value> {
    if !runtime_owner::installed(app).map_err(str::to_owned)? {
        return native::candidates(app);
    }
    tauri::async_runtime::block_on(runtime_owner::session(app, Call::Candidates {}))
        .map_err(str::to_owned)
}
pub(crate) fn prepare_job(
    app: &tauri::AppHandle,
    host: &Host,
    scope: Scope,
    id: &str,
) -> Result<PreparedJob> {
    scope.validate().map_err(str::to_owned)?;
    if !runtime_owner::installed(app).map_err(str::to_owned)? {
        let prepared = native::prepare_job(app, id)?;
        let summary = rpc::Prepared::project(String::new(), &prepared, host, &scope.context);
        return Ok(PreparedJob {
            native: Some(prepared),
            scope,
            summary,
        });
    }
    let summary = tauri::async_runtime::block_on(runtime_owner::session(
        app,
        Call::Prepare {
            scope: scope.clone(),
            job_id: id.into(),
        },
    ))
    .map_err(str::to_owned)?;
    Ok(PreparedJob {
        native: None,
        scope,
        summary,
    })
}
#[derive(Clone)]
pub(crate) enum RuntimeLease {
    Local(native::RuntimeLease),
    Remote {
        app: tauri::AppHandle,
        scope: Scope,
        lease: rpc::Lease,
    },
}
impl RuntimeLease {
    pub(crate) fn descriptor(&self) -> &native::ResourceDescriptor {
        match self {
            Self::Local(lease) => lease.descriptor(),
            Self::Remote { lease, .. } => &lease.descriptor,
        }
    }
    // These snapshot functions are called from Workspace's bounded blocking lane.
    pub(crate) fn status(&self) -> Result<Value> {
        match self {
            Self::Local(lease) => lease.status(),
            Self::Remote { app, scope, lease } => {
                tauri::async_runtime::block_on(runtime_owner::session(
                    app,
                    Call::Status {
                        scope: scope.clone(),
                        reference: lease.reference.clone(),
                    },
                ))
                .map_err(str::to_owned)
            }
        }
    }
    pub(crate) fn running_runs(&self) -> Result<Vec<String>> {
        match self {
            Self::Local(lease) => lease.running_runs(),
            Self::Remote { app, scope, lease } => {
                tauri::async_runtime::block_on(runtime_owner::session(
                    app,
                    Call::RunningRuns {
                        scope: scope.clone(),
                        reference: lease.reference.clone(),
                    },
                ))
                .map_err(str::to_owned)
            }
        }
    }
    pub(crate) async fn ready(&self) -> Result<bool> {
        match self {
            Self::Local(lease) => lease.ready().await,
            Self::Remote { app, scope, lease } => runtime_owner::session(
                app,
                Call::Ready {
                    scope: scope.clone(),
                    reference: lease.reference.clone(),
                },
            )
            .await
            .map_err(str::to_owned),
        }
    }
    pub(crate) async fn stop(&self, operation: &str) -> Result<()> {
        // Preserve creator authority even before reaching the agent.
        if !self.descriptor().created {
            return Err("session_runtime_borrowed".into());
        }
        match self {
            Self::Local(lease) => lease.stop(operation).await,
            Self::Remote { app, scope, lease } => runtime_owner::session(
                app,
                Call::Stop {
                    scope: scope.clone(),
                    reference: lease.reference.clone(),
                    operation: operation.into(),
                },
            )
            .await
            .map_err(str::to_owned),
        }
    }
}
pub(crate) struct RemoteWitness {
    app: tauri::AppHandle,
    scope: Scope,
    operation: String,
    cancelled: AtomicBool,
    uncertain: AtomicBool,
    lease: Mutex<Option<RuntimeLease>>,
    publisher: Box<dyn Fn(RuntimeLease) -> Result<()> + Send + Sync>,
}
pub(crate) enum RuntimeStartWitness {
    Local(native::RuntimeStartWitness),
    Remote(Arc<RemoteWitness>),
}
impl RuntimeStartWitness {
    pub(crate) fn cancel(&self) {
        match self {
            Self::Local(witness) => witness.cancel(),
            Self::Remote(witness) => {
                if witness.cancelled.swap(true, Ordering::AcqRel) {
                    return;
                }
                let witness = witness.clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(receipt) = runtime_owner::session::<rpc::Receipt>(
                        &witness.app,
                        Call::Cancel {
                            scope: witness.scope.clone(),
                            operation: witness.operation.clone(),
                        },
                    )
                    .await
                    {
                        let _ = witness.observe(receipt);
                    }
                });
            }
        }
    }
    pub(crate) fn lease(&self) -> Option<RuntimeLease> {
        match self {
            Self::Local(witness) => witness.lease().map(RuntimeLease::Local),
            Self::Remote(witness) => witness
                .lease
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone(),
        }
    }
    pub(crate) fn uncertain(&self) -> bool {
        matches!(self, Self::Remote(witness) if witness.uncertain.load(Ordering::Acquire))
    }
}
impl RemoteWitness {
    fn observe(&self, receipt: rpc::Receipt) -> Result<()> {
        self.uncertain.store(!receipt.finished, Ordering::Release);
        if let Some(lease) = receipt.lease {
            let lease = RuntimeLease::Remote {
                app: self.app.clone(),
                scope: self.scope.clone(),
                lease,
            };
            // Retain authority before publishing to the durable UI document.
            let publish = {
                let mut stored = self.lease.lock().unwrap_or_else(|p| p.into_inner());
                if stored.is_some() {
                    false
                } else {
                    *stored = Some(lease.clone());
                    true
                }
            };
            if publish {
                (self.publisher)(lease)?;
            }
        }
        if let Some(issue) = receipt.issue {
            return Err(issue);
        }
        if receipt.cancelled {
            return Err("session_runtime_cancelled".into());
        }
        if !receipt.finished {
            return Err("session_runtime_pending".into());
        }
        Ok(())
    }
}
pub(crate) async fn start_job(
    app: &tauri::AppHandle,
    prepared: &PreparedJob,
    operation: &str,
    witness: &RuntimeStartWitness,
) -> Result<RuntimeLease> {
    start(app, prepared, operation, false, witness).await
}
pub(crate) async fn acquire_service(
    app: &tauri::AppHandle,
    prepared: &PreparedJob,
    operation: &str,
    allow_borrow: bool,
    witness: &RuntimeStartWitness,
) -> Result<RuntimeLease> {
    if !allow_borrow {
        return Err("session_runtime_invalid".into());
    }
    start(app, prepared, operation, true, witness).await
}
async fn start(
    app: &tauri::AppHandle,
    prepared: &PreparedJob,
    operation: &str,
    service: bool,
    witness: &RuntimeStartWitness,
) -> Result<RuntimeLease> {
    match (prepared.native.as_ref(), witness) {
        (Some(prepared), RuntimeStartWitness::Local(witness)) => {
            if service {
                native::acquire_service(app, prepared, operation, true, witness)
                    .await
                    .map(RuntimeLease::Local)
            } else {
                native::start_job(app, prepared, operation, witness)
                    .await
                    .map(RuntimeLease::Local)
            }
        }
        (None, RuntimeStartWitness::Remote(witness)) => {
            if witness.scope != prepared.scope || witness.operation != operation {
                return Err("session_runtime_invalid".into());
            }
            if witness.cancelled.load(Ordering::Acquire) {
                return Err("session_runtime_cancelled".into());
            }
            witness.uncertain.store(true, Ordering::Release);
            let result = runtime_owner::session::<rpc::Receipt>(
                app,
                Call::Start {
                    scope: prepared.scope.clone(),
                    operation: operation.into(),
                    reference: prepared.summary.reference.clone(),
                    service,
                },
            )
            .await;
            // A transport failure may follow a real process creation. Query the
            // already registered receipt; never send Start a second time.
            let receipt = match result {
                Ok(receipt) => receipt,
                Err(_) => runtime_owner::session(
                    app,
                    Call::Receipt {
                        scope: prepared.scope.clone(),
                        operation: operation.into(),
                    },
                )
                .await
                .map_err(str::to_owned)?,
            };
            witness.observe(receipt)?;
            witness
                .lease
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
                .ok_or_else(|| "session_runtime_pending".into())
        }
        _ => Err("session_runtime_invalid".into()),
    }
}

/// Read existing receipts in bounded batches; this never executes a saved plan.
pub(crate) fn restore(
    app: &tauri::AppHandle,
    operations: Vec<rpc::RecoveryOperation>,
) -> Result<Vec<Option<rpc::Receipt>>> {
    let mut receipts = Vec::with_capacity(operations.len());
    for batch in operations.chunks(32) {
        let values: Vec<Option<rpc::Receipt>> =
            tauri::async_runtime::block_on(runtime_owner::session(
                app,
                Call::Restore {
                    operations: batch.to_vec(),
                },
            ))
            .map_err(str::to_owned)?;
        if values.len() != batch.len() {
            return Err("session_runtime_invalid".into());
        }
        receipts.extend(values);
    }
    Ok(receipts)
}
