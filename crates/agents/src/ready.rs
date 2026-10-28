//! A pi task takes sends only once pi reads its input. Before that, pasted text and its Enter
//! can sit in the terminal until pi's folder-trust question reads them, and Enter there trusts
//! the folder for good. devbox's extension says when pi is ready (`is_ready_report`).
use crate::{
    events::{announce, changed},
    model::{AgentTask, AgentsError},
    profile::Hooks,
    service::{AgentsService, Result},
    state::AgentState,
    store,
    tracker::Binding,
};
use devbox_protocol::{Done, Internal};
use std::{collections::HashMap, sync::Mutex, time::Duration};

/// How long after its tool started a launch that never said it is ready takes sends anyway, so
/// a task whose extension did not load is not held forever. Its pane patterns still apply.
pub(crate) const READY_FALLBACK_MS: i64 = 30_000;

/// How long pi's trust question must stay open before it is announced. pi asks it even in a
/// folder whose trust is decided, and then goes on within tens of milliseconds.
const TRUST_NOTICE_DELAY: Duration = Duration::from_secs(2);

/// What this daemon has seen of a task's latest launch. Kept in memory only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LaunchSeen {
    pub launch: String,
    /// When this daemon first saw the launch's tool start: its setup event, or the poll that
    /// found the launch's phase file saying so first.
    pub tool_started_ms: Option<i64>,
    /// pi said it reads its input.
    pub ready: bool,
}

/// The launches seen, by task. A report for another launch replaces the task's entry, and a
/// send only counts the entry of the task's current launch.
#[derive(Default)]
pub(crate) struct Launches(Mutex<HashMap<String, LaunchSeen>>);

impl Launches {
    fn update(&self, task: &str, launch: &str, f: impl FnOnce(&mut LaunchSeen)) {
        let mut all = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let seen = all.entry(task.to_owned()).or_default();
        if seen.launch != launch {
            *seen = LaunchSeen {
                launch: launch.to_owned(),
                ..LaunchSeen::default()
            };
        }
        f(seen);
    }
    pub(crate) fn tool_started(&self, task: &str, launch: &str, at_ms: i64) {
        self.update(task, launch, |seen| {
            seen.tool_started_ms.get_or_insert(at_ms);
        });
    }
    pub(crate) fn ready(&self, task: &str, launch: &str) {
        self.update(task, launch, |seen| seen.ready = true);
    }
    pub(crate) fn get(&self, task: &str) -> Option<LaunchSeen> {
        let all = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        all.get(task).cloned()
    }
}

/// Whether a send may reach the terminal of `binding`'s launch.
pub(crate) fn may_send(
    binding: &Binding,
    seen: Option<&LaunchSeen>,
    now_ms: i64,
    daemon_started_ms: i64,
) -> bool {
    let seen = seen.filter(|seen| binding.launch_id.as_deref() == Some(seen.launch.as_str()));
    if seen.is_some_and(|seen| seen.ready) {
        return true;
    }
    let Some(started) = seen.and_then(|seen| seen.tool_started_ms) else {
        // A launch from before this daemon may have said it is ready while no daemon listened.
        // Otherwise the wait counts from the launch. A resume has no setup, so its tool starts
        // as it launches. A new task's start is recorded when its setup event or the poll turns
        // it Running. Only a report naming a session can do that first, and pi's extension
        // sends one only after its ready report.
        return binding.launch_ms < daemon_started_ms
            || now_ms.saturating_sub(binding.launch_ms) >= READY_FALLBACK_MS;
    };
    // Preparation and setup can take long, so the wait counts from when the tool started.
    now_ms.saturating_sub(started.max(binding.launch_ms)) >= READY_FALLBACK_MS
}

impl AgentsService {
    /// Whether the task's tool says when it is ready: pi, through devbox's extension.
    pub(crate) fn says_ready(&self, task: &AgentTask) -> bool {
        crate::profile::resolve(&self.profiles, &task.profile).is_some_and(|p| p.hooks == Hooks::Pi)
    }
    /// Refuses a send while the task's pi launch has not said it reads its input.
    pub(crate) async fn check_ready(&self, task: &AgentTask, now_ms: i64) -> Result<()> {
        if !self.says_ready(task) {
            return Ok(());
        }
        let binding = self.launch_binding(&task.id).await?;
        if may_send(
            &binding,
            self.launches.get(&task.id).as_ref(),
            now_ms,
            self.started_ms,
        ) {
            return Ok(());
        }
        // A question pi asks while it starts, its trust question first, is what to answer.
        Err(if task.state == AgentState::Waiting {
            AgentsError::AwaitingInput
        } else {
            AgentsError::ToolStarting
        }
        .into())
    }
    /// Records pi's ready report when `launch` is the task's current launch.
    pub(crate) async fn record_ready(&self, id: &str, launch: Option<String>) -> Result<Done> {
        let unchanged = || Done {
            rev: self.db.rev().to_string(),
        };
        let Some(launch) = launch.filter(|launch| !launch.is_empty()) else {
            return Ok(unchanged());
        };
        let id = id.to_owned();
        let hub = self.hub.clone();
        let launches = self.launches.clone();
        let result = self
            .db
            .write_checked_with_commit(
                move |tx| {
                    let Some(mut task) = store::get(tx, &id)? else {
                        return Ok(Err(()));
                    };
                    // Checked in the writer, so a resume committed first is never opened.
                    if store::binding(tx, &id)?.and_then(|binding| binding.launch_id)
                        != Some(launch.clone())
                    {
                        return Ok(Err(()));
                    }
                    launches.ready(&id, &launch);
                    // The app stops timing the launch.
                    if task.starting_since_ms.take().is_none() {
                        return Ok(Err(()));
                    }
                    store::put(tx, &task)?;
                    Ok(Ok(task))
                },
                move |task, rev| changed(&hub, task, rev, false),
            )
            .await
            .internal("record pi ready")?;
        Ok(match result {
            Ok((_, rev)) => Done {
                rev: rev.to_string(),
            },
            Err(()) => unchanged(),
        })
    }
    /// Announces pi's trust question after a delay, only if the task still waits on it then.
    /// The task itself waits at once; only the notice, which can raise a notification, waits.
    pub(crate) fn announce_later(&self, asked: AgentTask) {
        let (db, hub, stop) = (self.db.clone(), self.hub.clone(), self.stop.clone());
        self.background.spawn(async move {
            tokio::select! {
                _ = stop.cancelled() => return,
                _ = tokio::time::sleep(TRUST_NOTICE_DELAY) => {}
            }
            let id = asked.id.clone();
            // Read in the writer, so the notice follows every report committed before it.
            let still = db
                .write_checked_with_commit(
                    move |tx| {
                        Ok(match store::get(tx, &id)? {
                            Some(task)
                                if task.state == AgentState::Waiting
                                    && task.attention == asked.attention =>
                            {
                                Ok(task)
                            }
                            _ => Err(()),
                        })
                    },
                    move |task, rev| announce(&hub, task, rev),
                )
                .await;
            if let Err(error) = still {
                tracing::warn!(?error, "pi trust notice skipped");
            }
        });
    }
}

#[cfg(test)]
#[path = "ready_tests.rs"]
mod tests;
