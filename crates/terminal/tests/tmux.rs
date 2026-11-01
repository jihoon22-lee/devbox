use devbox_terminal::tmux::{Tmux, TmuxError, supports_extended_keys};
use std::time::Duration;
struct Fixture {
    home: tempfile::TempDir,
    tmux: Tmux,
}
impl Fixture {
    fn new() -> Self {
        Self::with_conf(devbox_terminal::TMUX_CONF)
    }
    fn with_conf(config: &str) -> Self {
        let home = tempfile::tempdir().expect("fixture home");
        let conf = home.path().join("tmux.conf");
        std::fs::write(&conf, config).expect("fixture config");
        let socket = format!("devbox-test-{}", uuid::Uuid::new_v4().simple());
        Self {
            home,
            tmux: Tmux::with(socket, conf),
        }
    }
    async fn extended_keys(&self) -> String {
        let shown = self.tmux.run(&["show", "-s", "extended-keys"]).await;
        shown.expect("show extended-keys").trim().to_owned()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let socket = std::process::Command::new("tmux")
            .args([
                "-L",
                self.tmux.socket(),
                "display-message",
                "-p",
                "#{socket_path}",
            ])
            .env_remove("TMUX_TMPDIR")
            .env_remove("TMUX")
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|path| path.trim().to_owned());
        let _ = std::process::Command::new("tmux")
            .args(["-L", self.tmux.socket(), "kill-server"])
            .env_remove("TMUX_TMPDIR")
            .env_remove("TMUX")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        // tmux leaves the socket file of a stopped server behind; this fixture created it.
        if let Some(socket) = socket.filter(|path| path.ends_with(self.tmux.socket())) {
            let _ = std::fs::remove_file(socket);
        }
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
#[test]
fn tmux_conf_enables_extended_keys_quietly() {
    let conf = devbox_terminal::TMUX_CONF;
    let lines: Vec<&str> = conf
        .lines()
        .filter(|l| l.contains("extended-keys"))
        .collect();
    // `-q` lets tmux 3.2-3.4, which lack the format option, skip it without an error.
    assert_eq!(
        lines,
        [
            "set -s extended-keys on",
            "set -sq extended-keys-format csi-u"
        ]
    );
    // `always` would hand extended keys to programs that never asked for them.
    assert!(!conf.contains("always"));
}
#[tokio::test]
async fn server_started_from_the_shipped_conf_has_extended_keys_on() {
    let fixture = Fixture::new();
    fixture
        .tmux
        .start_direct()
        .await
        .expect("start fixture tmux");
    assert_eq!(fixture.extended_keys().await, "extended-keys on");
}
#[tokio::test]
async fn applying_extended_keys_reaches_a_server_started_without_them() {
    // A server that outlived an update was started from a config without the two lines
    // and never reads it again.
    let old: String = (devbox_terminal::TMUX_CONF.lines())
        .filter(|line| !line.contains("extended-keys"))
        .map(|line| format!("{line}\n"))
        .collect();
    let fixture = Fixture::with_conf(&old);
    fixture
        .tmux
        .start_direct()
        .await
        .expect("start fixture tmux");
    assert_eq!(fixture.extended_keys().await, "extended-keys off");
    fixture
        .tmux
        .apply_extended_keys()
        .await
        .expect("apply to the running server");
    assert_eq!(fixture.extended_keys().await, "extended-keys on");
    // Every daemon start applies them again.
    fixture
        .tmux
        .apply_extended_keys()
        .await
        .expect("apply again");
    assert_eq!(fixture.extended_keys().await, "extended-keys on");
}
#[tokio::test]
async fn applying_extended_keys_without_a_server_starts_nothing() {
    let fixture = Fixture::new();
    // tmux reports `No such file or directory` only while its socket file does not exist.
    let socket_missing = || async {
        let probe = fixture.tmux.run(&["list-sessions"]).await;
        matches!(probe, Err(TmuxError::Failed { stderr, .. }) if stderr.contains("No such file or directory"))
    };
    assert!(socket_missing().await);
    fixture
        .tmux
        .apply_extended_keys()
        .await
        .expect("no server is not an error");
    assert!(socket_missing().await, "no server or socket was created");
}
#[test]
fn only_tmux_3_5_or_newer_carries_extended_keys() {
    // `tmux -V` ends its line with a newline.
    for version in [
        "tmux 3.5",
        "tmux 3.5a",
        "tmux 3.6",
        "tmux 3.6\n",
        "tmux next-3.6",
        "tmux 4.0",
        "tmux 10.1",
    ] {
        assert!(supports_extended_keys(version), "{version:?}");
    }
    // Unknown counts as older: Shift+Enter then stays a plain Enter.
    for version in [
        "tmux 3.4",
        "tmux 3.4\n",
        "tmux 3.3a",
        "tmux 3.2",
        "tmux next-3.4",
        "tmux master",
        "tmux 3.5-rc",
        "tmux openbsd-7.6",
        "garbage 9.9",
        "",
    ] {
        assert!(!supports_extended_keys(version), "{version:?}");
    }
}
/// The one place tmux decides what Shift+Enter becomes for a program that never asked for
/// extended keys. From 3.5, which is when the app sends Shift+Enter as an extended key, that
/// program gets Enter. Older tmux may drop the key, so there the app keeps it a plain Enter.
#[tokio::test]
async fn shift_enter_reaches_a_program_that_did_not_ask_for_extended_keys_as_enter() {
    let fixture = Fixture::new();
    let t = &fixture.tmux;
    t.start_direct().await.expect("start fixture tmux");
    // One hex line per byte the program reads, without line editing or echo.
    let dump = "sh -c 'stty -icanon -icrnl -echo min 1 time 0; echo ready; while :; do dd bs=1 count=1 2>/dev/null | od -An -tx1; done'";
    t.new_session("keys", fixture.home.path(), Some(dump), &[])
        .await
        .expect("byte dump session");
    let read = || async {
        let screen = t.capture_tail("keys", 50).await.expect("capture");
        let (_, after) = screen.split_once("ready")?;
        Some(
            after
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    };
    let until = async |done: &dyn Fn(&[String]) -> bool| {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match read().await {
                    Some(bytes) if done(&bytes) => return bytes,
                    _ => tokio::time::sleep(Duration::from_millis(20)).await,
                }
            }
        })
        .await
        .expect("byte dump deadline")
    };
    until(&|_| true).await;
    let sent = t.run(&["send-keys", "-t", "=keys:", "S-Enter"]).await;
    // `x` marks the end of whatever tmux wrote for Shift+Enter.
    t.run(&["send-keys", "-t", "=keys:", "-l", "x"])
        .await
        .expect("send marker");
    let bytes = until(&|bytes| bytes.iter().any(|byte| byte == "78")).await;
    let version = t.run(&["-V"]).await.expect("tmux -V");
    if t.extended_keys().await {
        assert_eq!(bytes, ["0d", "78"], "{version} {sent:?}");
    } else {
        assert!(
            bytes == ["78"] || bytes == ["0d", "78"],
            "{version} {sent:?}: {bytes:?}"
        );
    }
}
