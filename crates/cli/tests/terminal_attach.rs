mod support;
use devbox_terminal::*;
use std::time::Duration;
use support::{TestDaemon, bounded};
struct Fixture(TestDaemon);
impl Fixture {
    fn start() -> Self {
        let mut daemon = TestDaemon::fixture();
        daemon.paths.ensure_dirs().expect("fixture directories");
        std::fs::write(
            daemon.paths.instance_config_dir.join("tmux.conf"),
            TMUX_CONF,
        )
        .expect("fixture tmux config");
        std::fs::write(
            daemon.paths.config_dir.join("config.toml"),
            "[[terminal.profiles]]\nname='Nu'\ncommand='nu'\n",
        )
        .expect("fixture profiles");
        daemon.launch();
        Self(daemon)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.0.stop();
        let _ = std::process::Command::new("tmux")
            .args(["-L", &format!("devbox-{}", self.0.instance), "kill-server"])
            .env_remove("TMUX")
            .env_remove("TMUX_TMPDIR")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}
#[tokio::test]
async fn attach_duplex_and_history() {
    let fixture = Fixture::start();
    let client = fixture.0.client().await;
    let terminal = bounded(client.call::<TerminalCreate>(&CreateSession {
        project_id: None,
        cwd: Some(fixture.0.home.path().display().to_string()),
        title: None,
        command: Some("bash --norc --noprofile".into()),
        attach_zellij: None,
    }))
    .await
    .expect("create");
    let stream = bounded(client.open_stream(
        "terminal.attach",
        serde_json::json!({
            "id":terminal.id,"cols":100,"rows":30,"readOnly":false
        }),
    ))
    .await
    .expect("attach");
    stream
        .write(b"for i in $(seq 1 80); do printf 'row-%s\\n' $i; done\r")
        .await
        .expect("input");
    let out = stream.read_until(b"row-80", Duration::from_secs(1)).await;
    assert!(String::from_utf8_lossy(&out).contains("row-80"));
    stream.end().await.expect("close");
    let again = bounded(client.open_stream(
        "terminal.attach",
        serde_json::json!({
            "id":terminal.id,"cols":20,"rows":10,"readOnly":true
        }),
    ))
    .await
    .expect("reattach");
    let out = again.read_available(Duration::from_millis(500)).await;
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("row-1\r\n"), "history: {text:?}");
    let prefill = text
        .split('\u{1b}')
        .next()
        .expect("plain fixture history prefix");
    assert!(
        !prefill.contains("row-80"),
        "visible screen must not be included in history: {prefill:?}"
    );
    assert!(
        text.contains("row-80"),
        "PTY must redraw the current screen"
    );
}
impl Fixture {
    async fn create(&self, client: &devbox_cli::client::Client) -> String {
        bounded(client.call::<TerminalCreate>(&CreateSession {
            project_id: None,
            cwd: Some(self.0.home.path().display().to_string()),
            title: None,
            command: Some("bash --norc --noprofile".into()),
            attach_zellij: None,
        }))
        .await
        .expect("create")
        .id
    }
    async fn tmux(&self, args: &[&str]) -> String {
        let output = bounded(
            tokio::process::Command::new("tmux")
                .args(["-L", &format!("devbox-{}", self.0.instance)])
                .args(args)
                .env_remove("TMUX")
                .env_remove("TMUX_TMPDIR")
                .output(),
        )
        .await
        .expect("tmux");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
    async fn attach(
        &self,
        client: &devbox_cli::client::Client,
        id: &str,
        read_only: bool,
    ) -> devbox_cli::client::ClientStream {
        bounded(client.open_stream(
            "terminal.attach",
            serde_json::json!({"id":id,"cols":100,"rows":30,"readOnly":read_only}),
        ))
        .await
        .expect("attach")
    }
    async fn no_clients(&self, id: &str) {
        bounded(async {
            loop {
                if self
                    .tmux(&["list-clients", "-t", id, "-F", "#{client_pid}"])
                    .await
                    .trim()
                    .is_empty()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
    }
}
#[tokio::test]
async fn read_only_ignores_input_size_and_resize_is_connection_owned() {
    let fixture = Fixture::start();
    let client = fixture.0.client().await;
    let id = fixture.create(&client).await;
    let owner = fixture.attach(&client, &id, false).await;
    owner.read_available(Duration::from_millis(150)).await;
    let before = fixture
        .tmux(&[
            "list-panes",
            "-t",
            &id,
            "-F",
            "#{pane_width} #{pane_height}",
        ])
        .await;
    let reader = bounded(client.open_stream(
        "terminal.attach",
        serde_json::json!({"id":id,"cols":5,"rows":5,"readOnly":true}),
    ))
    .await
    .expect("reader");
    reader
        .write(b"touch readonly-sentinel\r")
        .await
        .expect("ignored input");
    bounded(client.call::<TerminalResize>(&ResizeStream {
        stream_id: reader.id,
        cols: 7,
        rows: 8,
    }))
    .await
    .expect("ignored resize");
    reader.read_available(Duration::from_millis(150)).await;
    assert!(!fixture.0.home.path().join("readonly-sentinel").exists());
    assert_eq!(
        before,
        fixture
            .tmux(&[
                "list-panes",
                "-t",
                &id,
                "-F",
                "#{pane_width} #{pane_height}"
            ])
            .await
    );
    let other = fixture.0.client().await;
    let error = bounded(other.call::<TerminalResize>(&ResizeStream {
        stream_id: owner.id,
        cols: 50,
        rows: 20,
    }))
    .await
    .expect_err("foreign owner");
    assert!(error.to_string().contains("terminal.not_found"));
    bounded(client.call::<TerminalResize>(&ResizeStream {
        stream_id: owner.id,
        cols: 90,
        rows: 25,
    }))
    .await
    .expect("owner resize");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        fixture
            .tmux(&[
                "list-panes",
                "-t",
                &id,
                "-F",
                "#{pane_width} #{pane_height}"
            ])
            .await
            .trim(),
        "90 25"
    );
}
#[tokio::test]
async fn half_close_retains_output_and_drop_reaps_only_attach_client() {
    let fixture = Fixture::start();
    let client = fixture.0.client().await;
    let id = fixture.create(&client).await;
    let stream = fixture.attach(&client, &id, false).await;
    stream.read_available(Duration::from_millis(100)).await;
    let pid: u32 = fixture
        .tmux(&["list-clients", "-t", &id, "-F", "#{client_pid}"])
        .await
        .trim()
        .parse()
        .expect("attach pid");
    stream
        .write(b"sleep 0.2; printf 'halfclose-%s\\n' output\r")
        .await
        .expect("input");
    stream.finish_input().await.expect("half-close");
    assert!(
        String::from_utf8_lossy(&stream.read_available(Duration::from_millis(600)).await)
            .contains("halfclose-output")
    );
    drop(stream);
    fixture.no_clients(&id).await;
    bounded(async {
        while std::path::Path::new(&format!("/proc/{pid}")).exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        fixture
            .tmux(&["list-sessions", "-F", "#{session_name}"])
            .await
            .contains(&id)
    );
    let stream = fixture.attach(&client, &id, false).await;
    stream
        .write(b"yes credit-stall\r")
        .await
        .expect("large output");
    tokio::time::sleep(Duration::from_millis(500)).await;
    drop(stream);
    fixture.no_clients(&id).await;
    assert!(
        fixture
            .tmux(&["list-sessions", "-F", "#{session_name}"])
            .await
            .contains(&id)
    );
}
#[tokio::test]
async fn invalid_size_and_unowned_tmux_session_are_typed_errors() {
    let fixture = Fixture::start();
    let client = fixture.0.client().await;
    let id = fixture.create(&client).await;
    for (cols, rows) in [(0, 20), (80, 0)] {
        let error = bounded(client.raw_call(
            "terminal.attach",
            serde_json::json!({"id":id,"cols":cols,"rows":rows,"readOnly":false}),
            None,
        ))
        .await
        .expect_err("size");
        assert!(error.to_string().contains("terminal.invalid_size"));
    }
    fixture
        .tmux(&["new-session", "-d", "-s", "foreign", "sleep 30"])
        .await;
    let error = bounded(client.raw_call(
        "terminal.attach",
        serde_json::json!({"id":"foreign","cols":80,"rows":24,"readOnly":false}),
        None,
    ))
    .await
    .expect_err("metadata ownership");
    assert!(error.to_string().contains("terminal.not_found"));
}
