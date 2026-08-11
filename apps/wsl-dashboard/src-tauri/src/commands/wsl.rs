use crate::core::models::{ContainerInfo, DistroInfo, GitStatus};
use crate::core::parsers::{parse_docker_ps, parse_git_status, parse_wsl_list};
use tokio::process::Command;

/// WSL 배포판 목록을 조회한다.
#[tauri::command]
pub async fn list_distros() -> Result<Vec<DistroInfo>, String> {
    let output = run_wsl(&["-l", "-v"], None).await?;
    Ok(parse_wsl_list(&output))
}

/// 임의의 WSL 명령을 지정한 배포판에서 실행하고 출력을 반환한다.
#[tauri::command]
pub async fn run_wsl_command(distro: String, command: String) -> Result<String, String> {
    let output = run_wsl(&["-d", &distro, "--", &command], None).await?;
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

/// 기본 터미널(wt.exe)을 열어 WSL 셸을 실행한다.
#[tauri::command]
pub fn open_terminal(distro: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("wt.exe")
            .arg("-w")
            .arg("new")
            .arg("nt")
            .arg("wsl.exe")
            .arg("-d")
            .arg(&distro)
            .spawn()
            .map_err(|e| format!("터미널 실행 실패: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = distro;
        // 개발 중(비 Windows)에는 동작하지 않는다.
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

/// wsl.exe 출력 디코딩.
///
/// 파이프로 실행된 `wsl.exe -l -v`는 UTF-16LE(BOM 또는 다량의 NUL)로 출력한다.
/// 이 경우 UTF-16LE로, 그 외에는 UTF-8로 해석한다. (distro 이름에 NUL이 남아
/// 다음 명령의 인자가 되면 "null byte found in provided data" 오류가 발생한다)
fn decode_output(bytes: &[u8]) -> String {
    let bom = bytes.starts_with(&[0xFF, 0xFE]);
    let null_ratio = if bytes.is_empty() {
        0
    } else {
        bytes.iter().filter(|b| **b == 0).count() * 4 / bytes.len()
    };
    if bom || (bytes.len() >= 2 && null_ratio >= 1) {
        let start = if bom { 2 } else { 0 };
        let units: Vec<u16> = bytes[start..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::decode_output;

    #[test]
    fn decodes_plain_utf8() {
        assert_eq!(decode_output(b"Ubuntu\n"), "Ubuntu\n");
    }

    #[test]
    fn decodes_utf16le_with_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for ch in "Ubuntu".encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        let s = decode_output(&bytes);
        assert_eq!(s, "Ubuntu");
    }

    #[test]
    fn decodes_utf16le_without_bom() {
        let mut bytes = Vec::new();
        for ch in "docker-desktop".encode_utf16() {
            bytes.extend_from_slice(&ch.to_le_bytes());
        }
        let s = decode_output(&bytes);
        assert_eq!(s, "docker-desktop");
    }

    #[test]
    fn keeps_utf8_with_non_ascii() {
        let s = decode_output("프로토콜".as_bytes());
        assert!(s.contains("프로토콜"));
    }
}
