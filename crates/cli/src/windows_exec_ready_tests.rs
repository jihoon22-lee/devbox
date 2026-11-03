//! Direct/native readiness parity against a fake daemon gate; no Windows host resources.
use super::*;
use devbox_protocol::{
    frame::FrameCodec,
    message::{ClientMessage, ServerMessage, json_frame, parse_json},
};
use futures::{SinkExt, StreamExt};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    net::UnixListener,
    sync::{Mutex, Notify, mpsc},
};
use tokio_util::codec::Framed;

fn identity() -> RunIdentity {
    RunIdentity {
        run_id: "owned-run".into(),
        attempt_id: "owned-attempt".into(),
    }
}
fn frame(event: WindowsOutput) -> Vec<u8> {
    let id = identity();
    let body = serde_json::to_vec(&WindowsEvent {
        run_id: id.run_id,
        attempt_id: id.attempt_id,
        event,
    })
    .expect("event");
    [(body.len() as u32).to_le_bytes().to_vec(), body].concat()
}
/// The gate answers not-ready until call `accept_at`, then accepts.
fn gate(calls: &Arc<AtomicUsize>, accept_at: usize) -> impl FnMut() -> std::future::Ready<bool> {
    let calls = calls.clone();
    move || std::future::ready(calls.fetch_add(1, Ordering::SeqCst) + 1 >= accept_at)
}
async fn until(calls: &AtomicUsize, count: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while calls.load(Ordering::SeqCst) < count {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("gate polled");
}

#[tokio::test]
async fn direct_poll_keeps_asking_the_gate_after_not_ready_until_accepted() {
    let calls = Arc::new(AtomicUsize::new(0));
    tokio::time::timeout(
        Duration::from_secs(2),
        poll_ready(
            gate(&calls, 3),
            &Notify::new(),
            Cadence::fixed(Duration::from_millis(10)),
        ),
    )
    .await
    .expect("accepted after retries");
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn no_port_service_never_asks_the_gate() {
    let calls = Arc::new(AtomicUsize::new(0));
    let never = tokio::time::timeout(
        Duration::from_millis(50),
        readiness(
            &[],
            gate(&calls, 1),
            &Notify::new(),
            Cadence::fixed(Duration::from_millis(1)),
        ),
    )
    .await;
    assert!(never.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn native_ready_hint_with_not_ready_gate_keeps_the_stream_and_later_succeeds() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel::<Vec<u8>>(8);
    let rx = Mutex::new(rx);
    let read = || async { Ok(rx.lock().await.recv().await) };
    let id = identity();
    // A long cadence proves the second poll was woken by the native Ready hint.
    let relayed = relay(
        read,
        &id,
        &[3000],
        gate(&calls, 2),
        Cadence::fixed(Duration::from_secs(60)),
    );
    let native = async {
        until(&calls, 1).await;
        tx.send(frame(WindowsOutput::Ready)).await.expect("hint");
        until(&calls, 2).await;
        tx.send(frame(WindowsOutput::Heartbeat))
            .await
            .expect("alive");
        tx.send(frame(WindowsOutput::Finished { code: 7 }))
            .await
            .expect("finish");
    };
    let (code, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(relayed, native)
    })
    .await
    .expect("relay completes");
    assert_eq!(code.expect("not-ready never aborts the stream"), 7);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "accepted once, then stops polling"
    );
}

#[test]
fn cadence_is_one_second_inside_the_ready_window_then_backs_off_to_thirty() {
    let cadence = Cadence::windows(10);
    let second = Duration::from_secs(1);
    assert_eq!(cadence.next(None, Duration::ZERO), second);
    assert_eq!(cadence.next(Some(second), Duration::from_secs(9)), second);
    let mut delay = second;
    let mut seen = Vec::new();
    for _ in 0..7 {
        delay = cadence.next(Some(delay), Duration::from_secs(11));
        seen.push(delay.as_secs());
    }
    assert_eq!(seen, [2, 4, 8, 16, 30, 30, 30]);
    // A zero window (no explicit timeout) backs off from the first not-ready reply.
    assert_eq!(
        Cadence::windows(0).next(Some(second), Duration::ZERO),
        2 * second
    );
}

#[tokio::test(start_paused = true)]
async fn gate_polls_follow_the_back_off_and_a_ready_hint_still_wakes_at_once() {
    let start = tokio::time::Instant::now();
    let polls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let hint = Arc::new(Notify::new());
    let polling = {
        let polls = polls.clone();
        let hint = hint.clone();
        tokio::spawn(async move {
            let accept = || {
                polls.lock().expect("polls").push(start.elapsed().as_secs());
                std::future::ready(false)
            };
            poll_ready(accept, &hint, Cadence::windows(3)).await;
        })
    };
    tokio::time::sleep(Duration::from_millis(18_500)).await;
    // 1 s inside the 3 s readiness window, then 2, 4, 8 s (next would be 16 s later).
    assert_eq!(*polls.lock().expect("polls"), [0, 1, 2, 3, 5, 9, 17]);
    hint.notify_one();
    tokio::time::sleep(Duration::from_millis(1)).await;
    assert_eq!(*polls.lock().expect("polls"), [0, 1, 2, 3, 5, 9, 17, 18]);
    polling.abort();
}

/// A fake daemon on an owned socket. It serves one connection: a Welcome for its Hello, then one
/// `windows.ready` answer per entry of `answers`. It returns what it was sent and then stops,
/// closing the connection and the listener the way a restarting daemon does.
async fn daemon(listener: UnixListener, answers: &[WindowsReadiness]) -> Vec<String> {
    let (stream, _) = listener.accept().await.expect("owned connection");
    let mut framed = Framed::new(stream, FrameCodec::default());
    let mut seen = Vec::new();
    let hello = framed.next().await.expect("hello").expect("hello frame");
    let ClientMessage::Hello { .. } = parse_json(&hello).expect("client message") else {
        panic!("a new connection must start with Hello");
    };
    seen.push("hello".to_owned());
    let welcome = ServerMessage::Welcome {
        version: crate::VERSION.into(),
        protocol: devbox_protocol::PROTOCOL_VERSION,
        daemon_id: "owned-fake".into(),
    };
    framed.send(json_frame(0, &welcome)).await.expect("welcome");
    for answer in answers {
        let frame = framed
            .next()
            .await
            .expect("request")
            .expect("request frame");
        let ClientMessage::Request {
            id, method, params, ..
        } = parse_json(&frame).expect("client message")
        else {
            panic!("only requests follow Hello");
        };
        let asked: RunIdentity = serde_json::from_str(params.get()).expect("run identity");
        assert_eq!(asked, identity());
        seen.push(method);
        let result = serde_json::value::to_raw_value(answer).expect("answer");
        let reply = ServerMessage::Response {
            id,
            result: Some(result),
            error: None,
            diagnostic_id: None,
        };
        framed.send(json_frame(0, &reply)).await.expect("reply");
    }
    seen
}

#[tokio::test]
async fn ready_poll_reconnects_after_a_daemon_restart() {
    let temp = tempfile::tempdir().expect("owned socket directory");
    let socket = temp.path().join("daemon.sock");
    let first = UnixListener::bind(&socket).expect("owned socket");
    let connect = || Client::connect(&socket, ClientKind::Cli, "owned-instance");
    let restart = async {
        // Not ready yet; then the daemon restarts, and its socket is gone for a while.
        let before = daemon(first, &[WindowsReadiness::NotReady]).await;
        std::fs::remove_file(&socket).expect("owned socket");
        tokio::time::sleep(Duration::from_millis(400)).await;
        let listener = UnixListener::bind(&socket).expect("owned socket");
        let after = daemon(listener, &[WindowsReadiness::Ready]).await;
        (before, after)
    };
    let id = identity();
    let poll = async {
        let gate = Gate::new(Some(connect().await.expect("first Hello")), connect);
        let ask = || gate.ask(&id);
        poll_ready(
            ask,
            &Notify::new(),
            Cadence::fixed(Duration::from_millis(50)),
        )
        .await;
    };
    let ((before, after), ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(restart, poll)
    })
    .await
    .expect("the gate accepts after the daemon restarts");
    assert_eq!(before, ["hello", "windows.ready"]);
    // A fresh Hello first; then only the readiness question is asked again.
    assert_eq!(after, ["hello", "windows.ready"]);
}

#[tokio::test(start_paused = true)]
async fn reconnects_back_off_from_250_ms_to_5_s_until_the_run_ends() {
    let start = tokio::time::Instant::now();
    let attempts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let tries = attempts.clone();
    let connect = move || {
        let elapsed = start.elapsed().as_millis();
        tries.lock().expect("attempts").push(elapsed);
        std::future::ready(Err::<Client, _>(ClientError::ConnectionLost))
    };
    // The connection is already lost and the daemon does not come back.
    let gate = Gate::new(None, connect);
    let id = identity();
    let ask = || gate.ask(&id);
    let hint = Notify::new();
    let poll = poll_ready(ask, &hint, Cadence::fixed(Duration::from_secs(1)));
    tokio::select! {
        () = poll => panic!("no daemon ever answered"),
        () = tokio::time::sleep(Duration::from_secs(20)) => {}
    }
    let schedule = [250, 750, 1750, 3750, 7750, 12750, 17750];
    assert_eq!(*attempts.lock().expect("attempts"), schedule);
    // The run ended: its poll was dropped, and nothing reconnects after that.
    tokio::time::sleep(Duration::from_secs(60)).await;
    assert_eq!(*attempts.lock().expect("attempts"), schedule);
}
