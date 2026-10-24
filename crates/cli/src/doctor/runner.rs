//! Bounded read-only probes. Never retain command stderr or authentication output.
use devbox_tools::doctor::{ProbeIssue, Probed};
use std::{
    future::Future,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{io::AsyncReadExt, runtime::Runtime, sync::Semaphore};
pub(super) const LIMIT: usize = 65_536;
pub(super) const DEADLINE: Duration = Duration::from_secs(2);
fn runtime() -> Probed<&'static Runtime> {
    static RUNTIME: OnceLock<Probed<Runtime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .max_blocking_threads(2)
                .enable_all()
                .build()
                .map_err(|_| ProbeIssue::Unavailable)
        })
        .as_ref()
        .map_err(|issue| *issue)
}
pub(super) fn run<T>(future: impl Future<Output = Probed<T>>) -> Probed<T> {
    runtime()?.block_on(async {
        tokio::time::timeout(DEADLINE, future)
            .await
            .map_err(|_| ProbeIssue::Timeout)?
    })
}
fn io_issue(error: std::io::Error) -> ProbeIssue {
    if error.kind() == std::io::ErrorKind::NotFound {
        ProbeIssue::Missing
    } else {
        ProbeIssue::Unavailable
    }
}
async fn io_task<T: Send + 'static>(job: impl FnOnce() -> Probed<T> + Send + 'static) -> Probed<T> {
    static SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = SLOTS
        .get_or_init(|| Arc::new(Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| ProbeIssue::Unavailable)?;
    // The permit stays with the job after timeout; at most two OS filesystem
    // calls can remain blocked, rather than leaking a thread per doctor run.
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        job()
    })
    .await
    .map_err(|_| ProbeIssue::Unavailable)?
}
pub(super) async fn file_exists(path: PathBuf) -> Probed<bool> {
    io_task(move || match std::fs::metadata(path) {
        Ok(meta) => Ok(meta.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_issue(error)),
    })
    .await
}
pub(super) async fn read_text(path: PathBuf) -> Probed<String> {
    io_task(move || {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
            .open(path)
            .map_err(io_issue)?;
        let meta = file.metadata().map_err(io_issue)?;
        if !meta.is_file() || meta.len() > LIMIT as u64 {
            return Err(ProbeIssue::InvalidData);
        }
        let mut bytes = Vec::new();
        file.take(LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io_issue)?;
        if bytes.len() > LIMIT {
            return Err(ProbeIssue::InvalidData);
        }
        String::from_utf8(bytes).map_err(|_| ProbeIssue::InvalidData)
    })
    .await
}
pub(super) async fn is_executable(path: PathBuf) -> Probed<bool> {
    io_task(move || Ok(super::executable(&path))).await
}
pub(super) async fn which(tool: String, path: String) -> Probed<bool> {
    io_task(move || {
        Ok(path
            .split(':')
            .filter(|part| !part.is_empty() && Path::new(part).is_absolute())
            .any(|part| super::executable(&Path::new(part).join(&tool))))
    })
    .await
}
pub(super) struct Output {
    pub success: bool,
    pub code: Option<i32>,
    pub text: String,
}
struct OwnedGroup(i32);
impl Drop for OwnedGroup {
    fn drop(&mut self) {
        // Only the fresh child's process group is owned by this probe.
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(self.0),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
}
pub(super) async fn command(program: &str, args: &[&str], capture: bool) -> Probed<Output> {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .process_group(0)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("NO_COLOR", "1")
        .env("RUSTUP_AUTO_INSTALL", "0")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("DISABLE_AUTOUPDATER", "1");
    if program == "systemctl" {
        use devbox_core::Env;
        command.env(
            "XDG_RUNTIME_DIR",
            format!("/run/user/{}", devbox_core::OsEnv.uid()),
        );
    }
    let mut child = command.spawn().map_err(io_issue)?;
    let _group = OwnedGroup(
        child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .ok_or(ProbeIssue::Unavailable)?,
    );
    let mut stdout = child.stdout.take();
    let read = async {
        let mut bytes = Vec::new();
        if let Some(stdout) = stdout.take() {
            stdout
                .take(LIMIT as u64 + 1)
                .read_to_end(&mut bytes)
                .await
                .map_err(io_issue)?;
        }
        if bytes.len() > LIMIT {
            return Err(ProbeIssue::InvalidData);
        }
        String::from_utf8(bytes).map_err(|_| ProbeIssue::InvalidData)
    };
    let wait = async { child.wait().await.map_err(io_issue) };
    let (text, status) = tokio::try_join!(read, wait)?;
    Ok(Output {
        success: status.success(),
        code: status.code(),
        text,
    })
}
pub(super) async fn version(program: &str, args: &[&str]) -> Probed<Option<String>> {
    match command(program, args, true).await {
        Ok(output) => Ok(output.success.then(|| output.text.trim().to_owned())),
        Err(ProbeIssue::Missing) => Ok(None),
        Err(issue) => Err(issue),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn doctor_probe_deadline_is_two_seconds() {
        let start = std::time::Instant::now();
        let result = run(async {
            tokio::time::sleep(Duration::from_secs(3)).await;
            Ok(())
        });
        assert_eq!(result, Err(ProbeIssue::Timeout));
        assert!(start.elapsed() < Duration::from_secs(4));
    }
    #[test]
    fn doctor_timeout_terminates_only_its_owned_child_group() {
        let temp = tempfile::tempdir().expect("owned fixture");
        let pid_file = temp.path().join("child.pid");
        let result = run(command(
            "/bin/sh",
            &[
                "-c",
                "echo $$ > \"$1\"; exec sleep 60",
                "fixture",
                pid_file.to_str().expect("fixture path"),
            ],
            false,
        ));
        assert!(matches!(result, Err(ProbeIssue::Timeout)));
        let pid: u32 = std::fs::read_to_string(pid_file)
            .expect("owned pid")
            .trim()
            .parse()
            .expect("pid number");
        for _ in 0..100 {
            if !Path::new(&format!("/proc/{pid}")).exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("owned probe child was not reaped");
    }
    #[test]
    fn doctor_probe_limits_output_and_discards_status_output() {
        assert!(matches!(
            run(command("/bin/sh", &["-c", "head -c 70000 /dev/zero"], true)),
            Err(ProbeIssue::InvalidData)
        ));
        let output = run(command(
            "/bin/sh",
            &["-c", "printf fixture-secret; printf diagnostic >&2; exit 1"],
            false,
        ))
        .expect("owned fixture command");
        assert!(!output.success);
        assert!(output.text.is_empty());
    }
    #[test]
    fn doctor_file_probe_checks_presence_without_reading_and_rejects_non_regular_reads() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let temp = tempfile::tempdir().expect("owned fixture");
        let auth = temp.path().join("auth.json");
        std::fs::write(&auth, "fixture-secret").expect("fixture write");
        std::fs::set_permissions(&auth, std::fs::Permissions::from_mode(0o0))
            .expect("fixture permissions");
        assert_eq!(run(file_exists(auth.clone())), Ok(true));
        let link = temp.path().join("config.toml");
        symlink(&auth, &link).expect("owned symlink");
        assert!(run(read_text(link)).is_err());
        assert!(run(read_text(temp.path().to_owned())).is_err());
    }
}
