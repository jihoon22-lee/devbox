//! git 하위 프로세스의 안정적 실행.
//!
//! 배경: repo-manager·workbench·life-log가 `tokio::process::Command`로 `git`을
//! 실행했을 때 Windows 릴리스 빌드에서 실패해 각각 `?`/`n/a`/0으로 폴백했다.
//! devbox-manager의 환경 진단(`std::process::Command` + `wsl.exe`)은 정상 동작해,
//! 여기서는 (1) std 기반 실행과 (2) Git for Windows 절대 경로 해석으로 통일한다.
//!
//! 제약: 순수 실행 로직만 담는다. git 출력 파싱은 각 앱이 소유한다.

use std::io::{ErrorKind, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// Bounded, read-only git execution for export/report consumers.
///
/// `run` predates the bounded consumer contract and intentionally retains its
/// legacy stderr behaviour for existing callers. New consumers that handle
/// user-controlled paths or data must use this function: stderr is discarded,
/// stdout has a hard byte limit, and the child is killed after `timeout`.
/// Callers receive stable error codes only; neither a path nor an OS message is
/// returned to the UI or export document.
pub fn run_bounded(
    args: &[&str],
    cwd: &str,
    timeout: Duration,
    max_stdout_bytes: usize,
) -> Result<String, String> {
    if cwd.is_empty()
        || cwd.len() > 4_096
        || cwd.chars().any(char::is_control)
        || args.len() > 32
        || args
            .iter()
            .any(|arg| arg.len() > 4_096 || arg.chars().any(char::is_control))
    {
        return Err("git_invalid_arguments".into());
    }

    let mut command = Command::new(resolve_git());
    command
        .args(["-C", cwd])
        .args(args)
        // A reporting command must never inherit the desktop application's
        // console/input handle and wait for an interactive prompt.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // Git diagnostics can contain a path, remote URL, or credential. They
        // are deliberately not read or returned to the caller.
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = command
        .spawn()
        .map_err(|_| "git_spawn_failed".to_string())?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("git_stdout_unavailable".into());
    };

    let overflow = Arc::new(AtomicBool::new(false));
    let read_failed = Arc::new(AtomicBool::new(false));
    let overflow_for_reader = Arc::clone(&overflow);
    let read_failed_for_reader = Arc::clone(&read_failed);
    let reader = std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = Vec::with_capacity(max_stdout_bytes.min(16 * 1024));
        let mut chunk = [0u8; 8 * 1024];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    if bytes.len().saturating_add(read) > max_stdout_bytes {
                        overflow_for_reader.store(true, Ordering::Release);
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => {
                    read_failed_for_reader.store(true, Ordering::Release);
                    break;
                }
            }
        }
        bytes
    });

    let deadline = Instant::now().checked_add(timeout);
    let status = loop {
        if overflow.load(Ordering::Acquire) {
            terminate_child(&mut child);
            let _ = reader.join();
            return Err("git_output_too_large".into());
        }
        if read_failed.load(Ordering::Acquire) {
            terminate_child(&mut child);
            let _ = reader.join();
            return Err("git_output_read_failed".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                terminate_child(&mut child);
                let _ = reader.join();
                return Err("git_wait_failed".into());
            }
        }
        if deadline.is_none_or(|value| Instant::now() >= value) {
            terminate_child(&mut child);
            let _ = reader.join();
            return Err("git_timeout".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };

    let bytes = reader.join().map_err(|_| "git_reader_failed".to_string())?;
    if overflow.load(Ordering::Acquire) {
        return Err("git_output_too_large".into());
    }
    if read_failed.load(Ordering::Acquire) {
        return Err("git_output_read_failed".into());
    }
    if !status.success() {
        return Err("git_failed".into());
    }
    String::from_utf8(bytes).map_err(|_| "git_output_invalid_utf8".into())
}

fn terminate_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn resolve_git_falls_back_to_path_on_non_windows() {
        // KNOWN_GIT_PATHS 탐색은 Windows 전용이다. 다른 플랫폼은 항상 PATH의 git을 쓴다.
        assert_eq!(resolve_git(), PathBuf::from("git"));
    }

    fn init_repo(dir: &std::path::Path) {
        std::fs::create_dir_all(dir).unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success());
        // author 미설정 환경(CI)에서도 커밋 가능하도록 로컬 config로 고정.
        for (key, value) in [("user.email", "test@example.com"), ("user.name", "test")] {
            assert!(std::process::Command::new("git")
                .args(["config", key, value])
                .current_dir(dir)
                .status()
                .unwrap()
                .success());
        }
    }

    #[test]
    fn run_returns_stdout_on_success() {
        let tmp = std::env::temp_dir().join(format!("devbox-git-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        init_repo(&tmp);

        let out = run(
            &["status", "--porcelain", "--branch"],
            &tmp.to_string_lossy(),
        )
        .unwrap();
        assert!(
            out.starts_with("## "),
            "branch 헤더로 시작해야 한다: {out:?}"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_returns_stderr_as_error_on_failure() {
        let tmp =
            std::env::temp_dir().join(format!("devbox-git-test-notrepo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // .git이 없는 디렉터리에서 status를 실행하면 실패해야 하고, 그 에러가
        // git이 낸 stderr 문구를 담고 있어야 한다 (빈 문자열로 삼켜지면 안 된다).
        let err = run(&["status"], &tmp.to_string_lossy()).unwrap_err();
        assert!(!err.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn run_errors_on_nonexistent_cwd() {
        let err = run(&["status"], "/no/such/directory/devbox-git-test").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn bounded_runner_rejects_control_input_without_echoing_it() {
        let err = run_bounded(
            &["log\n--format=%ct"],
            "/safe/project",
            std::time::Duration::from_secs(1),
            1024,
        )
        .unwrap_err();
        assert_eq!(err, "git_invalid_arguments");
        assert!(!err.contains("safe/project"));
    }

    #[test]
    fn bounded_runner_covers_success_overflow_and_sanitized_failure() {
        let tmp =
            std::env::temp_dir().join(format!("devbox-git-bounded-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        init_repo(&tmp);
        let cwd = tmp.to_string_lossy();
        let args = ["status", "--porcelain", "--branch"];

        let output = run_bounded(&args, &cwd, Duration::from_secs(2), 4 * 1024).unwrap();
        assert!(output.starts_with("## "));
        assert_eq!(
            run_bounded(&args, &cwd, Duration::from_secs(2), 1).unwrap_err(),
            "git_output_too_large"
        );

        let non_repo = tmp.with_extension("not-a-repository");
        let _ = std::fs::remove_dir_all(&non_repo);
        std::fs::create_dir_all(&non_repo).unwrap();
        let failure = run_bounded(
            &["status"],
            &non_repo.to_string_lossy(),
            Duration::from_secs(2),
            4 * 1024,
        )
        .unwrap_err();
        assert_eq!(failure, "git_failed");
        assert!(!failure.contains(&*cwd));
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_dir_all(&non_repo);
    }
}
