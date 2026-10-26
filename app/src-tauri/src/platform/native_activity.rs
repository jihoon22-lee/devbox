//! App-owned lifetime tracking survives transport ACKs, reconnects and adapter cancellation.
use crate::core::native_activity::{Ledger, Observation, observe};
use devbox_protocol::windows::{ObservationControl, RunIdentity, RunWindows};
use native::{ChildProof, JobProof};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
mod native;

#[derive(Default, Clone)]
pub struct Activity(Arc<Mutex<Inner>>);
#[derive(Default)]
struct Inner {
    ledger: Ledger,
    entries: BTreeMap<u64, Entry>,
}
struct Entry {
    run: RunIdentity,
    control: ObservationControl,
    child: Option<Arc<ChildProof>>,
    job: Option<JobProof>,
}
pub struct Tracking {
    activity: Activity,
    id: u64,
}
impl Activity {
    /// Admission and final updater refresh both require QuitState's outer policy lock.
    pub fn reserve(&self, request: &RunWindows, sealed: bool) -> Option<Tracking> {
        if !request.tracked() {
            return None;
        }
        let mut state = self.0.lock().expect("native activity");
        refresh(&mut state);
        if state.entries.values().any(|entry| {
            entry.run.run_id == request.run_id && entry.run.attempt_id == request.attempt_id
        }) {
            return None;
        }
        let id = state.ledger.reserve(sealed)?;
        state.entries.insert(
            id,
            Entry {
                run: RunIdentity {
                    run_id: request.run_id.clone(),
                    attempt_id: request.attempt_id.clone(),
                },
                control: request.observation.clone(),
                child: None,
                job: None,
            },
        );
        Some(Tracking {
            activity: self.clone(),
            id,
        })
    }
    pub fn refresh(&self) -> (Option<u64>, u64) {
        let mut state = self.0.lock().expect("native activity");
        refresh(&mut state);
        state.ledger.snapshot()
    }
}
fn refresh(state: &mut Inner) {
    state.entries.retain(|id, entry| {
        let observation = observe(
            entry.child.as_ref().and_then(|child| child.exited()),
            entry.job.as_ref().and_then(JobProof::active),
        );
        state.ledger.observe(*id, observation);
        observation != Observation::Complete
    });
}
impl Tracking {
    /// Called exactly once, synchronously after spawn and before any user payload is written.
    pub fn spawned(self, child: Option<&tokio::process::Child>) -> bool {
        let Some(child) = child else {
            let mut state = self.activity.0.lock().expect("native activity");
            state.entries.remove(&self.id);
            state.ledger.not_spawned(self.id);
            return true;
        };
        let Some(child) = ChildProof::capture(child).map(Arc::new) else {
            return false;
        };
        let (run, control) = {
            let mut state = self.activity.0.lock().expect("native activity");
            let Some(entry) = state.entries.get_mut(&self.id) else {
                return false;
            };
            entry.child = Some(child.clone());
            (entry.run.clone(), entry.control.clone())
        };
        // Independent of bridge cancellation. One bounded blocking read at a time; a stuck filesystem
        // query leaves Unknown and never starts replacement queries or releases the attempt.
        tauri::async_runtime::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
            loop {
                let (query_child, query_run, query_control) =
                    (child.clone(), run.clone(), control.clone());
                let query = tokio::task::spawn_blocking(move || {
                    query_child.capture_job(&query_run, &query_control)
                });
                match tokio::time::timeout(Duration::from_secs(2), query).await {
                    Ok(Ok(Ok(Some(job)))) => {
                        let mut state = self.activity.0.lock().expect("native activity");
                        if let Some(entry) = state.entries.get_mut(&self.id) {
                            entry.job = Some(job);
                        }
                        refresh(&mut state);
                        return;
                    }
                    Ok(Ok(Ok(None))) if tokio::time::Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                    _ => return,
                }
            }
        });
        true
    }
}
