//! Windows service readiness gate.
use crate::{
    R, RuntimeService,
    exit_record::ExitStore,
    windows_copy_store::{self as store, AttemptPhase},
};
use devbox_protocol::{
    Fail, Internal,
    windows::{ListenerObservation, ObservationAssigned, RunIdentity, WindowsReadiness},
};
use std::{future::Future, pin::Pin, sync::Arc};

pub type WindowsListeners = Arc<
    dyn Fn(
            ObservationAssigned,
            Vec<u16>,
        ) -> Pin<Box<dyn Future<Output = ListenerObservation> + Send>>
        + Send
        + Sync,
>;
pub(crate) fn unavailable_listeners() -> WindowsListeners {
    Arc::new(|_, _| Box::pin(async { ListenerObservation::Unavailable }))
}

enum Candidate {
    Ready,
    NotReady,
    Probe(Box<ObservationAssigned>, Vec<u16>),
}
fn changed() -> Fail<crate::RuntimeError> {
    Fail::Internal("Windows readiness attempt changed".into())
}

impl RuntimeService {
    pub async fn windows_ready(&self, id: &RunIdentity) -> R<WindowsReadiness> {
        self.reconcile_one(&id.run_id).await?;
        self.windows_ready_gate(id).await
    }
    /// The only place a Windows run becomes ready. The probe runs unlocked so stop/restart
    /// are never blocked; the run and attempt are revalidated under the run lock afterwards.
    pub(crate) async fn windows_ready_gate(&self, id: &RunIdentity) -> R<WindowsReadiness> {
        let (assigned, ports) = match self.ready_candidate(id).await? {
            Candidate::Ready => return Ok(WindowsReadiness::Ready),
            Candidate::NotReady => return Ok(WindowsReadiness::NotReady),
            Candidate::Probe(assigned, ports) => (assigned, ports),
        };
        let observed =
            (self.execution()?.windows_listeners)((*assigned).clone(), ports.clone()).await;
        if observed != ListenerObservation::Owned {
            return Ok(WindowsReadiness::NotReady);
        }
        let _guard = self.run_lock(&id.run_id).await;
        match self.ready_candidate(id).await? {
            Candidate::Ready => Ok(WindowsReadiness::Ready),
            Candidate::Probe(current, required) if current == assigned && required == ports => {
                let mut value = self.stored(&id.run_id).await?;
                value.run.ready = true;
                self.save_run(&value).await?;
                Ok(WindowsReadiness::Ready)
            }
            _ => Ok(WindowsReadiness::NotReady),
        }
    }
    /// Read-only: the receipt is the durable committed assignment, never caller input.
    async fn ready_candidate(&self, id: &RunIdentity) -> R<Candidate> {
        let value = self.stored(&id.run_id).await?;
        let current = ExitStore::new(&self.execution()?.paths.data_dir.join("runs"), &id.run_id)
            .and_then(|store| store.current_attempt())
            .map_err(|_| changed())?;
        if !value.run.state.is_live()
            || value.run.attempt_id.as_deref() != Some(&id.attempt_id)
            || current.as_deref() != Some(&id.attempt_id)
        {
            return Err(changed());
        }
        if value.run.ready {
            return Ok(Candidate::Ready);
        }
        let windows = value.windows.ok_or_else(changed)?;
        let reference = windows.copy.ok_or_else(changed)?;
        let mut ports = match windows.def {
            crate::RunDef::Service(d) => d.ports_hint,
            _ => Vec::new(),
        };
        ports.sort_unstable();
        ports.dedup();
        let lookup = id.attempt_id.clone();
        let attempt = self
            .db
            .read(move |tx| store::current_attempt(tx, &reference.launch_id, &lookup))
            .await
            .internal("read Windows readiness receipt")?
            .ok_or_else(changed)?;
        match attempt.phase {
            AttemptPhase::Revoked | AttemptPhase::Closed => Err(changed()),
            // The gate never probes or authorizes a no-port service.
            _ if ports.is_empty() => Ok(Candidate::NotReady),
            AttemptPhase::Intent | AttemptPhase::Assigned => Ok(Candidate::NotReady),
            AttemptPhase::Committed => {
                let assigned = attempt
                    .assigned
                    .filter(|assigned| attempt.control.accepts(id, assigned))
                    .ok_or_else(changed)?;
                Ok(Candidate::Probe(Box::new(assigned), ports))
            }
        }
    }
}

#[cfg(test)]
#[path = "windows_ready_tests.rs"]
mod tests;
