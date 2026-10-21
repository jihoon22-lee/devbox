#[path = "../../../app/src-tauri/src/core/secrets.rs"]
mod native_secrets;
mod support;

use devbox_cli::{client::Client, daemon};
use devbox_protocol::message::ClientKind;
use devbox_runtime::*;
use devbox_secrets::{SecretsService, crypto::Key, keystore::DpapiKeyStore};
use std::{sync::Arc, time::Duration};
use support::{
    bounded,
    runtime::{RuntimeFixture, service},
};
use tokio_util::sync::CancellationToken;
const SECRET: &str = "owned-fixture-token-42";
struct Server {
    cancel: CancellationToken,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Server {
    async fn start(f: &RuntimeFixture) -> Self {
        let mut services = daemon::build_services(f.daemon.paths.clone()).expect("owned services");
        let secrets = Arc::new(
            SecretsService::new(services.db.clone(), f.daemon.paths.clone()).with_keystore(
                DpapiKeyStore::new(&f.daemon.paths.data_dir).with_executable("/bin/false".into()),
            ),
        );
        let mut execution = ExecutionConfig::new(f.daemon.paths.clone());
        execution.executable = env!("CARGO_BIN_EXE_devbox").into();
        execution.slice = Some(f.slice.clone());
        execution.windows_interop = f.daemon.home.path().join("absent-windows-interop");
        let runs = Arc::new(
            RuntimeService::new(
                services.db.clone(),
                services.hub.clone(),
                services.projects.clone(),
            )
            .with_execution(execution)
            .with_secrets(secrets.clone()),
        );
        let inner = Arc::get_mut(&mut services).expect("new service ownership");
        inner.secrets = secrets;
        inner.runs = runs;
        std::fs::remove_file(&f.daemon.socket).expect("owned stopped daemon socket");
        let listener = tokio::net::UnixListener::bind(&f.daemon.socket).expect("owned socket");
        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let task = tokio::spawn(daemon::server::serve(listener, services, shutdown));
        Self {
            cancel,
            task: Some(task),
        }
    }
    async fn stop(mut self) {
        self.cancel.cancel();
        bounded(self.task.take().expect("task"))
            .await
            .expect("join")
            .expect("shutdown");
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
async fn native(f: &RuntimeFixture) -> Client {
    bounded(Client::connect(
        &f.daemon.socket,
        ClientKind::App,
        &f.daemon.instance,
    ))
    .await
    .expect("native app client")
}
async fn bootstrap(f: &RuntimeFixture) -> Client {
    negotiate_native(f).await;
    let client = native(f).await;
    client
        .call::<devbox_secrets::Set>(&devbox_secrets::SetSecret {
            name: "TOKEN".into(),
            value: SECRET.into(),
        })
        .await
        .expect("fixture secret");
    client
}
fn secret_service(id: &str, command: &str) -> RunDef {
    let mut def = service(id, command, RestartPolicy::OnFailure);
    if let RunDef::Service(d) = &mut def {
        d.secrets = vec!["TOKEN".into()];
        d.env
            .insert("HEADER".into(), "Bearer {{secret:TOKEN}} suffix".into());
        d.restart_sec = 1;
    }
    def
}
fn ack_command(path: &std::path::Path, exit: bool) -> String {
    format!(
        "test \"${{#TOKEN}}\" = {} || exit 99; test \"$HEADER\" = \"Bearer $TOKEN suffix\" || exit 98; printf ACK\\n >> {}; printf '%s' \"${{TOKEN:0:6}}\"; sleep 0.05; printf '%s\\n' \"${{TOKEN:6}}\"; {}",
        SECRET.len(),
        devbox_core::sh_quote(&path.display().to_string()),
        if exit { "sleep 2; exit 3" } else { "sleep 30" }
    )
}
async fn ack_count(path: &std::path::Path, count: usize) {
    bounded(async {
        loop {
            if std::fs::read_to_string(path)
                .unwrap_or_default()
                .matches("ACK")
                .count()
                >= count
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await;
}
#[tokio::test]
#[ignore = "needs owned systemd user fixture"]
async fn run_with_secret_hides_value_from_argv_unit_properties_logs_and_rpc() {
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    let server = Server::start(&f).await;
    let app = bootstrap(&f).await;
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let ack = f.daemon.home.path().join("ack");
    f.define(
        &client,
        &project,
        secret_service("secret", &ack_command(&ack, true)),
        SaveTo::ThisPc,
    )
    .await;
    let run = f.start(&client, &project, "secret").await;
    ack_count(&ack, 2).await;
    assert_eq!(
        std::fs::read_to_string(&ack)
            .expect("ACKs")
            .matches("ACK")
            .count(),
        2
    );
    let props = f.show(&run.unit);
    assert!(!format!("{props:?}").contains(SECRET));
    let rows = f.runs(&client, &project).await;
    assert!(!serde_json::to_string(&rows).expect("runs").contains(SECRET));
    let logs = std::fs::read_to_string(
        devbox_runtime::exit_record::ExitStore::new(&f.daemon.paths.data_dir.join("runs"), &run.id)
            .expect("run log")
            .log_path(),
    )
    .expect("run log exists");
    assert!(!logs.contains(SECRET));
    assert!(logs.contains("[REDACTED]"));
    let states = f.show(&run.unit);
    let pid = states
        .get("MainPID")
        .filter(|p| p.as_str() != "0")
        .expect("second attempt alive");
    let mut pids = vec![pid.clone()];
    let mut checked = 0;
    while let Some(pid) = pids.pop() {
        let command = std::fs::read(format!("/proc/{pid}/cmdline")).expect("owned command");
        assert!(
            !command
                .windows(SECRET.len())
                .any(|w| w == SECRET.as_bytes())
        );
        checked += 1;
        if let Ok(children) = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children")) {
            pids.extend(children.split_whitespace().map(str::to_owned));
        }
    }
    assert!(
        checked >= 3,
        "helper, exec, and user child command lines checked"
    );
    drop(app);
    server.stop().await;
}
#[tokio::test]
#[ignore = "needs owned systemd user fixture"]
async fn windows_app_unlocks_when_daemon_interop_is_unavailable_and_reinjects_after_restart() {
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    let server = Server::start(&f).await;
    let app = bootstrap(&f).await;
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let ack = f.daemon.home.path().join("restart-ack");
    f.define(
        &client,
        &project,
        secret_service("restart", &ack_command(&ack, false)),
        SaveTo::ThisPc,
    )
    .await;
    let run = f.start(&client, &project, "restart").await;
    ack_count(&ack, 1).await;
    server.stop().await;
    drop(client);
    drop(app);
    // Already running child remains alive while the daemon is unavailable.
    assert_eq!(
        f.show(&run.unit).get("ActiveState").map(String::as_str),
        Some("active")
    );
    let unavailable = f
        .daemon
        .environment(env!("CARGO_BIN_EXE_devbox"))
        .args([
            "exec-attempt",
            "--run",
            &run.id,
            "--",
            "/usr/bin/touch",
            f.daemon
                .home
                .path()
                .join("forbidden")
                .to_str()
                .expect("path"),
        ])
        .status()
        .expect("unavailable helper");
    assert!(!unavailable.success());
    assert!(!f.daemon.home.path().join("forbidden").exists());
    let server = Server::start(&f).await;
    let app = native(&f).await;
    let status: devbox_protocol::secrets::KeyStatus = serde_json::from_str(
        &app.raw_call("secrets.key_status", devbox_secrets::Empty {}, None)
            .await
            .expect("locked status"),
    )
    .expect("status");
    assert!(status.locked);
    assert!(!status.key_mismatch);
    assert_eq!(status.key_id, Some(Key::from_bytes([7; 32]).id()));
    let locked = f
        .daemon
        .environment(env!("CARGO_BIN_EXE_devbox"))
        .args([
            "exec-attempt",
            "--run",
            &run.id,
            "--",
            "/usr/bin/touch",
            f.daemon
                .home
                .path()
                .join("forbidden")
                .to_str()
                .expect("path"),
        ])
        .status()
        .expect("locked helper");
    assert!(!locked.success());
    assert!(!f.daemon.home.path().join("forbidden").exists());
    negotiate_native(&f).await;
    let client = f.daemon.client().await;
    let restarted = client
        .call::<RuntimeRestart>(&RunId {
            run_id: run.id.clone(),
        })
        .await
        .expect("restart after unlock");
    ack_count(&ack, 2).await;
    assert_eq!(restarted.id, run.id);
    server.stop().await;
}

struct FixtureDpapi;
impl native_secrets::KeyProtector for FixtureDpapi {
    fn generate(&self) -> Result<zeroize::Zeroizing<Vec<u8>>, native_secrets::Locked> {
        Ok(zeroize::Zeroizing::new(vec![7; 32]))
    }
    fn seal(&self, key: &[u8]) -> Result<Vec<u8>, native_secrets::Locked> {
        assert_eq!(key, vec![7; 32]);
        Ok(b"owned-sealed-fixture".to_vec())
    }
    fn unseal(&self, blob: &[u8]) -> Result<zeroize::Zeroizing<Vec<u8>>, native_secrets::Locked> {
        assert_eq!(blob, b"owned-sealed-fixture");
        Ok(zeroize::Zeroizing::new(vec![7; 32]))
    }
}
async fn negotiate_native(f: &RuntimeFixture) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut wire = tokio::net::UnixStream::connect(&f.daemon.socket)
        .await
        .expect("native live socket");
    let mut native = native_secrets::NativeSecrets::default();
    let mut next = Some(native.start(&f.daemon.instance));
    let mut steps = 0;
    while let Some(request) = next {
        let request = zeroize::Zeroizing::new(request);
        wire.write_all(&request).await.expect("native frame");
        let mut length = [0; 4];
        bounded(wire.read_exact(&mut length))
            .await
            .expect("native response length");
        let length = u32::from_le_bytes(length) as usize;
        assert!(length < 512 * 1024);
        let mut response = vec![0; length + 4];
        response[..4].copy_from_slice(&(length as u32).to_le_bytes());
        bounded(wire.read_exact(&mut response[4..]))
            .await
            .expect("native response");
        assert!(
            !response
                .windows(SECRET.len())
                .any(|w| w == SECRET.as_bytes())
        );
        next = native.receive(&response, &FixtureDpapi);
        steps += 1;
        assert!(steps <= 4);
    }
    assert!(
        steps >= 3,
        "production native state machine completed handshake/status/unlock"
    );
    let client = native_client_status(f).await;
    assert!(!client.locked);
    assert_eq!(client.key_id, Some(Key::from_bytes([7; 32]).id()));
}
async fn native_client_status(f: &RuntimeFixture) -> devbox_protocol::secrets::KeyStatus {
    let client = native(f).await;
    serde_json::from_str(
        &client
            .raw_call("secrets.key_status", devbox_secrets::Empty {}, None)
            .await
            .expect("key status"),
    )
    .expect("typed key status")
}

#[tokio::test]
#[ignore = "needs owned systemd user fixture"]
async fn missing_secret_blocks_the_run_with_names() {
    let mut f = RuntimeFixture::new();
    f.daemon.stop();
    let server = Server::start(&f).await;
    let _app = bootstrap(&f).await;
    let client = f.daemon.client().await;
    let project = f.project(&client).await;
    let marker = f.daemon.home.path().join("must-not-run");
    let mut def = secret_service(
        "missing",
        &format!(
            "touch {}",
            devbox_core::sh_quote(&marker.display().to_string())
        ),
    );
    if let RunDef::Service(d) = &mut def {
        d.secrets = vec!["ABSENT".into()];
        d.env.clear();
    }
    f.define(&client, &project, def, SaveTo::ThisPc).await;
    let before = f.units();
    let result = client
        .call::<RuntimeStart>(&StartRun {
            project_id: project.clone(),
            id: "missing".into(),
            worktree: None,
        })
        .await;
    match result {
        Err(devbox_cli::client::ClientError::Rpc {
            code,
            detail: Some(detail),
            ..
        }) => {
            assert_eq!(code, "runtime.secret_missing");
            assert_eq!(detail.get(), r#"{"names":["ABSENT"]}"#);
        }
        _ => panic!("expected missing names"),
    }
    assert!(!marker.exists());
    assert_eq!(before, f.units());
    assert!(f.runs(&client, &project).await.is_empty());
    server.stop().await;
}

#[path = "support/runtime_windows_wrapper.rs"]
mod windows_wrapper;

#[path = "../../../app/src-tauri/src/core/windows.rs"]
mod native_window_protocol;
mod core {
    pub mod windows {
        pub use crate::native_window_protocol::powershell_args;
    }
}
