use crate::commands::terminal::SessionState;
use crate::core::models::{ContainerInfo, DistroInfo};
use crate::core::parsers::{decode_output, parse_docker_ps, parse_wsl_list_checked};
use crate::runtime_snapshot::request_snapshot_write;
use std::process::Stdio;
use std::sync::Arc;
use tauri::State;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::time::{timeout_at, Duration, Instant};

const MAX_WSL_STDOUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_WSL_STDERR_BYTES: usize = 64 * 1024;
const WSL_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
// Docker normally waits ten seconds for a Linux container before killing it.
const DOCKER_ACTION_TIMEOUT: Duration = Duration::from_secs(25);

const MAX_DISTRO_BYTES: usize = 128;
const MAX_CONTAINER_ID_BYTES: usize = 128;
const SAFE_WSL_ERROR: &str = "WSL 상태를 읽거나 명령을 실행하지 못했습니다.";
const SAFE_DOCKER_ERROR: &str = "Docker 상태를 안전하게 처리하지 못했습니다.";

/// Return one complete, single-flight dashboard snapshot. Resource data, Docker state and
/// terminal counts are collected by the same producer path that writes the read-only runtime
/// integration snapshot, so the UI never mixes generations.

pub async fn dashboard_snapshot(
    state: State<'_, Arc<SessionState>>,
) -> Result<crate::core::runtime_snapshot::DashboardSnapshot, String> {
    crate::runtime_snapshot::refresh_dashboard_snapshot(Arc::clone(state.inner())).await
}

const DOCKER_PS_FORMAT: &str = "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}";

/// WSL 배포판 목록을 조회한다. distro 모델은 `DistroInfo` 하나로 통일됐다
/// (wsl-dashboard의 `parse_wsl_list` 채택. 터미널 UI는 `.name`만 쓴다).

pub async fn list_distros(state: State<'_, Arc<SessionState>>) -> Result<Vec<DistroInfo>, String> {
    let output = run_wsl(&["-l", "-v"], None).await?;
    let distros = parse_wsl_list_checked(&output).map_err(|_| SAFE_WSL_ERROR.to_owned())?;
    request_snapshot_write(Arc::clone(state.inner()));
    Ok(distros)
}

/// Docker 컨테이너 목록을 조회한다 (기본 distro에서 docker CLI 실행).

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
    let output = run_wsl_bound(
        &["-d", &distro, "--", "docker", &action, "--", &container_id],
        None,
        None,
        DOCKER_ACTION_TIMEOUT,
    )
    .await?;
    let _ = output;
    Ok(())
}

/// Product container controls retain a native distro/executable lease through
/// fresh full-ID observation and action. Friendly aliases never become a kill PID.
pub async fn docker_action_owned(
    distro: &str,
    container_id: &str,
    action: &str,
    lease: &dyn crate::component::TerminalLaunchLease,
    request_budget: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + request_budget.min(Duration::from_secs(29));
    let distro = normalize_distro(distro)?;
    if !matches!(action, "start" | "stop" | "restart")
        || container_id.len() != 64
        || !container_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(SAFE_DOCKER_ERROR.into());
    }
    let output = run_wsl_bound(
        &[
            "-d",
            &distro,
            "--exec",
            "docker",
            "ps",
            "-a",
            "--no-trunc",
            "--format",
            DOCKER_PS_FORMAT,
        ],
        None,
        Some(lease),
        WSL_COMMAND_TIMEOUT.min(deadline.saturating_duration_since(Instant::now())),
    )
    .await?;
    let containers = parse_docker_ps(&output).map_err(|_| SAFE_DOCKER_ERROR)?;
    if containers
        .iter()
        .filter(|container| container.id == container_id)
        .count()
        != 1
    {
        return Err(SAFE_DOCKER_ERROR.into());
    }
    lease.revalidate()?;
    run_wsl_bound(
        &[
            "-d",
            &distro,
            "--exec",
            "docker",
            action,
            "--",
            container_id,
        ],
        None,
        Some(lease),
        DOCKER_ACTION_TIMEOUT.min(deadline.saturating_duration_since(Instant::now())),
    )
    .await?;
    Ok(())
}

/// `wsl.exe` 명령을 실행하고 bounded stdout만 반환한다. stderr, OS status, path와
/// command line은 호출자에게 반향하지 않는다.
async fn run_wsl(args: &[&str], cwd: Option<&str>) -> Result<String, String> {
    run_wsl_bound(args, cwd, None, WSL_COMMAND_TIMEOUT).await
}
async fn run_wsl_bound(
    args: &[&str],
    cwd: Option<&str>,
    lease: Option<&dyn crate::component::TerminalLaunchLease>,
    command_timeout: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + command_timeout;
    let mut argv = vec!["wsl.exe".to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    if let Some(lease) = lease {
        argv = lease.bind_argv(argv)?;
    }
    if Instant::now() >= deadline {
        return Err(SAFE_WSL_ERROR.into());
    }
    let (program, args) = argv.split_first().ok_or(SAFE_WSL_ERROR)?;
    let mut cmd = Command::new(program);
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
    let result = timeout_at(deadline, async {
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
