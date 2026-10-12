use crate::core::{
    deploy::{digest, exec_script, install_script},
    diagnostics::{ReportKind, describe},
    process::{self, ShellError},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub async fn install(
    distro: &str,
    bytes: Vec<u8>,
    cancel: CancellationToken,
) -> Result<(), ShellError> {
    let sha = digest(&bytes);
    let mut cmd = crate::platform::wsl::command(distro);
    cmd.args(["--exec", "sh", "-c", &install_script(), "sh", &sha]);
    process::run(cmd, bytes, Duration::from_secs(120), cancel).await?;
    Ok(())
}
pub async fn setup(
    distro: &str,
    instance: &str,
    cancel: CancellationToken,
) -> Result<(), ShellError> {
    let mut cmd = crate::platform::wsl::command(distro);
    cmd.args([
        "--exec",
        "env",
        &format!("DEVBOX_INSTANCE={instance}"),
        "sh",
        "-c",
        &exec_script(),
        "sh",
        "setup",
        "--json",
    ]);
    process::run(cmd, vec![], Duration::from_secs(90), cancel.clone())
        .await
        .map_err(|e| describe(e, ReportKind::Setup))?;
    let mut doctor = crate::platform::wsl::command(distro);
    doctor.args([
        "--exec",
        "env",
        &format!("DEVBOX_INSTANCE={instance}"),
        "sh",
        "-c",
        &exec_script(),
        "sh",
        "doctor",
        "--json",
    ]);
    process::run(doctor, vec![], Duration::from_secs(30), cancel)
        .await
        .map_err(|e| describe(e, ReportKind::Doctor))?;
    Ok(())
}
