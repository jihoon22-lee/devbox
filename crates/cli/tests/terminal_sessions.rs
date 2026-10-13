mod support;
use devbox_protocol::system::PingParams;
use devbox_terminal::*;
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
    let list = bounded(client.call::<TerminalList>(&ListSessions { project_id: None }))
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
    let list = bounded(client.call::<TerminalList>(&ListSessions { project_id: None }))
        .await
        .expect("renamed list");
    assert_eq!(list.items[0].title, "renamed");
    bounded(client.call::<TerminalClose>(&CloseSession { id: terminal.id }))
        .await
        .expect("close terminal");
    assert!(
        bounded(client.call::<TerminalList>(&ListSessions { project_id: None }))
            .await
            .expect("empty list")
            .items
            .is_empty()
    );
}
