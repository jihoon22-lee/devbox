use crate::core::{
    connection::{INSTANCE, IpcError},
    deploy::exec_script,
    process,
};
use std::time::Duration;
#[tauri::command]
pub async fn cleanup_wsl(
    conn: tauri::State<'_, super::connection::Connection>,
) -> Result<(), IpcError> {
    let _guard = super::deploy::maintenance_lock()?;
    let distro = conn.state().ready_distro()?.to_owned();
    // Stop and await only this app's supervisor attempt before removing the
    // channel binary. Explicit gate_retry resumes supervision later.
    conn.pause_for_cleanup().await?;
    let mut command = super::wsl::command(&distro);
    command.args([
        "--exec",
        "env",
        &format!("DEVBOX_INSTANCE={INSTANCE}"),
        "sh",
        "-c",
        &exec_script(),
        "sh",
        "uninstall",
        "--json",
    ]);
    process::run(
        command,
        vec![],
        Duration::from_secs(90),
        conn.operation_cancel(),
    )
    .await
    .map_err(|_| {
        IpcError::new("WSL 정리를 완료하지 못했습니다. 데이터와 설정은 정리 대상이 아닙니다")
    })?;
    Ok(())
}
