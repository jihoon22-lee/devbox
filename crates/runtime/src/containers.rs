//! Bounded Docker CLI calls. No shell and no Docker daemon ownership.
use crate::*;
use devbox_protocol::{Fail, Internal};
use serde::Deserialize;
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

type R<T> = Result<T, Fail<ContainerError>>;
pub(crate) const OUTPUT_LIMIT: usize = 256 * 1024;
const STDERR_LIMIT: usize = 16 * 1024;

#[derive(Clone)]
pub struct DockerContainers {
    program: PathBuf,
    // List should not stall the Runs screen. Actions allow Docker's normal stop grace.
    pub(crate) list_timeout: Duration,
    pub(crate) action_timeout: Duration,
}
impl Default for DockerContainers {
    fn default() -> Self {
        Self::new("docker".into())
    }
}
impl DockerContainers {
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            list_timeout: Duration::from_secs(3),
            action_timeout: Duration::from_secs(30),
        }
    }
    pub async fn list(&self, cancel: CancellationToken) -> R<ContainersResult> {
        let output = self
            .run(
                vec!["ps".into(), "-a".into(), "--format".into(), "json".into()],
                self.list_timeout,
                cancel,
            )
            .await?;
        match output {
            CliOutput::Missing => Ok(unavailable(ContainerUnavailableReason::CliMissing)),
            CliOutput::Finished { success: false, .. } => {
                Ok(unavailable(ContainerUnavailableReason::DaemonUnavailable))
            }
            CliOutput::Finished { stdout, .. } => parse_ps(&stdout),
        }
    }
    pub async fn action(
        &self,
        params: ContainerActionParams,
        cancel: CancellationToken,
    ) -> R<ContainerActionResult> {
        if !valid_id(&params.id) {
            return Err(ContainerError::InvalidId.into());
        }
        let args = container_args(&params.id, params.action)
            .map(str::to_owned)
            .to_vec();
        match self.run(args, self.action_timeout, cancel).await? {
            CliOutput::Missing => return Err(ContainerError::Unavailable.into()),
            CliOutput::Finished { success: false, .. } => return Err(ContainerError::Failed.into()),
            CliOutput::Finished { .. } => (),
        }
        let undo = (params.action == ContainerAction::Stop).then(|| ContainerActionParams {
            id: params.id.clone(),
            action: ContainerAction::Start,
        });
        Ok(ContainerActionResult {
            id: params.id,
            action: params.action,
            undo,
        })
    }
    async fn run(
        &self,
        args: Vec<String>,
        timeout: Duration,
        parent: CancellationToken,
    ) -> R<CliOutput> {
        let cancel = parent.child_token();
        // If the RPC future is dropped, the owned worker still kills AND reaps its child.
        let _guard = cancel.clone().drop_guard();
        let program = self.program.clone();
        tokio::spawn(async move { execute(program, args, timeout, cancel).await })
            .await
            .internal("join container CLI")?
    }
}

pub(crate) fn container_args(id: &str, action: ContainerAction) -> [&str; 2] {
    [
        match action {
            ContainerAction::Start => "start",
            ContainerAction::Stop => "stop",
            ContainerAction::Restart => "restart",
        },
        id,
    ]
}
fn valid_id(id: &str) -> bool {
    // Same ID/name grammar as Task 7 Container log sources; a leading option is impossible.
    !id.is_empty()
        && id.len() <= 256
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn unavailable(reason: ContainerUnavailableReason) -> ContainersResult {
    ContainersResult {
        available: false,
        reason: Some(reason),
        items: vec![],
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DockerRow {
    #[serde(rename = "ID")]
    id: String,
    names: String,
    image: String,
    state: String,
    status: String,
    ports: String,
    #[serde(default)]
    labels: String,
}
pub(crate) fn parse_ps(bytes: &[u8]) -> R<ContainersResult> {
    if bytes.len() > OUTPUT_LIMIT {
        return Err(ContainerError::OutputLimit.into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ContainerError::InvalidOutput)?;
    let mut items = vec![];
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: DockerRow =
            serde_json::from_str(line).map_err(|_| ContainerError::InvalidOutput)?;
        if !valid_id(&row.id) {
            return Err(ContainerError::InvalidOutput.into());
        }
        if items.len() >= 1024 {
            return Err(ContainerError::OutputLimit.into());
        }
        let compose_project = row.labels.split(',').find_map(|label| {
            label
                .strip_prefix("com.docker.compose.project=")
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        });
        items.push(ContainerInfo {
            id: row.id,
            name: row.names,
            image: row.image,
            state: row.state,
            status: row.status,
            ports: row.ports,
            compose_project,
        });
    }
    Ok(ContainersResult {
        available: true,
        reason: None,
        items,
    })
}
enum CliOutput {
    Missing,
    Finished { success: bool, stdout: Vec<u8> },
}
async fn read_bounded(reader: impl AsyncRead + Unpin, limit: usize) -> R<Vec<u8>> {
    let mut bytes = vec![];
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "container CLI output read failed");
            ContainerError::Failed
        })?;
    if bytes.len() > limit {
        return Err(ContainerError::OutputLimit.into());
    }
    Ok(bytes)
}
async fn execute(
    program: PathBuf,
    args: Vec<String>,
    timeout: Duration,
    cancel: CancellationToken,
) -> R<CliOutput> {
    if cancel.is_cancelled() {
        return Err(ContainerError::Cancelled.into());
    }
    let mut child = match tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CliOutput::Missing);
        }
        Err(error) => {
            tracing::warn!(%error, "container CLI spawn failed");
            return Err(ContainerError::Unavailable.into());
        }
    };
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(ContainerError::Cancelled.into()),
        _ = tokio::time::sleep(timeout) => Err(ContainerError::Timeout.into()),
        result = async {
            let (stdout, stderr) = tokio::try_join!(read_bounded(stdout, OUTPUT_LIMIT), read_bounded(stderr, STDERR_LIMIT))?;
            let status = child.wait().await.internal("wait container CLI")?;
            if !status.success() {
                tracing::warn!(status = ?status.code(), stderr = %String::from_utf8_lossy(&stderr), "container CLI failed");
            }
            Ok(CliOutput::Finished { success: status.success(), stdout })
        } => result,
    };
    if result.is_err() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    result
}
