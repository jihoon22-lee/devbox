use super::state::Services;
use devbox_protocol::{
    Fail,
    system::{RefreshEnvResult, SystemError},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio_util::sync::CancellationToken;

/// Shares daemon lifecycle intent with RPC handlers without terminating their tasks.
#[derive(Clone)]
pub(super) struct Runtime {
    managed: bool,
    pub shutdown: CancellationToken,
    restart: Arc<AtomicBool>,
}
impl Runtime {
    pub fn new(managed: bool) -> Self {
        Self {
            managed,
            shutdown: CancellationToken::new(),
            restart: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn restart_requested(&self) -> bool {
        self.restart.load(Ordering::SeqCst)
    }
    fn schedule_restart(&self) -> bool {
        if !self.managed {
            return false;
        }
        if !self.restart.swap(true, Ordering::SeqCst) {
            let shutdown = self.shutdown.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                shutdown.cancel();
            });
        }
        true
    }
}
pub async fn refresh_env(s: Arc<Services>) -> Result<RefreshEnvResult, Fail<SystemError>> {
    crate::setup::refresh_env(&s.paths).await.map_err(|error| {
        tracing::warn!(%error, "shell environment refresh failed");
        Fail::Domain(SystemError::EnvRefreshFailed)
    })?;
    Ok(RefreshEnvResult {
        restarting: s.runtime.schedule_restart(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn managed_restart_waits_one_second_and_requests_graceful_drain() {
        let runtime = Runtime::new(true);
        assert!(runtime.schedule_restart());
        assert!(runtime.restart_requested());
        assert!(!runtime.shutdown.is_cancelled());
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_millis(999)).await;
        assert!(!runtime.shutdown.is_cancelled());
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert!(runtime.shutdown.is_cancelled());
    }
    #[tokio::test]
    async fn foreground_and_fixture_runtime_never_schedule_restart() {
        let runtime = Runtime::new(false);
        assert!(!runtime.schedule_restart());
        assert!(!runtime.restart_requested());
        assert!(!runtime.shutdown.is_cancelled());
    }
}
