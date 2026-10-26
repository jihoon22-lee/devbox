//! Direct/native readiness parity against a fake daemon gate; no Windows host resources.
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::{Mutex, Notify, mpsc};

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
