use super::state::Services;
use devbox_protocol::{
    Fail,
    system::{RefreshEnvResult, SystemError},
};
use devbox_terminal::tmux::{Tmux, TmuxError};
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
/// The tmux server's global environment, which new sessions start with.
pub(crate) trait GlobalEnv {
    async fn set_global_env(&self, key: &str, value: Option<&str>) -> Result<(), TmuxError>;
}
impl GlobalEnv for Tmux {
    async fn set_global_env(&self, key: &str, value: Option<&str>) -> Result<(), TmuxError> {
        Tmux::set_global_env(self, key, value).await
    }
}
/// Refreshes the env file, then gives the tmux server the allowlisted keys that changed. The
/// server is its own unit and keeps its environment across a daemon restart; only sessions
/// created afterwards see the change. A failed update is logged: the refresh still succeeded.
async fn refresh_with(
    paths: &devbox_core::Paths,
    runner: &impl crate::setup::Runner,
    tmux: &impl GlobalEnv,
) -> Result<(), String> {
    for (key, value) in crate::setup::refresh_env_changes(paths, runner).await? {
        if let Err(error) = tmux.set_global_env(&key, value.as_deref()).await {
            tracing::warn!(%key, %error, "tmux global environment update failed");
        }
    }
    Ok(())
}
pub async fn refresh_env(s: Arc<Services>) -> Result<RefreshEnvResult, Fail<SystemError>> {
    refresh_with(&s.paths, &crate::setup::OsRunner, s.terminal.tmux())
        .await
        .map_err(|error| {
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
    struct FakeEnv(std::path::PathBuf);
    impl devbox_core::paths::Env for FakeEnv {
        fn var(&self, key: &str) -> Option<String> {
            (key == "HOME").then(|| self.0.display().to_string())
        }
        fn uid(&self) -> u32 {
            12345
        }
    }
    /// The login shell's `env -0`, captured without running a shell.
    struct Capture(Result<&'static [u8], &'static str>);
    impl crate::setup::Runner for Capture {
        async fn run(&self, cmd: &str, args: &[&str]) -> Result<Vec<u8>, String> {
            assert_eq!((cmd, args), ("bash", &["-lc", "env -0"][..]));
            self.0.map(<[u8]>::to_vec).map_err(Into::into)
        }
    }
    /// Records tmux updates; refuses the key named in `refuse`.
    #[derive(Default)]
    struct FakeTmux {
        calls: std::sync::Mutex<Vec<(String, Option<String>)>>,
        refuse: Option<&'static str>,
    }
    impl FakeTmux {
        fn calls(&self) -> Vec<(String, Option<String>)> {
            self.calls.lock().expect("owned fixture").clone()
        }
    }
    impl GlobalEnv for FakeTmux {
        async fn set_global_env(&self, key: &str, value: Option<&str>) -> Result<(), TmuxError> {
            self.calls
                .lock()
                .expect("owned fixture")
                .push((key.into(), value.map(Into::into)));
            match self.refuse {
                Some(refused) if refused == key => Err(TmuxError::Timeout),
                _ => Ok(()),
            }
        }
    }
    #[tokio::test]
    async fn refresh_env_updates_changed_allowlisted_keys_in_tmux() {
        let temp = tempfile::tempdir().expect("owned fixture");
        let paths = devbox_core::Paths::resolve(
            devbox_core::Instance::parse("test-refresh-tmux").expect("owned fixture"),
            &FakeEnv(temp.path().into()),
        )
        .expect("owned fixture");
        std::fs::create_dir_all(&paths.instance_config_dir).expect("owned fixture");
        let env = paths.instance_config_dir.join("env");
        std::fs::write(
            &env,
            "EDITOR=\"vi\"\nLANG=\"C\"\nPATH=\"/old/bin\"\nGITHUB_TOKEN=\"hand-added\"\n",
        )
        .expect("owned fixture");
        let captured = b"PATH=/new/bin:/usr/bin\0LANG=C\0TZ=Asia/Seoul\0NPM_TOKEN=fixture-secret\0";
        // tmux refusing one key is only logged: the refresh succeeded and the rest still apply.
        let tmux = FakeTmux {
            refuse: Some("EDITOR"),
            ..FakeTmux::default()
        };
        refresh_with(&paths, &Capture(Ok(captured)), &tmux)
            .await
            .expect("refresh succeeds");
        assert_eq!(
            tmux.calls(),
            [
                ("EDITOR".into(), None),
                ("PATH".into(), Some("/new/bin:/usr/bin".into())),
                ("TZ".into(), Some("Asia/Seoul".into())),
            ]
        );
        let saved = std::fs::read_to_string(&env).expect("owned fixture");
        assert!(saved.contains("PATH=\"/new/bin:/usr/bin\""));
        assert!(!saved.contains("fixture-secret"));
        // Nothing changed: tmux is not touched.
        let tmux = FakeTmux::default();
        refresh_with(&paths, &Capture(Ok(captured)), &tmux)
            .await
            .expect("refresh succeeds");
        assert!(tmux.calls().is_empty());
        // A failed capture changes neither the file nor tmux.
        refresh_with(&paths, &Capture(Err("owned capture failure")), &tmux)
            .await
            .expect_err("capture failure");
        assert!(tmux.calls().is_empty());
        assert_eq!(std::fs::read_to_string(&env).expect("owned fixture"), saved);
    }
    #[tokio::test]
    async fn foreground_and_fixture_runtime_never_schedule_restart() {
        let runtime = Runtime::new(false);
        assert!(!runtime.schedule_restart());
        assert!(!runtime.restart_requested());
        assert!(!runtime.shutdown.is_cancelled());
    }
}
