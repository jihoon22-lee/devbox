//! 환경 진단 (§15.4, Stage 5 — Devbox Manager 탭으로 먼저 검증).
//! read-only. 자동 설치·registry 수정·WSL reset을 하지 않는다.

use crate::core::catalog::CatalogApp;
use crate::core::data_inspector;
use serde::Serialize;
use std::fs;
use std::io::{ErrorKind, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::mem::size_of;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(target_os = "windows")]
use std::os::windows::io::AsRawHandle;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

const CATALOG_JSON: &str = include_str!("../../../../catalog.json");
const DIAGNOSIS_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_DIAGNOSIS_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_DIAGNOSIS_LINE_CHARS: usize = 256;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
    let redacted = data_inspector::redact_text(line, "doctor");
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
fn run_bounded_command(command: Command) -> Option<Vec<u8>> {
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
    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    #[cfg(unix)]
    command.process_group(0);

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

#[cfg(target_os = "windows")]
struct DiagnosisProcessTree {
    handle: HANDLE,
}

#[cfg(target_os = "windows")]
impl DiagnosisProcessTree {
    fn assign_to(child: &Child) -> Result<Self, ()> {
        let handle = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|_| ())?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .is_err()
        {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(());
        }
        let process = HANDLE(child.as_raw_handle());
        if unsafe { AssignProcessToJobObject(handle, process) }.is_err() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(());
        }
        Ok(Self { handle })
    }

    fn terminate(&mut self, child: &mut Child) {
        if unsafe { TerminateJobObject(self.handle, 1) }.is_err() {
            // Keep the root timeout fail-closed even if the job handle was
            // invalidated by an unusual Windows process-lifecycle race.
            let _ = child.kill();
        }
        let _ = child.wait();
    }

    fn terminate_descendants(&mut self) {
        let _ = unsafe { TerminateJobObject(self.handle, 0) };
    }

    fn close(self) {}
}

#[cfg(target_os = "windows")]
impl Drop for DiagnosisProcessTree {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(unix)]
struct DiagnosisProcessTree {
    process_group: i32,
}

#[cfg(unix)]
impl DiagnosisProcessTree {
    fn assign_to(child: &Child) -> Result<Self, ()> {
        let process_group = i32::try_from(child.id()).map_err(|_| ())?;
        (process_group > 0)
            .then_some(Self { process_group })
            .ok_or(())
    }

    fn terminate(&mut self, child: &mut Child) {
        self.terminate_descendants();
        let _ = child.kill();
        let _ = child.wait();
    }

    fn terminate_descendants(&mut self) {
        // `process_group(0)` made the diagnostic root its own group leader.
        // Negative PID targets the root and every ordinary helper descendant.
        let _ = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
    }

    fn close(self) {}
}

#[cfg(not(any(target_os = "windows", unix)))]
struct DiagnosisProcessTree;

#[cfg(not(any(target_os = "windows", unix)))]
impl DiagnosisProcessTree {
    fn assign_to(_child: &Child) -> Result<Self, ()> {
        Ok(Self)
    }

    fn terminate(&mut self, child: &mut Child) {
        let _ = child.kill();
        let _ = child.wait();
    }

    fn terminate_descendants(&mut self) {}

    fn close(self) {}
}

/// 전체 진단을 수집한다 (read-only). Support bundle도 이 고정된 진단 DTO를
/// 재사용하므로 filesystem 경로나 OS 오류를 public 결과에 넣지 않는다.
pub(crate) fn collect_diagnosis(app: &tauri::AppHandle) -> Vec<DiagnosisItem> {
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

    // devbox 앱 데이터 디렉터리 + 카탈로그 정합
    let catalog = crate::core::catalog::parse_catalog(CATALOG_JSON).unwrap_or_else(|_| {
        crate::core::catalog::Catalog {
            schema_version: 0,
            catalog_revision: None,
            apps: vec![],
        }
    });
    let mut dir_ok = 0;
    for app in &catalog.apps {
        let dir_ok_for_app = dirs::data_local_dir().is_some_and(|base| {
            let path = base.join(&app.identifier);
            data_inspector::safe_derived_path(&base, &path)
                && fs::symlink_metadata(path)
                    .ok()
                    .is_some_and(|metadata| metadata.is_dir())
        });
        if dir_ok_for_app {
            dir_ok += 1;
        }
    }
    items.push(DiagnosisItem {
        name: "devbox-data".into(),
        ok: dir_ok > 0,
        detail: format!(
            "카탈로그 {}개 · 데이터 디렉터리 존재 {}개",
            catalog.apps.len(),
            dir_ok
        ),
    });

    // 카탈로그 identifier 정합 (com.devbox. 접두사)
    let bad_ids: Vec<&String> = catalog
        .apps
        .iter()
        .map(|a: &CatalogApp| &a.identifier)
        .filter(|id| !id.starts_with("com.devbox."))
        .collect();
    items.push(DiagnosisItem {
        name: "catalog-ids".into(),
        ok: bad_ids.is_empty(),
        detail: if bad_ids.is_empty() {
            "모든 identifier가 com.devbox.*".into()
        } else {
            format!("비정상 identifier {}개", bad_ids.len())
        },
    });

    let metadata_ok = crate::commands::manager::data_dir_path(app)
        .ok()
        .zip(devbox_launch::runtime_catalog_path())
        .and_then(|(manager_root, catalog_path)| {
            catalog_path.parent().map(|common_root| {
                crate::core::runtime_metadata::runtime_metadata_consistent(
                    &manager_root,
                    common_root,
                    CATALOG_JSON,
                )
            })
        })
        .unwrap_or(false);
    items.push(DiagnosisItem {
        name: "runtime-metadata".into(),
        ok: metadata_ok,
        detail: if metadata_ok {
            "runtime catalog와 install-root locator 정합".into()
        } else {
            "runtime metadata를 다음 실행에 재동기화해야 함".into()
        },
    });

    items
}

/// 전체 진단 실행 (read-only).
#[tauri::command]
pub fn run_diagnosis(app: tauri::AppHandle) -> Vec<DiagnosisItem> {
    collect_diagnosis(&app)
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
