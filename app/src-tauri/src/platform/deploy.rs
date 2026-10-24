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

static MAINTENANCE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub(super) async fn maintenance_wait(
    cancel: CancellationToken,
) -> Result<tokio::sync::MutexGuard<'static, ()>, ShellError> {
    tokio::select! { biased; _ = cancel.cancelled() => Err(ShellError::Cancelled), guard = MAINTENANCE.lock() => Ok(guard) }
}
pub(super) fn maintenance_lock()
-> Result<tokio::sync::MutexGuard<'static, ()>, crate::core::connection::IpcError> {
    MAINTENANCE
        .try_lock()
        .map_err(|_| crate::core::connection::IpcError::new("다른 설치·정리 작업이 진행 중입니다"))
}
pub fn bundled_binary(app: &tauri::AppHandle) -> Result<Vec<u8>, ShellError> {
    use tauri::Manager;
    let path = app
        .path()
        .resolve("devbox-linux", tauri::path::BaseDirectory::Resource)
        .map_err(|error| ShellError::Invalid(error.to_string()))?;
    let resource_dir = path
        .parent()
        .ok_or_else(|| ShellError::Invalid("내장 리소스 경로 없음".into()))?;
    crate::core::self_check::validate_resources(resource_dir, env!("CARGO_PKG_VERSION"))?;
    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Err(ShellError::Invalid("내장 바이너리가 비어 있습니다".into()));
    }
    Ok(bytes)
}
pub async fn apply(
    distro: &str,
    instance: &str,
    bytes: Vec<u8>,
    cancel: CancellationToken,
) -> Result<(), ShellError> {
    let install_cancel = cancel.clone();
    crate::core::deploy::apply_deployment(
        || install(distro, bytes, install_cancel),
        || setup(distro, instance, cancel),
    )
    .await
}
#[tauri::command]
pub async fn redeploy_wsl(
    app: tauri::AppHandle,
    conn: tauri::State<'_, super::connection::Connection>,
) -> Result<(), crate::core::connection::IpcError> {
    use crate::core::connection::{INSTANCE, IpcError};
    let _guard = maintenance_lock()?;
    let distro = conn.state().ready_distro()?.to_owned();
    let bytes = bundled_binary(&app)
        .map_err(|_| IpcError::new("내장 WSL 실행 파일을 확인하지 못했습니다"))?;
    apply(&distro, INSTANCE, bytes, conn.operation_cancel())
        .await
        .map_err(|_| IpcError::new("WSL 다시 설정을 완료하지 못했습니다"))
}
