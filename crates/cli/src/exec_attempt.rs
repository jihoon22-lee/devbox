//! Each systemd attempt obtains a fresh, bounded payload before starting exec.
use std::{ffi::OsString, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;
pub async fn run(run: String, command: Vec<OsString>) -> anyhow::Result<std::process::ExitStatus> {
    devbox_runtime::exit_record::validate_identity(&run)?;
    let paths = crate::paths()?;
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Request<'a> {
        run_id: &'a str,
    }
    let payload = tokio::time::timeout(Duration::from_secs(10), async {
        let client = crate::client::Client::connect(
            &paths.socket,
            devbox_protocol::message::ClientKind::Cli,
            paths.instance.as_str(),
        )
        .await?;
        let stream = client
            .open_stream("secrets.run", Request { run_id: &run })
            .await?;
        stream.read_secret_payload().await
    })
    .await
    .map_err(|_| anyhow::anyhow!("secret attempt unavailable"))?
    .map_err(|_| anyhow::anyhow!("secret attempt unavailable"))?;
    let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .args([
            "--instance",
            paths.instance.as_str(),
            "exec",
            "--run",
            &run,
            "--secrets-stdin",
            "--",
        ])
        .args(command)
        .stdin(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("secret input pipe unavailable"))?;
    let write = input.write_all(&payload).await;
    drop(input);
    drop(payload);
    if write.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
        anyhow::bail!("secret input pipe unavailable")
    }
    Ok(child.wait().await?)
}
