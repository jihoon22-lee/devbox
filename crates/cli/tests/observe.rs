mod support;
use devbox_cli::{
    client::{Client, ClientError},
    daemon,
};
use devbox_observe::*;
use devbox_protocol::{
    message::ClientKind,
    system::{Ping, PingParams},
};
use std::{sync::Arc, time::Duration};
use support::bounded;

struct Fixture {
    daemon: support::TestDaemon,
    client: Client,
    cancel: tokio_util::sync::CancellationToken,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl Fixture {
    async fn start() -> Self {
        Self::with_slow_docker(false).await
    }
    async fn with_slow_docker(slow: bool) -> Self {
        let daemon = support::TestDaemon::fixture();
        let mut services = daemon::build_services(daemon.paths.clone()).expect("fixture result");
        // No test may contact a real Docker daemon, including read-only ps.
        let docker = daemon.home.path().join("fake-docker");
        if slow {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(
                &docker,
                "#!/bin/sh\nprintf '%s' \"$$\" > \"$0.pid\"\nexec /bin/sleep 60\n",
            )
            .expect("fixture result");
            std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o700))
                .expect("fixture result");
        }
        Arc::get_mut(&mut services).expect("fixture value").observe =
            Arc::new(ObserveService::new(docker));
        let listener = tokio::net::UnixListener::bind(&daemon.socket).expect("fixture result");
        let cancel = tokio_util::sync::CancellationToken::new();
        let task = tokio::spawn(daemon::server::serve(listener, services, cancel.clone()));
        let client = bounded(Client::connect(
            &daemon.socket,
            ClientKind::Test,
            &daemon.instance,
        ))
        .await
        .expect("fixture result");
        Self {
            daemon,
            client,
            cancel,
            task,
        }
    }
    async fn stop(self) {
        self.cancel.cancel();
        bounded(self.task)
            .await
            .expect("fixture result")
            .expect("fixture result");
    }
}
#[tokio::test]
async fn typed_observe_routes_report_owned_listener_process_system_and_guard_kill() {
    let f = Fixture::start().await;
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("fixture result");
    let port = listener.local_addr().expect("fixture result").port();
    let rows = bounded(f.client.call::<ObservePorts>(&ObserveQuery {}))
        .await
        .expect("fixture result");
    let owned = rows
        .iter()
        .find(|p| p.port == port && p.pid == Some(std::process::id()))
        .expect("fixture value");
    assert!(
        owned
            .start_time_ms
            .is_some_and(|start| start > 1_000_000_000_000)
    );
    let system = bounded(f.client.call::<ObserveSystem>(&ObserveQuery {}))
        .await
        .expect("fixture result");
    assert!(system.memory_total_bytes > 0);
    assert!(system.home_filesystem.is_some());
    let project = bounded(f.client.call::<devbox_projects::ProjectsAdd>(
        &devbox_projects::AddProject {
            path: f.daemon.home.path().display().to_string(),
        },
    ))
    .await
    .expect("fixture result");
    let mut child = support::ChildGuard(
        std::process::Command::new("sleep")
            .arg("60")
            .current_dir(f.daemon.home.path())
            .spawn()
            .expect("fixture result"),
    );
    let rows = bounded(f.client.call::<ObserveProcesses>(&ProcessesQuery {
        project_id: Some(project.id.clone()),
    }))
    .await
    .expect("fixture result");
    let process = rows
        .iter()
        .find(|p| p.pid == child.0.id())
        .expect("fixture value");
    assert_eq!(process.project_id.as_deref(), Some(project.id.as_str()));
    assert!(process.start_time_ms > 1_000_000_000_000);
    let error = bounded(f.client.call::<ObserveKill>(&KillProcess {
        pid: child.0.id(),
        signal: KillSignal::Term,
        expect: ProcessExpectation {
            start_time_ms: process.start_time_ms + 1,
        },
    }))
    .await
    .expect_err("expected failure");
    assert!(matches!(error, ClientError::Rpc { code, .. } if code == "observe.state_changed"));
    assert!(child.0.try_wait().expect("fixture result").is_none());
    let result = bounded(f.client.call::<ObserveKill>(&KillProcess {
        pid: child.0.id(),
        signal: KillSignal::Term,
        expect: ProcessExpectation {
            start_time_ms: process.start_time_ms,
        },
    }))
    .await
    .expect("fixture result");
    assert_eq!(result.pid, child.0.id());
    assert!(!child.0.wait().expect("fixture result").success());
    bounded(f.client.call::<Ping>(&PingParams {}))
        .await
        .expect("fixture result");
    f.stop().await;
}
#[tokio::test]
async fn production_readiness_rejects_unrelated_listener_and_accepts_its_owner() {
    let callback = daemon::observe::readiness();
    let check = |ports, pid| devbox_runtime::ReadyCheck {
        unit: "fixture-only.service".into(),
        pid,
        ports,
    };
    assert!(bounded(callback(check(vec![], None))).await);
    let unrelated = support::ChildGuard(
        std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("owned child"),
    );
    for address in ["127.0.0.1", "::1"] {
        let listener = std::net::TcpListener::bind((address, 0)).expect("owned listener");
        let port = listener.local_addr().expect("listener address").port();
        assert!(
            !bounded(callback(check(vec![port], Some(unrelated.0.id())))).await,
            "{address}: another process listening must not ready this service"
        );
        assert!(!bounded(callback(check(vec![port], None))).await);
        assert!(bounded(callback(check(vec![port], Some(std::process::id())))).await);
        drop(listener);
        assert!(!bounded(callback(check(vec![port], Some(std::process::id())))).await);
    }
}

#[tokio::test]
async fn unavailable_docker_does_not_block_other_rpc_and_is_reaped() {
    let f = Fixture::with_slow_docker(true).await;
    let query = f.client.call::<ObservePorts>(&ObserveQuery {});
    tokio::pin!(query);
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("fixture result");
    let port = listener.local_addr().expect("fixture result").port();
    let marker = f.daemon.home.path().join("fake-docker.pid");
    tokio::select! {
        result = &mut query => panic!("slow fake should be pending: {result:?}"),
        _ = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
            while !marker.exists() {
                assert!(tokio::time::Instant::now() < deadline);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        } => (),
    }
    tokio::time::timeout(
        Duration::from_millis(500),
        f.client.call::<Ping>(&PingParams {}),
    )
    .await
    .expect("fixture result")
    .expect("fixture result");
    let rows = tokio::time::timeout(Duration::from_secs(3), &mut query)
        .await
        .expect("fixture result")
        .expect("fixture result");
    assert!(rows.iter().any(|p| p.port == port));
    let pid = std::fs::read_to_string(marker).expect("fixture result");
    assert!(
        !std::path::Path::new("/proc").join(pid).exists(),
        "owned fake Docker child reaped"
    );
    f.cancel.cancel();
    // The query future borrows the client until this scope ends.
    bounded(f.task)
        .await
        .expect("fixture result")
        .expect("fixture result");
}
