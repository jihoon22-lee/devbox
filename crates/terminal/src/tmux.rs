//! Commands only target the devbox instance's dedicated server and exact sessions.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum TmuxError {
    #[error("tmux {operation}: {stderr}")]
    Failed { operation: String, stderr: String },
    #[error("tmux not runnable: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("tmux command timed out")]
    Timeout,
    #[error("invalid tmux session name")]
    InvalidSession,
}
impl TmuxError {
    fn missing(&self) -> bool {
        matches!(self, Self::Failed { stderr, .. } if stderr.contains("no server running") || stderr.contains("no sessions") || stderr.contains("can't find session") || (stderr.contains("error connecting to") && stderr.contains("No such file or directory")))
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct TmuxSession {
    pub name: String,
    pub attached: bool,
    pub command: String,
    pub path: String,
}
#[derive(Clone)]
pub struct Tmux {
    socket: String,
    conf: PathBuf,
}
impl Tmux {
    pub fn with(socket: String, conf: PathBuf) -> Self {
        Self { socket, conf }
    }
    pub fn for_paths(paths: &devbox_core::Paths) -> Self {
        Self::with(
            format!("devbox-{}", paths.instance.as_str()),
            paths.instance_config_dir.join("tmux.conf"),
        )
    }
    pub fn socket(&self) -> &str {
        &self.socket
    }
    pub fn conf(&self) -> &Path {
        &self.conf
    }
    fn cmd(&self) -> Command {
        let mut command = Command::new("tmux");
        command
            .args(["-L", &self.socket, "-f"])
            .arg(&self.conf)
            .env_remove("TMUX")
            .env_remove("TMUX_TMPDIR")
            .kill_on_drop(true);
        command
    }
    async fn output(&self, args: &[&str]) -> Result<Vec<u8>, TmuxError> {
        let out = tokio::time::timeout(Duration::from_secs(10), self.cmd().args(args).output())
            .await
            .map_err(|_| TmuxError::Timeout)??;
        if !out.status.success() {
            return Err(TmuxError::Failed {
                operation: args.first().unwrap_or(&"command").to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().into(),
            });
        }
        Ok(out.stdout)
    }
    pub async fn run(&self, args: &[&str]) -> Result<String, TmuxError> {
        self.output(args)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }
    pub async fn start_direct(&self) -> Result<(), TmuxError> {
        self.run(&["start-server", ";", "set", "-g", "exit-empty", "off"])
            .await
            .map(|_| ())
    }
    async fn wait_ready(&self, deadline: Duration) -> Result<(), TmuxError> {
        tokio::time::timeout(deadline, async {
            loop {
                match self.run(&["show-options", "-g", "exit-empty"]).await {
                    Ok(_) => return Ok(()),
                    Err(error) if error.missing() => {
                        tokio::time::sleep(Duration::from_millis(10)).await
                    }
                    Err(error) => return Err(error),
                }
            }
        })
        .await
        .map_err(|_| TmuxError::Timeout)?
    }
    pub async fn ensure_server(&self, instance: &devbox_core::Instance) -> Result<(), TmuxError> {
        if self
            .run(&["show-options", "-g", "exit-empty"])
            .await
            .is_ok()
        {
            return Ok(());
        }
        if !instance.is_test() {
            let mut command = Command::new("systemctl");
            command
                .args([
                    "--user",
                    "start",
                    &format!("devbox-tmux@{}.service", instance.as_str()),
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            if matches!(tokio::time::timeout(Duration::from_secs(10), command.status()).await, Ok(Ok(status)) if status.success())
            {
                // Type=exec confirms exec, not tmux socket readiness. Never
                // race the managed process by forking another server here.
                return self.wait_ready(Duration::from_secs(10)).await;
            }
        }
        self.start_direct().await
    }
    fn target(name: &str) -> Result<String, TmuxError> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(TmuxError::InvalidSession);
        }
        Ok(format!("={name}:"))
    }
    pub async fn new_session(
        &self,
        name: &str,
        cwd: &Path,
        command: Option<&str>,
        env: &[(String, String)],
    ) -> Result<(), TmuxError> {
        Self::target(name)?;
        let mut args = vec![
            "new-session".into(),
            "-d".into(),
            "-s".into(),
            name.into(),
            "-c".into(),
            cwd.to_string_lossy().into_owned(),
            "-x".into(),
            "200".into(),
            "-y".into(),
            "50".into(),
        ];
        for (key, value) in env {
            args.extend(["-e".into(), format!("{key}={value}")]);
        }
        if let Some(command) = command {
            args.push(command.into());
        }
        self.run(&args.iter().map(String::as_str).collect::<Vec<_>>())
            .await
            .map(|_| ())
    }
    pub async fn kill_session(&self, name: &str) -> Result<(), TmuxError> {
        Self::target(name)?;
        match self.run(&["kill-session", "-t", &format!("={name}")]).await {
            Ok(_) => Ok(()),
            Err(error) if error.missing() => Ok(()),
            Err(error) => Err(error),
        }
    }
    pub async fn kill_server(&self) {
        let _ = self.run(&["kill-server"]).await;
    }
    pub async fn list(&self) -> Result<Vec<TmuxSession>, TmuxError> {
        let output = match self.run(&["list-sessions", "-F", "#{session_name}\t#{session_attached}\t#{pane_current_command}\t#{pane_current_path}"]).await { Ok(out) => out, Err(error) if error.missing() => return Ok(vec![]), Err(error) => return Err(error) };
        Ok(output
            .lines()
            .filter_map(|line| {
                let mut fields = line.splitn(4, '\t');
                Some(TmuxSession {
                    name: fields.next()?.into(),
                    attached: fields.next()? != "0",
                    command: fields.next()?.into(),
                    path: fields.next()?.into(),
                })
            })
            .collect())
    }
    pub async fn capture_history(&self, name: &str, lines: u32) -> Result<Vec<u8>, TmuxError> {
        let target = Self::target(name)?;
        let history = self
            .run(&["display-message", "-p", "-t", &target, "#{history_size}"])
            .await?;
        let history: u32 = history.trim().parse().map_err(|_| TmuxError::Failed {
            operation: "history-size".into(),
            stderr: "invalid history size".into(),
        })?;
        if history == 0 || lines == 0 {
            return Ok(vec![]);
        }
        let lines = lines.min(history);
        let output = self
            .output(&[
                "capture-pane",
                "-p",
                "-e",
                "-J",
                "-S",
                &format!("-{lines}"),
                "-E",
                "-1",
                "-t",
                &target,
            ])
            .await?;
        let mut crlf = Vec::with_capacity(output.len());
        for byte in output {
            if byte == b'\n' {
                crlf.push(b'\r');
            }
            crlf.push(byte);
        }
        Ok(crlf)
    }
    pub async fn capture_tail(&self, name: &str, lines: u32) -> Result<String, TmuxError> {
        self.run(&[
            "capture-pane",
            "-p",
            "-J",
            "-S",
            &format!("-{lines}"),
            "-t",
            &Self::target(name)?,
        ])
        .await
    }
    pub async fn paste(&self, name: &str, text: &str, enter: bool) -> Result<(), TmuxError> {
        let target = Self::target(name)?;
        let buffer = format!("devbox-{}", uuid::Uuid::new_v4().simple());
        let result = async {
            let mut child = self
                .cmd()
                .args(["load-buffer", "-b", &buffer, "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?;
            let loaded = tokio::time::timeout(Duration::from_secs(10), async {
                let mut input = child.stdin.take().expect("piped tmux input");
                input.write_all(text.as_bytes()).await?;
                drop(input);
                child.wait_with_output().await
            })
            .await
            .map_err(|_| TmuxError::Timeout)??;
            if !loaded.status.success() {
                return Err(TmuxError::Failed {
                    operation: "load-buffer".into(),
                    stderr: String::from_utf8_lossy(&loaded.stderr).trim().into(),
                });
            }
            self.run(&["paste-buffer", "-p", "-d", "-b", &buffer, "-t", &target])
                .await?;
            if enter {
                self.run(&["send-keys", "-t", &target, "Enter"]).await?;
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = self.run(&["delete-buffer", "-b", &buffer]).await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn managed_server_readiness_waits_for_the_socket_without_spawning_a_second_server() {
        let home = tempfile::tempdir().expect("fixture directory");
        let conf = home.path().join("tmux.conf");
        std::fs::write(&conf, crate::TMUX_CONF).expect("fixture config");
        let tmux = Tmux::with(
            format!("devbox-test-{}", uuid::Uuid::new_v4().simple()),
            conf,
        );
        let delayed = tmux.clone();
        let start = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            delayed.start_direct().await
        });
        let ready = tmux.wait_ready(Duration::from_secs(2)).await;
        let started = start.await.expect("fixture startup");
        tmux.kill_server().await;
        started.expect("start owned fixture server");
        ready.expect("wait for managed server readiness");
    }
    #[tokio::test]
    async fn managed_server_readiness_timeout_does_not_start_an_unmanaged_server() {
        let home = tempfile::tempdir().expect("fixture directory");
        let conf = home.path().join("tmux.conf");
        std::fs::write(&conf, crate::TMUX_CONF).expect("fixture config");
        let tmux = Tmux::with(
            format!("devbox-test-{}", uuid::Uuid::new_v4().simple()),
            conf,
        );
        assert!(matches!(
            tmux.wait_ready(Duration::from_millis(30)).await,
            Err(TmuxError::Timeout)
        ));
        assert!(tmux.list().await.expect("no unmanaged server").is_empty());
    }
}
