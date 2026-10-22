//! 3c 선행 결정: 응답에서 캡처한 값은 글자 그대로 쓰고 `{{secret:X}}` 참조로 풀지 않는다.
//! 악성 서버가 응답에 참조를 넣어 다음 요청에서 비밀을 끌어내지 못하게 한다.
mod support;
use devbox_api::*;
use support::Server;
use support::api::*;

const SECRET: &str = "gh-secret-value";

fn headers(server: &Server, rows: &[(&str, &str)]) -> RequestSnapshot {
    RequestSnapshot {
        headers: rows
            .iter()
            .enumerate()
            .map(|(i, (n, v))| header(i as u32, n, v))
            .collect(),
        ..snapshot("POST", &server.url("/next"))
    }
}

#[tokio::test]
async fn captured_secret_reference_is_sent_as_literal_text() {
    let api = Harness::new().await;
    api.secret("GH", SECRET).await;
    let server = Server::echo().await;
    // ① 캡처 값 전체가 참조 ② 같은 요청에 글자 그대로의 참조(정당한 사용)가 함께 있음
    // ③ 캡처 두 개를 이어 참조 모양을 만듦, 그리고 본문 안의 캡처 값
    let cases: Vec<(&str, SendRequest, &str)> = vec![
        (
            "① exact",
            with_captured(
                headers(&server, &[("X-Echo", "{{token}}")]),
                &[("token", "{{secret:GH}}", true)],
            ),
            "{{secret:GH}}",
        ),
        (
            "② with literal reference",
            with_captured(
                headers(
                    &server,
                    &[
                        ("Authorization", "Bearer {{secret:GH}}"),
                        ("X-Echo", "{{token}}"),
                    ],
                ),
                &[("token", "{{secret:GH}}", true)],
            ),
            "{{secret:GH}}",
        ),
        (
            "③ composed captures",
            with_captured(
                headers(
                    &server,
                    &[
                        ("Authorization", "Bearer {{secret:GH}}"),
                        ("X-Echo", "{{a}}{{b}}"),
                    ],
                ),
                &[("a", "{{secret:", true), ("b", "GH}}", true)],
            ),
            "{{secret:GH}}",
        ),
    ];
    for (case, send, expected) in cases {
        let before = server.requests().len();
        api.complete(send).await;
        let seen = server.requests().remove(before);
        assert_eq!(seen.header("x-echo"), [expected], "{case}");
        assert!(!seen.header("x-echo").concat().contains(SECRET), "{case}");
    }
    // 본문에 넣은 캡처 값도 글자 그대로다.
    let before = server.requests().len();
    api.complete(with_captured(
        RequestSnapshot {
            body: Body::Json {
                text: r#"{"t":"{{token}}"}"#.into(),
            },
            ..headers(&server, &[])
        },
        &[("token", "{{secret:GH}}", true)],
    ))
    .await;
    let seen = server.requests().remove(before);
    assert_eq!(seen.body_text(), r#"{"t":"{{secret:GH}}"}"#);
}

/// 환경 값이 앞쪽 `{{secret:`를, 캡처 값이 나머지를 채우면 참조가 아니라 잘못된 참조로 거절한다.
#[tokio::test]
async fn capture_cannot_complete_an_environment_reference() {
    let api = Harness::new().await;
    api.secret("GH", SECRET).await;
    let server = Server::echo().await;
    let err = api
        .send(with_captured(
            headers(
                &server,
                &[
                    ("Authorization", "Bearer {{secret:GH}}"),
                    ("X-Echo", "{{o}}{{c}}"),
                ],
            ),
            &[("o", "{{secret:", false), ("c", "GH}}", true)],
        ))
        .await
        .err()
        .expect("broken reference");
    assert_eq!(err.code(), "api.invalid_input");
    assert!(!err.detail_text().contains(SECRET));
    assert_eq!(server.accepted_connections(), 0);
}

/// 캡처 값의 개행은 헤더 검사(CV-F05)에 그대로 걸린다(자리 표시 상태로 검사하지 않음).
#[tokio::test]
async fn captured_value_is_validated_after_substitution() {
    let api = Harness::new().await;
    let server = Server::echo().await;
    let err = api
        .send(with_captured(
            headers(&server, &[("X-Echo", "{{token}}")]),
            &[("token", "a\r\nInjected: 1", true)],
        ))
        .await
        .err()
        .expect("invalid header");
    assert_eq!(err.code(), "api.invalid_header");
    assert_eq!(server.accepted_connections(), 0);
}

/// 검토 권장(3c): 같은 이름이 환경과 캡처 두 출처로 오면 어느 쪽이 이기든 위험하다
/// (환경 쪽 전체 참조가 이기면 비밀로 풀린다). 순서와 상관없이 연결 전에 거절한다.
#[tokio::test]
async fn mixed_origin_duplicate_names_are_rejected() {
    let api = Harness::new().await;
    api.secret("GH", SECRET).await;
    let server = Server::echo().await;
    for order in [[false, true], [true, false]] {
        let err = api
            .send(with_captured(
                headers(&server, &[("X-Echo", "{{token}}")]),
                &[
                    ("token", "{{secret:GH}}", order[0]),
                    ("token", "{{secret:GH}}", order[1]),
                ],
            ))
            .await
            .err()
            .expect("mixed origins");
        assert_eq!(
            (err.code(), err.detail()["reason"].as_str()),
            ("api.invalid_input", Some("mixed_origin"))
        );
        assert!(!err.detail_text().contains(SECRET));
    }
    assert_eq!(server.accepted_connections(), 0);
    // 출처가 같은 중복 이름은 기존대로 뒤의 값이 이긴다(화면은 이름당 하나만 보낸다).
    for (origin, expected) in [(false, SECRET), (true, "second")] {
        let before = server.requests().len();
        api.complete(with_captured(
            headers(&server, &[("X-Echo", "{{token}}")]),
            &[
                ("token", "first", origin),
                (
                    "token",
                    if origin { "second" } else { "{{secret:GH}}" },
                    origin,
                ),
            ],
        ))
        .await;
        assert_eq!(
            server.requests().remove(before).header("x-echo"),
            [expected]
        );
    }
}

/// 검토 권장(3c): 캡처 값은 화면이 `sensitive: false`로 보내도 기록에서 가린다.
#[tokio::test]
async fn captured_values_are_masked_in_history_regardless_of_the_flag() {
    let api = Harness::new().await;
    let server = Server::echo().await;
    let mut send = with_captured(
        RequestSnapshot {
            body: Body::Json {
                text: r#"{"t":"{{token}}"}"#.into(),
            },
            ..snapshot("POST", &format!("{}?t={{{{token}}}}", server.url("/p")))
        },
        &[("token", "captured-token-value", true)],
    );
    send.env.variables[0].sensitive = false;
    api.complete(send).await;
    assert_eq!(server.only().body_text(), r#"{"t":"captured-token-value"}"#);
    api.only_history().await;
    let bytes = api.history_bytes();
    assert!(!contains(&bytes, "captured-token-value"));
    assert!(contains(&bytes, "[REDACTED]"));
}
