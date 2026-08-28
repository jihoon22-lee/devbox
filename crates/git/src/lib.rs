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

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(target_os = "windows")]
use std::mem::size_of;
#[cfg(target_os = "windows")]
use std::os::windows::io::AsRawHandle;
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};

/// Git for Windows 기본 설치 위치 (우선순위 순). GUI 앱이 물려받은 PATH에
/// git이 없어도 동작하도록 절대 경로를 우선한다.
#[cfg(target_os = "windows")]
const KNOWN_GIT_PATHS: &[&str] = &[
    r"C:\Program Files\Git\cmd\git.exe",
    r"C:\Program Files\Git\bin\git.exe",
    r"C:\Program Files (x86)\Git\cmd\git.exe",
    r"C:\Program Files (x86)\Git\bin\git.exe",
];

/// Repository-selection overrides must not redirect `git -C <validated cwd>`
/// to a different index, object database, or worktree inherited from the GUI
/// process. Credential/SSH/askpass variables are intentionally not removed;
/// Git and the user's configured credential helper continue to own auth.
const REPOSITORY_OVERRIDE_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_COMMON_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_PREFIX",
    "GIT_QUARANTINE_PATH",
    // Git's environment config injection can override core.worktree,
    // core.gitdir, hooks, and other repository-selection behavior even when
    // `-C` points at a validated directory.
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
];

// After Git's root exits, drain data that was already written to stdout but
// give an escaped descendant a finite window before joining the reader.
const READER_DRAIN_GRACE: Duration = Duration::from_millis(100);

fn clear_repository_overrides(command: &mut Command) {
    for name in REPOSITORY_OVERRIDE_ENV {
        command.env_remove(name);
    }
    // GIT_CONFIG_COUNT controls how many GIT_CONFIG_KEY_n/VALUE_n pairs Git
    // consumes. Removing the count is sufficient to disable the series, but
    // clear the bounded conventional range too so a child cannot accidentally
    // re-enable it if Git's environment parsing changes in a future release.
    for index in 0..32 {
        command.env_remove(format!("GIT_CONFIG_KEY_{index}"));
        command.env_remove(format!("GIT_CONFIG_VALUE_{index}"));
    }
}

/// Own the complete Git process tree on Windows. Git can spawn hooks,
/// credential helpers, SSH, and transport children, so killing only the root
/// `git.exe` is not a sufficient cancellation boundary.
#[cfg(target_os = "windows")]
struct ProcessTree {
    handle: HANDLE,
}

#[cfg(target_os = "windows")]
impl ProcessTree {
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
        let mut tree = Self { handle };
        if resume_suspended_process(child.id()).is_err() {
            tree.terminate_descendants();
            return Err(());
        }
        Ok(tree)
    }

    fn terminate(&mut self, child: &mut Child) {
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
        let _ = child.wait();
    }

    fn terminate_descendants(&mut self) {
        // The root may already have exited, but the stable Job handle still
        // owns every non-breakaway helper/hook/transport descendant.
        let _ = unsafe { TerminateJobObject(self.handle, 1) };
    }

    fn close(self) {}
}

/// `std::process::Command` retains the caller-supplied `CREATE_SUSPENDED`
/// flag but does not expose the primary thread handle. A newly created
/// suspended process has not executed user code and therefore has exactly one
/// thread. Resolve that thread by the exact child PID only after the process is
/// assigned to the Job Object, then resume it once. Any ambiguity or unexpected
/// suspend count fails closed while the Job still owns the process.
#[cfg(target_os = "windows")]
fn resume_suspended_process(process_id: u32) -> Result<(), ()> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(|_| ())?;
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut thread_id = None;
    if unsafe { Thread32First(snapshot, &mut entry) }.is_ok() {
        loop {
            if entry.th32OwnerProcessID == process_id
                && thread_id.replace(entry.th32ThreadID).is_some()
            {
                unsafe {
                    let _ = CloseHandle(snapshot);
                }
                return Err(());
            }
            if unsafe { Thread32Next(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, thread_id.ok_or(())?) }
        .map_err(|_| ())?;
    let previous_suspend_count = unsafe { ResumeThread(thread) };
    unsafe {
        let _ = CloseHandle(thread);
    }
    if previous_suspend_count == 1 {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(target_os = "windows")]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

#[cfg(unix)]
struct ProcessTree {
    process_group: i32,
}

#[cfg(unix)]
impl ProcessTree {
    fn assign_to(child: &Child) -> Result<Self, ()> {
        Ok(Self {
            process_group: i32::try_from(child.id()).map_err(|_| ())?,
        })
    }

    fn terminate(&mut self, child: &mut Child) {
        self.terminate_group();
        let _ = child.wait();
    }

    fn terminate_descendants(&mut self) {
        self.terminate_group();
    }

    fn close(self) {}

    fn terminate_group(&self) {
        // The child is spawned as its own process-group leader below. A
        // negative pid therefore addresses Git and every hook/helper child
        // without touching the desktop application's process group.
        let _ = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
    }
}

#[cfg(not(any(target_os = "windows", unix)))]
struct ProcessTree;

#[cfg(not(any(target_os = "windows", unix)))]
impl ProcessTree {
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
    clear_repository_overrides(&mut cmd);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: 콘솔 창 깜빡임 방지
    let out = cmd.output().map_err(|e| format!("git 실행 불가: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Bounded git execution for read-only and local mutation consumers.
///
/// `run` predates the bounded consumer contract and intentionally retains its
/// legacy stderr behaviour for existing callers. New consumers that handle
/// user-controlled paths or repository data must use this function: stdin and
/// stderr are closed, stdout has a hard byte limit, and the child is killed
/// after `timeout`. Callers receive stable error codes only; neither a path nor
/// an OS message is returned to the UI or export document.
pub fn run_bounded(
    args: &[&str],
    cwd: &str,
    timeout: Duration,
    max_stdout_bytes: usize,
) -> Result<String, String> {
    run_bounded_inner(args, cwd, timeout, max_stdout_bytes, false, None)
}

/// Run a bounded read-only Git command with cooperative cancellation. The
/// caller owns the signal and may use it for status/metadata reads that happen
/// before a longer remote mutation. Cancellation kills the child and drains
/// its bounded stdout before returning, just like the mutating variant.
pub fn run_bounded_with_cancel(
    args: &[&str],
    cwd: &str,
    timeout: Duration,
    max_stdout_bytes: usize,
    cancellation: &AtomicBool,
) -> Result<String, String> {
    run_bounded_inner(
        args,
        cwd,
        timeout,
        max_stdout_bytes,
        false,
        Some(cancellation),
    )
}

fn run_bounded_inner(
    args: &[&str],
    cwd: &str,
    timeout: Duration,
    max_stdout_bytes: usize,
    allow_message_controls: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<String, String> {
    let max_arg_bytes = if allow_message_controls {
        16 * 1024
    } else {
        4_096
    };
    // Selected-file mutations may legitimately carry hundreds of literal
    // pathspecs. Read-only callers retain the tighter surface, while the
    // mutating surface is still bounded by count, per-argument bytes, and an
    // aggregate argv budget before spawning Git.
    let max_arg_count = if allow_message_controls { 1_024 } else { 32 };
    let total_arg_bytes = args
        .iter()
        .try_fold(0usize, |total, arg| total.checked_add(arg.len()));
    if cwd.is_empty()
        || cwd.len() > 4_096
        || cwd.chars().any(char::is_control)
        || args.len() > max_arg_count
        || total_arg_bytes.is_none_or(|total| total > 256 * 1024)
        || args.iter().enumerate().any(|(index, arg)| {
            arg.len() > max_arg_bytes
                || arg.chars().any(|character| {
                    character.is_control()
                        && !(allow_message_controls
                            && is_commit_message_argument(args, index)
                            && matches!(character, '\n' | '\r' | '\t'))
                })
        })
    {
        return Err("git_invalid_arguments".into());
    }
    if cancellation.is_some_and(|signal| signal.load(Ordering::Acquire)) {
        return Err("git_cancelled".into());
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
    clear_repository_overrides(&mut command);
    #[cfg(target_os = "windows")]
    command.creation_flags(CREATE_NO_WINDOW.0 | CREATE_SUSPENDED.0);
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command
        .spawn()
        .map_err(|_| "git_spawn_failed".to_string())?;
    let mut process_tree = match ProcessTree::assign_to(&child) {
        Ok(process_tree) => process_tree,
        Err(()) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("git_process_tree_unavailable".into());
        }
    };
    let Some(stdout) = child.stdout.take() else {
        process_tree.terminate(&mut child);
        return Err("git_stdout_unavailable".into());
    };

    let overflow = Arc::new(AtomicBool::new(false));
    let read_failed = Arc::new(AtomicBool::new(false));
    let reader_stop = Arc::new(AtomicBool::new(false));
    let overflow_for_reader = Arc::clone(&overflow);
    let read_failed_for_reader = Arc::clone(&read_failed);
    let stop_for_reader = Arc::clone(&reader_stop);
    let reader = std::thread::spawn(move || {
        let mut stdout = stdout;
        #[cfg(windows)]
        use std::os::windows::io::AsRawHandle;
        #[cfg(windows)]
        use windows::Win32::Foundation::{ERROR_BROKEN_PIPE, HANDLE};
        #[cfg(windows)]
        use windows::Win32::System::Pipes::PeekNamedPipe;

        #[cfg(windows)]
        let stdout_handle = HANDLE(stdout.as_raw_handle());
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
        let mut bytes = Vec::with_capacity(max_stdout_bytes.min(16 * 1024));
        let mut chunk = [0u8; 8 * 1024];
        let mut stop_deadline = None;
        loop {
            // The root may have exited while an owned (or, on Unix, an
            // escaped) descendant still has the pipe open. Drain bytes that
            // were already written, but never let a descendant defeat the
            // supervisor's bounded join by writing forever.
            if stop_for_reader.load(Ordering::Acquire) {
                let deadline =
                    stop_deadline.get_or_insert_with(|| Instant::now() + READER_DRAIN_GRACE);
                if Instant::now() >= *deadline {
                    break;
                }
            }

            // ChildStdout is a synchronous Windows pipe. A blocking Read can
            // outlive the bounded drain window when a breakaway descendant
            // keeps the write end open, so poll availability before reading.
            // Unix uses O_NONBLOCK above and can read directly.
            #[cfg(windows)]
            let read_len = {
                let mut available = 0u32;
                if let Err(error) = unsafe {
                    PeekNamedPipe(stdout_handle, None, 0, None, Some(&mut available), None)
                } {
                    // A normal child exit closes the write end and may make
                    // PeekNamedPipe report ERROR_BROKEN_PIPE. That is EOF,
                    // not a failed read; any other error remains fail-closed.
                    if !stop_for_reader.load(Ordering::Acquire)
                        && error.code() != ERROR_BROKEN_PIPE.to_hresult()
                    {
                        read_failed_for_reader.store(true, Ordering::Release);
                    }
                    break;
                }
                if available == 0 {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                available.min(chunk.len() as u32) as usize
            };
            #[cfg(not(windows))]
            let read_len = chunk.len();

            match stdout.read(&mut chunk[..read_len]) {
                Ok(0) => break,
                Ok(read) => {
                    if bytes.len().saturating_add(read) > max_stdout_bytes {
                        overflow_for_reader.store(true, Ordering::Release);
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                #[cfg(windows)]
                Err(error) if error.kind() == ErrorKind::BrokenPipe => break,
                #[cfg(unix)]
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if stop_for_reader.load(Ordering::Acquire)
                        && stop_deadline.is_some_and(|deadline| Instant::now() >= deadline)
                    {
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

    let deadline = Instant::now().checked_add(timeout);
    let status = loop {
        if overflow.load(Ordering::Acquire) {
            process_tree.terminate(&mut child);
            reader_stop.store(true, Ordering::Release);
            process_tree.close();
            let _ = reader.join();
            return Err("git_output_too_large".into());
        }
        if read_failed.load(Ordering::Acquire) {
            process_tree.terminate(&mut child);
            reader_stop.store(true, Ordering::Release);
            process_tree.close();
            let _ = reader.join();
            return Err("git_output_read_failed".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if cancellation.is_some_and(|signal| signal.load(Ordering::Acquire)) {
                    process_tree.terminate(&mut child);
                    reader_stop.store(true, Ordering::Release);
                    process_tree.close();
                    let _ = reader.join();
                    return Err("git_cancelled".into());
                }
            }
            Err(_) => {
                process_tree.terminate(&mut child);
                reader_stop.store(true, Ordering::Release);
                process_tree.close();
                let _ = reader.join();
                return Err("git_wait_failed".into());
            }
        }
        if deadline.is_none_or(|value| Instant::now() >= value) {
            process_tree.terminate(&mut child);
            reader_stop.store(true, Ordering::Release);
            process_tree.close();
            let _ = reader.join();
            return Err("git_timeout".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    };

    // Root Git may exit while a hook/helper descendant still owns the stdout
    // pipe. Tear down the owned Job Object/process group once, then tell the
    // Unix nonblocking reader to stop after draining currently available
    // bytes. No Drop implementation sends a second signal after the root PID
    // has been reaped.
    process_tree.terminate_descendants();
    reader_stop.store(true, Ordering::Release);
    process_tree.close();
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

/// Commit messages may contain ordinary line breaks, but no other Git argv
/// position may carry controls.  Keeping this exception tied to the
/// `--message`/`-m` slot prevents a future caller from treating the broad
/// mutation argument cap as permission to pass a newline-bearing path,
/// command, remote, or hook option.
fn is_commit_message_argument(args: &[&str], index: usize) -> bool {
    // The mutating runner is shared by commands other than `commit` (for
    // example cleanup and remote operations).  A generic `--message=` check
    // would accidentally allow control bytes in an arbitrary argument for a
    // future caller.  Only an argument following the actual `commit` command
    // can opt into the line-break allowance.
    let mut command_index = 0usize;
    while let Some(argument) = args.get(command_index) {
        match *argument {
            "--no-pager" | "--no-optional-locks" | "--literal-pathspecs" => {
                command_index += 1;
            }
            "-c" => {
                // Git's `-c key=value` is a global option. Skip its value so
                // a config value equal to `commit` cannot be mistaken for
                // the subcommand itself.
                command_index = command_index.saturating_add(2);
            }
            value if value.starts_with("--config=") => {
                command_index += 1;
            }
            _ => break,
        }
    }
    if args.get(command_index) != Some(&"commit") {
        return false;
    }
    if index <= command_index
        || args
            .iter()
            .position(|argument| *argument == "--")
            .is_some_and(|separator| index >= separator)
    {
        return false;
    }
    args.get(index)
        .is_some_and(|argument| argument.starts_with("--message="))
        || index
            .checked_sub(1)
            .and_then(|previous| args.get(previous))
            .is_some_and(|argument| matches!(*argument, "--message" | "-m"))
}

/// Run a bounded local Git mutation without giving the child an interactive
/// terminal or collecting diagnostics. Git's normal config and credential
/// helper resolution remain intact: devbox does not provide, inspect, or save
/// credentials. Closing stdin prevents Git itself (or a hook) from waiting for
/// a secret prompt; configured credential helpers can still use their own
/// operating-system-backed mechanism when a Git operation needs one.
pub fn run_mutating(
    args: &[&str],
    cwd: &str,
    timeout: Duration,
    max_stdout_bytes: usize,
) -> Result<String, String> {
    run_bounded_inner(args, cwd, timeout, max_stdout_bytes, true, None)
}

/// Run a local mutation with a cooperative cancellation signal.  The signal
/// is checked before spawning Git and while it is running; cancellation kills
/// the child and drains its bounded stdout before returning.  The caller must
/// still apply its own request-sequence guard because a cancellation can race
/// with a child that has already completed.
pub fn run_mutating_with_cancel(
    args: &[&str],
    cwd: &str,
    timeout: Duration,
    max_stdout_bytes: usize,
    cancellation: &AtomicBool,
) -> Result<String, String> {
    run_bounded_inner(
        args,
        cwd,
        timeout,
        max_stdout_bytes,
        true,
        Some(cancellation),
    )
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

    #[test]
    fn repository_overrides_are_removed_without_disabling_credential_helpers() {
        let mut command = Command::new(resolve_git());
        command.env("GIT_DIR", "untrusted-repository-override");
        command.env("GIT_INDEX_FILE", "untrusted-index-override");
        command.env("GIT_CONFIG_PARAMETERS", "'core.worktree=untrusted'");
        command.env("GIT_CONFIG_COUNT", "1");
        command.env("GIT_CONFIG_KEY_0", "core.worktree");
        command.env("GIT_CONFIG_VALUE_0", "untrusted");
        command.env("GIT_ASKPASS", "configured-credential-helper");
        clear_repository_overrides(&mut command);
        let environment = command
            .get_envs()
            .map(|(name, value)| (name.to_owned(), value.map(ToOwned::to_owned)))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            environment.get(std::ffi::OsStr::new("GIT_DIR")),
            Some(&None)
        );
        assert_eq!(
            environment.get(std::ffi::OsStr::new("GIT_INDEX_FILE")),
            Some(&None)
        );
        for name in [
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_KEY_0",
            "GIT_CONFIG_VALUE_0",
        ] {
            assert_eq!(
                environment.get(std::ffi::OsStr::new(name)),
                Some(&None),
                "repository config override {name} must be removed",
            );
        }
        assert_eq!(
            environment.get(std::ffi::OsStr::new("GIT_ASKPASS")),
            Some(&Some(std::ffi::OsString::from(
                "configured-credential-helper"
            )))
        );
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

    #[test]
    fn mutating_runner_allows_bounded_commit_message_controls_only_for_child_argv() {
        let tmp =
            std::env::temp_dir().join(format!("devbox-git-mutating-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        init_repo(&tmp);
        std::fs::write(tmp.join("fixture.txt"), "fixture\n").unwrap();
        let cwd = tmp.to_string_lossy();
        run_mutating(
            &["add", "--", "fixture.txt"],
            &cwd,
            Duration::from_secs(2),
            4 * 1024,
        )
        .unwrap();
        run_mutating(
            &["commit", "-m", "summary\n\nbody\t", "--"],
            &cwd,
            Duration::from_secs(2),
            4 * 1024,
        )
        .unwrap();
        assert_eq!(
            run_bounded(
                &["commit", "-m", "not\nallowed", "--"],
                &cwd,
                Duration::from_secs(2),
                4 * 1024,
            )
            .unwrap_err(),
            "git_invalid_arguments"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn mutating_runner_rejects_controls_outside_the_commit_message_slot() {
        let error = run_mutating(
            &["status\n--porcelain"],
            "/safe/project",
            Duration::from_secs(1),
            1024,
        )
        .unwrap_err();
        assert_eq!(error, "git_invalid_arguments");

        let error = run_mutating(
            &["--message=not-a-command\nwith-controls"],
            "/safe/project",
            Duration::from_secs(1),
            1024,
        )
        .unwrap_err();
        assert_eq!(error, "git_invalid_arguments");

        let error = run_mutating(
            &[
                "-c",
                "commit",
                "status",
                "--message=not-a-command\nwith-controls",
            ],
            "/safe/project",
            Duration::from_secs(1),
            1024,
        )
        .unwrap_err();
        assert_eq!(error, "git_invalid_arguments");

        let error = run_mutating(
            &["commit", "--", "-m", "not-a-message\nwith-controls"],
            "/safe/project",
            Duration::from_secs(1),
            1024,
        )
        .unwrap_err();
        assert_eq!(error, "git_invalid_arguments");

        let error = run_mutating(
            &["commit", "--author", "name\nemail", "--"],
            "/safe/project",
            Duration::from_secs(1),
            1024,
        )
        .unwrap_err();
        assert_eq!(error, "git_invalid_arguments");
    }

    #[test]
    fn mutating_runner_accepts_a_bounded_selected_path_batch() {
        let tmp =
            std::env::temp_dir().join(format!("devbox-git-many-paths-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        init_repo(&tmp);
        let mut owned = vec![
            "status".to_string(),
            "--porcelain".to_string(),
            "--".to_string(),
        ];
        owned.extend((0..=32).map(|index| format!("file-{index}.txt")));
        let args = owned.iter().map(String::as_str).collect::<Vec<_>>();
        run_mutating(
            &args,
            &tmp.to_string_lossy(),
            Duration::from_secs(2),
            4 * 1024,
        )
        .unwrap();

        let read_only_error = run_bounded(
            &args,
            &tmp.to_string_lossy(),
            Duration::from_secs(2),
            4 * 1024,
        )
        .unwrap_err();
        assert_eq!(read_only_error, "git_invalid_arguments");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn mutating_runner_honors_pre_cancel_without_spawning_a_command() {
        let cancellation = AtomicBool::new(true);
        let error = run_mutating_with_cancel(
            &["status"],
            "/safe/project",
            Duration::from_secs(2),
            4 * 1024,
            &cancellation,
        )
        .unwrap_err();
        assert_eq!(error, "git_cancelled");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_process_tree_policy_kills_descendants_on_job_close() {
        assert_eq!(JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0, 0x2000);
    }

    #[cfg(unix)]
    #[test]
    fn cancellable_runner_kills_a_running_hook() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;

        let tmp = std::env::temp_dir().join(format!(
            "devbox-git-cancel-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        init_repo(&tmp);
        let hook = tmp.join(".git/hooks/pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&hook, permissions).unwrap();

        let cancellation = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancellation);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            signal.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let result = run_mutating_with_cancel(
            &["commit", "--allow-empty", "-m", "cancel fixture", "--"],
            &tmp.to_string_lossy(),
            Duration::from_secs(5),
            4 * 1024,
            &cancellation,
        );
        thread.join().unwrap();
        assert_eq!(result.unwrap_err(), "git_cancelled");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!tmp.join(".git/COMMIT_EDITMSG").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn runner_does_not_wait_for_an_escaped_descendant_holding_stdout() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = std::env::temp_dir().join(format!(
            "devbox-git-escaped-writer-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        init_repo(&tmp);
        let hook = tmp.join(".git/hooks/pre-commit");
        std::fs::write(
            &hook,
            "#!/bin/sh\nsetsid sh -c 'echo $$ > escaped-writer.pid; sleep 10' &\ni=0\nwhile [ ! -s escaped-writer.pid ] && [ \"$i\" -lt 100 ]; do\n  i=$((i + 1))\n  sleep 0.01\ndone\ntest -s escaped-writer.pid\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&hook, permissions).unwrap();

        let started = Instant::now();
        run_mutating(
            &[
                "commit",
                "--allow-empty",
                "-m",
                "escaped writer fixture",
                "--",
            ],
            &tmp.to_string_lossy(),
            Duration::from_secs(5),
            4 * 1024,
        )
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));

        let pid_path = tmp.join("escaped-writer.pid");
        let pid = (0..20)
            .find_map(|_| {
                let pid = std::fs::read_to_string(&pid_path)
                    .ok()
                    .and_then(|value| value.trim().parse::<i32>().ok());
                if pid.is_none() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                pid
            })
            .expect("escaped writer must publish its pid");
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
