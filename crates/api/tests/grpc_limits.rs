//! gRPC 응답 상한(S5-5 검토 1): 응답 메시지 4MiB 디코딩 상한, 서버 스트림 10,000개 상한.
mod support;
use devbox_api::*;
use serde_json::Value;
use std::time::Duration;
use support::api::*;
use support::grpc::*;
use support::lines::{Lines, open};

const T: Duration = Duration::from_secs(20);

fn call(server: &GrpcServer, method: &str, json: &str, proto: &str) -> GrpcCall {
    GrpcCall {
        target: server.target(),
        method: method.into(),
        json: json.into(),
        metadata: vec![],
        tls: TlsOptions::default(),
        env: EnvSnapshot::default(),
        timeout_ms: 15_000,
        proto_files: Some(vec![proto.to_owned()]),
        origin: None,
    }
}

async fn start_call(h: &Harness, p: GrpcCall) -> Result<Lines, Rpc> {
    open(|ctx| h.api.grpc_call(ctx, p)).await
}

fn count(events: &[Value], kind: &str) -> usize {
    events.iter().filter(|e| e["type"] == kind).count()
}

#[tokio::test]
async fn grpc_responses_over_the_decode_limit_end_the_call() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let json = format!(r#"{{"name":"big","count":{}}}"#, 4 * 1024 * 1024 + 1);
    let mut lines = start_call(&h, call(&server, "demo.Echo/Unary", &json, &path))
        .await
        .expect("call");
    let events = lines.all(T).await;
    assert_eq!(count(&events, "message"), 0, "{events:?}");
    let end = events
        .iter()
        .find(|e| e["type"] == "trailers")
        .expect("trailers");
    // tonic 디코딩 상한(4MiB)은 OUT_OF_RANGE(11)로 끝난다. 응답 본문은 화면에 오지 않는다.
    assert_eq!(end["code"], 11, "{end}");
    assert!(
        end["message"]
            .as_str()
            .is_some_and(|m| m.contains("4194304")),
        "{end}"
    );
}

#[tokio::test]
async fn grpc_server_streams_stop_after_ten_thousand_messages() {
    let h = Harness::new().await;
    let dir = tempfile::tempdir().expect("owned fixture");
    let (path, _) = proto_file(dir.path());
    let server = GrpcServer::start(None).await;
    let mut lines = start_call(
        &h,
        call(
            &server,
            "demo.Echo/Server",
            r#"{"name":"m","count":10001}"#,
            &path,
        ),
    )
    .await
    .expect("call");
    let events = lines.all(T).await;
    assert_eq!(count(&events, "message"), 10_000);
    let failed = events
        .iter()
        .find(|e| e["type"] == "failed")
        .expect("failed");
    assert_eq!(failed["error"]["code"], "api.too_large");
    assert_eq!(failed["error"]["detail"]["limit"], 10_000);
    assert_eq!(count(&events, "trailers"), 0);
}
