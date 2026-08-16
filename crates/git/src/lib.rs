//! git 하위 프로세스의 안정적 실행.
//!
//! 배경: repo-manager·workbench·life-log가 `tokio::process::Command`로 `git`을
//! 실행했을 때 Windows 릴리스 빌드에서 실패해 각각 `?`/`n/a`/0으로 폴백했다.
//! devbox-manager의 환경 진단(`std::process::Command` + `wsl.exe`)은 정상 동작해,
//! 여기서는 (1) std 기반 실행과 (2) Git for Windows 절대 경로 해석으로 통일한다.
//!
//! 제약: 순수 실행 로직만 담는다. git 출력 파싱은 각 앱이 소유한다.

use std::path::PathBuf;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Git for Windows 기본 설치 위치 (우선순위 순). GUI 앱이 물려받은 PATH에
/// git이 없어도 동작하도록 절대 경로를 우선한다.
#[cfg(target_os = "windows")]
const KNOWN_GIT_PATHS: &[&str] = &[
    r"C:\Program Files\Git\cmd\git.exe",
    r"C:\Program Files\Git\bin\git.exe",
    r"C:\Program Files (x86)\Git\cmd\git.exe",
    r"C:\Program Files (x86)\Git\bin\git.exe",
];

/// 실행에 쓸 git 프로그램 경로. 기본 설치 경로가 있으면 절대 경로, 없으면 `git`(PATH).
pub fn resolve_git() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        for p in KNOWN_GIT_PATHS {
            let p = PathBuf::from(p);
            if p.exists() {
                return p;
            }
        }
    }
    PathBuf::from("git")
}

/// `git -C <cwd> <args...>`를 실행해 stdout을 반환한다. 실패 시 stderr를 에러로.
pub fn run(args: &[&str], cwd: &str) -> Result<String, String> {
    let mut cmd = std::process::Command::new(resolve_git());
    cmd.args(["-C", cwd]).args(args);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: 콘솔 창 깜빡임 방지
    let out = cmd.output().map_err(|e| format!("git 실행 불가: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_git_returns_a_program() {
        // 어느 플랫폼이든 `git`(PATH) 또는 절대 경로 중 하나를 반환한다.
        let p = resolve_git();
        assert!(!p.as_os_str().is_empty());
    }
}
