mod support;
use devbox_protocol::stream::STREAM_WINDOW;
use devbox_protocol::system::{Ping, PingParams};
use std::time::Duration;
use support::{TestDaemon, bounded};

#[tokio::test]
async fn stalled_stream_preserves_ping_and_resumes_on_small_credit() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let stream =
        bounded(client.open_stream("debug.echo", serde_json::json!({"bytes": 4 * 1024 * 1024})))
            .await
            .expect("fixture operation succeeds");
    let initial = stream.read_available(Duration::from_millis(150)).await;
    assert_eq!(initial.len(), STREAM_WINDOW as usize);
    bounded(client.call::<Ping>(&PingParams {}))
        .await
        .expect("fixture operation succeeds");
    stream
        .credit(1024)
        .await
        .expect("fixture operation succeeds");
    assert_eq!(
        stream
            .read_available(Duration::from_millis(150))
            .await
            .len(),
        1024
    );
    client
        .close_channel()
        .await
        .expect("fixture operation succeeds");
    assert!(stream.wait_end(Duration::from_secs(2)).await);
}
#[tokio::test]
async fn input_fin_flushes_echo_before_end_and_client_drop_closes_stream() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let stream = bounded(client.open_stream("debug.echo", serde_json::json!({"bytes": 0})))
        .await
        .expect("fixture operation succeeds");
    stream
        .write(b"hello")
        .await
        .expect("fixture operation succeeds");
    stream
        .finish_input()
        .await
        .expect("fixture operation succeeds");
    assert_eq!(
        stream.read_available(Duration::from_secs(2)).await,
        b"hello"
    );
    assert!(stream.wait_end(Duration::from_secs(2)).await);
    let next = bounded(client.open_stream("debug.echo", serde_json::json!({"bytes": 0})))
        .await
        .expect("fixture operation succeeds");
    drop(client);
    assert!(next.wait_end(Duration::from_secs(2)).await);
}

#[tokio::test]
async fn closing_and_dropping_stalled_streams_preserve_sibling_stream_and_rpc() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    for explicitly_close in [true, false] {
        let stalled = bounded(client.open_stream(
            "debug.echo",
            serde_json::json!({"bytes": STREAM_WINDOW + 1}),
        ))
        .await
        .expect("open stalled stream");
        assert_eq!(
            stalled
                .read_available(Duration::from_millis(100))
                .await
                .len(),
            STREAM_WINDOW as usize
        );
        if explicitly_close {
            stalled.close().await.expect("close only stream");
            assert!(stalled.wait_end(Duration::from_secs(1)).await);
        }
        drop(stalled);
        bounded(client.call::<Ping>(&PingParams {}))
            .await
            .expect("connection remains usable");
        let sibling = bounded(client.open_stream("debug.echo", serde_json::json!({"bytes":0})))
            .await
            .expect("open sibling stream");
        sibling.write(b"still alive").await.expect("write sibling");
        sibling
            .finish_input()
            .await
            .expect("half close sibling input");
        assert_eq!(
            sibling.read_available(Duration::from_secs(1)).await,
            b"still alive"
        );
    }
}
