//! A dependency service that is already running is reused, not launched again.
use crate::{run_store::StoredRun, *};
use devbox_protocol::Internal;
use std::time::Duration;
use tokio::sync::OwnedMutexGuard;

impl RuntimeService {
    /// Serializes service starts of one definition identity (project, worktree,
    /// definition id), so a concurrent graph finds the started run live.
    pub(crate) async fn service_start_lock(
        &self,
        prepared: &PreparedGraph,
        def: &RunDef,
    ) -> Option<OwnedMutexGuard<()>> {
        let RunDef::Service(_) = def else {
            return None;
        };
        let identity = (&prepared.project_id, &prepared.worktree, def.id());
        Some(self.run_lock(&format!("service:{identity:?}")).await)
    }

    /// Recorded live runs a service dependency may reuse, best first.
    /// Task dependencies are one-shot and always run again.
    pub(crate) async fn reusable_dependencies(
        &self,
        prepared: &PreparedGraph,
        def: &RunDef,
    ) -> R<Vec<StoredRun>> {
        let RunDef::Service(_) = def else {
            return Ok(Vec::new());
        };
        let project = prepared.project_id.clone();
        let worktree = prepared.worktree.clone();
        let id = def.id().to_owned();
        self.db
            .read(move |tx| {
                run_store::live_services(tx, &project, Some((worktree.as_deref(), &id)))
            })
            .await
            .internal("read running dependency")
    }

    /// The first reusable run that is still live once reconciled. The caller
    /// holds `service_start_lock`, so no other graph launches a copy meanwhile.
    pub(crate) async fn running_dependency(
        &self,
        prepared: &PreparedGraph,
        def: &RunDef,
    ) -> R<Option<Run>> {
        for candidate in self.reusable_dependencies(prepared, def).await? {
            let run = self.reconcile_one(&candidate.run.id).await?;
            if run.state.is_live() {
                return Ok(Some(run));
            }
        }
        Ok(None)
    }

    /// Waits until a launched or reused node satisfies its graph: a service is
    /// ready and a task dependency exited 0. The requested node returns its result.
    pub(crate) async fn settle(&self, mut run: Run, def: &RunDef, final_node: bool) -> R<Run> {
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
                self.coordinate(tokio::time::sleep(Duration::from_millis(50)))
                    .await?;
                run = self.reconcile_one(&run.id).await?;
            }
            if run.state != RunState::Exited || run.exit.as_ref().and_then(|e| e.code) != Some(0) {
                return Err(RuntimeError::DependencyFailed {
                    id: def.id().into(),
                }
                .into());
            }
        }
        Ok(run)
    }
}
