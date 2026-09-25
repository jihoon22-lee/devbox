//! 환경 진단 (§15.4, Stage 5 — Devbox Manager 탭으로 먼저 검증).
//! read-only. 자동 설치·registry 수정·WSL reset을 하지 않는다.

use crate::core::redaction;
use serde::Serialize;
use std::io::{ErrorKind, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DIAGNOSIS_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DIAGNOSIS_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_DIAGNOSIS_LINE_CHARS: usize = 256;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(ts_rs::TS)]
pub struct DiagnosisItem {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

fn version_of(cmd: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> Option<String> {
    let mut c = Command::new(cmd);
    c.args(args);
    // `wsl.exe --version`/`-l -v`는 UTF-16LE로 출력된다 (공용 crates/wsl 디코더).
    let output = run_bounded_command(c)?;
    let text = devbox_wsl::output::decode_output(&output);
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let redacted = redaction::redact_text(line, "doctor");
    let redacted = redacted
        .chars()
        .take(MAX_DIAGNOSIS_LINE_CHARS)
        .collect::<String>();
    (!redacted.is_empty()).then_some(redacted)
}

/// Execute a fixed diagnostic binary with a hard timeout, bounded stdout, no
/// interactive stdin/stderr, and process-tree cleanup. Environment diagnosis
/// must not be able to hang the Manager or leave a helper process running after
/// the user switches tabs.
pub(crate) fn run_bounded_command(command: Command) -> Option<Vec<u8>> {
    run_bounded_command_with_limits(command, DIAGNOSIS_TIMEOUT, MAX_DIAGNOSIS_OUTPUT_BYTES)
}

fn run_bounded_command_with_limits(
    mut command: Command,
    timeout: Duration,
    max_output_bytes: usize,
) -> Option<Vec<u8>> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    process_tree::ProcessTree::prepare_std(&mut command);

    let mut child = command.spawn().ok()?;
    let mut process_tree = match DiagnosisProcessTree::assign_to(&child) {
        Ok(process_tree) => process_tree,
        Err(()) => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    let Some(stdout) = child.stdout.take() else {
        process_tree.terminate(&mut child);
        process_tree.close();
        return None;
    };
    let overflow = Arc::new(AtomicBool::new(false));
    let read_failed = Arc::new(AtomicBool::new(false));
    let reader_stop = Arc::new(AtomicBool::new(false));
    let overflow_for_reader = Arc::clone(&overflow);
    let read_failed_for_reader = Arc::clone(&read_failed);
    let stop_for_reader = Arc::clone(&reader_stop);
    let reader = std::thread::spawn(move || {
        let mut stdout = stdout;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let descriptor = stdout.as_raw_fd();
            let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
            if flags < 0
                || unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                read_failed_for_reader.store(true, Ordering::Release);
                return Vec::new();
            }
        }
        let mut bytes = Vec::with_capacity(max_output_bytes.min(16 * 1024));
        let mut chunk = [0u8; 8 * 1024];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    if bytes.len().saturating_add(read) > max_output_bytes {
                        overflow_for_reader.store(true, Ordering::Release);
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                #[cfg(unix)]
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if stop_for_reader.load(Ordering::Acquire) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => {
                    if !stop_for_reader.load(Ordering::Acquire) {
                        read_failed_for_reader.store(true, Ordering::Release);
                    }
                    break;
                }
            }
        }
        bytes
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        if overflow.load(Ordering::Acquire) || read_failed.load(Ordering::Acquire) {
            process_tree.terminate(&mut child);
            reader_stop.store(true, Ordering::Release);
            process_tree.close();
            let _ = reader.join();
            return None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                process_tree.terminate(&mut child);
                reader_stop.store(true, Ordering::Release);
                process_tree.close();
                let _ = reader.join();
                return None;
            }
        }
        if Instant::now() >= deadline {
            process_tree.terminate(&mut child);
            reader_stop.store(true, Ordering::Release);
            process_tree.close();
            let _ = reader.join();
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };

    // A successful root process can still have a helper holding stdout open.
    // Close the owned process group/job before joining the bounded reader.
    process_tree.terminate_descendants();
    reader_stop.store(true, Ordering::Release);
    process_tree.close();
    let bytes = reader.join().ok()?;
    if overflow.load(Ordering::Acquire) || read_failed.load(Ordering::Acquire) || !status.success()
    {
        return None;
    }
    Some(bytes)
}

struct DiagnosisProcessTree(Option<process_tree::ProcessTree>);
impl DiagnosisProcessTree {
    fn assign_to(child: &Child) -> Result<Self, ()> {
        process_tree::ProcessTree::assign_std(child).map(|tree| Self(Some(tree)))
    }
    fn terminate(&mut self, child: &mut Child) {
        self.terminate_descendants();
        let _ = child.kill();
        let _ = child.wait();
    }
    fn terminate_descendants(&mut self) {
        if let Some(tree) = self.0.take() {
            let _ = tree.signal_and_release(true);
        }
    }
    fn close(self) {}
}

/// 전체 진단을 수집한다 (read-only). Support bundle도 이 고정된 진단 DTO를
/// 재사용하므로 filesystem 경로나 OS 오류를 public 결과에 넣지 않는다.
pub(crate) fn collect_diagnosis(_app: &tauri::AppHandle) -> Vec<DiagnosisItem> {
    let mut items = Vec::new();

    // WSL
    let wsl =
        version_of("wsl.exe", &["--version"]).or_else(|| version_of("wsl.exe", &["-l", "-v"]));
    match wsl {
        Some(v) => items.push(DiagnosisItem {
            name: "wsl".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "wsl".into(),
            ok: false,
            detail: "wsl.exe 조회 불가 — WSL 설치 필요".into(),
        }),
    }

    // Git — GUI 앱이 물려받은 PATH에 git이 없어도 Git for Windows 기본 설치
    // 경로를 우선 시도한다 (crates/git와 동일 근거, PATH만 보면 오탐할 수 있다).
    match version_of(devbox_git::resolve_git(), &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "git".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "git".into(),
            ok: false,
            detail: "git 미설치".into(),
        }),
    }

    // Node / pnpm
    match version_of("node", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "node".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "node".into(),
            ok: false,
            detail: "node 미설치".into(),
        }),
    }
    match version_of("pnpm", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "pnpm".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "pnpm".into(),
            ok: false,
            detail: "pnpm 미설치".into(),
        }),
    }

    // Rust
    match version_of("rustc", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "rustc".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "rustc".into(),
            ok: false,
            detail: "rustc 미설치".into(),
        }),
    }
    match version_of("cargo", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "cargo".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "cargo".into(),
            ok: false,
            detail: "cargo 미설치".into(),
        }),
    }

    // Docker
    match version_of("docker", &["--version"]) {
        Some(v) => items.push(DiagnosisItem {
            name: "docker".into(),
            ok: true,
            detail: v,
        }),
        None => items.push(DiagnosisItem {
            name: "docker".into(),
            ok: false,
            detail: "docker CLI 미설치".into(),
        }),
    }

    items
}

/// 전체 진단 실행 (read-only).
pub async fn run_diagnosis(app: tauri::AppHandle) -> Result<Vec<DiagnosisItem>, String> {
    tauri::async_runtime::spawn_blocking(move || collect_diagnosis(&app))
        .await
        .map_err(|_| "환경 진단 작업을 완료할 수 없습니다.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn bounded_diagnosis_command_rejects_stdout_overflow() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf '123456789'"]);
        assert!(run_bounded_command_with_limits(command, Duration::from_secs(1), 8).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn bounded_diagnosis_command_terminates_a_hung_process_group() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5"]);
        let started = Instant::now();
        assert!(
            run_bounded_command_with_limits(command, Duration::from_millis(40), 1024).is_none()
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn version_output_is_bounded_and_redacted_before_ui_use() {
        let output = version_of(
            "sh",
            &[
                "-c",
                "printf 'username: alice alice@example.com /home/alice/project'",
            ],
        )
        .unwrap();
        assert!(!output.contains("alice"));
        assert!(!output.contains("/home/alice"));
    }

    #[test]
    fn diagnosis_limits_are_explicit() {
        assert_eq!(DIAGNOSIS_TIMEOUT, Duration::from_secs(2));
        assert_eq!(MAX_DIAGNOSIS_OUTPUT_BYTES, 64 * 1024);
        assert_eq!(MAX_DIAGNOSIS_LINE_CHARS, 256);
    }
}

#[cfg(test)]
mod retired_catalog_tests {
    #[test]
    fn doctor_has_no_v07_catalog_checks() {
        let source = include_str!("doctor.rs");
        for name in ["devbox-data", "catalog-ids", "runtime-metadata"] {
            let needle = format!("name: \"{name}\".into()");
            assert!(!source.contains(&needle), "{name}");
        }
        assert!(!source.contains(&["CATALOG", "_JSON"].concat()));
    }
}
