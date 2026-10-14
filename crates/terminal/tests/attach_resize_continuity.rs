mod support;
use devbox_protocol::{
    Ctx,
    frame::Frame,
    stream::{Credit, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx, parse_stream},
};
use devbox_terminal::*;
use std::{
    io::{BufRead, BufReader},
    process::{Child, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct Output(Mutex<Vec<u8>>);
impl StreamSink for Output {
    fn push(
        &self,
        frame: Frame,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            self.0
                .lock()
                .expect("output")
                .extend_from_slice(parse_stream(&frame.body).expect("frame").2);
            true
        })
    }
}
struct Opener {
    id: u32,
    ends: Mutex<Option<StreamEnds>>,
    opened: Arc<AtomicBool>,
}
impl StreamOpener for Opener {
    fn owns(&self, id: u32) -> bool {
        id == self.id
    }
    fn open(&self) -> Result<StreamEnds, StreamError> {
        self.opened.store(true, Ordering::SeqCst);
        self.ends
            .lock()
            .expect("opener")
            .take()
            .ok_or(StreamError::Closed)
    }
}
fn context(
    id: u32,
    output: Arc<Output>,
    closed: CancellationToken,
    opened: Arc<AtomicBool>,
) -> Ctx {
    let (_input, input) = tokio::sync::mpsc::channel(4);
    Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(Opener {
            id,
            opened,
            ends: Mutex::new(Some(StreamEnds {
                id,
                tx: StreamTx::new(id, 0, Arc::new(Credit::new(256 * 1024)), output, closed),
                input,
            })),
        })),
    }
}
fn resize_context(id: u32) -> Ctx {
    Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(Opener {
            id,
            opened: Arc::new(AtomicBool::new(false)),
            ends: Mutex::new(None),
        })),
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
async fn seeded(h: &support::Harness) -> String {
    let session = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some(
                "for i in $(seq 1 300); do printf 'number-%05d\\n' $i; done; exec sleep 60".into(),
            ),
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
            .contains("number-00300")
    })
    .await;
    session.id
}
async fn gate(h: &support::Harness, id: &str) {
    h.terminal
        .tmux()
        .run(&[
            "set-hook",
            "-t",
            id,
            "after-capture-pane",
            "set-buffer -b fixture-captured ready; wait-for fixture-initial-boundary",
        ])
        .await
        .expect("owned queue gate");
}
async fn captured(h: &support::Harness) {
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-buffers", "-F", "#{buffer_name}"])
            .await
            .expect("snapshot buffers")
            .contains("fixture-captured")
    })
    .await;
}
async fn release(h: &support::Harness, id: &str) {
    h.terminal
        .tmux()
        .run(&["set-hook", "-u", "-t", id, "after-capture-pane"])
        .await
        .expect("remove owned hook");
    h.terminal
        .tmux()
        .run(&["wait-for", "-S", "fixture-initial-boundary"])
        .await
        .expect("release owned queue");
}
fn all_rows(output: &Output) {
    let bytes = output.0.lock().expect("output").clone();
    let text = String::from_utf8_lossy(&bytes);
    let (history, live) = text.split_once('\u{1b}').expect("history then redraw");
    for i in 1..=300 {
        let row = format!("number-{i:05}");
        assert_eq!(
            history.match_indices(&row).count() + usize::from(live.contains(&row)),
            1,
            "{row} missing/duplicated across history/initial screen"
        );
    }
}

struct ChangingOpener {
    base: Arc<dyn StreamOpener>,
    socket: String,
    id: String,
    child: Mutex<Option<Child>>,
    output: Mutex<Option<BufReader<ChildStdout>>>,
}
impl StreamOpener for ChangingOpener {
    fn open(&self) -> Result<StreamEnds, StreamError> {
        // The service already read the readonly grid. Model another writable
        // attach's ordinary control-helper resize before the atomic snapshot.
        let mut child = Command::new("tmux")
            .args([
                "-L",
                &self.socket,
                "-C",
                "attach",
                "-t",
                &self.id,
                ";",
                "refresh-client",
                "-C",
                "100,30",
                ";",
                "display-message",
                "-p",
                "fixture-new-grid-ready",
            ])
            .env_remove("TMUX")
            .env_remove("TMUX_TMPDIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("owned changing helper");
        let stdout = child.stdout.take().expect("helper stdout");
        *self.child.lock().expect("helper") = Some(child);
        let (ready, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut output = BufReader::new(stdout);
            let mut line = String::new();
            while output.read_line(&mut line).is_ok_and(|n| n != 0) {
                if line.trim_end() == "fixture-new-grid-ready" {
                    let _ = ready.send(output);
                    return;
                }
                line.clear();
            }
        });
        *self.output.lock().expect("retained helper stdout") = Some(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("helper resize acknowledgement"),
        );
        self.base.open()
    }
}
impl Drop for ChangingOpener {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.get_mut().expect("helper").take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[tokio::test]
async fn readonly_grid_change_before_atomic_snapshot_preserves_numbered_rows() {
    let h = support::Harness::new();
    let id = seeded(&h).await;
    let owner_output = Arc::new(Output::default());
    let owner_closed = CancellationToken::new();
    h.terminal
        .attach(
            context(
                10,
                owner_output.clone(),
                owner_closed.clone(),
                Arc::new(AtomicBool::new(false)),
            ),
            AttachSession {
                id: id.clone(),
                cols: 100,
                rows: 10,
                read_only: false,
            },
        )
        .await
        .expect("initial writer");
    until(async || {
        String::from_utf8_lossy(&owner_output.0.lock().expect("output")).contains("number-00300")
    })
    .await;
    assert_eq!(
        h.terminal
            .tmux()
            .run(&["list-panes", "-t", &id, "-F", "#{pane_height}"])
            .await
            .expect("original grid")
            .trim(),
        "10"
    );
    let output = Arc::new(Output::default());
    let closed = CancellationToken::new();
    let mut ctx = context(
        1,
        output.clone(),
        closed.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    let changer = Arc::new(ChangingOpener {
        base: ctx.streams.take().expect("base opener"),
        socket: h.terminal.tmux().socket().into(),
        id: id.clone(),
        child: Mutex::new(None),
        output: Mutex::new(None),
    });
    ctx.streams = Some(changer.clone());
    let attached = h
        .terminal
        .attach(
            ctx,
            AttachSession {
                id: id.clone(),
                cols: 5,
                rows: 5,
                read_only: true,
            },
        )
        .await
        .expect("readonly attach");
    assert_eq!(
        (attached.cols, attached.rows),
        (100, 30),
        "actual readonly dimensions after concurrent resize"
    );
    assert_eq!(
        h.terminal
            .tmux()
            .run(&["list-panes", "-t", &id, "-F", "#{pane_height}"])
            .await
            .expect("changed grid")
            .trim(),
        "30"
    );
    until(async || {
        String::from_utf8_lossy(&output.0.lock().expect("output")).contains("number-00300")
    })
    .await;
    closed.cancel();
    owner_closed.cancel();
    all_rows(&output);
}
#[tokio::test]
async fn immediate_resize_cannot_change_grid_before_initial_boundary() {
    let h = Arc::new(support::Harness::new());
    let id = seeded(&h).await;
    gate(&h, &id).await;
    let output = Arc::new(Output::default());
    let closed = CancellationToken::new();
    let attached = h.clone();
    let target = id.clone();
    let ctx = context(
        1,
        output.clone(),
        closed.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    let request = tokio::spawn(async move {
        attached
            .terminal
            .attach(
                ctx,
                AttachSession {
                    id: target,
                    cols: 100,
                    rows: 30,
                    read_only: false,
                },
            )
            .await
    });
    captured(&h).await;
    let returned_early = request.is_finished();
    if returned_early {
        h.terminal
            .resize(
                resize_context(1),
                ResizeStream {
                    stream_id: 1,
                    cols: 100,
                    rows: 10,
                },
            )
            .await
            .expect("early resize");
    }
    release(&h, &id).await;
    request.await.expect("attach task").expect("attach");
    if !returned_early {
        h.terminal
            .resize(
                resize_context(1),
                ResizeStream {
                    stream_id: 1,
                    cols: 100,
                    rows: 10,
                },
            )
            .await
            .expect("resize after initial boundary");
    }
    until(async || {
        String::from_utf8_lossy(&output.0.lock().expect("output")).contains("number-00300")
            || closed.is_cancelled()
    })
    .await;
    closed.cancel();
    all_rows(&output);
}
#[tokio::test]
async fn competing_writable_attach_waits_for_readonly_initial_boundary() {
    let h = Arc::new(support::Harness::new());
    let id = seeded(&h).await;
    let owner_output = Arc::new(Output::default());
    let owner_closed = CancellationToken::new();
    h.terminal
        .attach(
            context(
                10,
                owner_output.clone(),
                owner_closed.clone(),
                Arc::new(AtomicBool::new(false)),
            ),
            AttachSession {
                id: id.clone(),
                cols: 100,
                rows: 30,
                read_only: false,
            },
        )
        .await
        .expect("owner");
    until(async || {
        String::from_utf8_lossy(&owner_output.0.lock().expect("output")).contains("number-00300")
    })
    .await;
    gate(&h, &id).await;
    let output = Arc::new(Output::default());
    let closed = CancellationToken::new();
    let attached = h.clone();
    let target = id.clone();
    let ctx = context(
        1,
        output.clone(),
        closed.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    let first = tokio::spawn(async move {
        attached
            .terminal
            .attach(
                ctx,
                AttachSession {
                    id: target,
                    cols: 5,
                    rows: 5,
                    read_only: true,
                },
            )
            .await
    });
    captured(&h).await;
    let returned_early = first.is_finished();
    h.terminal
        .tmux()
        .run(&[
            "set-hook",
            "-t",
            &id,
            "after-refresh-client",
            "set-buffer -b fixture-grid-ready ready; wait-for fixture-grid-prepared",
        ])
        .await
        .expect("owned competing helper gate");
    let competing = h.clone();
    let target = id.clone();
    let competitor_closed = CancellationToken::new();
    let opened = Arc::new(AtomicBool::new(false));
    let ctx = context(
        2,
        Arc::new(Output::default()),
        competitor_closed.clone(),
        opened.clone(),
    );
    let second = tokio::spawn(async move {
        competing
            .terminal
            .attach(
                ctx,
                AttachSession {
                    id: target,
                    cols: 100,
                    rows: 10,
                    read_only: false,
                },
            )
            .await
    });
    until(async || opened.load(Ordering::SeqCst)).await;
    if returned_early {
        until(async || {
            h.terminal
                .tmux()
                .run(&["list-buffers", "-F", "#{buffer_name}"])
                .await
                .expect("helper boundary")
                .contains("fixture-grid-ready")
        })
        .await;
        until(async || {
            h.terminal
                .tmux()
                .run(&["list-panes", "-t", &id, "-F", "#{pane_height}"])
                .await
                .expect("competing grid")
                .trim()
                == "10"
        })
        .await;
    } else {
        assert!(
            !second.is_finished(),
            "same-session initialization must wait"
        );
        assert_eq!(
            h.terminal
                .tmux()
                .run(&["list-panes", "-t", &id, "-F", "#{pane_height}"])
                .await
                .expect("stable readonly grid")
                .trim(),
            "30"
        );
        // This other session has no hook and must proceed while the first is gated.
        let other = h
            .terminal
            .create(CreateSession {
                project_id: None,
                cwd: Some(h.home.path().display().to_string()),
                title: None,
                command: Some("exec sleep 60".into()),
                attach_zellij: None,
            })
            .await
            .expect("other session");
        let other_closed = CancellationToken::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            h.terminal.attach(
                context(
                    3,
                    Arc::new(Output::default()),
                    other_closed.clone(),
                    Arc::new(AtomicBool::new(false)),
                ),
                AttachSession {
                    id: other.id,
                    cols: 80,
                    rows: 24,
                    read_only: true,
                },
            ),
        )
        .await
        .expect("other session stays independent")
        .expect("other attach");
        other_closed.cancel();
    }
    release(&h, &id).await;
    first
        .await
        .expect("readonly task")
        .expect("readonly attach");
    until(async || {
        String::from_utf8_lossy(&output.0.lock().expect("output")).contains("number-00300")
    })
    .await;
    h.terminal
        .tmux()
        .run(&["set-hook", "-u", "-t", &id, "after-refresh-client"])
        .await
        .expect("remove owned helper hook");
    h.terminal
        .tmux()
        .run(&["wait-for", "-S", "fixture-grid-prepared"])
        .await
        .expect("release owned helper");
    second
        .await
        .expect("competing task")
        .expect("competing attach");
    closed.cancel();
    competitor_closed.cancel();
    owner_closed.cancel();
    all_rows(&output);
}
