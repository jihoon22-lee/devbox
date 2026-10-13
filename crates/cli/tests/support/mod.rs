#![allow(dead_code)]
use devbox_cli::client::Client;
use devbox_core::{Env, Instance, Paths};
use devbox_protocol::message::ClientKind;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
mod agents;

pub struct ChildGuard(pub Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub struct TestDaemon {
    pub home: tempfile::TempDir,
    pub instance: String,
    pub socket: PathBuf,
    pub paths: Paths,
    pub child: Option<ChildGuard>,
    pub extra_env: Vec<(String, String)>,
    pub foreground: bool,
}
impl TestDaemon {
    pub fn fixture() -> Self {
        let home = tempfile::tempdir().expect("create fixture directory");
        std::fs::write(home.path().join(".gitconfig"), "[user]\nname = devbox-test\nemail = test@devbox.invalid\n[init]\ndefaultBranch = main\n").expect("write fixture Git identity");
        let instance = format!("test-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let runtime = home.path().join("run");
        std::fs::create_dir_all(&runtime).expect("create fixture directory");
        struct FixtureEnv(PathBuf, PathBuf);
        impl Env for FixtureEnv {
            fn var(&self, key: &str) -> Option<String> {
                match key {
                    "HOME" => Some(self.0.display().to_string()),
                    "XDG_RUNTIME_DIR" => Some(self.1.display().to_string()),
                    _ => None,
                }
            }
            fn uid(&self) -> u32 {
                1000
            }
        }
        let paths = Paths::resolve(
            Instance::parse(&instance).expect("parse fixture instance"),
            &FixtureEnv(home.path().into(), runtime),
        )
        .expect("parse fixture instance");
        Self {
            home,
            instance,
            socket: paths.socket.clone(),
            paths,
            child: None,
            extra_env: vec![("DEVBOX_NO_SCOPE".into(), "1".into())],
            foreground: true,
        }
    }
    pub fn command(&self) -> Command {
        let mut command = self.environment(env!("CARGO_BIN_EXE_devbox"));
        command.arg("daemon");
        if self.foreground {
            command.arg("--foreground");
        }
        command
    }
    pub fn environment(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .env("HOME", self.home.path())
            .env(
                "XDG_RUNTIME_DIR",
                self.socket.parent().expect("fixture path has parent"),
            )
            .env("DEVBOX_INSTANCE", &self.instance)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.home.path().join("stderr.log"))
                    .expect("create fixture file"),
            );
        for key in [
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
            "XDG_CONFIG_HOME",
            "LISTEN_FDS",
            "LISTEN_PID",
            "LISTEN_FDNAMES",
            "NOTIFY_SOCKET",
            "RUST_LOG",
            "TMUX",
            "TMUX_TMPDIR",
            "DEVBOX_NO_SCOPE",
        ] {
            command.env_remove(key);
        }
        command.envs(self.extra_env.iter().map(|(k, v)| (k, v)));
        command
    }
    pub fn start() -> Self {
        let mut daemon = Self::fixture();
        daemon.launch();
        daemon
    }
    pub fn launch(&mut self) {
        self.child = Some(ChildGuard(
            self.command().spawn().expect("spawn fixture process"),
        ));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = self
                .child
                .as_mut()
                .expect("fixture child exists")
                .0
                .try_wait()
                .expect("inspect fixture process");
            assert!(
                status.is_none(),
                "daemon exited: {:?}: {}",
                status,
                std::fs::read_to_string(self.home.path().join("stderr.log")).unwrap_or_default()
            );
            if std::os::unix::net::UnixStream::connect(&self.socket).is_ok() {
                break;
            }
            assert!(Instant::now() < deadline, "daemon readiness deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub async fn client(&self) -> Client {
        bounded(Client::connect(
            &self.socket,
            ClientKind::Test,
            &self.instance,
        ))
        .await
        .expect("connect fixture client")
    }
    pub fn stop(&mut self) {
        self.child.take();
    }
    pub fn restart(&mut self) {
        self.stop();
        self.launch();
    }
}
pub async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("operation deadline")
}
pub fn exits(mut child: ChildGuard) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().expect("inspect fixture process") {
            return status;
        }
        assert!(Instant::now() < deadline, "child exit deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.stop();
        self.kill_tmux_server();
    }
}
