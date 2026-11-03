//! The restart step: setup restarts the daemon unit only when its install changed, when the
//! unit is not running this install, or when asked to. A fake user manager answers every
//! `systemctl` call; `/proc` is either a fake root or the real entry of this test process.
use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::PathBuf;

const ENV: &[u8] = b"PATH=/usr/bin\0";
const SHOW: &str =
    "systemctl --user show --property=ActiveState,MainPID -- devbox@test-restart.service";
const RELOAD: &str = "systemctl --user daemon-reload";
const RESTART: &str = "systemctl --user restart devbox@test-restart.service";

struct FakeEnv(PathBuf);
impl devbox_core::paths::Env for FakeEnv {
    fn var(&self, key: &str) -> Option<String> {
        (key == "HOME").then(|| self.0.display().to_string())
    }
    fn uid(&self) -> u32 {
        12345
    }
}
/// The user manager as setup sees it: the captured login environment, the answer to
/// `systemctl show` (`None` fails the call) and the `/proc` root of the daemon's `exe` link.
struct Manager {
    env: &'static [u8],
    show: Option<String>,
    proc: PathBuf,
    calls: std::sync::Mutex<Vec<String>>,
}
impl Manager {
    fn new(show: Option<String>) -> Self {
        Self {
            env: ENV,
            show,
            proc: "/proc".into(),
            calls: Default::default(),
        }
    }
}
impl Runner for Manager {
    async fn run(&self, cmd: &str, args: &[&str]) -> Result<Vec<u8>, String> {
        let call = format!("{cmd} {}", args.join(" "));
        self.calls.lock().expect("calls").push(call.clone());
        match cmd {
            "bash" => Ok(self.env.to_vec()),
            _ if call == SHOW => self
                .show
                .clone()
                .map(String::into_bytes)
                .ok_or_else(|| "fixture failure".into()),
            _ => Ok(vec![]),
        }
    }
    fn proc(&self) -> &Path {
        &self.proc
    }
}
/// `systemctl show` for an active unit whose main process is `pid`.
fn active(pid: u32) -> Option<String> {
    Some(format!("MainPID={pid}\nActiveState=active\n"))
}
/// This test process stands in for a daemon that runs this very executable.
fn current() -> Manager {
    Manager::new(active(std::process::id()))
}
/// A fake `/proc` whose process 4242 runs `target`, linked the way the kernel shows it.
fn running(home: &Path, target: &Path) -> Manager {
    let root = home.join("proc");
    std::fs::create_dir_all(root.join("4242")).expect("owned fake proc");
    symlink(target, root.join("4242/exe")).expect("owned exe link");
    Manager {
        proc: root,
        ..Manager::new(active(4242))
    }
}
/// A first install in an owned HOME. Everything is new, so it restarts without asking.
async fn installed(home: &Path) -> devbox_core::Paths {
    let paths = devbox_core::Paths::resolve(
        devbox_core::Instance::parse("test-restart").expect("owned instance"),
        &FakeEnv(home.into()),
    )
    .expect("owned paths");
    let first = Manager::new(None);
    let report = install(&paths, &first, false).await;
    assert!(report.steps.iter().all(|step| step.ok), "{report:?}");
    let calls = first.calls.into_inner().expect("calls");
    assert_eq!(calls.last().map(String::as_str), Some(RESTART));
    paths
}
/// Installs again; returns the restart step and every call made.
async fn reinstall(
    paths: &devbox_core::Paths,
    manager: Manager,
    force: bool,
) -> (Step, Vec<String>) {
    let mut report = install(paths, &manager, force).await;
    assert_eq!(report.steps.len(), 5, "{report:?}");
    let step = report.steps.pop().expect("restart step");
    (step, manager.calls.into_inner().expect("calls"))
}
fn skipped(step: &Step) -> bool {
    step.id == "restart" && step.ok && step.message == "건너뜀(변경 없음)"
}
fn restarted(step: &Step, calls: &[String]) -> bool {
    step.id == "restart"
        && step.ok
        && step.message.is_empty()
        && calls.last().map(String::as_str) == Some(RESTART)
}
fn stamp(path: &Path) -> (u64, i64, i64) {
    let meta = std::fs::metadata(path).expect("owned file");
    (meta.ino(), meta.mtime(), meta.mtime_nsec())
}
fn asked(calls: &[String]) -> bool {
    calls.iter().any(|call| call == SHOW)
}

#[tokio::test]
async fn unchanged_install_skips_the_daemon_restart() {
    let home = tempfile::tempdir().expect("owned home");
    let paths = installed(home.path()).await;
    let unit = paths.xdg_config_home.join("systemd/user/devbox@.service");
    let env = paths.instance_config_dir.join("env");
    let before = [stamp(&unit), stamp(&env)];
    let (step, calls) = reinstall(&paths, current(), false).await;
    assert!(skipped(&step), "{step:?}");
    // Reload and enable still run; then setup asks the manager once and restarts nothing.
    assert_eq!(calls[..2], ["bash -lc env -0", RELOAD]);
    assert!(
        calls[2].starts_with("systemctl --user enable --now "),
        "{calls:?}"
    );
    assert_eq!(calls[3..], [SHOW]);
    // Identical files are left as they are.
    assert_eq!([stamp(&unit), stamp(&env)], before);
}

#[tokio::test]
async fn changed_binary_restarts() {
    let home = tempfile::tempdir().expect("owned home");
    let paths = installed(home.path()).await;
    // The daemon still runs the file an earlier install placed; setup runs from another one.
    let older = home.path().join("devbox-older");
    std::fs::write(&older, "older build").expect("owned binary");
    let (step, calls) = reinstall(&paths, running(home.path(), &older), false).await;
    assert!(asked(&calls), "{calls:?}");
    assert!(restarted(&step, &calls), "{step:?} {calls:?}");
}

#[tokio::test]
async fn deleted_or_replaced_daemon_binary_restarts() {
    let home = tempfile::tempdir().expect("owned home");
    let paths = installed(home.path()).await;
    // The file the daemon started from was replaced at this same path: the kernel marks it.
    let mut deleted = std::env::current_exe()
        .expect("this executable")
        .into_os_string();
    deleted.push(" (deleted)");
    let (step, calls) = reinstall(&paths, running(home.path(), Path::new(&deleted)), false).await;
    assert!(asked(&calls), "{calls:?}");
    assert!(restarted(&step, &calls), "{step:?} {calls:?}");
}

#[tokio::test]
async fn unknown_main_pid_restarts() {
    let home = tempfile::tempdir().expect("owned home");
    let paths = installed(home.path()).await;
    for show in [
        Some("MainPID=0\nActiveState=active\n"),
        Some("ActiveState=active\n"),
        Some("MainPID=abc\nActiveState=active\n"),
        Some(""),
        None,
    ] {
        let manager = Manager::new(show.map(str::to_owned));
        let (step, calls) = reinstall(&paths, manager, false).await;
        assert!(asked(&calls), "{show:?} {calls:?}");
        assert!(restarted(&step, &calls), "{show:?} {step:?}");
    }
    // A MainPID whose `/proc` entry cannot be read: gone, or not ours.
    let manager = Manager {
        proc: home.path().join("empty-proc"),
        ..Manager::new(active(4242))
    };
    let (step, calls) = reinstall(&paths, manager, false).await;
    assert!(asked(&calls), "{calls:?}");
    assert!(restarted(&step, &calls), "{step:?} {calls:?}");
}

#[tokio::test]
async fn inactive_daemon_restarts() {
    let home = tempfile::tempdir().expect("owned home");
    let paths = installed(home.path()).await;
    let pid = std::process::id();
    for show in [
        "MainPID=0\nActiveState=inactive\n".to_owned(),
        "MainPID=0\nActiveState=failed\n".to_owned(),
        // Only `active` is current, even with a main process that runs this executable.
        format!("MainPID={pid}\nActiveState=activating\n"),
        format!("MainPID={pid}\nActiveState=deactivating\n"),
    ] {
        let (step, calls) = reinstall(&paths, Manager::new(Some(show.clone())), false).await;
        assert!(asked(&calls), "{show:?} {calls:?}");
        assert!(restarted(&step, &calls), "{show:?} {step:?}");
    }
}

#[tokio::test]
async fn changed_unit_restarts() {
    let home = tempfile::tempdir().expect("owned home");
    let paths = installed(home.path()).await;
    let (step, _) = reinstall(&paths, current(), false).await;
    assert!(skipped(&step), "unchanged control: {step:?}");
    // A managed unit on disk differs from what setup writes now: it is rewritten and applied.
    let unit = paths.xdg_config_home.join("systemd/user/devbox@.service");
    let body = std::fs::read_to_string(&unit).expect("owned unit");
    std::fs::write(&unit, body.replace("RestartSec=1", "RestartSec=9")).expect("owned unit");
    let (step, calls) = reinstall(&paths, current(), false).await;
    assert_eq!(std::fs::read_to_string(&unit).expect("owned unit"), body);
    assert!(restarted(&step, &calls), "{step:?} {calls:?}");
    // daemon-reload loads the new unit first; a known change needs no state query.
    let index = |wanted: &str| calls.iter().position(|call| call == wanted).expect(wanted);
    assert!(index(RELOAD) < index(RESTART), "{calls:?}");
    assert!(!asked(&calls), "{calls:?}");
}

#[tokio::test]
async fn changed_env_restarts() {
    let home = tempfile::tempdir().expect("owned home");
    let paths = installed(home.path()).await;
    let (step, _) = reinstall(&paths, current(), false).await;
    assert!(skipped(&step), "unchanged control: {step:?}");
    // The login shell now has another PATH: the env file changes and the daemon must read it.
    let manager = Manager {
        env: b"PATH=/opt/tool/bin:/usr/bin\0",
        ..current()
    };
    let (step, calls) = reinstall(&paths, manager, false).await;
    assert_eq!(
        std::fs::read_to_string(paths.instance_config_dir.join("env")).expect("owned env"),
        "PATH=\"/opt/tool/bin:/usr/bin\"\n"
    );
    assert!(restarted(&step, &calls), "{step:?} {calls:?}");
    assert!(!asked(&calls), "{calls:?}");
}

#[tokio::test]
async fn forced_restart_restarts_even_when_nothing_changed() {
    let home = tempfile::tempdir().expect("owned home");
    let paths = installed(home.path()).await;
    let (step, _) = reinstall(&paths, current(), false).await;
    assert!(skipped(&step), "unchanged control: {step:?}");
    // [다시 설정] and [다시 시도] run `setup --restart`: the same current daemon is restarted.
    let (step, calls) = reinstall(&paths, current(), true).await;
    assert!(restarted(&step, &calls), "{step:?} {calls:?}");
    assert_eq!(calls[1], RELOAD);
    assert!(!asked(&calls), "{calls:?}");
}

#[test]
fn publication_reports_whether_the_file_changed() {
    let temp = tempfile::tempdir().expect("owned directory");
    let file = temp.path().join("env");
    assert!(publish(&file, "PATH=\"/usr/bin\"\n", false).expect("first publication"));
    let before = stamp(&file);
    assert!(!publish(&file, "PATH=\"/usr/bin\"\n", false).expect("same body"));
    assert_eq!(stamp(&file), before, "an identical file is not rewritten");
    assert!(publish(&file, "PATH=\"/bin\"\n", false).expect("new body"));
    assert_eq!(
        std::fs::read_to_string(&file).expect("owned file"),
        "PATH=\"/bin\"\n"
    );
    // The same bytes in a file others can read are made private again, which is a change.
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("owned file");
    assert!(publish(&file, "PATH=\"/bin\"\n", false).expect("private again"));
    let mode = std::fs::metadata(&file)
        .expect("owned file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    // A foreign file is still refused before any comparison, even with the same bytes.
    let foreign = temp.path().join("foreign.service");
    std::fs::write(&foreign, "PATH=\"/bin\"\n").expect("owned file");
    assert!(publish(&foreign, "PATH=\"/bin\"\n", true).is_err());
}
