//! Read-only, bounded accounting for the exact scopes owned by live agent tasks.
use std::{
    collections::HashMap,
    path::PathBuf,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

const MAX_OUTPUT: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
enum ResourceError {
    #[error("invalid scope name")]
    InvalidScope,
    #[error("resource provider failed")]
    Provider,
    #[error("resource query timed out")]
    Timeout,
    #[error("resource output exceeds limit")]
    OutputLimit,
    #[error("invalid resource output")]
    InvalidOutput,
    #[error("resource provider IO failed")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
struct Accounting {
    invocation: Option<String>,
    cpu_ns: Option<u64>,
    memory: Option<u64>,
}

#[derive(Debug)]
struct Baseline {
    invocation: String,
    cpu_ns: u64,
    at: Instant,
}

pub(crate) struct ResourceSampler {
    program: PathBuf,
    timeout: Duration,
    previous: HashMap<String, Baseline>,
    launches: HashMap<String, i64>,
}

impl Default for ResourceSampler {
    fn default() -> Self {
        Self {
            program: "systemctl".into(),
            timeout: TIMEOUT,
            previous: HashMap::new(),
            launches: HashMap::new(),
        }
    }
}

impl ResourceSampler {
    /// Callers supply only scopes for owned tasks with a live agent terminal.
    /// Retaining the exact eligible set prevents a disappeared task's baseline reuse.
    pub(crate) fn retain(&mut self, scopes: &[String]) {
        self.previous.retain(|scope, _| scopes.contains(scope));
        self.launches.retain(|scope, _| scopes.contains(scope));
    }

    pub(crate) fn reset(&mut self, scope: &str) {
        self.previous.remove(scope);
    }

    pub(crate) async fn sample(
        &mut self,
        _id: &str,
        scope: &str,
        launch_ms: i64,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> (Option<f64>, Option<u64>) {
        if self.launches.insert(scope.into(), launch_ms) != Some(launch_ms) {
            self.reset(scope);
        }
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ResourceError::Provider),
            result = self.query(scope) => result,
        };
        match result {
            Ok(accounting) => self.record(scope, accounting, Instant::now()),
            Err(error) => {
                self.previous.remove(scope);
                tracing::debug!(%error, "agent resource accounting unavailable");
                (None, None)
            }
        }
    }

    fn record(
        &mut self,
        scope: &str,
        accounting: Accounting,
        at: Instant,
    ) -> (Option<f64>, Option<u64>) {
        let previous = self.previous.remove(scope);
        let cpu = match (accounting.invocation, accounting.cpu_ns) {
            (Some(invocation), Some(cpu_ns)) => {
                let percent = previous.and_then(|previous| {
                    if previous.invocation != invocation {
                        return None;
                    }
                    let elapsed = at.checked_duration_since(previous.at)?.as_secs_f64();
                    let delta = cpu_ns.checked_sub(previous.cpu_ns)?;
                    if elapsed <= 0.0 {
                        return None;
                    }
                    let percent = delta as f64 / 1_000_000_000.0 / elapsed * 100.0;
                    percent.is_finite().then_some(percent)
                });
                self.previous.insert(
                    scope.into(),
                    Baseline {
                        invocation,
                        cpu_ns,
                        at,
                    },
                );
                percent
            }
            _ => None,
        };
        (cpu, accounting.memory)
    }

    async fn query(&self, scope: &str) -> Result<Accounting, ResourceError> {
        // No shell/option injection, and never query a foreign service/slice name.
        if !scope.starts_with("devbox-")
            || !scope.ends_with(".scope")
            || !scope
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return Err(ResourceError::InvalidScope);
        }
        let mut command = Command::new(&self.program);
        command
            .args([
                "--user",
                "show",
                "--no-pager",
                "-p",
                "CPUUsageNSec",
                "-p",
                "MemoryCurrent",
                "-p",
                "LoadState",
                "-p",
                "ActiveState",
                "-p",
                "InvocationID",
                "--",
                scope,
            ])
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn()?;
        #[cfg(unix)]
        let _group = OwnedGroup(child.id().ok_or(ResourceError::Provider)?);
        let stdout = child.stdout.take().ok_or(ResourceError::Provider)?;
        let stderr = child.stderr.take().ok_or(ResourceError::Provider)?;
        let run = async {
            let (stdout, _) = tokio::try_join!(read_bounded(stdout), read_bounded(stderr))?;
            if !child.wait().await?.success() {
                return Err(ResourceError::Provider);
            }
            let output = std::str::from_utf8(&stdout).map_err(|_| ResourceError::InvalidOutput)?;
            parse(output)
        };
        let result = tokio::time::timeout(self.timeout, run)
            .await
            .unwrap_or(Err(ResourceError::Timeout));
        if result.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        result
    }
}

async fn read_bounded(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, ResourceError> {
    let mut output = Vec::new();
    reader
        .take((MAX_OUTPUT + 1) as u64)
        .read_to_end(&mut output)
        .await?;
    if output.len() > MAX_OUTPUT {
        return Err(ResourceError::OutputLimit);
    }
    Ok(output)
}

fn parse(output: &str) -> Result<Accounting, ResourceError> {
    let mut fields = HashMap::new();
    for line in output.lines() {
        let (key, value) = line.split_once('=').ok_or(ResourceError::InvalidOutput)?;
        if fields.insert(key, value).is_some() {
            return Err(ResourceError::InvalidOutput);
        }
    }
    if fields.get("LoadState") != Some(&"loaded") || fields.get("ActiveState") != Some(&"active") {
        return Err(ResourceError::Provider);
    }
    let counter = |key| {
        fields
            .get(key)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value != u64::MAX)
    };
    let invocation = fields
        .get("InvocationID")
        .filter(|value| {
            value.len() == 32
                && value.bytes().all(|c| c.is_ascii_hexdigit())
                && value.bytes().any(|c| c != b'0')
        })
        .map(|value| (*value).to_owned());
    Ok(Accounting {
        invocation,
        cpu_ns: counter("CPUUsageNSec"),
        memory: counter("MemoryCurrent"),
    })
}

// Cancellation drops this guard as well as kill_on_drop's direct child.
#[cfg(unix)]
struct OwnedGroup(u32);
#[cfg(unix)]
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        if let Ok(pid) = i32::try_from(self.0) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
