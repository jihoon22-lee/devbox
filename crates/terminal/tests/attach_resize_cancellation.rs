mod support;
use devbox_protocol::{
    Ctx,
    frame::Frame,
    stream::{Credit, StreamEnds, StreamError, StreamOpener, StreamSink, StreamTx},
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
struct Sink;
impl StreamSink for Sink {
    fn push(
        &self,
        _: Frame,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(async { true })
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
fn context(id: u32, closed: CancellationToken, opened: Arc<AtomicBool>) -> Ctx {
    let (_input, input) = tokio::sync::mpsc::channel(4);
    Ctx {
        channel: 0,
        cancel: CancellationToken::new(),
        streams: Some(Arc::new(Opener {
            id,
            opened,
            ends: Mutex::new(Some(StreamEnds {
                id,
                tx: StreamTx::new(id, 0, Arc::new(Credit::new(0)), Arc::new(Sink), closed),
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
struct Resume {
    pid: i32,
    stopped: bool,
}
impl Resume {
    fn stop(pid: i32) -> Self {
        // This PID is obtained from the unique fixture server's owned client.
        assert!(
            std::process::Command::new("kill")
                .args(["-STOP", &pid.to_string()])
                .status()
                .expect("signal owned client")
                .success(),
            "stop owned attachment client"
        );
        Self { pid, stopped: true }
    }
    fn resume(&mut self) {
        assert!(
            std::process::Command::new("kill")
                .args(["-CONT", &self.pid.to_string()])
                .status()
                .expect("signal owned client")
                .success(),
            "resume owned attachment client"
        );
        self.stopped = false;
    }
}
impl Drop for Resume {
    fn drop(&mut self) {
        if self.stopped {
            let _ = std::process::Command::new("kill")
                .args(["-CONT", &self.pid.to_string()])
                .status();
        }
    }
}
#[tokio::test]
async fn cancelled_issued_resize_keeps_session_gate_until_client_ack() {
    let h = Arc::new(support::Harness::new());
    let session = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some(
                "for i in $(seq 1 300); do printf 'row-%05d\\n' $i; done; exec sleep 60".into(),
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
            .expect("seeded screen")
            .contains("row-00300")
    })
    .await;
    let owner_closed = CancellationToken::new();
    h.terminal
        .attach(
            context(1, owner_closed.clone(), Arc::new(AtomicBool::new(false))),
            AttachSession {
                id: session.id.clone(),
                cols: 100,
                rows: 10,
                read_only: false,
            },
        )
        .await
        .expect("zero-credit owner");
    let clients = h
        .terminal
        .tmux()
        .run(&[
            "list-clients",
            "-t",
            &session.id,
            "-F",
            "#{client_pid} #{client_height}",
        ])
        .await
        .expect("owned client");
    let pid: i32 = clients
        .lines()
        .find(|line| line.split_whitespace().count() == 2)
        .expect("owned ordinary client")
        .split_whitespace()
        .next()
        .expect("pid")
        .parse()
        .expect("numeric pid");
    let mut resume = Resume::stop(pid);
    let resized = h.clone();
    let request = tokio::spawn(async move {
        resized
            .terminal
            .resize(
                context(
                    1,
                    CancellationToken::new(),
                    Arc::new(AtomicBool::new(false)),
                ),
                ResizeStream {
                    stream_id: 1,
                    cols: 100,
                    rows: 30,
                },
            )
            .await
    });
    until(async || {
        let output = std::process::Command::new("stty")
            .args(["-F", &format!("/proc/{pid}/fd/0"), "size"])
            .output()
            .expect("read owned tty size");
        output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "30 100"
    })
    .await;
    assert!(
        !request.is_finished(),
        "resize must await stopped client's acknowledgement"
    );
    request.abort();
    assert!(request.await.expect_err("cancelled caller").is_cancelled());
    let reader_closed = CancellationToken::new();
    let opened = Arc::new(AtomicBool::new(false));
    let ctx = context(2, reader_closed.clone(), opened.clone());
    let attached = h.clone();
    let target = session.id.clone();
    let reader = tokio::spawn(async move {
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
    until(async || opened.load(Ordering::SeqCst)).await;
    assert!(
        !reader.is_finished(),
        "cancelled resize must retain only its own session gate"
    );
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
            context(3, other_closed.clone(), Arc::new(AtomicBool::new(false))),
            AttachSession {
                id: other.id,
                cols: 5,
                rows: 5,
                read_only: true,
            },
        ),
    )
    .await
    .expect("other session independent")
    .expect("other attach");
    other_closed.cancel();
    resume.resume();
    let reader = tokio::time::timeout(Duration::from_secs(5), reader)
        .await
        .expect("ack releases same-session gate")
        .expect("reader task")
        .expect("reader attach");
    assert_eq!(
        (reader.cols, reader.rows),
        (100, 30),
        "readonly dimensions follow acknowledged resize"
    );
    owner_closed.cancel();
    reader_closed.cancel();
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-clients", "-t", &session.id])
            .await
            .expect("client cleanup")
            .is_empty()
    })
    .await;
}

#[tokio::test]
async fn cancelled_grid_preparation_restores_owner_before_next_readonly_boundary() {
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
    let owner_closed = CancellationToken::new();
    h.terminal
        .attach(
            context(1, owner_closed.clone(), Arc::new(AtomicBool::new(false))),
            AttachSession {
                id: session.id.clone(),
                cols: 100,
                rows: 30,
                read_only: false,
            },
        )
        .await
        .expect("owner");
    h.terminal
        .tmux()
        .run(&[
            "set-hook",
            "-t",
            &session.id,
            "after-refresh-client",
            "wait-for fixture-owned-prep",
        ])
        .await
        .expect("owned preparation gate");
    let preparing = h.clone();
    let target = session.id.clone();
    let pending_closed = CancellationToken::new();
    let ctx = context(2, pending_closed.clone(), Arc::new(AtomicBool::new(false)));
    let request = tokio::spawn(async move {
        preparing
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
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-panes", "-t", &session.id, "-F", "#{pane_height}"])
            .await
            .expect("prepared smaller grid")
            .trim()
            == "10"
    })
    .await;
    request.abort();
    assert!(
        request
            .await
            .expect_err("aborted preparing caller")
            .is_cancelled()
    );
    assert!(pending_closed.is_cancelled());
    let reader_closed = CancellationToken::new();
    let reader = tokio::time::timeout(
        Duration::from_secs(5),
        h.terminal.attach(
            context(3, reader_closed.clone(), Arc::new(AtomicBool::new(false))),
            AttachSession {
                id: session.id.clone(),
                cols: 5,
                rows: 5,
                read_only: true,
            },
        ),
    )
    .await
    .expect("owned cleanup releases next boundary")
    .expect("readonly attach");
    assert_eq!(
        (reader.cols, reader.rows),
        (100, 30),
        "temporary helper must be removed before next readonly grid query"
    );
    assert_eq!(
        h.terminal
            .tmux()
            .run(&["list-panes", "-t", &session.id, "-F", "#{pane_height}"])
            .await
            .expect("restored grid")
            .trim(),
        "30"
    );
    owner_closed.cancel();
    reader_closed.cancel();
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-clients", "-t", &session.id])
            .await
            .expect("cleanup")
            .is_empty()
    })
    .await;
}
#[tokio::test]
async fn normal_close_during_issued_resize_hands_cleanup_to_ack_owner() {
    let h = Arc::new(support::Harness::new());
    let session = h
        .terminal
        .create(CreateSession {
            project_id: None,
            cwd: Some(h.home.path().display().to_string()),
            title: None,
            command: Some(
                "for i in $(seq 1 300); do printf 'row-%05d\\n' $i; done; exec sleep 60".into(),
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
            .expect("seeded screen")
            .contains("row-00300")
    })
    .await;
    let owner_closed = CancellationToken::new();
    h.terminal
        .attach(
            context(1, owner_closed.clone(), Arc::new(AtomicBool::new(false))),
            AttachSession {
                id: session.id.clone(),
                cols: 100,
                rows: 10,
                read_only: false,
            },
        )
        .await
        .expect("zero-credit owner");
    let clients = h
        .terminal
        .tmux()
        .run(&[
            "list-clients",
            "-t",
            &session.id,
            "-F",
            "#{client_pid} #{client_height}",
        ])
        .await
        .expect("owned client");
    let pid: i32 = clients
        .lines()
        .find(|line| line.split_whitespace().count() == 2)
        .expect("owned ordinary client")
        .split_whitespace()
        .next()
        .expect("pid")
        .parse()
        .expect("numeric pid");
    let mut resume = Resume::stop(pid);
    let resized = h.clone();
    let request = tokio::spawn(async move {
        resized
            .terminal
            .resize(
                context(
                    1,
                    CancellationToken::new(),
                    Arc::new(AtomicBool::new(false)),
                ),
                ResizeStream {
                    stream_id: 1,
                    cols: 100,
                    rows: 30,
                },
            )
            .await
    });
    until(async || {
        let output = std::process::Command::new("stty")
            .args(["-F", &format!("/proc/{pid}/fd/0"), "size"])
            .output()
            .expect("read owned tty size");
        output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "30 100"
    })
    .await;
    owner_closed.cancel();
    tokio::time::timeout(Duration::from_secs(5), request)
        .await
        .expect("close during ACK must not deadlock")
        .expect("resize task")
        .expect_err("closed stream resize");
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-clients", "-t", &session.id])
            .await
            .expect("closed peer cleanup")
            .is_empty()
    })
    .await;
    resume.stopped = false; // Reaper killed and reaped this stopped fixture PID.
    let reader_closed = CancellationToken::new();
    tokio::time::timeout(
        Duration::from_secs(2),
        h.terminal.attach(
            context(2, reader_closed.clone(), Arc::new(AtomicBool::new(false))),
            AttachSession {
                id: session.id.clone(),
                cols: 5,
                rows: 5,
                read_only: true,
            },
        ),
    )
    .await
    .expect("failed ACK releases cleanup lease")
    .expect("next reader");
    reader_closed.cancel();
    until(async || {
        h.terminal
            .tmux()
            .run(&["list-clients", "-t", &session.id])
            .await
            .expect("reader cleanup")
            .is_empty()
    })
    .await;
}
