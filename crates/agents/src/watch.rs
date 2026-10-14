//! Quiet terminal output is secondary evidence; hook attention always wins.
use crate::{
    model::AgentTask,
    profile::{self, ProfileError},
    service::{AgentsService, Result, apply_event, changed, now_ms},
    state::{AgentEvent, AgentState},
    store,
    tracker::Binding,
};
use devbox_protocol::Internal;
use regex::Regex;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
const QUIET: Duration = Duration::from_secs(2);

pub fn matches_waiting(patterns: &[Regex], tail: &str) -> bool {
    let mut last = tail
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(6)
        .collect::<Vec<_>>();
    last.reverse();
    patterns
        .iter()
        .any(|pattern| pattern.is_match(&last.join("\n")))
}
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub task: AgentTask,
    pub binding: Option<Binding>,
}
impl Snapshot {
    fn current(&self, task: &AgentTask, binding: &Option<Binding>) -> bool {
        &self.task == task && &self.binding == binding
    }
}
struct Observation {
    snapshot: Snapshot,
    tail: String,
    since: Instant,
    suppressed: bool,
}
impl Observation {
    fn observe(&mut self, snapshot: Snapshot, tail: String, now: Instant) -> bool {
        let changed_task = !self.snapshot.current(&snapshot.task, &snapshot.binding);
        let changed_tail = self.tail != tail;
        // After sending input, an unchanged approval screen is stale until the tool redraws.
        let instructed = self.snapshot.task.activity_seq != snapshot.task.activity_seq
            && self.snapshot.task.session_id == snapshot.task.session_id
            && self.snapshot.task.terminal_id == snapshot.task.terminal_id;
        if changed_task
            && (self.snapshot.task.state == AgentState::Waiting || instructed)
            && snapshot.task.state == AgentState::Running
        {
            self.suppressed = !changed_tail;
        }
        if changed_tail {
            self.suppressed = false;
        }
        if changed_task || changed_tail {
            self.since = now;
        }
        self.snapshot = snapshot;
        self.tail = tail;
        !self.suppressed && now.saturating_duration_since(self.since) >= QUIET
    }
}
fn eligible(task: &AgentTask) -> bool {
    task.terminal_id.is_some()
        && (task.state == AgentState::Running
            || (task.state == AgentState::Waiting
                && task.attention.as_ref().is_some_and(|a| a.kind == "output")))
}
impl AgentsService {
    pub(crate) async fn watched_tasks(&self) -> Result<Vec<AgentTask>> {
        Ok(self
            .all_tasks(None)
            .await?
            .into_iter()
            .filter(eligible)
            .collect())
    }
    pub(crate) async fn output_event(
        &self,
        snapshot: Snapshot,
        waiting: bool,
    ) -> Result<Option<AgentTask>> {
        let hub = self.hub.clone();
        let result = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut current) = store::get(tx, &snapshot.task.id)? else {
                        return Ok(Err(()));
                    };
                    let binding = store::binding(tx, &current.id)?;
                    if !snapshot.current(&current, &binding) || !eligible(&current) {
                        return Ok(Err(()));
                    }
                    let previous = current.state;
                    let event = if waiting {
                        AgentEvent::OutputWaiting
                    } else {
                        AgentEvent::OutputResumed
                    };
                    if !apply_event(&mut current, &event, now_ms()) {
                        return Ok(Err(()));
                    }
                    store::put(tx, &current)?;
                    Ok(Ok((current, previous)))
                },
                move |(task, previous), rev| changed(&hub, task, rev, task.state != *previous),
            )
            .await
            .internal("apply terminal output observation")?;
        Ok(result.ok().map(|((task, _), _)| task))
    }
}
pub(crate) fn spawn(service: &Arc<AgentsService>) -> std::result::Result<(), ProfileError> {
    let patterns = profile::compiled(&service.profiles())?;
    let svc = service.clone();
    svc.background.clone().spawn(async move {
        let mut observations: HashMap<String, Observation> = HashMap::new();
        let mut interval = tokio::time::interval(QUIET);
        loop {
            tokio::select! { _ = svc.stop.cancelled() => break, _ = interval.tick() => {} }
            let Ok(tasks) = svc.watched_tasks().await else { continue };
            let live: Vec<_> = tasks.iter().map(|t| t.id.clone()).collect();
            observations.retain(|id, _| live.contains(id));
            for task in tasks {
                let Some(patterns) = patterns.get(&task.profile).filter(|p| !p.is_empty()) else { continue };
                let id = task.id.clone();
                let binding = match svc.db.read(move |tx| store::binding(tx, &id)).await { Ok(b) => b, Err(_) => continue };
                let session = task.terminal_id.as_deref().expect("eligible terminal");
                let tail = tokio::select! { _ = svc.stop.cancelled() => return, tail = svc.terminal.tmux().capture_tail(session, 15) => tail };
                let Ok(tail) = tail else { observations.remove(&task.id); continue };
                let snapshot = Snapshot { task, binding };
                let now = Instant::now();
                let Some(observation) = observations.get_mut(&snapshot.task.id) else {
                    observations.insert(snapshot.task.id.clone(), Observation { snapshot, tail, since: now, suppressed: false });
                    continue;
                };
                if !observation.observe(snapshot.clone(), tail, now) { continue }
                let waiting = matches_waiting(patterns, &observation.tail);
                if let Ok(Some(task)) = svc.output_event(snapshot, waiting).await {
                    observation.snapshot.task = task;
                }
            }
        }
    });
    Ok(())
}
#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
