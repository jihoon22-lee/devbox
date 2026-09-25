#![cfg(all(target_os = "linux", feature = "helper"))]
use std::{
    io::Read,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Owner(Child, bool);
impl Owner {
    fn wait(&mut self, code: i32) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                self.1 = true;
                assert_eq!(status.code(), Some(code));
                return;
            }
            assert!(
                Instant::now() < until,
                "native process owner did not retire"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn cancel(&self) {
        assert!(!self.1);
        assert_eq!(unsafe { libc::kill(self.0.id() as i32, libc::SIGTERM) }, 0);
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if !self.1 {
            // The Child has not been reaped; this PID remains owned. TERM asks
            // the supervisor to retire its descendants even after a test panic.
            unsafe {
                libc::kill(self.0.id() as i32, libc::SIGTERM);
            }
            let _ = self.0.wait();
        }
    }
}
fn launch(script: &str, directory: &Path) -> Owner {
    Owner(
        Command::new(env!("CARGO_BIN_EXE_devbox-workspace-wsl"))
            .args(["--supervise", "--", "/bin/sh", "-c", script, "fixture"])
            .arg(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        false,
    )
}
fn marker(path: &Path) -> i32 {
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        if let Ok(value) = std::fs::read_to_string(path) {
            if let Ok(pid) = value.trim().parse::<i32>() {
                return pid;
            }
        }
        assert!(Instant::now() < until, "owned process marker missing");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn gone(pid: i32) {
    assert!(
        !Path::new(&format!("/proc/{pid}")).exists(),
        "descendant was not reaped"
    );
}

#[test]
fn root_exit_preserves_status_and_reaps_detached_pipe_holders() {
    let directory = tempfile::tempdir().unwrap();
    let mut owner = launch(
        "setsid /bin/sh -c 'echo $$ > \"$1/detached\"; sleep 20 & echo $! > \"$1/leaf\"; wait' fixture \"$1\" & while [ ! -s \"$1/leaf\" ]; do sleep 0.01; done; echo root-output; exit 7",
        directory.path());
    let detached = marker(&directory.path().join("detached"));
    let leaf = marker(&directory.path().join("leaf"));
    owner.wait(7);
    gone(detached);
    gone(leaf);
    let mut output = String::new();
    owner
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert_eq!(output, "root-output\n");
}

#[test]
fn cancellation_reaps_detached_descendants_without_touching_other_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let mut other = launch("sleep 20", directory.path());
    let mut owner = launch(
        "setsid /bin/sh -c 'echo $$ > \"$1/detached\"; sleep 20 & echo $! > \"$1/leaf\"; wait' fixture \"$1\" & wait",
        directory.path());
    let detached = marker(&directory.path().join("detached"));
    let leaf = marker(&directory.path().join("leaf"));
    owner.cancel();
    owner.wait(143);
    gone(detached);
    gone(leaf);
    assert!(other.0.try_wait().unwrap().is_none());
    other.cancel();
    other.wait(143);
}

#[test]
fn invalid_launch_never_runs_a_program() {
    let status = Command::new(env!("CARGO_BIN_EXE_devbox-workspace-wsl"))
        .args(["--supervise", "--", "relative-program"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(125));
}

#[test]
fn self_reexecution_preserves_nested_owner_exit_status() {
    let mut owner = Owner(
        Command::new(env!("CARGO_BIN_EXE_devbox-workspace-wsl"))
            .args([
                "--supervise",
                "--",
                "/proc/self/exe",
                "--supervise",
                "--",
                "/bin/sh",
                "-c",
                "exit 11",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        false,
    );
    owner.wait(11);
}

#[test]
fn parent_death_retires_the_owned_tree() {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "isolated_parent_death_fixture", "--ignored"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}

// Isolate the fixture's own adoption from parallel test processes. The test
// harness above reaps this fixture, which in turn reaps the orphaned supervisor.
#[test]
#[ignore]
fn isolated_parent_death_fixture() {
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    let directory = tempfile::tempdir().unwrap();
    let mut parent = Command::new("/bin/sh")
        .args(["-c", r#""$1" --supervise -- /bin/sh -c 'echo $$ > "$1/root"; sleep 20 & echo $! > "$1/leaf"; wait' fixture "$2" & echo $! > "$2/owner"; while [ ! -f "$2/release" ]; do sleep 0.01; done; exit 9"#, "fixture"])
        .arg(env!("CARGO_BIN_EXE_devbox-workspace-wsl")).arg(directory.path())
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let owner = marker(&directory.path().join("owner"));
    let root = marker(&directory.path().join("root"));
    let leaf = marker(&directory.path().join("leaf"));
    std::fs::write(directory.path().join("release"), b"exit").unwrap();
    assert_eq!(parent.wait().unwrap().code(), Some(9));
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let mut status = 0;
        let result = unsafe { libc::waitpid(owner, &mut status, libc::WNOHANG) };
        if result == owner {
            assert!(libc::WIFEXITED(status));
            assert_eq!(libc::WEXITSTATUS(status), 143);
            break;
        }
        assert_eq!(result, 0);
        assert!(Instant::now() < until, "orphaned supervisor did not retire");
        std::thread::sleep(Duration::from_millis(5));
    }
    gone(root);
    gone(leaf);
}
