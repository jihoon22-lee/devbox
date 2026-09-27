//! Actual execution leases and start witnesses stay in this process across UI loss.
use runtime_engine::component::sessions::{
    self as native, PreparedJob, RuntimeLease, RuntimeStartWitness,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use workspace_core::{
    session_registry::{Begin, Registry, Scope},
    session_rpc::{Call, Lease, Prepared, Receipt},
    Host,
};
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Default)]
pub struct Sessions {
    registry: Mutex<Registry<PreparedJob, RuntimeStartWitness>>,
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
fn issue(error: String) -> &'static str {
    match error.as_str() {
        "session_runtime_cancelled" => "session_runtime_cancelled",
        "session_runtime_borrowed" => "session_runtime_borrowed",
        "session_runtime_definition_changed" => "session_runtime_definition_changed",
        "session_runtime_source_changed" => "session_runtime_source_changed",
        "session_runtime_changed" => "session_runtime_changed",
        "session_runtime_invalid" => "session_runtime_invalid",
        _ => "session_runtime_unavailable",
    }
}
fn encode(value: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|_| "session_runtime_invalid")
}
impl Sessions {
    fn receipt(&self, scope: &Scope, operation: &str) -> Result<Receipt> {
        let receipt = self
            .registry
            .lock()
            .map_err(|_| "busy")?
            .receipt(scope, operation)?;
        let lease = receipt
            .witness
            .and_then(|witness| witness.lease())
            .map(|lease| Lease {
                reference: receipt.reference,
                descriptor: lease.descriptor().clone(),
            });
        Ok(Receipt {
            finished: receipt.finished,
            cancelled: receipt.cancelled,
            issue: receipt.issue.map(str::to_owned),
            lease,
        })
    }
    fn lease(&self, scope: &Scope, reference: &str) -> Result<RuntimeLease> {
        self.registry
            .lock()
            .map_err(|_| "busy")?
            .by_reference(scope, reference)?
            .witness
            .and_then(|witness| witness.lease())
            .ok_or("session_runtime_pending")
    }
    pub async fn dispatch(&self, app: &tauri::AppHandle, host: &Host, call: Call) -> Result<Value> {
        call.validate()?;
        if let Some(context) = call.live_context() {
            host.projects()?.binding(context)?;
        }
        match call {
            Call::Candidates {} => native::candidates(app).map_err(issue),
            Call::Prepare { scope, job_id } => {
                let prepared = native::prepare_job(app, &job_id).map_err(issue)?;
                let reference = self.registry.lock().map_err(|_| "busy")?.prepare(
                    scope.clone(),
                    prepared.clone(),
                    now(),
                )?;
                encode(Prepared::project(
                    reference,
                    &prepared,
                    host,
                    &scope.context,
                ))
            }
            Call::Revalidate { scope, reference } => {
                let prepared = self.registry.lock().map_err(|_| "busy")?.prepared(
                    &scope,
                    &reference,
                    now(),
                )?;
                prepared.revalidate(app).map_err(issue)?;
                if prepared.task().is_some_and(|task| {
                    !workspace_core::task_sources::matches_context(
                        host,
                        &scope.context,
                        &task.source_root,
                        &task.project_identity,
                    )
                }) {
                    return Err("session_runtime_source_changed");
                }
                Ok(json!(true))
            }
            Call::Start {
                scope,
                operation,
                reference,
                service,
            } => {
                let begin = self.registry.lock().map_err(|_| "busy")?.begin(
                    &scope,
                    &operation,
                    &reference,
                    Arc::new(RuntimeStartWitness::default()),
                    now(),
                )?;
                if let Begin::New {
                    witness, prepared, ..
                } = begin
                {
                    // The witness is registered before any effect. Errors after
                    // process creation still leave its exact lease in receipt().
                    let result = if prepared.task().is_some_and(|task| {
                        !workspace_core::task_sources::matches_context(
                            host,
                            &scope.context,
                            &task.source_root,
                            &task.project_identity,
                        )
                    }) {
                        Err("session_runtime_source_changed".to_owned())
                    } else if service {
                        native::acquire_service(app, &prepared, &operation, true, &witness).await
                    } else {
                        native::start_job(app, &prepared, &operation, &witness).await
                    };
                    self.registry.lock().map_err(|_| "busy")?.finish(
                        &scope,
                        &operation,
                        result.err().map(issue),
                    )?;
                }
                encode(self.receipt(&scope, &operation)?)
            }
            Call::Receipt { scope, operation } => encode(self.receipt(&scope, &operation)?),
            Call::Cancel { scope, operation } => {
                if let Some(witness) = self
                    .registry
                    .lock()
                    .map_err(|_| "busy")?
                    .cancel(&scope, &operation)?
                {
                    witness.cancel();
                }
                encode(self.receipt(&scope, &operation)?)
            }
            Call::Status { scope, reference } => {
                self.lease(&scope, &reference)?.status().map_err(issue)
            }
            Call::Ready { scope, reference } => Ok(json!(self
                .lease(&scope, &reference)?
                .ready()
                .await
                .map_err(issue)?)),
            Call::RunningRuns { scope, reference } => encode(
                self.lease(&scope, &reference)?
                    .running_runs()
                    .map_err(issue)?,
            ),
            Call::Stop {
                scope,
                reference,
                operation,
            } => {
                self.lease(&scope, &reference)?
                    .stop(&operation)
                    .await
                    .map_err(issue)?;
                Ok(Value::Null)
            }
        }
    }
}
