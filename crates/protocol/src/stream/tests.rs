use super::*;
use std::sync::Mutex;
#[derive(Default)]
struct Sink(Mutex<Vec<Frame>>);
impl StreamSink for Sink {
    fn push(&self, frame: Frame) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            self.0
                .lock()
                .expect("fixture operation succeeds")
                .push(frame);
            true
        })
    }
}
#[tokio::test]
async fn exhausted_credit_resumes_with_a_partial_chunk() {
    let sink = Arc::new(Sink::default());
    let credit = Arc::new(Credit::new(8));
    let closed = CancellationToken::new();
    let tx = StreamTx::new(1, 7, credit.clone(), sink.clone(), closed.clone());
    tx.write(b"12345678")
        .await
        .expect("fixture operation succeeds");
    let writer = tokio::spawn(async move { tx.write(b"abcdefgh").await });
    tokio::task::yield_now().await;
    assert_eq!(sink.0.lock().expect("fixture operation succeeds").len(), 1);
    credit.add(2);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while sink.0.lock().expect("fixture operation succeeds").len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("fixture operation succeeds");
    assert_eq!(
        parse_stream(&sink.0.lock().expect("fixture operation succeeds")[1].body),
        Some((1, 0, &b"ab"[..]))
    );
    assert!(!writer.is_finished());
    closed.cancel();
    assert_eq!(
        writer.await.expect("fixture operation succeeds"),
        Err(StreamError::Closed)
    );
}
#[tokio::test]
async fn large_writes_split_and_finish_is_terminal() {
    let sink = Arc::new(Sink::default());
    let tx = StreamTx::new(
        1,
        7,
        Arc::new(Credit::new(STREAM_WINDOW)),
        sink.clone(),
        CancellationToken::new(),
    );
    tx.write(&vec![42; MAX_STREAM_CHUNK + 3])
        .await
        .expect("fixture operation succeeds");
    tx.finish().await;
    tx.finish().await;
    assert_eq!(tx.write(b"late").await, Err(StreamError::Closed));
    let frames = sink.0.lock().expect("fixture operation succeeds");
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].body.len(), MAX_STREAM_CHUNK + 5);
    assert_eq!(parse_stream(&frames[2].body), Some((1, FLAG_FIN, &[][..])));
}
#[test]
fn malformed_stream_bodies_are_rejected() {
    assert_eq!(parse_stream(&[0, 0]), None);
    assert_eq!(parse_stream(&[1, 0, 0, 0, 2]), None);
}

#[tokio::test]
async fn credit_overflow_is_clamped_and_cancel_wins_with_credit_available() {
    let credit = Credit::new(STREAM_WINDOW);
    credit.add(u32::MAX);
    let closed = CancellationToken::new();
    assert_eq!(
        credit
            .take(STREAM_WINDOW, &closed)
            .await
            .expect("fixture operation succeeds"),
        STREAM_WINDOW
    );
    let mut waiting = Box::pin(credit.take(1, &closed));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(10), &mut waiting)
            .await
            .is_err()
    );
    closed.cancel();
    credit.add(1);
    assert_eq!(waiting.await, Err(StreamError::Closed));
}
