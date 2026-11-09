//! Quiet terminal output is secondary evidence; hook attention always wins.
use crate::{
    model::AgentTask,
    profile::{self, ProfileError},
    service::{AgentsService, CodexTurnEnd, Result, apply_event, changed, now_ms},
    state::{AgentEvent, AgentState},
    store,
    tracker::Binding,
};
use devbox_protocol::Internal;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};
const QUIET: Duration = Duration::from_secs(2);
/// Rows of pane history captured per read, by the watcher and by a send alike. The capture also
/// holds every visible row: `capture_tail` reads from that far above the screen to its bottom.
const TAIL_ROWS: u32 = 15;

/// The compiled waiting patterns of one profile.
#[derive(Debug, Clone, Default)]
pub struct WaitingPatterns {
    /// Read over the last 6 non-empty rows, where a tool draws its prompts.
    pub(crate) tail: Vec<Regex>,
    /// Read over the whole capture: dialogs a tool draws in the middle of the screen, above the
    /// rows `tail` reads.
    pub(crate) screen: Vec<Regex>,
}
impl WaitingPatterns {
    pub(crate) fn is_empty(&self) -> bool {
        self.tail.is_empty() && self.screen.is_empty()
    }
}

/// The last 6 non-empty rows of `capture`, top to bottom, where a tool draws its prompts.
fn last_rows(capture: &str) -> Vec<&str> {
    let mut last = capture
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(6)
        .collect::<Vec<_>>();
    last.reverse();
    last
}
/// Whether `capture`, the pane as `capture_tail` reads it (the screen and up to 15 rows above
/// it), shows a prompt.
pub fn matches_waiting(patterns: &WaitingPatterns, capture: &str) -> bool {
    let last = last_rows(capture).join("\n");
    patterns.tail.iter().any(|pattern| pattern.is_match(&last))
        || patterns
            .screen
            .iter()
            .any(|pattern| pattern.is_match(capture))
}
/// Whether `capture` shows Codex at its prompt with nothing running: the rows the tail patterns
/// read hold the prompt box (a row that starts with `› `) and its `? for shortcuts` footer, and
/// no working turn's status (`Working (`, `esc to interrupt`). Codex draws the prompt box and the
/// footer while it works too (codex-0.164.0/working.txt), and a dialog in the middle of the screen
/// leaves them in place, so this counts only where no waiting pattern matches. Evidence:
/// crates/agents/tests/fixtures/codex-0.167.0/resume-ready.txt (real capture).
pub(crate) fn shows_prompt(capture: &str) -> bool {
    let rows = last_rows(capture);
    rows.iter().any(|row| row.starts_with("› "))
        && rows.iter().any(|row| row.contains("? for shortcuts"))
        && !rows
            .iter()
            .any(|row| row.contains("Working (") || row.contains("esc to interrupt"))
}
impl AgentsService {
    /// Whether the pane of `session` shows, right now, a prompt the watcher would report as
    /// waiting. A send asks this itself: the watcher reports a prompt only after 2 to about 6 s
    /// of unchanged output, and the Enter of a send in that gap would answer the prompt. Without
    /// published patterns (no watcher) or patterns for the profile, there is nothing to match
    /// and the pane is not read.
    pub(crate) async fn pane_is_waiting(&self, task: &AgentTask, session: &str) -> Result<bool> {
        let Some(patterns) = self
            .waiting_patterns
            .get()
            .and_then(|all| all.get(&task.profile))
            .filter(|patterns| !patterns.is_empty())
        else {
            return Ok(false);
        };
        let tail = self
            .terminal
            .tmux()
            .capture_tail(session, TAIL_ROWS)
            .await
            .internal("capture agent pane before send")?;
        Ok(matches_waiting(patterns, &tail))
    }
}
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub task: AgentTask,
    pub binding: Option<Binding>,
}
impl Snapshot {
    pub(crate) fn current(&self, task: &AgentTask, binding: &Option<Binding>) -> bool {
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
pub(crate) fn eligible(task: &AgentTask) -> bool {
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
/// What the output watcher keeps between its reads of the panes.
pub(crate) struct Watcher {
    patterns: Arc<HashMap<String, WaitingPatterns>>,
    /// The profiles of the Codex family, whose pane shows when a resumed launch is ready.
    codex: HashSet<String>,
    observations: HashMap<String, Observation>,
}
impl Watcher {
    /// Compiles the profiles' waiting patterns and publishes them for sends.
    pub(crate) fn new(service: &AgentsService) -> std::result::Result<Self, ProfileError> {
        let profiles = service.profiles();
        let compiled = Arc::new(profile::compiled(&profiles)?);
        // Profiles do not change while the service lives, so the first publication stays valid.
        let patterns = service.waiting_patterns.get_or_init(|| compiled).clone();
        let codex = profiles
            .items
            .into_iter()
            .filter(|p| p.hooks == profile::Hooks::Codex)
            .map(|p| p.id)
            .collect();
        Ok(Self {
            patterns,
            codex,
            observations: HashMap::new(),
        })
    }
    /// Whether the panes of `profile`'s tasks are read: only for a profile with waiting patterns.
    fn reads(&self, profile: &str) -> bool {
        self.patterns.get(profile).is_some_and(|p| !p.is_empty())
    }
    /// Forgets every task but `tasks`, the ones still watched.
    pub(crate) fn keep(&mut self, tasks: &[AgentTask]) {
        self.observations
            .retain(|id, _| tasks.iter().any(|task| &task.id == id));
    }
    /// Reads `tail`, the pane of the snapshot's task captured at `now`. Once the pane has sat
    /// still for [`QUIET`], what it shows is applied to the task: a prompt waits for the user, its
    /// end resumes the task, and a launch that resumed Codex's session, at Codex's prompt before
    /// any turn, waits for the next instruction.
    pub(crate) async fn read(
        &mut self,
        service: &Arc<AgentsService>,
        snapshot: Snapshot,
        tail: String,
        now: Instant,
    ) {
        let Some(patterns) = self
            .patterns
            .get(&snapshot.task.profile)
            .filter(|p| !p.is_empty())
        else {
            return;
        };
        let Some(observation) = self.observations.get_mut(&snapshot.task.id) else {
            let id = snapshot.task.id.clone();
            let observation = Observation {
                snapshot,
                tail,
                since: now,
                suppressed: false,
            };
            self.observations.insert(id, observation);
            return;
        };
        if !observation.observe(snapshot.clone(), tail, now) {
            return;
        }
        let waiting = matches_waiting(patterns, &observation.tail);
        // Codex reports only finished turns, and a resumed session has none until the user
        // gives an instruction.
        if !waiting
            && self.codex.contains(&snapshot.task.profile)
            && snapshot
                .binding
                .as_ref()
                .is_some_and(|binding| binding.awaits_first_instruction(&snapshot.task))
            && shows_prompt(&observation.tail)
        {
            let id = snapshot.task.id.clone();
            let _ = service
                .complete_codex(&id, CodexTurnEnd::Ready(Box::new(snapshot)))
                .await;
            return;
        }
        if let Ok(Some(task)) = service.output_event(snapshot, waiting).await {
            observation.snapshot.task = task;
        }
    }
}
pub(crate) fn spawn(service: &Arc<AgentsService>) -> std::result::Result<(), ProfileError> {
    let mut watcher = Watcher::new(service)?;
    let svc = service.clone();
    svc.background.clone().spawn(async move {
        let mut interval = tokio::time::interval(QUIET);
        loop {
            tokio::select! { _ = svc.stop.cancelled() => break, _ = interval.tick() => {} }
            let Ok(tasks) = svc.watched_tasks().await else {
                continue;
            };
            watcher.keep(&tasks);
            for task in tasks {
                if !watcher.reads(&task.profile) {
                    continue;
                }
                let id = task.id.clone();
                let Ok(binding) = svc.db.read(move |tx| store::binding(tx, &id)).await else {
                    continue;
                };
                let session = task.terminal_id.as_deref().expect("eligible terminal");
                let tail = tokio::select! { _ = svc.stop.cancelled() => return, tail = svc.terminal.tmux().capture_tail(session, TAIL_ROWS) => tail };
                let Ok(tail) = tail else {
                    watcher.observations.remove(&task.id);
                    continue;
                };
                let snapshot = Snapshot { task, binding };
                watcher.read(&svc, snapshot, tail, Instant::now()).await;
            }
        }
    });
    Ok(())
}
#[cfg(test)]
#[path = "watch_tests.rs"]
mod tests;
