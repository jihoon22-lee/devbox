//! GraphQL 송신·엔드포인트 검사·취소(v0.9.0 http-client-engine 이식).
mod support;
use devbox_api::*;
use std::time::Duration;
use support::api::*;
use support::{Reply, Server};

fn gql(query: &str, variables: &str, name: Option<&str>, method: GraphqlMethod) -> Body {
    Body::Graphql {
        query: query.into(),
        variables: variables.into(),
        operation_name: name.map(Into::into),
        method,
    }
}

#[tokio::test]
async fn graphql_post_sends_standard_body_and_projects_data_and_errors_without_secret_echo() {
    let api = Harness::new().await;
    let server = Server::start(support::handler(|_| async {
        Reply::ok(r#"{"data":{"viewer":{"id":"42","echo":"query-secret"}},"errors":[{"message":"token leaked","path":["viewer",0],"extensions":{"token":"graphql-secret"}}]}"#)
            .header("content-type", "application/json")
    }))
    .await;
    let events = api
        .complete(with_env(
            RequestSnapshot {
                headers: vec![
                    header(0, "Authorization", "Bearer {{gql}}"),
                    header(1, "Content-Type", "text/plain"),
                ],
                body: gql(
                    "query Viewer { viewer(token: \"{{q}}\") { id echo } }",
                    r#"{"id":"42"}"#,
                    Some("Viewer"),
                    GraphqlMethod::Post,
                ),
                ..snapshot("POST", &server.url("/graphql"))
            },
            &[("gql", "graphql-secret", true), ("q", "query-secret", true)],
        ))
        .await;
    let text = String::from_utf8(body(&events)).expect("utf8");
    assert!(!text.contains("graphql-secret") && !text.contains("query-secret"));
    assert!(text.contains("token leaked") && text.contains("\"42\""));
    let seen = server.only();
    assert!(seen.raw().starts_with("POST /graphql HTTP/1.1"));
    assert_eq!(seen.header("content-type"), ["application/json"]);
    let observed = seen.body_text();
    assert!(observed.contains("\"operationName\":\"Viewer\""));
    assert!(observed.contains("\"query\":\"query Viewer"));
    assert!(observed.contains("\"variables\":{\"id\":\"42\"}"));
}

#[tokio::test]
async fn graphql_get_uses_encoded_query_parameters_without_request_body() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    api.complete(request(RequestSnapshot {
        query: vec![pair("filter", "a&b")],
        body: gql("{ viewer { id } }", "", None, GraphqlMethod::Get),
        ..snapshot("GET", &server.url("/graphql"))
    }))
    .await;
    let seen = server.only();
    assert!(
        seen.target.starts_with("/graphql?filter=a%26b&query="),
        "{}",
        seen.target
    );
    assert!(seen.target.contains("&variables=%7B%7D"));
    assert!(seen.body.is_empty());
    assert!(seen.header("content-type").is_empty());
}

#[tokio::test]
async fn graphql_endpoint_rejects_userinfo_fragment_and_credential_query_before_connecting() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let addr = server.addr;
    for (url, query, reason) in [
        (
            format!("http://user:password@{addr}/graphql"),
            vec![],
            "userinfo",
        ),
        (
            format!("http://{addr}/graphql?access_token=literal"),
            vec![],
            "credential_query",
        ),
        (
            format!("http://{addr}/graphql"),
            vec![pair("access_token", "raw")],
            "credential_query",
        ),
        (
            format!("http://{addr}/graphql#fragment"),
            vec![],
            "fragment",
        ),
    ] {
        let err = api
            .send(request(RequestSnapshot {
                query,
                body: gql("{ viewer { id } }", "", None, GraphqlMethod::Post),
                ..snapshot("POST", &url)
            }))
            .await
            .err()
            .expect("rejected");
        assert_eq!(
            (err.code(), err.detail()["reason"].as_str()),
            ("api.invalid_url", Some(reason)),
            "{url}"
        );
        assert!(!err.detail_text().contains("password") && !err.detail_text().contains("literal"));
    }
    assert_eq!(server.accepted_connections(), 0);
}

#[tokio::test]
async fn graphql_preflight_rejects_header_and_query_bounds_without_network() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let base = || RequestSnapshot {
        body: gql("{ viewer { id } }", "", None, GraphqlMethod::Post),
        ..snapshot("POST", &server.url("/graphql"))
    };
    let too_many = (0..=100)
        .map(|i| header(i, &format!("x-{i}"), "ok"))
        .collect();
    let err = api
        .send(request(RequestSnapshot {
            headers: too_many,
            ..base()
        }))
        .await
        .err()
        .expect("rows");
    assert_eq!(
        (err.code(), err.detail()["limit"].as_u64()),
        ("api.too_large", Some(100))
    );
    let large = vec![header(0, "x-large", &"x".repeat(64 * 1024))];
    let err = api
        .send(request(RequestSnapshot {
            headers: large,
            ..base()
        }))
        .await
        .err()
        .expect("bytes");
    assert_eq!(
        (err.code(), err.detail()["limit"].as_u64()),
        ("api.too_large", Some(64 * 1024))
    );
    let err = api
        .send(request(RequestSnapshot {
            headers: vec![header(2, "X Bad", "1")],
            ..base()
        }))
        .await
        .err()
        .expect("CV-F05");
    assert_eq!(
        (err.code(), err.detail()["index"].as_u64()),
        ("api.invalid_header", Some(2))
    );
    let err = api
        .send(request(RequestSnapshot {
            body: gql("{ a }", "[1]", None, GraphqlMethod::Post),
            ..base()
        }))
        .await
        .err()
        .expect("variables");
    assert_eq!(
        (err.code(), err.detail()["reason"].as_str()),
        ("api.invalid_body", Some("graphql_variables"))
    );
    assert_eq!(server.accepted_connections(), 0);
}

#[tokio::test]
async fn graphql_cancellation_stops_before_network_io() {
    let api = Harness::new().await;
    let server = Server::ok().await;
    let sent = api
        .send(request(RequestSnapshot {
            body: gql("{ viewer { id } }", "", None, GraphqlMethod::Post),
            ..snapshot("POST", &server.url("/graphql"))
        }))
        .await
        .expect("open");
    sent.close();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(server.accepted_connections(), 0);
}

#[tokio::test]
async fn graphql_cancellation_interrupts_an_in_flight_request() {
    let api = Harness::new().await;
    let server = Server::start(support::handler(|_| async {
        Reply::ok("{}").delayed(Duration::from_secs(30))
    }))
    .await;
    let mut sent = api
        .send(request(RequestSnapshot {
            body: gql("{ viewer { id } }", "", None, GraphqlMethod::Post),
            ..snapshot("POST", &server.url("/graphql"))
        }))
        .await
        .expect("open");
    assert!(
        server
            .wait(|s| s.requests().len() == 1, Duration::from_secs(5))
            .await
    );
    let at = std::time::Instant::now();
    sent.close();
    assert!(
        server
            .wait(|s| s.closed_connections() == 1, Duration::from_millis(500))
            .await
    );
    assert!(at.elapsed() < Duration::from_millis(500));
    while let Some(event) = sent.next(Duration::from_millis(100)).await {
        assert!(!matches!(
            event,
            SendEvent::End { .. } | SendEvent::Head { .. }
        ));
    }
}
