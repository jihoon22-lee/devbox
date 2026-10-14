mod support;
use devbox_protocol::stream::{
    Credit, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx, parse_stream,
};
use devbox_terminal::*;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct Output(Mutex<Vec<u8>>);
impl StreamSink for Output {
    fn push(
        &self,
        frame: devbox_protocol::frame::Frame,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            let (_, _, bytes) = parse_stream(&frame.body).expect("stream frame");
            self.0.lock().expect("output").extend_from_slice(bytes);
            true
        })
    }
}
struct GatedOpener {
    ends: Mutex<Option<StreamEnds>>,
    socket: String,
    id: String,
    gate: PathBuf,
}
impl StreamOpener for GatedOpener {
    fn open(&self) -> Result<StreamEnds, StreamError> {
        // On the old implementation this is exactly after capture_history and
        // before spawning the client. Complete a burst larger than the viewport.
        std::fs::write(&self.gate, "release").expect("release producer");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let output = std::process::Command::new("tmux")
                .args(["-L", &self.socket, "capture-pane", "-p", "-t", &self.id])
                .env_remove("TMUX")
                .env_remove("TMUX_TMPDIR")
                .output()
                .expect("poll owned pane");
            assert!(output.status.success(), "poll owned pane failed");
            if String::from_utf8_lossy(&output.stdout).contains("burst-00300") {
                break;
            }
            assert!(Instant::now() < deadline, "producer did not complete burst");
            std::thread::sleep(Duration::from_millis(5));
        }
        self.ends
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
async fn continuity(read_only: bool, cols: u16, rows: u16, suffix: &str) {
    let h = support::Harness::new();
    let script = h.home.path().join("producer.sh");
    let gate = h.home.path().join("release");
    std::fs::write(&script, format!("for ((i=1;i<=2100;i++)); do printf 'seed-%05d\\n' \"$i\"; done\ntouch seeded\nwhile [ ! -f release ]; do sleep 0.005; done\nfor ((i=1;i<=300;i++)); do printf 'burst-%05d{suffix}\\n' \"$i\"; done\nexec sleep 60\n")).expect("producer");
    let session = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some(format!(
                "bash {}",
                devbox_core::sh_quote(&script.display().to_string())
            )),
            attach_zellij: None,
        })
        .await
        .expect("create");
    until(async || h.home.path().join("seeded").exists()).await;
    until(async || {
        h.terminal
            .tmux()
            .capture_tail(&session.id, 1)
            .await
            .expect("seed screen")
            .contains("seed-02100")
    })
    .await;
    h.terminal
        .tmux()
        .run(&[
            "set-buffer",
            "-b",
            "fixture-preserved",
            "owned fixture data",
        ])
        .await
        .expect("unrelated buffer");
    let policy = h
        .terminal
        .tmux()
        .run(&["show-options", "-wv", "-t", &session.id, "window-size"])
        .await
        .expect("sizing policy");
    let output = Arc::new(Output::default());
    let closed = CancellationToken::new();
    let (_input, input) = tokio::sync::mpsc::channel(4);
    let ctx = devbox_protocol::Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(GatedOpener {
            ends: Mutex::new(Some(StreamEnds {
                id: 1,
                tx: StreamTx::new(
                    1,
                    0,
                    Arc::new(Credit::new(256 * 1024)),
                    output.clone(),
                    closed.clone(),
                ),
                input,
            })),
            socket: h.terminal.tmux().socket().into(),
            id: session.id.clone(),
            gate,
        })),
    };
    let attached = h
        .terminal
        .attach(
            ctx,
            AttachSession {
                id: session.id.clone(),
                cols,
                rows,
                read_only,
            },
        )
        .await
        .expect("attach");
    until(async || {
        String::from_utf8_lossy(&output.0.lock().expect("output")).contains("burst-00300")
    })
    .await;
    assert_eq!(
        policy,
        h.terminal
            .tmux()
            .run(&["show-options", "-wv", "-t", &session.id, "window-size"])
            .await
            .expect("preserved sizing policy")
    );
    assert_eq!(
        h.terminal
            .tmux()
            .run(&["list-buffers", "-F", "#{buffer_name}"])
            .await
            .expect("owned buffer cleanup")
            .trim(),
        "fixture-preserved"
    );
    closed.cancel();
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-clients", "-t", &session.id])
            .await
            .expect("clients")
            .is_empty()
    })
    .await;
    let bytes = output.0.lock().expect("output").clone();
    if let Some(directory) = std::env::var_os("DEVBOX_TERMINAL_STREAM_ARTIFACT_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).expect("artifact directory");
        let label = if read_only {
            "readonly"
        } else if suffix.is_empty() {
            "writable"
        } else {
            "reflow"
        };
        std::fs::write(
            directory.join(format!("terminal-stream-{label}.bin")),
            &bytes,
        )
        .expect("stream artifact");
        std::fs::write(
            directory.join(format!("terminal-stream-{label}.json")),
            format!(
                "{{\"cols\":{},\"rows\":{}}}\n",
                attached.cols, attached.rows
            ),
        )
        .expect("stream dimensions artifact");
    }
    let text = String::from_utf8_lossy(&bytes);
    let (prefill, live) = text.split_once('\u{1b}').expect("history then PTY redraw");
    let mut previous = 0;
    for i in 1..=300 {
        let line = format!("burst-{i:05}");
        let positions: Vec<_> = prefill
            .match_indices(&line)
            .map(|(position, _)| position)
            .collect();
        // tmux may redraw a visible row multiple times on the wire. A row must
        // belong to exactly one side of the history/initial-screen boundary.
        assert_eq!(
            positions.len() + usize::from(live.contains(&line)),
            1,
            "{line} missing or duplicated across history/live: {positions:?}"
        );
        let position = text.find(&line).expect("numbered row");
        assert!(position >= previous, "burst chronology changed at {line}");
        previous = position;
    }
    if suffix.is_empty() {
        assert_eq!(
            prefill.lines().filter(|line| !line.is_empty()).count(),
            2000,
            "last 2000 history rows only; viewport padding is blank"
        );
    } else {
        assert!(
            prefill.lines().filter(|line| !line.is_empty()).count() < 2000,
            "wrapped rows must be joined"
        );
    }
    assert!(
        !prefill.contains("burst-00300"),
        "visible screen duplicated in history"
    );
    assert!(!prefill.contains("seed-00001"), "history cap ignored");
}
#[tokio::test]
async fn gated_busy_reattach_keeps_history_and_initial_redraw_contiguous() {
    continuity(true, 5, 5, "").await;
}
#[tokio::test]
async fn gated_busy_writable_attach_keeps_lines_moved_by_initial_resize() {
    continuity(false, 100, 20, "").await;
}
#[tokio::test]
async fn writable_width_reflow_keeps_numbered_rows_across_history_and_redraw() {
    continuity(false, 30, 20, &"x".repeat(80)).await;
}

struct PlainOpener(Mutex<Option<StreamEnds>>);
impl StreamOpener for PlainOpener {
    fn open(&self) -> Result<StreamEnds, StreamError> {
        self.0
            .lock()
            .expect("opener")
            .take()
            .ok_or(StreamError::Closed)
    }
}
#[tokio::test]
async fn empty_history_does_not_prefill_the_first_visible_row() {
    let h = support::Harness::new();
    let session = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some("printf 'visible-only-marker'; exec sleep 60".into()),
            attach_zellij: None,
        })
        .await
        .expect("create");
    until(async || {
        h.terminal
            .tmux()
            .capture_tail(&session.id, 1)
            .await
            .expect("screen")
            .contains("visible-only-marker")
    })
    .await;
    let output = Arc::new(Output::default());
    let closed = CancellationToken::new();
    let (_input, input) = tokio::sync::mpsc::channel(4);
    h.terminal
        .attach(
            devbox_protocol::Ctx {
                channel: 0,
                cancel: CancellationToken::new(),
                streams: Some(Arc::new(PlainOpener(Mutex::new(Some(StreamEnds {
                    id: 1,
                    tx: StreamTx::new(
                        1,
                        0,
                        Arc::new(Credit::new(256 * 1024)),
                        output.clone(),
                        closed.clone(),
                    ),
                    input,
                }))))),
            },
            AttachSession {
                id: session.id.clone(),
                cols: 5,
                rows: 5,
                read_only: true,
            },
        )
        .await
        .expect("attach");
    until(async || {
        String::from_utf8_lossy(&output.0.lock().expect("output")).contains("visible-only-marker")
    })
    .await;
    let bytes = output.0.lock().expect("output").clone();
    assert_eq!(
        bytes.first(),
        Some(&0x1b),
        "empty history must begin with PTY redraw"
    );
    closed.cancel();
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-clients", "-t", &session.id])
            .await
            .expect("clients")
            .is_empty()
    })
    .await;
    assert!(
        h.terminal
            .tmux()
            .run(&["list-buffers"])
            .await
            .expect("buffer cleanup")
            .is_empty()
    );
}

#[tokio::test]
async fn rejected_initial_grid_reaps_the_preparation_client() {
    let h = support::Harness::new();
    let session = h
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
    let closed = CancellationToken::new();
    let (_input, input) = tokio::sync::mpsc::channel(4);
    let error = h
        .terminal
        .attach(
            devbox_protocol::Ctx {
                channel: 0,
                cancel: CancellationToken::new(),
                streams: Some(Arc::new(PlainOpener(Mutex::new(Some(StreamEnds {
                    id: 1,
                    tx: StreamTx::new(
                        1,
                        0,
                        Arc::new(Credit::new(0)),
                        Arc::new(Output::default()),
                        closed.clone(),
                    ),
                    input,
                }))))),
            },
            AttachSession {
                id: session.id.clone(),
                cols: u16::MAX,
                rows: u16::MAX,
                read_only: false,
            },
        )
        .await
        .expect_err("tmux rejects oversized control-client grid");
    assert!(matches!(error, devbox_protocol::Fail::Internal(_)));
    assert!(
        closed.is_cancelled(),
        "failed initialization closes the stream"
    );
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-clients", "-t", &session.id])
            .await
            .expect("clients")
            .is_empty()
    })
    .await;
    assert!(
        h.terminal
            .tmux()
            .run(&["list-buffers"])
            .await
            .expect("buffers")
            .is_empty()
    );
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("live session")
            .iter()
            .any(|item| item.name == session.id)
    );
}

#[tokio::test]
async fn abort_during_grid_preparation_closes_stream_and_reaps_only_owned_client() {
    let h = Arc::new(support::Harness::new());
    let session = h
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
    // Block only this owned helper command queue after its resize; pane I/O and
    // the dedicated fixture server continue normally.
    h.terminal
        .tmux()
        .run(&[
            "set-hook",
            "-t",
            &session.id,
            "after-refresh-client",
            "wait-for fixture-grid-gate",
        ])
        .await
        .expect("owned queue gate");
    let closed = CancellationToken::new();
    let (_input, input) = tokio::sync::mpsc::channel(4);
    let ctx = devbox_protocol::Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(PlainOpener(Mutex::new(Some(StreamEnds {
            id: 1,
            tx: StreamTx::new(
                1,
                0,
                Arc::new(Credit::new(0)),
                Arc::new(Output::default()),
                closed.clone(),
            ),
            input,
        }))))),
    };
    let attached = h.clone();
    let id = session.id.clone();
    let request = tokio::spawn(async move {
        attached
            .terminal
            .attach(
                ctx,
                AttachSession {
                    id,
                    cols: 100,
                    rows: 20,
                    read_only: false,
                },
            )
            .await
    });
    let mut pid = String::new();
    until(async || {
        pid = h
            .terminal
            .tmux()
            .run(&["list-clients", "-t", &session.id, "-F", "#{client_pid}"])
            .await
            .expect("owned helper");
        !pid.trim().is_empty()
    })
    .await;
    assert!(
        !request.is_finished(),
        "fixture must abort during preparation"
    );
    request.abort();
    assert!(request.await.expect_err("aborted attach").is_cancelled());
    assert!(
        closed.is_cancelled(),
        "aborted initialization must release stream registry"
    );
    until(async || !std::path::Path::new(&format!("/proc/{}", pid.trim())).exists()).await;
    assert!(
        h.terminal
            .tmux()
            .run(&["list-buffers"])
            .await
            .expect("buffers")
            .is_empty()
    );
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("live session")
            .iter()
            .any(|item| item.name == session.id)
    );
}
