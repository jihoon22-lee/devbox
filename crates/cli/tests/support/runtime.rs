//! Real user-systemd fixtures: every mutable unit and file is uniquely owned.
use super::{TestDaemon, bounded};
use devbox_cli::client::Client;
use devbox_runtime::*;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, Instant},
};

pub struct RuntimeFixture {
    pub daemon: TestDaemon,
    pub slice: String,
    bus: String,
    anchor: String,
    control_dir: PathBuf,
    owns_resources: bool,
}
impl RuntimeFixture {
    pub fn new() -> Self {
        let mut daemon = TestDaemon::fixture();
        daemon.extra_env.retain(|(key, _)| key != "DEVBOX_NO_SCOPE");
        Self::with_daemon(daemon)
    }
    pub fn with_daemon(mut daemon: TestDaemon) -> Self {
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata("/proc/self").expect("process uid").uid();
        let bus = format!("unix:path=/run/user/{uid}/bus");
        let slice = format!("devbox-{}.slice", daemon.instance);
        let anchor = format!("devbox-{}-fixture.service", daemon.instance);
        let control_dir = PathBuf::from(format!("/run/user/{uid}/systemd/user.control/{slice}.d"));
        daemon.extra_env.extend([
            ("DBUS_SESSION_BUS_ADDRESS".into(), bus.clone()),
            ("DEVBOX_TEST_SLICE".into(), slice.clone()),
        ]);
        daemon.paths.ensure_dirs().expect("fixture paths");
        // Fixed fixture values, never capture the user's environment or secrets.
        let env =
            devbox_cli::setup::capture_env(b"PATH=/usr/local/bin:/usr/bin:/bin\0LANG=C.UTF-8\0");
        std::fs::write(daemon.paths.instance_config_dir.join("env"), env).expect("fixture env");
        let mut fixture = Self {
            daemon,
            slice,
            bus,
            anchor,
            control_dir,
            owns_resources: false,
        };
        assert!(fixture.units().is_empty(), "fresh instance has no units");
        assert!(
            std::fs::symlink_metadata(&fixture.control_dir)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
            "fresh fixture control directory must not exist"
        );
        let existing = fixture.success(
            "systemctl",
            &[
                "--user",
                "list-units",
                "--all",
                "--output=json",
                "--",
                &fixture.slice,
                &fixture.anchor,
            ],
        );
        let existing: Vec<serde::de::IgnoredAny> =
            serde_json::from_slice(&existing.stdout).expect("existing fixture resources");
        assert!(
            existing.is_empty(),
            "fixture must not reuse an existing slice or anchor"
        );
        fixture.owns_resources = true;
        fixture.success(
            "systemd-run",
            &[
                "--user",
                "--unit",
                &fixture.anchor,
                "--slice",
                &fixture.slice,
                "--collect",
                "--property=Nice=10",
                "--property=MemoryHigh=6G",
                "--property=MemoryMax=8G",
                "--property=MemorySwapMax=1G",
                "--property=CPUQuota=400%",
                "--",
                "/bin/sleep",
                "600",
            ],
        );
        let cpus = allowed_cpus();
        fixture.success(
            "systemctl",
            &[
                "--user",
                "set-property",
                "--runtime",
                &fixture.slice,
                "MemoryHigh=6G",
                "MemoryMax=8G",
                "MemorySwapMax=1G",
                "CPUQuota=400%",
                &format!("AllowedCPUs={cpus}"),
            ],
        );
        let properties = fixture.show(&fixture.slice);
        for (name, value) in [
            ("MemoryHigh", "6442450944"),
            ("MemoryMax", "8589934592"),
            ("MemorySwapMax", "1073741824"),
        ] {
            assert_eq!(properties.get(name).map(String::as_str), Some(value));
        }
        eprintln!(
            "owned runtime slice={} memoryHigh=6GiB memoryMax=8GiB swapMax=1GiB cpus={cpus} quota=400%",
            fixture.slice
        );
        fixture.daemon.launch();
        fixture
    }
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new("/usr/bin/timeout");
        command
            .args(["15s", program])
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus);
        command
    }
    fn output(&self, program: &str, args: &[&str]) -> Output {
        self.command(program)
            .args(args)
            .output()
            .expect("owned systemd command")
    }
    fn success(&self, program: &str, args: &[&str]) -> Output {
        let output = self.output(program, args);
        assert!(
            output.status.success(),
            "{program} {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }
    pub fn show(&self, unit: &str) -> BTreeMap<String, String> {
        assert!(
            unit == self.slice || unit.starts_with(&format!("devbox-{}-", self.daemon.instance))
        );
        let output = self.output("systemctl", &["--user", "show", "--", unit]);
        let fields: BTreeMap<_, _> = String::from_utf8(output.stdout)
            .expect("systemd UTF-8")
            .lines()
            .filter_map(|line| {
                line.split_once('=')
                    .map(|(key, value)| (key.into(), value.into()))
            })
            .collect();
        assert!(
            output.status.success()
                || fields
                    .get("LoadState")
                    .is_some_and(|state| state == "not-found"),
            "systemd show failed"
        );
        fields
    }
    pub fn units(&self) -> Vec<String> {
        #[derive(Deserialize)]
        struct Unit {
            unit: String,
        }
        let pattern = format!("devbox-{}-run-*", self.daemon.instance);
        let output = self.success(
            "systemctl",
            &[
                "--user",
                "list-units",
                "--all",
                "--output=json",
                "--",
                &pattern,
            ],
        );
        serde_json::from_slice::<Vec<Unit>>(&output.stdout)
            .expect("unit list")
            .into_iter()
            .map(|unit| unit.unit)
            .collect()
    }
    pub fn kill_main(&self, run: &Run) {
        let properties = self.show(&run.unit);
        assert_eq!(properties.get("Slice"), Some(&self.slice));
        assert_eq!(
            properties
                .get("MainPID")
                .and_then(|pid| pid.parse::<u32>().ok()),
            run.pid
        );
        assert!(run.pid.is_some_and(|pid| pid > 1));
        let executable = std::fs::read_link(format!("/proc/{}/exe", run.pid.expect("owned pid")))
            .expect("owned executable");
        assert_eq!(
            executable,
            std::fs::canonicalize(env!("CARGO_BIN_EXE_devbox")).expect("fixture executable")
        );
        // Target the verified owned unit, avoiding a raw PID-reuse race.
        self.success(
            "systemctl",
            &[
                "--user",
                "kill",
                "--kill-whom=main",
                "--signal=KILL",
                "--",
                &run.unit,
            ],
        );
    }
    pub async fn start_limit_notified(&self, id: &str) -> bool {
        let migrations = devbox_projects::MIGRATIONS
            .iter()
            .chain(devbox_terminal::MIGRATIONS)
            .chain(devbox_agents::MIGRATIONS)
            .chain(devbox_runtime::MIGRATIONS)
            .chain(devbox_secrets::MIGRATIONS)
            .chain(devbox_api::MIGRATIONS)
            .chain(devbox_webhooks::MIGRATIONS)
            .chain(devbox_search::MIGRATIONS)
            .chain(devbox_activity::MIGRATIONS)
            .copied()
            .collect::<Vec<_>>();
        let db = devbox_core::Db::open(
            &self.daemon.paths.db_path,
            &self.daemon.paths.backups_dir,
            env!("CARGO_PKG_VERSION"),
            &migrations,
        )
        .expect("owned DB");
        let id = id.to_owned();
        db.read(move |connection| {
            connection.query_row(
                "SELECT json_extract(body,'$.start_limit_notified') FROM runtime_runs WHERE id=?1",
                [id],
                |row| row.get(0),
            )
        })
        .await
        .expect("persisted notification marker")
    }
    pub async fn project(&self, client: &Client) -> String {
        let repo = self.daemon.git_repo("project");
        self.daemon.add_project(client, &repo).await
    }
    pub async fn define(&self, client: &Client, project: &str, def: RunDef, save_to: SaveTo) {
        bounded(client.call::<RuntimeDefine>(&DefineRun {
            project_id: project.into(),
            def,
            save_to,
        }))
        .await
        .expect("define fixture run");
    }
    pub async fn start(&self, client: &Client, project: &str, id: &str) -> Run {
        bounded(client.call::<RuntimeStart>(&StartRun {
            project_id: project.into(),
            id: id.into(),
            worktree: None,
        }))
        .await
        .unwrap_or_else(|error| {
            panic!(
                "start fixture run: {error:?}; daemon={}",
                std::fs::read_to_string(self.daemon.home.path().join("stderr.log"))
                    .unwrap_or_default()
            )
        })
    }
    pub async fn runs(&self, client: &Client, project: &str) -> Vec<Run> {
        bounded(client.call::<RuntimeRuns>(&RunsQuery {
            project_id: Some(project.into()),
            limit: 100,
        }))
        .await
        .expect("fixture runs")
        .items
    }
    pub async fn wait(
        &self,
        client: &Client,
        project: &str,
        id: &str,
        expected: RunState,
        timeout: Duration,
    ) -> Run {
        let deadline = Instant::now() + timeout;
        loop {
            let run = self
                .runs(client, project)
                .await
                .into_iter()
                .find(|run| run.id == id)
                .expect("persisted run");
            if run.state == expected {
                return run;
            }
            assert!(
                Instant::now() < deadline,
                "expected {expected:?}, got {run:?}; unit={:?}; daemon={}",
                self.show(&run.unit),
                std::fs::read_to_string(self.daemon.home.path().join("stderr.log"))
                    .unwrap_or_default()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    pub async fn start_limit(&self, run: &Run) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let properties = self.show(&run.unit);
            if properties
                .get("Result")
                .is_some_and(|value| value == "start-limit-hit")
                || properties
                    .get("ActiveState")
                    .is_some_and(|value| value == "failed")
                    && properties
                        .get("SubState")
                        .is_some_and(|value| value == "failed")
                    && properties
                        .get("Result")
                        .is_some_and(|value| value == "exit-code")
                    && properties
                        .get("NRestarts")
                        .is_some_and(|value| value == "3")
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "start-limit deadline: {properties:?}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    pub async fn approve(&self, client: &Client, project: &str) {
        use devbox_projects::*;
        let preview = bounded(client.call::<ProjectsTrustPreview>(&TrustPreviewParams {
            id: project.into(),
            operation: TrustOperation::Run,
            agent_id: None,
            worktrees: vec![],
            worktree: None,
        }))
        .await
        .expect("run trust preview");
        assert!(Path::new(&preview.canonical_path).starts_with(self.daemon.home.path()));
        bounded(client.call::<ProjectsTrustApprove>(&TrustApprove {
            id: project.into(),
            expected_canonical_path: preview.canonical_path,
            expected_digest: preview.digest.expect("configuration digest"),
            expected_scope_version: preview.scope_version,
        }))
        .await
        .expect("approve owned fixture snapshot");
    }
}
impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        self.daemon.stop();
        if !self.owns_resources {
            return;
        }
        // List before stopping the slice: failed units remain until reset-failed.
        let output = self.output(
            "systemctl",
            &[
                "--user",
                "list-units",
                "--all",
                "--plain",
                "--no-legend",
                "--",
                &format!("devbox-{}-run-*", self.daemon.instance),
            ],
        );
        let prefix = format!("devbox-{}-run-", self.daemon.instance);
        let units: Vec<_> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter(|unit| unit.starts_with(&prefix))
            .map(str::to_owned)
            .collect();
        let _ = self.output("systemctl", &["--user", "stop", "--", &self.slice]);
        for unit in units.iter().chain([&self.anchor, &self.slice]) {
            let _ = self.output("systemctl", &["--user", "stop", "--", unit]);
            let _ = self.output("systemctl", &["--user", "reset-failed", "--", unit]);
        }
        // set-property --runtime persists these drop-ins beyond stop/reset-failed.
        // Remove only this fresh fixture's exact files; do not reload the manager.
        if let Err(error) = remove_control_files(&self.control_dir) {
            if std::thread::panicking() {
                eprintln!(
                    "owned runtime control cleanup failed: {}: {error}",
                    self.control_dir.display()
                );
            } else {
                panic!(
                    "owned runtime control cleanup failed: {}: {error}",
                    self.control_dir.display()
                );
            }
        }
    }
}
fn remove_control_files(directory: &Path) -> std::io::Result<()> {
    let metadata = match std::fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "owned control directory was replaced",
        ));
    }
    for property in [
        "AllowedCPUs",
        "CPUQuota",
        "MemoryHigh",
        "MemoryMax",
        "MemorySwapMax",
    ] {
        match std::fs::remove_file(directory.join(format!("50-{property}.conf"))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    // Unknown files are retained and reported rather than recursively deleted.
    std::fs::remove_dir(directory)
}
fn allowed_cpus() -> String {
    let status = std::fs::read_to_string("/proc/self/status").expect("CPU affinity");
    let list = status
        .lines()
        .find_map(|line| line.strip_prefix("Cpus_allowed_list:"))
        .expect("CPU list")
        .trim();
    let mut cpus = Vec::new();
    for part in list.split(',') {
        let (first, last) = part.split_once('-').unwrap_or((part, part));
        for cpu in first.parse::<u32>().expect("first CPU")..=last.parse().expect("last CPU") {
            if cpus.len() < 4 {
                cpus.push(cpu.to_string());
            }
        }
    }
    cpus.join(",")
}
pub fn task(id: &str, command: &str) -> RunDef {
    RunDef::Task(TaskDef {
        id: id.into(),
        command: command.into(),
        ..Default::default()
    })
}
pub fn service(id: &str, command: &str, restart: RestartPolicy) -> RunDef {
    RunDef::Service(ServiceDef {
        id: id.into(),
        command: command.into(),
        restart,
        start_limit: StartLimit {
            burst: 3,
            interval_sec: 60,
        },
        ..Default::default()
    })
}
pub async fn file_exists(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "fixture file deadline: {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
