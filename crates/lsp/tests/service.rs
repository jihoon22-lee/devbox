#![cfg(unix)]
mod support;
use devbox_lsp::{Language, OpenLsp};
use serde_json::json;
use support::*;
#[tokio::test]
async fn two_clients_share_one_owned_process_and_stream_close_preserves_sibling() {
    let fx = Fixture::new();
    let p = OpenLsp {
        project_id: "p".into(),
        language: Language::Rust,
    };
    let (a, b) = tokio::join!(
        fx.service.open(fx.ctx(), p.clone(), fx.project()),
        fx.service.open(fx.ctx(), p, fx.project())
    );
    let mut a = fx.client(a.expect("first LSP").stream_id);
    let mut b = fx.client(b.expect("second LSP").stream_id);
    a.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .await;
    b.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .await;
    let ar = a.reply().await;
    let br = b.reply().await;
    assert_eq!(ar["result"]["pid"], br["result"]["pid"]);
    assert_eq!(br["result"]["initializeCount"], 1);
    a.send(json!({"jsonrpc":"2.0","id":"same","method":"test/echo","params":{"client":"a"}}))
        .await;
    b.send(json!({"jsonrpc":"2.0","id":"same","method":"test/echo","params":{"client":"b"}}))
        .await;
    assert_eq!(a.reply().await["result"]["client"], "a");
    assert_eq!(b.reply().await["result"]["client"], "b");
    a.close();
    b.send(json!({"jsonrpc":"2.0","id":9,"method":"test/echo","params":"alive"}))
        .await;
    assert_eq!(b.reply().await["result"], "alive");
    fx.service.shutdown().await;
}
#[tokio::test]
async fn exhausted_credit_closes_only_that_session_and_restart_changes_generation() {
    let fx = Fixture::new();
    let p = OpenLsp {
        project_id: "p".into(),
        language: Language::Rust,
    };
    let mut slow = fx.client(
        fx.service
            .open(fx.ctx(), p.clone(), fx.project())
            .await
            .expect("slow")
            .stream_id,
    );
    let mut live = fx.client(
        fx.service
            .open(fx.ctx(), p.clone(), fx.project())
            .await
            .expect("live")
            .stream_id,
    );
    for client in [&mut slow, &mut live] {
        client
            .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
            .await;
        client.reply().await;
    }
    slow.send(json!({"jsonrpc":"2.0","id":2,"method":"test/large"}))
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(3), slow.closed.cancelled())
        .await
        .expect("credit fail closed");
    live.send(json!({"jsonrpc":"2.0","id":2,"method":"test/echo","params":"live"}))
        .await;
    assert_eq!(live.reply().await["result"], "live");
    let before_info = fx.service.servers("p").await.items[0].clone();
    let before = before_info.generation;
    fx.service
        .restart(p.clone(), fx.project())
        .await
        .expect("manual restart");
    tokio::time::timeout(std::time::Duration::from_secs(2), live.closed.cancelled())
        .await
        .expect("old generation closed");
    assert!(fx.service.servers("p").await.items[0].generation > before);
    let mut next = fx.client(
        fx.service
            .open(fx.ctx(), p, fx.project())
            .await
            .expect("new generation")
            .stream_id,
    );
    next.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .await;
    let result = next.reply().await;
    assert_eq!(result["result"]["initializeCount"], 1);
    assert_ne!(
        result["result"]["pid"].as_u64(),
        before_info.pid.map(u64::from)
    );
    fx.service.shutdown().await;
    assert!(
        !std::path::Path::new(&format!(
            "/proc/{}",
            result["result"]["pid"].as_u64().expect("pid")
        ))
        .exists()
    );
}

#[tokio::test]
async fn abnormal_exit_closes_old_stream_and_restarts_owned_server() {
    let fx = Fixture::new();
    let p = OpenLsp {
        project_id: "p".into(),
        language: Language::Rust,
    };
    let mut client = fx.client(
        fx.service
            .open(fx.ctx(), p.clone(), fx.project())
            .await
            .expect("open")
            .stream_id,
    );
    client
        .send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .await;
    client.reply().await;
    let before = fx.service.servers("p").await.items[0].clone();
    client
        .send(json!({"jsonrpc":"2.0","method":"test/crash"}))
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(2), client.closed.cancelled())
        .await
        .expect("crashed stream");
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            if fx.service.servers("p").await.items[0].generation > before.generation {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("backoff restart");
    let mut next = fx.client(
        fx.service
            .open(fx.ctx(), p, fx.project())
            .await
            .expect("reopen")
            .stream_id,
    );
    next.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .await;
    let reply = next.reply().await;
    assert_eq!(reply["result"]["initializeCount"], 1);
    fx.service.shutdown().await;
}
