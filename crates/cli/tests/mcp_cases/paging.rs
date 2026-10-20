use super::{Mcp, TestDaemon, body};
use devbox_protocol::{
    frame::FrameCodec,
    message::{ClientMessage, ServerMessage, json_frame, parse_json},
};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::codec::Framed;

type Peer = Framed<tokio::net::UnixStream, FrameCodec>;
async fn accept(listener: &tokio::net::UnixListener) -> Peer {
    let (stream, _) = listener.accept().await.unwrap();
    let mut peer = Framed::new(stream, FrameCodec::default());
    let hello = peer.next().await.unwrap().unwrap();
    assert!(matches!(
        parse_json::<ClientMessage>(&hello).unwrap(),
        ClientMessage::Hello { .. }
    ));
    peer.send(json_frame(
        0,
        &ServerMessage::Welcome {
            version: devbox_cli::VERSION.into(),
            protocol: devbox_protocol::PROTOCOL_VERSION,
            daemon_id: "owned-page-fixture".into(),
        },
    ))
    .await
    .unwrap();
    peer
}
async fn request(peer: &mut Peer, expected: &str) -> (u32, Value) {
    let frame = peer.next().await.unwrap().unwrap();
    let ClientMessage::Request {
        id, method, params, ..
    } = parse_json::<ClientMessage>(&frame).unwrap()
    else {
        panic!("expected RPC")
    };
    assert_eq!(method, expected);
    (id, serde_json::from_str(params.get()).unwrap())
}
async fn reply(peer: &mut Peer, id: u32, result: Value) {
    peer.send(json_frame(
        0,
        &ServerMessage::Response {
            id,
            result: Some(serde_json::value::to_raw_value(&result).unwrap()),
            error: None,
            diagnostic_id: None,
        },
    ))
    .await
    .unwrap();
}
fn cursor(offset: u64) -> Value {
    json!({"sources":[{"source":{"kind":"run","runId":"owned"},"identity":"owned", "stream":"stdout","offset":offset}]})
}
fn line(index: u32) -> Value {
    json!({"src":{"kind":"run","runId":"owned"},"ts":"2026-10-20T01:02:03.004Z","stream":"stdout","text":format!("line-{index:03}")})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn logs_continue_empty_and_short_pages_without_losing_recent_lines() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).unwrap();
    let task = tokio::spawn(async move {
        let mut peer = accept(&listener).await;
        let (id, args) = request(&mut peer, "logs.search").await;
        assert_eq!(args["limit"], 256);
        assert!(args.get("before").is_none());
        reply(&mut peer, id, json!({"items":[],"nextCursor":cursor(1000)})).await;
        let (id, args) = request(&mut peer, "logs.search").await;
        assert_eq!(args["before"], cursor(1000));
        assert_eq!(args["limit"], 256);
        reply(&mut peer,id,json!({"items":(400..500).rev().map(line).collect::<Vec<_>>(),"nextCursor":cursor(900)})).await;
        let (id, args) = request(&mut peer, "logs.search").await;
        assert_eq!(args["before"], cursor(900));
        assert_eq!(args["limit"], 256);
        reply(&mut peer,id,json!({"items":(144..400).rev().map(line).collect::<Vec<_>>(),"nextCursor":cursor(500)})).await;
        let (id, args) = request(&mut peer, "logs.search").await;
        assert_eq!(args["before"], cursor(500));
        assert_eq!(args["limit"], 144);
        reply(
            &mut peer,
            id,
            json!({"items":(0..144).rev().map(line).collect::<Vec<_>>(),"nextCursor":null}),
        )
        .await;
        assert!(peer.next().await.is_none());
    });
    let mut m = Mcp::start(&d);
    let result = body(&m.tool("devbox_logs", json!({"runId":"owned","lines":500})));
    let items = result["items"].as_array().unwrap();
    assert_eq!(items.len(), 500);
    for (i, item) in items.iter().enumerate() {
        assert_eq!(item["text"], format!("line-{i:03}"));
    }
    assert_eq!(result["truncated"], false);
    m.finish();
    task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_empty_cursor_is_bounded_by_page_budget() {
    let d = TestDaemon::fixture();
    let listener = tokio::net::UnixListener::bind(&d.socket).unwrap();
    let task = tokio::spawn(async move {
        let mut peer = accept(&listener).await;
        for _ in 0..64 {
            let (id, _) = request(&mut peer, "logs.search").await;
            reply(&mut peer, id, json!({"items":[],"nextCursor":cursor(100)})).await;
        }
        assert!(peer.next().await.is_none());
    });
    let mut m = Mcp::start(&d);
    let result = body(&m.tool("devbox_logs", json!({"runId":"owned","lines":500})));
    assert_eq!(result["items"], json!([]));
    assert_eq!(result["truncated"], true);
    m.finish();
    task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timed_out_write_is_not_retried_and_logs_timeout_closes_scan() {
    for tool in ["devbox_run_task", "devbox_logs"] {
        let d = TestDaemon::fixture();
        d.write_config("[mcp]\nwrite=true\n");
        let listener = tokio::net::UnixListener::bind(&d.socket).unwrap();
        let expected = if tool == "devbox_run_task" {
            "runtime.start"
        } else {
            "logs.search"
        };
        let task = tokio::spawn(async move {
            let mut peer = accept(&listener).await;
            request(&mut peer, expected).await;
            assert!(
                tokio::time::timeout(Duration::from_secs(7), peer.next())
                    .await
                    .unwrap()
                    .is_none()
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(200), listener.accept())
                    .await
                    .is_err(),
                "must not reconnect or retry"
            );
        });
        let mut m = Mcp::start(&d);
        let args = if tool == "devbox_run_task" {
            json!({"projectId":"owned","id":"task"})
        } else {
            json!({"runId":"owned","lines":500})
        };
        let result = m.tool(tool, args);
        assert_eq!(result["result"]["isError"], true);
        assert!(
            result["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("시간이 초과")
        );
        m.finish();
        task.await.unwrap();
    }
}
