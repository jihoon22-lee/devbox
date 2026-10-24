//! SSE 세션(S5-4): 재연결·Last-Event-ID·닫기·연결 전 검사·가림·상한.
mod support;
use devbox_api::*;
use std::time::Duration;
use support::api::*;
use support::realtime::*;

const T: Duration = Duration::from_secs(10);

fn sse(url: &str) -> SseOpen {
    SseOpen {
        url: url.into(),
        headers: vec![],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        last_event_id: None,
        origin: None,
    }
}
fn write(text: &str) -> SseStep {
    SseStep::Write(text.into())
}
fn kinds(events: &[RealtimeEvent]) -> Vec<String> {
    events.iter().map(shape).collect()
}

#[tokio::test]
async fn sse_reconnects_with_last_event_id() {
    let h = Harness::new().await;
    let server = SseServer::start(vec![
        vec![write("retry: 50\nid: 7\nevent: tick\ndata: a\ndata: b\n\n")],
        vec![write("id: 8\ndata: c\n\n"), SseStep::Hold],
    ])
    .await;
    let mut io = open_sse(&h.api, sse(&server.url("/events")))
        .await
        .expect("sse open");
    let mut events = vec![];
    while events.len() < 5 {
        events.push(io.next_event(T).await.expect("event"));
    }
    assert_eq!(
        kinds(&events),
        vec![
            "Open { protocol: None }".to_string(),
            "sse:Some(\"7\"):Some(\"tick\"):a\nb".into(),
            "Reconnecting { attempt: 1, last_event_id: Some(\"7\") }".into(),
            "Open { protocol: None }".into(),
            "sse:Some(\"8\"):Some(\"message\"):c".into(),
        ]
    );
    let requests = server.requests();
    assert_eq!(seen_header(&requests[0], "last-event-id"), None);
    assert_eq!(seen_header(&requests[1], "last-event-id"), Some("7"));
    assert_eq!(
        seen_header(&requests[0], "accept"),
        Some("text/event-stream")
    );
    // 화면이 닫으면 연결을 끊고 다시 연결하지 않는다.
    io.close();
    assert!(
        server.wait(|s| s.ended() == 2, T).await,
        "held connection closes"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.accepted(), 2, "no reconnect after the screen closed");
}

#[tokio::test]
async fn sse_closing_during_the_retry_wait_stops_reconnecting() {
    let h = Harness::new().await;
    // 대기 30초: 닫기가 대기를 끊지 않으면 세션 task(와 세션 자리)가 30초 동안 남는다.
    let server = SseServer::start(vec![vec![write("retry: 30000\ndata: once\n\n")]]).await;
    let mut io = open_sse(&h.api, sse(&server.url("/")))
        .await
        .expect("sse open");
    loop {
        if let RealtimeEvent::Reconnecting { .. } = io.next_event(T).await.expect("event") {
            break;
        }
    }
    io.close();
    // 세션 task가 끝나 화면 쪽 송신부가 모두 버려져야 받는 쪽이 끝난다(FIN 없이).
    let rest = io.events(Duration::from_secs(2)).await;
    assert!(rest.is_empty(), "{rest:?}");
    assert_eq!(
        server.accepted(),
        1,
        "the retry sleep is cancelled by closing"
    );
}

#[tokio::test]
async fn sse_gives_up_after_five_reconnects_without_events() {
    let h = Harness::new().await;
    let server = SseServer::start(vec![
        vec![write("retry: 10\ndata: first\n\n")],
        vec![write(": nothing\n")],
    ])
    .await;
    let mut io = open_sse(&h.api, sse(&server.url("/")))
        .await
        .expect("sse open");
    let events = io.events(T).await;
    let attempts: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            RealtimeEvent::Reconnecting { attempt, .. } => Some(*attempt),
            _ => None,
        })
        .collect();
    assert_eq!(attempts, vec![1, 2, 3, 4, 5], "{:?}", kinds(&events));
    assert!(
        matches!(events.last(), Some(RealtimeEvent::Failed { error }) if error.code == "api.network"),
        "{:?}",
        kinds(&events)
    );
    assert_eq!(server.accepted(), 6);
}

/// 연결마다 이벤트를 하나씩 주고 끊는 서버도 다시 연결을 끝없이 일으키지 못한다(1분에 30번까지).
#[tokio::test]
async fn sse_reconnects_are_bounded_even_when_every_connection_delivers_an_event() {
    let h = Harness::new().await;
    let server = SseServer::start(vec![vec![write("retry: 10\ndata: e\n\n")]]).await;
    let mut io = open_sse(&h.api, sse(&server.url("/")))
        .await
        .expect("sse open");
    let mut reconnects = 0;
    let last = loop {
        let event = io.next_event(T).await.expect("event");
        match &event {
            RealtimeEvent::Reconnecting { .. } => reconnects += 1,
            RealtimeEvent::Failed { .. } | RealtimeEvent::Closed { .. } => break event,
            _ => {}
        }
        // 상한이 없으면 여기서 멈춘다(시험이 끝없이 돌지 않게).
        if reconnects > 60 {
            break event;
        }
    };
    assert_eq!(reconnects, 30, "reconnects within the window are capped");
    let RealtimeEvent::Failed { error } = last else {
        panic!("session must fail after the cap: {}", shape(&last));
    };
    assert_eq!(error.code, "api.network");
    assert!(
        format!("{:?}", error.detail).contains("reconnect_limit"),
        "{:?}",
        error.detail
    );
    io.close();
}

#[tokio::test]
async fn sse_rejected_responses_do_not_reconnect() {
    let h = Harness::new().await;
    for (case, head, reason) in [
        ("status", SseStep::Head(500, "text/event-stream"), "status"),
        (
            "content type",
            SseStep::Head(200, "text/html"),
            "content_type",
        ),
    ] {
        let server = SseServer::start(vec![vec![head, write("retry: 10\ndata: x\n\n")]]).await;
        let mut io = open_sse(&h.api, sse(&server.url("/")))
            .await
            .expect("sse open");
        let events = io.events(T).await;
        match events.first() {
            Some(RealtimeEvent::Failed { error }) => {
                assert_eq!(error.code, "api.stream_rejected", "{case}");
                let detail: serde_json::Value =
                    serde_json::from_str(error.detail.as_ref().expect("detail").get())
                        .expect("json");
                assert_eq!(detail["reason"], reason, "{case}");
            }
            other => panic!("{case}: {other:?}"),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(server.accepted(), 1, "{case}: no reconnect");
    }
}

#[tokio::test]
async fn sse_initial_last_event_id_and_headers_are_checked_before_connecting() {
    let h = Harness::new().await;
    h.secret("GH", "gh-fixture-value").await;
    let server = SseServer::start(vec![vec![write("data: x\n\n"), SseStep::Hold]]).await;
    let bad: Vec<(&str, SseOpen, &str)> = vec![
        (
            "newline id",
            SseOpen {
                last_event_id: Some("a\nb".into()),
                ..sse(&server.url("/"))
            },
            "api.invalid_input",
        ),
        (
            "long id",
            SseOpen {
                last_event_id: Some("i".repeat(257)),
                ..sse(&server.url("/"))
            },
            "api.invalid_input",
        ),
        (
            "reserved header",
            SseOpen {
                headers: vec![header(0, "Last-Event-ID", "1")],
                ..sse(&server.url("/"))
            },
            "api.invalid_header",
        ),
        (
            "bad header",
            SseOpen {
                headers: vec![header(0, "X Bad", "1")],
                ..sse(&server.url("/"))
            },
            "api.invalid_header",
        ),
        (
            "ws scheme",
            sse(&format!("ws://{}/", server.addr)),
            "api.invalid_url",
        ),
    ];
    for (case, open, code) in bad {
        let err = open_sse(&h.api, open)
            .await
            .err()
            .unwrap_or_else(|| panic!("{case}"));
        assert_eq!(err.code(), code, "{case}");
    }
    assert_eq!(server.accepted(), 0, "rejected before connecting");
    let mut io = open_sse(
        &h.api,
        SseOpen {
            headers: vec![header(0, "Authorization", "Bearer {{secret:GH}}")],
            last_event_id: Some("3".into()),
            ..sse(&server.url("/"))
        },
    )
    .await
    .expect("sse open");
    assert!(matches!(
        io.next_event(T).await,
        Some(RealtimeEvent::Open { .. })
    ));
    let seen = &server.requests()[0];
    assert_eq!(seen_header(seen, "last-event-id"), Some("3"));
    assert_eq!(
        seen_header(seen, "authorization"),
        Some("Bearer gh-fixture-value")
    );
    io.close();
}

#[tokio::test]
async fn sse_event_fields_with_secrets_are_redacted_but_reconnect_uses_the_raw_id() {
    let h = Harness::new().await;
    h.secret("GH", "gh-fixture-value").await;
    let server = SseServer::start(vec![
        vec![write("retry: 10\nid: gh-fixture-value\nevent: gh-fixture-value\ndata: token gh-fixture-value\n\n")],
        vec![SseStep::Head(200, "text/event-stream"), SseStep::Hold],
    ])
    .await;
    let mut io = open_sse(
        &h.api,
        SseOpen {
            headers: vec![header(0, "X-Key", "{{secret:GH}}")],
            ..sse(&server.url("/"))
        },
    )
    .await
    .expect("sse open");
    let mut events = vec![];
    while events.len() < 4 {
        events.push(io.next_event(T).await.expect("event"));
    }
    let text = format!("{events:?}");
    assert!(
        !text.contains("gh-fixture-value"),
        "events leak the secret: {text}"
    );
    assert!(
        kinds(&events).contains(
            &"sse:Some(\"[REDACTED]\"):Some(\"[REDACTED]\"):token [REDACTED]".to_string()
        ),
        "{text}"
    );
    assert!(server.wait(|s| s.requests().len() == 2, T).await);
    assert_eq!(
        seen_header(&server.requests()[1], "last-event-id"),
        Some("gh-fixture-value")
    );
    io.close();
    assert!(
        h.history_files().is_empty(),
        "no history file for realtime sessions"
    );
}

#[tokio::test]
async fn sse_line_without_newline_fails_closed_without_reconnect() {
    let h = Harness::new().await;
    let server = SseServer::start(vec![vec![
        write("retry: 10\n"),
        write(&"x".repeat(70 * 1024)),
        SseStep::Hold,
    ]])
    .await;
    let mut io = open_sse(&h.api, sse(&server.url("/")))
        .await
        .expect("sse open");
    let events = io.events(T).await;
    match events.last() {
        Some(RealtimeEvent::Failed { error }) => {
            assert_eq!(error.code, "api.network");
            let detail: serde_json::Value =
                serde_json::from_str(error.detail.as_ref().expect("detail").get()).expect("json");
            assert_eq!(detail["kind"], "sse_line_too_long");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        server.wait(|s| s.ended() == 1, T).await,
        "connection closed"
    );
    assert_eq!(server.accepted(), 1);
}
