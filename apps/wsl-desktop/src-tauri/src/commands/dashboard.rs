use crate::commands::terminal::SessionState;
use crate::core::models::{ContainerInfo, DistroInfo};
use crate::core::parsers::{decode_output, parse_docker_ps, parse_wsl_list_checked};
use crate::runtime_snapshot::request_snapshot_write;
use std::process::Stdio;
use std::sync::Arc;
use tauri::State;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::time::{timeout, Duration};

const MAX_WSL_STDOUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_WSL_STDERR_BYTES: usize = 64 * 1024;
const WSL_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WSL_COMMAND_BYTES: usize = 4 * 1024;
const MAX_DISTRO_BYTES: usize = 128;
const MAX_CONTAINER_ID_BYTES: usize = 128;
const SAFE_WSL_ERROR: &str = "WSL 상태를 읽거나 명령을 실행하지 못했습니다.";
const SAFE_DOCKER_ERROR: &str = "Docker 상태를 안전하게 처리하지 못했습니다.";

/// Return one complete, single-flight dashboard snapshot. Resource data, Docker state and
/// terminal counts are collected by the same producer path that writes the read-only runtime
/// integration snapshot, so the UI never mixes generations.
#[tauri::command]
pub async fn dashboard_snapshot(
    state: State<'_, Arc<SessionState>>,
) -> Result<crate::core::runtime_snapshot::DashboardSnapshot, String> {
    crate::runtime_snapshot::refresh_dashboard_snapshot(Arc::clone(state.inner())).await
}

const DOCKER_PS_FORMAT: &str = "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}";

/// WSL 배포판 목록을 조회한다. distro 모델은 `DistroInfo` 하나로 통일됐다
/// (wsl-dashboard의 `parse_wsl_list` 채택. 터미널 UI는 `.name`만 쓴다).
#[tauri::command]
pub async fn list_distros(state: State<'_, Arc<SessionState>>) -> Result<Vec<DistroInfo>, String> {
    let output = run_wsl(&["-l", "-v"], None).await?;
    let distros = parse_wsl_list_checked(&output).map_err(|_| SAFE_WSL_ERROR.to_owned())?;
    request_snapshot_write(Arc::clone(state.inner()));
    Ok(distros)
}

/// 임의의 WSL 명령을 지정한 배포판에서 실행하고 출력을 반환한다.
#[tauri::command]
pub async fn run_wsl_command(distro: String, command: String) -> Result<String, String> {
    if command.is_empty() || command.len() > MAX_WSL_COMMAND_BYTES {
        return Err(SAFE_WSL_ERROR.into());
    }
    let distro = normalize_distro(&distro)?;
    let argv =
        devbox_wsl::argv::build_exec_argv(&distro, None, &command).map_err(|_| SAFE_WSL_ERROR)?;
    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    let output = run_wsl(&refs, None).await?;
    Ok(output)
}

/// Docker 컨테이너 목록을 조회한다 (기본 distro에서 docker CLI 실행).
#[tauri::command]
pub async fn docker_ps(
    state: State<'_, Arc<SessionState>>,
    distro: String,
) -> Result<Vec<ContainerInfo>, String> {
    let distro = normalize_distro(&distro)?;
    // Docker가 소유한 필드만 명시적으로 요청한다. 기본 table 출력은 COMMAND/CREATED의
    // 가변 공백 때문에 STATUS와 PORTS의 경계를 정확히 복원할 수 없다. --no-trunc는
    // detail에서 Docker가 반환한 ID/image/status/ports 원문을 그대로 보여 주기 위함이다.
    let output = run_wsl(
        &[
            "-d",
            &distro,
            "--",
            "docker",
            "ps",
            "-a",
            "--no-trunc",
            "--format",
            DOCKER_PS_FORMAT,
        ],
        None,
    )
    .await?;
    let containers = parse_docker_ps(&output).map_err(str::to_string)?;
    request_snapshot_write(Arc::clone(state.inner()));
    Ok(containers)
}

/// Docker 컨테이너를 start/stop/restart 한다.
#[tauri::command]
pub async fn docker_action(
    distro: String,
    container_id: String,
    action: String,
) -> Result<(), String> {
    if !matches!(action.as_str(), "start" | "stop" | "restart") {
        return Err(SAFE_DOCKER_ERROR.into());
    }
    let distro = normalize_distro(&distro).map_err(|_| SAFE_DOCKER_ERROR.to_owned())?;
    let container_id = normalize_container_id(&container_id)?;
    let output = run_wsl(
        &["-d", &distro, "--", "docker", &action, "--", &container_id],
        None,
    )
    .await?;
    let _ = output;
    Ok(())
}

/// `wsl.exe` 명령을 실행하고 bounded stdout만 반환한다. stderr, OS status, path와
/// command line은 호출자에게 반향하지 않는다.
async fn run_wsl(args: &[&str], cwd: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("wsl.exe");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: 콘솔 창 깜빡임 방지
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|_| SAFE_WSL_ERROR.to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SAFE_WSL_ERROR.to_owned())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| SAFE_WSL_ERROR.to_owned())?;
    let result = timeout(WSL_COMMAND_TIMEOUT, async {
        let (stdout, _) = tokio::try_join!(
            read_bounded(stdout, MAX_WSL_STDOUT_BYTES),
            drain_bounded(stderr, MAX_WSL_STDERR_BYTES),
        )
        .map_err(|_| SAFE_WSL_ERROR.to_owned())?;
        let status = child.wait().await.map_err(|_| SAFE_WSL_ERROR.to_owned())?;
        if !status.success() {
            return Err(SAFE_WSL_ERROR.to_owned());
        }
        Ok(decode_output(&stdout))
    })
    .await;

    match result {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => {
            terminate_child(&mut child).await;
            Err(error)
        }
        Err(_) => {
            terminate_child(&mut child).await;
            Err(SAFE_WSL_ERROR.into())
        }
    }
}

fn normalize_distro(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_DISTRO_BYTES
        || devbox_wsl::distro::validate_distro_name(value).is_err()
    {
        return Err(SAFE_WSL_ERROR.into());
    }
    Ok(value.to_owned())
}

fn normalize_container_id(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_CONTAINER_ID_BYTES
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(SAFE_DOCKER_ERROR.into());
    }
    Ok(value.to_owned())
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> Result<Vec<u8>, ()> {
    let mut output = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = reader.read(&mut buffer).await.map_err(|_| ())?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > max_bytes {
            return Err(());
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

async fn drain_bounded<R: AsyncRead + Unpin>(mut reader: R, max_bytes: usize) -> Result<(), ()> {
    let mut total = 0_usize;
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = reader.read(&mut buffer).await.map_err(|_| ())?;
        if count == 0 {
            return Ok(());
        }
        total = total.saturating_add(count);
        if total > max_bytes {
            return Err(());
        }
    }
}

async fn terminate_child(child: &mut Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_distro_and_container_inputs_are_bounded_and_argv_safe() {
        assert_eq!(normalize_distro(" Ubuntu 24.04 ").unwrap(), "Ubuntu 24.04");
        assert!(normalize_distro("Ubuntu;rm").is_err());
        assert!(normalize_distro(&"x".repeat(MAX_DISTRO_BYTES + 1)).is_err());

        assert_eq!(
            normalize_container_id("container_name-1").unwrap(),
            "container_name-1"
        );
        assert!(normalize_container_id("-rf").is_err());
        assert!(normalize_container_id("name/with-slash").is_err());
        assert!(normalize_container_id(&"x".repeat(MAX_CONTAINER_ID_BYTES + 1)).is_err());
    }
}
