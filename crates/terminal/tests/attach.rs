mod support;
use devbox_protocol::stream::{
    Credit, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx,
};
use devbox_terminal::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
struct Sink;
impl StreamSink for Sink {
    fn push(
        &self,
        _: devbox_protocol::frame::Frame,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(async { true })
    }
}
struct Opener(Mutex<Option<StreamEnds>>);
impl StreamOpener for Opener {
    fn open(&self) -> Result<StreamEnds, StreamError> {
        self.0
            .lock()
            .expect("opener")
            .take()
            .ok_or(StreamError::Closed)
    }
}
async fn until(mut condition: impl AsyncFnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture deadline");
}
#[tokio::test]
async fn zero_credit_history_does_not_starve_input_or_attachment_cleanup() {
    let h = support::Harness::new();
    let terminal = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some("bash --norc --noprofile".into()),
            attach_zellij: None,
        })
        .await
        .expect("create");
    h.terminal
        .tmux()
        .run(&[
            "send-keys",
            "-t",
            &terminal.id,
            "for i in $(seq 1 80); do echo row-$i; done",
            "Enter",
        ])
        .await
        .expect("history");
    until(async || {
        !h.terminal
            .tmux()
            .capture_history(&terminal.id, 2000)
            .await
            .expect("capture")
            .is_empty()
    })
    .await;
    let closed = CancellationToken::new();
    let (input, rx) = tokio::sync::mpsc::channel(4);
    let tx = StreamTx::new(
        1,
        0,
        Arc::new(Credit::new(0)),
        Arc::new(Sink),
        closed.clone(),
    );
    let ctx = devbox_protocol::Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(Opener(Mutex::new(Some(StreamEnds {
            id: 1,
            tx,
            input: rx,
        }))))),
    };
    let attached = h
        .terminal
        .attach(
            ctx,
            AttachSession {
                id: terminal.id.clone(),
                cols: 100,
                rows: 30,
                read_only: false,
            },
        )
        .await
        .expect("attach");
    assert_eq!((attached.cols, attached.rows), (100, 30));
    // The app sends Shift+Enter as an extended key only when this instance's server is 3.5+.
    let version = std::process::Command::new("tmux")
        .args(["-L", h.terminal.tmux().socket()])
        .args(["display-message", "-p", "#{version}"])
        .env_remove("TMUX")
        .env_remove("TMUX_TMPDIR")
        .output()
        .expect("server version");
    let version = String::from_utf8_lossy(&version.stdout);
    assert_eq!(
        attached.extended_keys,
        tmux::supports_extended_keys(&version),
        "{version}"
    );
    until(async || {
        let clients = h
            .terminal
            .tmux()
            .run(&[
                "list-clients",
                "-t",
                &terminal.id,
                "-F",
                "#{client_pid} #{client_height}",
            ])
            .await
            .expect("clients");
        clients.lines().count() == 1 && clients.split_whitespace().count() == 2
    })
    .await;
    let pid = h
        .terminal
        .tmux()
        .run(&["list-clients", "-t", &terminal.id, "-F", "#{client_pid}"])
        .await
        .expect("pid");
    input
        .send(b"touch credit-stalled-input\r".to_vec().into())
        .await
        .expect("input");
    until(async || h.home.path().join("credit-stalled-input").exists()).await;
    closed.cancel();
    until(async || !std::path::Path::new(&format!("/proc/{}", pid.trim())).exists()).await;
    assert!(
        h.terminal
            .tmux()
            .run(&["list-buffers", "-F", "#{buffer_name}"])
            .await
            .expect("buffers")
            .is_empty()
    );
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("live sessions")
            .iter()
            .any(|s| s.name == terminal.id)
    );
}
#[tokio::test]
async fn cancelled_request_and_missing_stream_support_do_not_spawn_attach_clients() {
    let h = support::Harness::new();
    let terminal = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some("bash --norc --noprofile".into()),
            attach_zellij: None,
        })
        .await
        .expect("create");
    for cancelled in [false, true] {
        let cancel = CancellationToken::new();
        if cancelled {
            cancel.cancel();
        }
        let error = h
            .terminal
            .attach(
                devbox_protocol::Ctx {
                    channel: 0,
                    cancel,
                    streams: None,
                },
                AttachSession {
                    id: terminal.id.clone(),
                    cols: 80,
                    rows: 24,
                    read_only: true,
                },
            )
            .await
            .expect_err("unavailable");
        assert!(matches!(
            error,
            devbox_protocol::Fail::Domain(TerminalError::StreamUnavailable {})
        ));
        assert!(
            h.terminal
                .tmux()
                .run(&["list-clients", "-t", &terminal.id])
                .await
                .expect("clients")
                .is_empty()
        );
    }
}

#[tokio::test]
async fn read_only_attach_reports_the_actual_grid_without_resizing_it() {
    let h = support::Harness::new();
    let terminal = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some("bash --norc --noprofile".into()),
            attach_zellij: None,
        })
        .await
        .expect("create");
    let pane_size = || async {
        h.terminal
            .tmux()
            .run(&[
                "list-panes",
                "-t",
                &terminal.id,
                "-F",
                "#{pane_width} #{pane_height}",
            ])
            .await
            .expect("grid")
    };
    let before = pane_size().await;
    let actual: Vec<u16> = before
        .split_whitespace()
        .map(|v| v.parse().expect("dimension"))
        .collect();
    let closed = CancellationToken::new();
    let (_input, input) = tokio::sync::mpsc::channel(4);
    let ctx = devbox_protocol::Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(Opener(Mutex::new(Some(StreamEnds {
            id: 1,
            tx: StreamTx::new(
                1,
                0,
                Arc::new(Credit::new(0)),
                Arc::new(Sink),
                closed.clone(),
            ),
            input,
        }))))),
    };
    let attached = h
        .terminal
        .attach(
            ctx,
            AttachSession {
                id: terminal.id.clone(),
                cols: 5,
                rows: 5,
                read_only: true,
            },
        )
        .await
        .expect("read-only attach");
    assert_eq!((attached.cols, attached.rows), (actual[0], actual[1]));
    until(async || {
        !h.terminal
            .tmux()
            .run(&["list-clients", "-t", &terminal.id])
            .await
            .expect("clients")
            .is_empty()
    })
    .await;
    assert_eq!(before, pane_size().await);
    closed.cancel();
}

/// Every attach asks the instance's running server for its version. `tmux -V` only names the
/// binary on PATH, and the server can be older: it outlives a tmux upgrade or a PATH change.
#[cfg(unix)]
#[tokio::test]
async fn extended_keys_come_from_the_running_server_on_every_attach() {
    use std::os::unix::fs::PermissionsExt;
    const CHILD: &str = "DEVBOX_TEST_TERMINAL_SERVER_VERSION";
    if let Some(dir) = std::env::var_os(CHILD) {
        server_version_child(dir.into()).await;
        return;
    }
    // PATH changes only in an owned child, never the test runner. Its wrapper prints
    // `client-version` for `tmux -V` and `server-version` for the `#{version}` format, where
    // `fail` fails the query. Everything else runs the installed tmux.
    let real = std::process::Command::new("sh")
        .args(["-c", "command -v tmux"])
        .output()
        .expect("locate installed tmux");
    assert!(real.status.success());
    let real = String::from_utf8(real.stdout).expect("tmux executable path");
    let fixture = tempfile::tempdir().expect("owned wrapper directory");
    let wrapper = fixture.path().join("tmux");
    std::fs::write(
        &wrapper,
        format!(
            r#"#!/bin/sh
for arg in "$@"; do
    case "$arg" in
        -V) exec cat "$DEVBOX_TEST_TERMINAL_SERVER_VERSION/client-version" ;;
        '#{{version}}')
            read -r version < "$DEVBOX_TEST_TERMINAL_SERVER_VERSION/server-version"
            if [ "$version" = fail ]; then echo 'fixture: no version' >&2; exit 1; fi
            echo "$version"
            exit 0 ;;
    esac
done
exec {real} "$@"
"#,
            real = devbox_core::sh_quote(real.trim())
        ),
    )
    .expect("wrapper script");
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))
        .expect("owned executable");
    let path = std::env::join_paths(std::iter::once(fixture.path().to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("runner PATH")),
    ))
    .expect("child-only PATH");
    let mut child = tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    child
        .args([
            "--exact",
            "extended_keys_come_from_the_running_server_on_every_attach",
            "--nocapture",
        ])
        .env(CHILD, fixture.path())
        .env("PATH", path)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(20), child.output())
        .await
        .expect("isolated attach fixture deadline")
        .expect("fixture subprocess");
    assert!(
        output.status.success(),
        "isolated attach fixture: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
/// The socket file tmux leaves behind. Declared before the harness, so it is removed after the
/// harness has killed the server, on failure too.
struct SocketFile(Option<String>);
impl Drop for SocketFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}
#[cfg(unix)]
async fn server_version_child(dir: std::path::PathBuf) {
    let mut socket_file = SocketFile(None);
    let h = support::Harness::new();
    let terminal = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some("exec sleep 60".into()),
            attach_zellij: None,
        })
        .await
        .expect("create");
    let path = h
        .terminal
        .tmux()
        .run(&["display-message", "-p", "#{socket_path}"])
        .await
        .expect("socket path");
    let path = path.trim();
    if path.ends_with(h.terminal.tmux().socket()) {
        socket_file.0 = Some(path.to_owned());
    }
    for (client, server, expected) in [
        // A 3.4 server that outlived an upgrade of the binary to 3.6.
        ("tmux 3.6", "3.4", false),
        // A failed question is false, and is not kept: the next attach asks again.
        ("tmux 3.6", "fail", false),
        ("tmux 3.4", "3.6", true),
    ] {
        std::fs::write(dir.join("client-version"), format!("{client}\n")).expect("version");
        std::fs::write(dir.join("server-version"), format!("{server}\n")).expect("version");
        let closed = CancellationToken::new();
        let (_input, input) = tokio::sync::mpsc::channel(4);
        let ctx = devbox_protocol::Ctx {
            channel: 0,
            cancel: CancellationToken::new(),
            streams: Some(Arc::new(Opener(Mutex::new(Some(StreamEnds {
                id: 1,
                tx: StreamTx::new(
                    1,
                    0,
                    Arc::new(Credit::new(0)),
                    Arc::new(Sink),
                    closed.clone(),
                ),
                input,
            }))))),
        };
        let attached = h
            .terminal
            .attach(
                ctx,
                AttachSession {
                    id: terminal.id.clone(),
                    cols: 80,
                    rows: 24,
                    read_only: true,
                },
            )
            .await
            .expect("attach");
        assert_eq!(
            attached.extended_keys, expected,
            "tmux -V {client:?}, server {server:?}"
        );
        closed.cancel();
    }
}
