//! WebSocket 세션 상한·자원(S5-4): 느린 화면의 버림 수 보고, 잘못된 입력·큰 메시지의 실패, 닫기 → 소켓·세션 자리 해제.
//! `realtime.rs`가 600줄을 넘지 않게 나눈 바이너리다.
mod support;
use devbox_api::*;
use futures::{SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use std::time::Duration;
use support::api::*;
use support::realtime::*;
use tokio_tungstenite::tungstenite::Message;

const T: Duration = Duration::from_secs(10);

/// 01 §5.1 흐름 제어: 화면 credit이 멈춰도 데몬은 소켓을 계속 읽고, 상한을 넘은 새 메시지는 버린다.
#[tokio::test]
async fn ws_slow_client_drops_and_reports_count() {
    const N: usize = 2_000;
    const SIZE: usize = 16 * 1024;
    let h = Harness::new().await;
    let server = WsServer::start(behavior(|mut socket: WsSocket, state| async move {
        for n in 0..N {
            let mut text = format!("{n:06}:");
            text.push_str(&"x".repeat(SIZE - text.len()));
            if socket.send(Message::text(text)).await.is_err() {
                return;
            }
        }
        state.flood_done.store(true, Ordering::SeqCst);
        state.changed.notify_waiters();
        let _ = socket.close(None).await;
        while socket.next().await.is_some() {}
    }))
    .await;
    // 열림 이벤트만 쓸 수 있는 credit, 돌려주지 않는다.
    let screen = Screen {
        initial: 64,
        hold: true,
    };
    let mut io = open_ws_with(&h.api, ws(&server.url("/flood")), screen)
        .await
        .expect("ws open");
    // 32MiB를 서버가 다 쓰고 닫기 응답까지 받는다 = 화면이 멈춘 동안에도 데몬이 소켓을 끝까지 읽었다.
    // (다 쓴 것만으로는 커널 버퍼에 남은 메시지가 있어 화면 재개 뒤 늦게 들어올 수 있다.)
    assert!(
        server
            .wait(
                |s| s.state.flood_done.load(Ordering::SeqCst) && s.ended() == 1,
                Duration::from_secs(20)
            )
            .await,
        "daemon kept reading the socket while the screen was stalled"
    );
    // 그동안 다른 RPC는 그대로 응답한다.
    let other = support::Server::ok().await;
    let events = tokio::time::timeout(T, h.complete(request(snapshot("GET", &other.url("/x")))))
        .await
        .expect("another RPC completes while the stream is stalled");
    assert_eq!(head(&events).0, 200);
    io.release();
    let events = io.events(Duration::from_secs(20)).await;
    let dropped_at = events
        .iter()
        .position(|e| matches!(e, RealtimeEvent::Dropped { .. }))
        .expect("dropped notice");
    let RealtimeEvent::Dropped { count } = events[dropped_at] else {
        unreachable!()
    };
    assert!(count > 0);
    // 열림과 이미 쓰던 메시지 하나 뒤, 큐에 남은 메시지보다 먼저 온다.
    assert!(
        matches!(events[0], RealtimeEvent::Open { .. }),
        "{:?}",
        shape(&events[0])
    );
    assert!(
        dropped_at <= 2,
        "dropped notice comes first after resume (at {dropped_at})"
    );
    let received: Vec<usize> = events
        .iter()
        .filter_map(|e| match e {
            RealtimeEvent::Message { data: Some(d), .. } => d[..6].parse().ok(),
            _ => None,
        })
        .collect();
    // 새 메시지를 버리므로 받은 것은 앞에서부터 이어진다.
    assert_eq!(received, (0..received.len()).collect::<Vec<_>>());
    assert_eq!(
        received.len() as u64 + count,
        N as u64,
        "every message is shown or counted"
    );
    assert!(received.len() < N);
}

#[tokio::test]
async fn ws_invalid_input_fails_closed_and_closes_the_socket() {
    let h = Harness::new().await;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("malformed line", b"{not json}\n".to_vec()),
        (
            "unknown field",
            br#"{"type":"text","data":"x","extra":1}"#.iter().chain(b"\n").copied().collect(),
        ),
        (
            "reserved close code",
            serde_json::to_vec(&WsInput::Close {
                code: Some(1005),
                reason: None,
            })
            .expect("json")
            .into_iter()
            .chain(*b"\n")
            .collect(),
        ),
        (
            "close reason over 123 bytes",
            serde_json::to_vec(&WsInput::Close {
                code: Some(1000),
                reason: Some("r".repeat(124)),
            })
            .expect("json")
            .into_iter()
            .chain(*b"\n")
            .collect(),
        ),
        (
            "bad base64",
            br#"{"type":"binary","dataBase64":"***"}"#.iter().chain(b"\n").copied().collect(),
        ),
        ("line over the input limit", vec![b'x'; 7 * 1024 * 1024]),
    ];
    for (case, line) in cases {
        let server = WsServer::start(echo()).await;
        let mut io = open_ws(&h.api, ws(&server.url("/")))
            .await
            .expect("ws open");
        first_open(&mut io).await;
        io.send_raw(&line).await;
        let events = io.events(T).await;
        assert!(
            matches!(events.last(), Some(RealtimeEvent::Failed { error }) if error.code == "api.invalid_input"),
            "{case}: {:?}",
            events.iter().map(shape).collect::<Vec<_>>()
        );
        assert!(
            server.wait(|s| s.ended() == 1, T).await,
            "{case}: socket closed"
        );
    }
}

#[tokio::test]
async fn ws_oversized_message_fails_instead_of_buffering() {
    let h = Harness::new().await;
    let server = WsServer::start(behavior(|mut socket: WsSocket, _| async move {
        let _ = socket
            .send(Message::binary(vec![7u8; 5 * 1024 * 1024]))
            .await;
        while socket.next().await.is_some() {}
    }))
    .await;
    let mut io = open_ws(&h.api, ws(&server.url("/")))
        .await
        .expect("ws open");
    let events = io.events(T).await;
    match events.last() {
        Some(RealtimeEvent::Failed { error }) => {
            assert_eq!(error.code, "api.too_large");
        }
        other => panic!("{other:?}"),
    }
}

/// 화면이 스트림을 닫으면 소켓이 닫히고 세션 자리가 돌아온다(태스크 누수 없음).
#[tokio::test]
async fn ws_closing_streams_releases_sockets_and_session_slots() {
    let h = Harness::new().await;
    let server = WsServer::start(echo()).await;
    let mut open = vec![];
    for _ in 0..MAX_SESSIONS {
        let mut io = open_ws(&h.api, ws(&server.url("/")))
            .await
            .expect("ws open");
        first_open(&mut io).await;
        open.push(io);
    }
    let err = open_ws(&h.api, ws(&server.url("/")))
        .await
        .err()
        .expect("limit");
    assert_eq!(err.code(), "api.too_many_streams");
    assert_eq!(
        server.accepted(),
        MAX_SESSIONS,
        "the refused session never connects"
    );
    for io in &open {
        io.close();
    }
    assert!(
        server.wait(|s| s.ended() == MAX_SESSIONS, T).await,
        "every socket closes when its stream closes ({} ended)",
        server.ended()
    );
    let mut again = None;
    for _ in 0..50 {
        if let Ok(io) = open_ws(&h.api, ws(&server.url("/"))).await {
            again = Some(io);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    first_open(again.as_mut().expect("a slot is free again")).await;
}
