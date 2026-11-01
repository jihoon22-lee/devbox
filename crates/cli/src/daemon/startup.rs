//! What the daemon started without (`system.info.startupProblems`), and the background
//! retry of agent initialization while tmux cannot list sessions.
use super::state::Services;
use devbox_agents::{AgentsError, KnownTasks};
use devbox_core::config::Disabled;
use devbox_protocol::{Fail, system::StartupProblem};
use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Config problems stay until the next start; the agents one clears when a retry succeeds.
pub(super) struct Startup {
    pub disabled: Disabled,
    config: Vec<StartupProblem>,
    agents: Mutex<Option<String>>,
}

impl Startup {
    pub fn new(disabled: Disabled, config: Vec<StartupProblem>) -> Self {
        for problem in &config {
            tracing::warn!(?problem, "started without part of the configuration");
        }
        Self {
            disabled,
            config,
            agents: Mutex::default(),
        }
    }

    pub fn problems(&self) -> Vec<StartupProblem> {
        let mut problems = self.config.clone();
        if let Some(message) = self.agents.lock().expect("startup problems").clone() {
            problems.push(StartupProblem::AgentsReconcile { message });
        }
        problems
    }

    fn agents(&self, error: Option<&Fail<AgentsError>>) {
        let message = error.map(|error| match error {
            Fail::Domain(error) => error.to_string(),
            Fail::Internal(message) => message.clone(),
        });
        *self.agents.lock().expect("startup problems") = message;
    }
}

/// Why `[windows]` is off, for `windows.config_invalid`. A `[windows]` in `config.toml`
/// is only ignored, so it is not a reason.
pub(super) fn windows_reason(problems: &[StartupProblem], local: &Path) -> String {
    problems
        .iter()
        .find_map(|problem| match problem {
            StartupProblem::ConfigFile { file, message } => Some(format!("{file}: {message}")),
            StartupProblem::ConfigSection {
                file,
                section,
                message,
                ..
            } if section == "windows" && Path::new(file) == local => {
                Some(format!("{file} [windows]: {message}"))
            }
            _ => None,
        })
        .unwrap_or_else(|| "[windows] did not load".into())
}

/// The first agents initialization, before the daemon serves requests. On failure the
/// daemon still starts: the problem is reported and [`retry_agents`] takes over with the
/// tasks as they are now, which is all a late reconcile may touch.
pub(super) async fn initialize_agents(services: &Services) -> Option<KnownTasks> {
    let error = services.agents.initialize().await.err()?;
    services.startup.agents(Some(&error));
    tracing::warn!(
        ?error,
        "agent tasks not reconciled; retrying in the background"
    );
    // No request has been served yet, so no task was created or relaunched meanwhile.
    Some(services.agents.known_tasks().await.unwrap_or_else(|error| {
        tracing::warn!(
            ?error,
            "agent tasks unreadable; a retry reconciles none of them"
        );
        KnownTasks::default()
    }))
}

/// Retries agent initialization until it succeeds or `shutdown` is cancelled. Await the
/// handle before shutting agents down.
pub(super) fn retry_agents(
    services: Arc<Services>,
    known: KnownTasks,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        retry(shutdown, || async {
            let result = services.agents.initialize_known(&known).await;
            services.startup.agents(result.as_ref().err());
            if result.is_ok() {
                tracing::info!("agent tasks reconciled after a retry");
            }
            result.is_ok()
        })
        .await;
    })
}

/// 1 s, 2 s, 5 s, 10 s, then every 30 s.
fn delay(attempt: usize) -> Duration {
    Duration::from_secs([1, 2, 5, 10].get(attempt).copied().unwrap_or(30))
}

/// Runs `attempt` on the retry schedule until it returns true or `shutdown` is cancelled.
async fn retry<F, Fut>(shutdown: CancellationToken, mut attempt: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    for n in 0.. {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(delay(n)) => {}
        }
        let done = tokio::select! {
            _ = shutdown.cancelled() => return,
            done = attempt() => done,
        };
        if done {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Instant;

    #[tokio::test(start_paused = true)]
    async fn retries_on_the_schedule_until_an_attempt_succeeds() {
        let start = Instant::now();
        let attempts = std::cell::RefCell::new(Vec::new());
        retry(CancellationToken::new(), || async {
            attempts.borrow_mut().push(start.elapsed().as_secs());
            attempts.borrow().len() == 7
        })
        .await;
        assert_eq!(*attempts.borrow(), [1, 3, 8, 18, 48, 78, 108]);
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_stops_waiting_and_running_attempts() {
        let shutdown = CancellationToken::new();
        let waiting = tokio::spawn(retry(shutdown.clone(), || async { false }));
        tokio::time::sleep(Duration::from_secs(4)).await;
        shutdown.cancel();
        waiting.await.expect("retry stops between attempts");
        let shutdown = CancellationToken::new();
        let cancel = shutdown.clone();
        let running = retry(shutdown, move || {
            cancel.cancel();
            std::future::pending::<bool>()
        });
        tokio::time::timeout(Duration::from_secs(5), running)
            .await
            .expect("retry stops during an attempt");
    }
}
