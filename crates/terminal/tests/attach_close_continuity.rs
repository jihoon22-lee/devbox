mod support;
use devbox_protocol::{
    Ctx,
    frame::Frame,
    stream::{Credit, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx, parse_stream},
};
use devbox_terminal::*;
use std::{
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

#[tokio::test]
async fn closing_writer_cannot_restore_grid_during_readonly_initialization() {
    let h = Arc::new(support::Harness::new());
    let id = seeded(&h).await;
    let owner_closed = CancellationToken::new();
    let newer_closed = CancellationToken::new();
    for (stream, rows, closed) in [
        (10, 30, owner_closed.clone()),
        (11, 10, newer_closed.clone()),
    ] {
        h.terminal
            .attach(
                context(
                    stream,
                    Arc::new(Output::default()),
                    closed,
                    Arc::new(AtomicBool::new(false)),
                ),
                AttachSession {
                    id: id.clone(),
                    cols: 100,
                    rows,
                    read_only: false,
                },
            )
            .await
            .expect("writer attach");
    }
    assert_eq!(
        h.terminal
            .tmux()
            .run(&["display-message", "-p", "-t", &id, "#{pane_height}"])
            .await
            .expect("newer grid")
            .trim(),
        "10"
    );
    // Delay only the dimensions RPC after it observed 10 rows. The snapshot
    // and attach command queue is unmodified and never yields.
    h.terminal
        .tmux()
        .run(&[
            "set-hook",
            "-t",
            &id,
            "after-list-panes",
            "set-buffer -b fixture-dimensions ready; wait-for fixture-dimensions-release",
        ])
        .await
        .expect("owned dimensions gate");
    let output = Arc::new(Output::default());
    let readonly_closed = CancellationToken::new();
    let attach_h = h.clone();
    let attach_id = id.clone();
    let ctx = context(
        12,
        output.clone(),
        readonly_closed.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    let request = tokio::spawn(async move {
        attach_h
            .terminal
            .attach(
                ctx,
                AttachSession {
                    id: attach_id,
                    cols: 5,
                    rows: 5,
                    read_only: true,
                },
            )
            .await
    });
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-buffers", "-F", "#{buffer_name}"])
            .await
            .expect("dimensions marker")
            .contains("fixture-dimensions")
    })
    .await;
    newer_closed.cancel();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        h.terminal
            .tmux()
            .run(&["display-message", "-p", "-t", &id, "#{pane_height}"])
            .await
            .expect("grid held through readonly boundary")
            .trim(),
        "10"
    );
    h.terminal
        .tmux()
        .run(&["set-hook", "-u", "-t", &id, "after-list-panes"])
        .await
        .expect("remove dimensions hook");
    h.terminal
        .tmux()
        .run(&["wait-for", "-S", "fixture-dimensions-release"])
        .await
        .expect("release dimensions");
    let attached = request
        .await
        .expect("request task")
        .expect("readonly attach");
    assert_eq!((attached.cols, attached.rows), (100, 10));
    until(async || {
        h.terminal
            .tmux()
            .run(&["display-message", "-p", "-t", &id, "#{pane_height}"])
            .await
            .expect("restored grid")
            .trim()
            == "30"
    })
    .await;
    until(async || {
        String::from_utf8_lossy(&output.0.lock().expect("output")).contains("number-00300")
    })
    .await;
    readonly_closed.cancel();
    owner_closed.cancel();
    all_rows(&output);
}
