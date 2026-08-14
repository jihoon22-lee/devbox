use crate::core::models::{ContainerInfo, DistroInfo, GitStatus};
use crate::core::parsers::{decode_output, parse_docker_ps, parse_git_status, parse_wsl_list};
use tokio::process::Command;

/// WSL 배포판 목록을 조회한다. distro 모델은 `DistroInfo` 하나로 통일됐다
/// (wsl-dashboard의 `parse_wsl_list` 채택. 터미널 UI는 `.name`만 쓴다).
#[tauri::command]
pub async fn list_distros() -> Result<Vec<DistroInfo>, String> {
    let output = run_wsl(&["-l", "-v"], None).await?;
    Ok(parse_wsl_list(&output))
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
pub async fn docker_ps(distro: String) -> Result<Vec<ContainerInfo>, String> {
    let output = run_wsl(&["-d", &distro, "--", "docker", "ps", "-a"], None).await?;
    Ok(parse_docker_ps(&output))
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

/// 등록된 프로젝트 경로들의 git 상태를 조회한다.
// TODO(workbench): 프로젝트 목록과 git 상태는 Workbench의 ProjectProfile로 이관한다.
// (docs/product-opportunities.md §3.1, §15.2). Workbench 출시 전까지 여기서 유지한다.
#[tauri::command]
pub async fn git_status(projects: Vec<String>) -> Result<Vec<GitStatus>, String> {
    let mut out = Vec::new();
    for path in projects {
        let mut cmd = Command::new("git");
        cmd.args(["-C", &path, "status", "--porcelain", "--branch"]);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let result = cmd.output().await;
        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                out.push(parse_git_status(&path, &stdout));
            }
            Err(_) => out.push(GitStatus {
                path,
                branch: "n/a".into(),
                changes: 0,
                clean: false,
            }),
        }
    }
    Ok(out)
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
