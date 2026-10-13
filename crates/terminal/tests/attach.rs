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
