//! WebSocket 세션(S5-4): 왕복·닫기·흐름 제어·연결 전 검사·비밀 가림·TLS·자원 해제.
mod support;
use devbox_api::*;
use futures::{SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use std::time::Duration;
use support::api::*;
use support::realtime::*;
use support::tls::Pki;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

const T: Duration = Duration::from_secs(10);

fn var(name: &str, value: &str, origin: VarOrigin) -> EnvVar {
    EnvVar {
        name: name.into(),
        value: value.into(),
        sensitive: origin == VarOrigin::Capture,
        origin,
    }
}

#[tokio::test]
async fn ws_text_and_binary_messages_round_trip() {
    let h = Harness::new().await;
    let server = WsServer::start(echo()).await;
    let mut io = open_ws(&h.api, ws(&server.url("/echo")))
        .await
        .expect("ws open");
    first_open(&mut io).await;
    io.send(WsInput::Text {
        data: "ping".into(),
    })
    .await;
    let sent = io.next_event(T).await.expect("sent echo");
    assert_eq!(
        shape(&sent),
        "message:Sent:Text:Some(\"ping\"):None",
        "the screen sees what was sent"
    );
    assert_eq!(
        shape(&io.next_event(T).await.expect("pong")),
        shape(&received_text("pong:ping"))
    );
    let bytes = [0u8, 159, 146, 150, 255];
    let encoded = data_encoding::BASE64.encode(&bytes);
    io.send(WsInput::Binary {
        data_base64: encoded.clone(),
    })
    .await;
    let _ = io.next_event(T).await.expect("sent binary");
    match io.next_event(T).await.expect("binary echo") {
        RealtimeEvent::Message {
            direction: Direction::Received,
            kind: MessageKind::Binary,
            data: None,
            data_base64: Some(b64),
            ..
        } => assert_eq!(
            data_encoding::BASE64.decode(b64.as_bytes()).expect("b64"),
            bytes
        ),
        other => panic!("binary echo: {other:?}"),
    }
    io.send(WsInput::Close {
        code: Some(4000),
        reason: Some("bye".into()),
    })
    .await;
    let rest = io.events(T).await;
    assert!(
        rest.iter().any(|e| matches!(e, RealtimeEvent::Closed { code: Some(4000), reason: Some(r) } if r == "bye")),
        "{rest:?}"
    );
    assert!(
        server.wait(|s| s.ended() == 1, T).await,
        "server connection ended"
    );
    assert_eq!(
        h.history_rows().await,
        0,
        "realtime sessions are not recorded"
    );
}

#[tokio::test]
async fn ws_server_close_finishes_the_stream_with_its_code() {
    let h = Harness::new().await;
    let server = WsServer::start(behavior(|mut socket: WsSocket, _| async move {
        let _ = socket.send(Message::text("hello")).await;
        let _ = socket
            .close(Some(CloseFrame {
                code: CloseCode::from(4001),
                reason: "done".into(),
            }))
            .await;
        while socket.next().await.is_some() {}
    }))
    .await;
    let mut io = open_ws(&h.api, ws(&server.url("/")))
        .await
        .expect("ws open");
    let events = io.events(T).await;
    let shapes: Vec<String> = events.iter().map(shape).collect();
    assert_eq!(shapes[1], shape(&received_text("hello")), "{shapes:?}");
    assert!(
        matches!(events.last(), Some(RealtimeEvent::Closed { code: Some(4001), reason: Some(r) }) if r == "done"),
        "{shapes:?}"
    );
}

#[tokio::test]
async fn ws_subprotocol_is_negotiated_and_an_unrequested_choice_fails() {
    let h = Harness::new().await;
    let chat = WsServer::spawn(echo(), None, Some("chat")).await;
    let mut io = open_ws(
        &h.api,
        WsOpen {
            subprotocols: vec!["chat".into(), "v2".into()],
            ..ws(&chat.url("/"))
        },
    )
    .await
    .expect("ws open");
    match io.next_event(T).await.expect("open") {
        RealtimeEvent::Open { protocol } => assert_eq!(protocol.as_deref(), Some("chat")),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        seen_header(&chat.requests()[0], "sec-websocket-protocol"),
        Some("chat, v2")
    );
    io.close();
    let other = WsServer::spawn(echo(), None, Some("other")).await;
    let mut io = open_ws(
        &h.api,
        WsOpen {
            subprotocols: vec!["chat".into()],
            ..ws(&other.url("/"))
        },
    )
    .await
    .expect("ws open");
    let events = io.events(T).await;
    assert!(
        matches!(events.first(), Some(RealtimeEvent::Failed { error }) if error.code == "api.stream_rejected"),
        "{events:?}"
    );
}

/// (이름, 요청, 오류 코드, 기대 detail의 (index, field)).
type Case = (
    &'static str,
    WsOpen,
    &'static str,
    Option<(u64, &'static str)>,
);

#[tokio::test]
async fn ws_invalid_header_is_rejected_before_connecting() {
    let h = Harness::new().await;
    h.secret("NL", "fixture-secret\nInjected: 1").await;
    let server = WsServer::start(echo()).await;
    let base = || ws(&server.url("/cv"));
    let with = |headers: Vec<HeaderRow>| WsOpen { headers, ..base() };
    let cases: Vec<Case> = vec![
        (
            "① name with spaces",
            with(vec![header(0, "X API Key", "1")]),
            "api.invalid_header",
            Some((0, "name")),
        ),
        (
            "② newline in value",
            with(vec![header(0, "X-Ok", "a\nb")]),
            "api.invalid_header",
            Some((0, "value")),
        ),
        (
            "③ substituted value",
            WsOpen {
                env: EnvSnapshot {
                    variables: vec![var("bad", "x\r\nInjected: 1", VarOrigin::Environment)],
                },
                ..with(vec![header(0, "X-Ok", "{{bad}}")])
            },
            "api.invalid_header",
            Some((0, "value")),
        ),
        (
            "④ resolved secret value",
            with(vec![header(2, "Authorization", "Bearer {{secret:NL}}")]),
            "api.invalid_header",
            Some((2, "value")),
        ),
        (
            "⑤ handshake header",
            with(vec![header(1, "Sec-WebSocket-Key", "x")]),
            "api.invalid_header",
            Some((1, "name")),
        ),
        (
            "⑥ host header",
            with(vec![header(0, "Host", "evil.test")]),
            "api.invalid_header",
            Some((0, "name")),
        ),
        (
            "⑦ subprotocol token",
            WsOpen {
                subprotocols: vec!["a b".into()],
                ..base()
            },
            "api.invalid_input",
            None,
        ),
        (
            "⑧ http scheme",
            ws(&format!("http://{}/", server.addr)),
            "api.invalid_url",
            None,
        ),
        (
            "⑨ userinfo",
            ws(&format!("ws://u:p@{}/", server.addr)),
            "api.invalid_url",
            None,
        ),
        (
            "⑩ missing variable",
            ws(&format!("ws://{}/{{{{nope}}}}", server.addr)),
            "api.variable_missing",
            None,
        ),
    ];
    for (case, open, code, at) in cases {
        let err = match open_ws(&h.api, open).await {
            Ok(_) => panic!("{case}: must be rejected before connecting"),
            Err(err) => err,
        };
        assert_eq!(err.code(), code, "{case}");
        if let Some((index, field)) = at {
            assert_eq!(err.detail()["index"], index, "{case}");
            assert_eq!(err.detail()["field"], field, "{case}");
        }
        let text = err.detail_text();
        for leaked in ["fixture-secret", "Injected", "evil.test", "u:p"] {
            assert!(!text.contains(leaked), "{case}: detail leaks {leaked}");
        }
    }
    assert_eq!(server.accepted(), 0, "no connection reached the fixture");
}

#[tokio::test]
async fn ws_composed_references_mixed_origins_and_captures_follow_send_rules() {
    let h = Harness::new().await;
    h.secret("GH", "gh-fixture-value").await;
    let server = WsServer::start(echo()).await;
    let base = || ws(&server.url("/"));
    let composed = WsOpen {
        headers: vec![header(0, "X-Token", "{{a}}GH}}")],
        env: EnvSnapshot {
            variables: vec![var("a", "{{secret:", VarOrigin::Environment)],
        },
        ..base()
    };
    let err = open_ws(&h.api, composed)
        .await
        .err()
        .expect("composed reference");
    assert_eq!(err.code(), "api.invalid_input");
    let mixed = WsOpen {
        headers: vec![header(0, "X-Token", "{{t}}")],
        env: EnvSnapshot {
            variables: vec![
                var("t", "{{secret:GH}}", VarOrigin::Environment),
                var("t", "captured", VarOrigin::Capture),
            ],
        },
        ..base()
    };
    let err = open_ws(&h.api, mixed).await.err().expect("mixed origin");
    assert_eq!(err.code(), "api.invalid_input");
    assert_eq!(server.accepted(), 0);
    // 캡처 값은 참조 모양이어도 글자 그대로 보낸다(비밀로 풀지 않는다).
    let captured = WsOpen {
        headers: vec![header(0, "X-Token", "{{t}}")],
        env: EnvSnapshot {
            variables: vec![var("t", "{{secret:GH}}", VarOrigin::Capture)],
        },
        ..base()
    };
    let mut io = open_ws(&h.api, captured).await.expect("captured literal");
    first_open(&mut io).await;
    assert_eq!(
        seen_header(&server.requests()[0], "x-token"),
        Some("{{secret:GH}}")
    );
    io.close();
}

#[tokio::test]
async fn ws_secret_is_sent_in_the_handshake_and_redacted_in_every_event() {
    let h = Harness::new().await;
    h.secret("GH", "gh-fixture-value").await;
    let server = WsServer::start(behavior(|mut socket: WsSocket, _| async move {
        let _ = socket.send(Message::text("token=gh-fixture-value")).await;
        let _ = socket
            .send(Message::binary(b"bin gh-fixture-value".to_vec()))
            .await;
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Text(text) = message {
                let _ = socket.send(Message::text(format!("echo {text}"))).await;
                let _ = socket
                    .close(Some(CloseFrame {
                        code: CloseCode::Normal,
                        reason: "bye gh-fixture-value".into(),
                    }))
                    .await;
            }
        }
    }))
    .await;
    let mut io = open_ws(
        &h.api,
        WsOpen {
            headers: vec![header(0, "Authorization", "Bearer {{secret:GH}}")],
            ..ws(&server.url("/s"))
        },
    )
    .await
    .expect("ws open");
    first_open(&mut io).await;
    io.send(WsInput::Text {
        data: "mine gh-fixture-value".into(),
    })
    .await;
    let events = io.events(T).await;
    assert_eq!(
        seen_header(&server.requests()[0], "authorization"),
        Some("Bearer gh-fixture-value"),
        "the handshake carries the resolved secret"
    );
    let text = format!("{events:?}");
    assert!(
        !text.contains("gh-fixture-value"),
        "events leak the secret: {text}"
    );
    assert!(text.contains("[REDACTED]"), "{text}");
    let binary = events.iter().find_map(|e| match e {
        RealtimeEvent::Message {
            data_base64: Some(b64),
            ..
        } => Some(data_encoding::BASE64.decode(b64.as_bytes()).expect("b64")),
        _ => None,
    });
    assert_eq!(binary.as_deref(), Some(b"bin [REDACTED]".as_slice()));
    assert!(
        h.history_files().is_empty(),
        "no history file for realtime sessions"
    );
}

fn tls(ca: Option<String>, cert: Option<String>, key: Option<String>, verify: bool) -> TlsOptions {
    TlsOptions {
        verify,
        ca_path: ca,
        client_cert_path: cert,
        client_key_path: key,
    }
}

/// C 표 이식(`tls_stream_configuration_is_not_silently_ignored`): v0.9.0은 스트림 TLS 설정을 거절했고
/// v1은 wss에도 CA·검증 끔·클라이언트 인증서를 그대로 적용한다. 잘못된 재료는 연결 전에 거절한다.
#[tokio::test]
async fn tls_stream_configuration_is_not_silently_ignored() {
    let h = Harness::new().await;
    let pki = Pki::new();
    let server = WsServer::spawn(echo(), Some(pki.server(false)), None).await;
    let url = server.url("/tls");
    let ca = pki.write("ca.pem", &pki.ca_pem);
    let broken = pki.write(
        "broken.pem",
        "-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----\n",
    );
    let open = |tls: TlsOptions| WsOpen { tls, ..ws(&url) };

    let err = open_ws(&h.api, open(tls(Some(broken), None, None, true)))
        .await
        .err()
        .expect("bad CA");
    assert_eq!(err.code(), "api.tls_invalid");
    let err = open_ws(
        &h.api,
        open(tls(
            None,
            Some(pki.write("c.pem", &pki.client_cert_pem)),
            None,
            true,
        )),
    )
    .await
    .err()
    .expect("half identity");
    assert_eq!(err.code(), "api.tls_invalid");
    assert_eq!(server.accepted(), 0, "rejected before connecting");

    // 지정 CA를 쓰면 연결된다.
    let mut io = open_ws(&h.api, open(tls(Some(ca.clone()), None, None, true)))
        .await
        .expect("ca");
    first_open(&mut io).await;
    io.close();
    // CA 없이 검증하면 소유 CA를 믿지 않는다.
    let mut io = open_ws(&h.api, open(tls(None, None, None, true)))
        .await
        .expect("open");
    let events = io.events(T).await;
    assert!(
        matches!(events.first(), Some(RealtimeEvent::Failed { error }) if error.code == "api.network"),
        "{events:?}"
    );
    // 검증 끔은 그대로 적용된다.
    let mut io = open_ws(&h.api, open(tls(None, None, None, false)))
        .await
        .expect("verify off");
    first_open(&mut io).await;
    io.close();

    // 클라이언트 인증서를 요구하는 서버: 지정한 인증서를 보낸다.
    let mutual = WsServer::spawn(echo(), Some(pki.server(true)), None).await;
    let identity = tls(
        Some(ca.clone()),
        Some(pki.write("client.pem", &pki.client_cert_pem)),
        Some(pki.write("client.key", &pki.client_key_pem)),
        true,
    );
    let mut io = open_ws(
        &h.api,
        WsOpen {
            tls: identity,
            ..ws(&mutual.url("/m"))
        },
    )
    .await
    .expect("mtls");
    first_open(&mut io).await;
    assert!(
        mutual.state.client_cert.load(Ordering::SeqCst),
        "client certificate was presented"
    );
    io.close();
    let mut io = open_ws(
        &h.api,
        WsOpen {
            tls: tls(Some(ca), None, None, true),
            ..ws(&mutual.url("/m"))
        },
    )
    .await
    .expect("no identity");
    let events = io.events(T).await;
    assert!(
        matches!(events.first(), Some(RealtimeEvent::Failed { .. })),
        "{events:?}"
    );
}
