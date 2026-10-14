use devbox_terminal::tmux::Tmux;
use std::time::Duration;
struct Fixture {
    home: tempfile::TempDir,
    tmux: Tmux,
}
impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().expect("fixture home");
        let conf = home.path().join("tmux.conf");
        std::fs::write(&conf, include_str!("../assets/tmux.conf")).expect("fixture config");
        let socket = format!("devbox-test-{}", uuid::Uuid::new_v4().simple());
        Self {
            home,
            tmux: Tmux::with(socket, conf),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", self.tmux.socket(), "kill-server"])
            .env_remove("TMUX_TMPDIR")
            .env_remove("TMUX")
            .status();
    }
}
#[tokio::test]
async fn creates_lists_captures_pastes_and_kills_only_owned_sessions() {
    let fixture = Fixture::new();
    let t = &fixture.tmux;
    t.start_direct().await.expect("start fixture tmux");
    t.new_session(
        "s1",
        fixture.home.path(),
        Some("bash --norc --noprofile"),
        &[("DEVBOX_X".into(), "one ' two".into())],
    )
    .await
    .expect("new session");
    assert!(
        t.capture_history("s1", 2000)
            .await
            .expect("empty history")
            .is_empty(),
        "visible screen must not be replayed as history"
    );
    t.paste(
        "s1",
        "for i in $(seq 1 120); do echo line-$i; done; printf 'value=%s\\n' \"$DEVBOX_X\"",
        true,
    )
    .await
    .expect("paste shell command");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if t.capture_tail("s1", 10)
                .await
                .expect("capture output")
                .contains("value=one ' two")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("shell output deadline");
    let sessions = t.list().await.expect("list sessions");
    let s = sessions
        .iter()
        .find(|s| s.name == "s1")
        .expect("owned session");
    assert_eq!(
        s.path,
        fixture
            .home
            .path()
            .canonicalize()
            .expect("cwd")
            .display()
            .to_string()
    );
    let history = String::from_utf8(t.capture_history("s1", 2000).await.expect("history"))
        .expect("UTF8 history");
    assert!(history.contains("line-1\r\n"), "{history}");
    t.new_session("s10", fixture.home.path(), Some("bash --norc"), &[])
        .await
        .expect("sibling session");
    t.kill_session("s1")
        .await
        .expect("close exact owned session");
    assert_eq!(
        t.list()
            .await
            .expect("sibling survives")
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["s10"]
    );
    t.kill_session("s10").await.expect("close sibling");
    assert!(t.list().await.expect("empty server").is_empty());
}
