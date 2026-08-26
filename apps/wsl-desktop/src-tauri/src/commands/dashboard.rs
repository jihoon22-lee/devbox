use crate::commands::terminal::SessionState;
use crate::core::models::{ContainerInfo, DistroInfo};
use crate::core::parsers::{decode_output, parse_docker_ps, parse_wsl_list};
use crate::runtime_snapshot::request_snapshot_write;
use std::sync::Arc;
use tauri::State;
use tokio::process::Command;

const DOCKER_PS_FORMAT: &str = "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}";

/// WSL 배포판 목록을 조회한다. distro 모델은 `DistroInfo` 하나로 통일됐다
/// (wsl-dashboard의 `parse_wsl_list` 채택. 터미널 UI는 `.name`만 쓴다).
#[tauri::command]
pub async fn list_distros(state: State<'_, Arc<SessionState>>) -> Result<Vec<DistroInfo>, String> {
    let output = run_wsl(&["-l", "-v"], None).await?;
    let distros = parse_wsl_list(&output);
    request_snapshot_write(Arc::clone(state.inner()));
    Ok(distros)
}

/// 임의의 WSL 명령을 지정한 배포판에서 실행하고 출력을 반환한다.
#[tauri::command]
pub async fn run_wsl_command(distro: String, command: String) -> Result<String, String> {
    let argv =
        devbox_wsl::argv::build_exec_argv(&distro, None, &command).map_err(|e| e.to_string())?;
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
        return Err(format!("지원하지 않는 액션: {action}"));
    }
    let output = run_wsl(
        &["-d", &distro, "--", "docker", &action, &container_id],
        None,
    )
    .await?;
    if output.to_lowercase().contains("error") {
        return Err(output);
    }
    Ok(())
}

/// `wsl.exe` 명령을 실행하고 stdout을 반환한다.
async fn run_wsl(args: &[&str], cwd: Option<&str>) -> Result<String, String> {
    let mut cmd = Command::new("wsl.exe");
    cmd.args(args);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: 콘솔 창 깜빡임 방지
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("wsl.exe 실행 실패: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("명령 실패 ({}): {}", output.status, stderr.trim()));
    }
    Ok(decode_output(&output.stdout))
}
