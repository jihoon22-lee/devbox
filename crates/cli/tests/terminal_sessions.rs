mod support;
use devbox_protocol::system::PingParams;
use devbox_terminal::*;
use std::time::{Duration, Instant};
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
/// The socket file tmux leaves behind once the fixture's server is killed.
struct SocketFile(std::path::PathBuf);
impl SocketFile {
    fn of(daemon: &TestDaemon) -> Self {
        use std::os::unix::fs::MetadataExt;
        let uid = std::fs::metadata(daemon.home.path())
            .expect("fixture owner")
            .uid();
        Self(format!("/tmp/tmux-{uid}/devbox-{}", daemon.instance).into())
    }
}
impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
#[tokio::test]
async fn daemon_start_gives_a_running_tmux_server_extended_keys() {
    let daemon = TestDaemon::fixture();
    daemon.paths.ensure_dirs().expect("fixture directories");
    // Declared before the fixture, so it is dropped once the fixture has killed the server.
    let _socket = SocketFile::of(&daemon);
    let tmux = tmux::Tmux::for_paths(&daemon.paths);
    let mut fixture = Fixture(daemon);
    // A server that outlived an update was started from a config without the options.
    let before: String = (TMUX_CONF.lines())
        .filter(|line| !line.contains("extended-keys"))
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(tmux.conf(), before).expect("config before the update");
    tmux.start_direct().await.expect("start the old server");
    std::fs::write(tmux.conf(), TMUX_CONF).expect("config after the update");
    let shown = || async {
        let shown = tmux.run(&["show", "-s", "extended-keys"]).await;
        shown.expect("show extended-keys").trim().to_owned()
    };
    assert_eq!(shown().await, "extended-keys off");
    fixture.0.launch();
    let deadline = Instant::now() + Duration::from_secs(5);
    while shown().await != "extended-keys on" {
        assert!(
            Instant::now() < deadline,
            "daemon start applied the options"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
#[tokio::test]
async fn terminal_rpc_metadata_and_tmux_survive_daemon_restart() {
    let mut fixture = Fixture::start();
    let client = fixture.0.client().await;
    let profiles = bounded(client.call::<Profiles>(&PingParams {}))
        .await
        .expect("profiles");
    assert_eq!(
        profiles
            .items
            .iter()
            .map(|p| p.command.as_str())
            .collect::<Vec<_>>(),
        ["bash -l", "nu"]
    );
    let mut events = bounded(client.subscribe("terminal.changed"))
        .await
        .expect("terminal subscription");
    let terminal = bounded(client.call::<TerminalCreate>(&CreateSession {
        project_id: None,
        cwd: Some(fixture.0.home.path().display().to_string()),
        title: Some("session".into()),
        command: Some("bash --norc --noprofile".into()),
        attach_zellij: None,
    }))
    .await
    .expect("create terminal");
    assert_eq!(
        bounded(events.next_event())
            .await
            .expect("create event")
            .rev,
        "1"
    );
    fixture.0.restart();
    let client = fixture.0.client().await;
    let list = bounded(client.call::<TerminalList>(&ListSessions {
        project_id: None,
        after_id: None,
    }))
    .await
    .expect("list after restart");
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].id, terminal.id);
    assert!(list.items[0].alive);
    bounded(client.call::<TerminalRename>(&RenameSession {
        id: terminal.id.clone(),
        title: "renamed".into(),
    }))
    .await
    .expect("rename terminal");
    let list = bounded(client.call::<TerminalList>(&ListSessions {
        project_id: None,
        after_id: None,
    }))
    .await
    .expect("renamed list");
    assert_eq!(list.items[0].title, "renamed");
    bounded(client.call::<TerminalClose>(&CloseSession { id: terminal.id }))
        .await
        .expect("close terminal");
    assert!(
        bounded(client.call::<TerminalList>(&ListSessions {
            project_id: None,
            after_id: None
        }))
        .await
        .expect("empty list")
        .items
        .is_empty()
    );
}
