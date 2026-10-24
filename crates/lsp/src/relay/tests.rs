use super::*;
use crate::adapters::DocumentGrant;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
fn packet(value: Value) -> RelayMessage {
    RelayMessage {
        json: value.to_string(),
        lease_id: None,
    }
}
fn parse(text: &str) -> Value {
    serde_json::from_str(text).expect("JSON")
}
fn access() -> DocumentAccess {
    Arc::new(|uri, lease_id| {
        Box::pin(async move {
            Ok(DocumentGrant {
                uri,
                disk_text: "disk".into(),
                lease_id,
            })
        })
    })
}
async fn initialized() -> Relay {
    let mut r = Relay::new("file:///owned".into(), 1);
    r.attach(1).expect("attach");
    r.attach(2).expect("attach");
    let first = r
        .client(
            1,
            packet(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})),
            &access(),
        )
        .await
        .expect("init");
    assert_eq!(first.server.len(), 1);
    let id = parse(&first.server[0])["id"].clone();
    r.server(
        &json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{}}}).to_string(),
        &access(),
    )
    .await
    .expect("reply");
    r.client(
        2,
        packet(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})),
        &access(),
    )
    .await
    .expect("shared init");
    r
}
#[tokio::test]
async fn two_clients_share_initialize_and_rewrite_colliding_request_ids() {
    let mut r = Relay::new("file:///owned".into(), 7);
    r.attach(1).expect("attach");
    r.attach(2).expect("attach");
    let init = packet(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"rootUri":"file:///outside","capabilities":{"general":{"positionEncodings":["utf-8"]}}}}),
    );
    let first = r.client(1, init.clone(), &access()).await.expect("init");
    assert_eq!(first.server.len(), 1);
    let sent = parse(&first.server[0]);
    assert_eq!(sent["params"]["rootUri"], "file:///owned");
    assert_eq!(
        sent["params"]["capabilities"]["general"]["positionEncodings"],
        json!(["utf-16"])
    );
    assert!(
        r.client(2, init, &access())
            .await
            .expect("wait")
            .server
            .is_empty()
    );
    let out=r.server(&json!({"jsonrpc":"2.0","id":sent["id"],"result":{"capabilities":{"positionEncoding":"utf-16"}}}).to_string(),&access()).await.expect("reply");
    assert_eq!(out.clients.len(), 2);
    assert_eq!(out.server.len(), 1);
    for (client, raw) in &out.clients {
        assert!([1, 2].contains(client));
        assert_eq!(parse(raw)["id"], 1);
    }
    let a = r
        .client(
            1,
            packet(json!({"jsonrpc":"2.0","id":"same","method":"workspace/symbol","params":{}})),
            &access(),
        )
        .await
        .expect("a");
    let b = r
        .client(
            2,
            packet(json!({"jsonrpc":"2.0","id":"same","method":"workspace/symbol","params":{}})),
            &access(),
        )
        .await
        .expect("b");
    let aid = parse(&a.server[0])["id"].clone();
    let bid = parse(&b.server[0])["id"].clone();
    assert_ne!(aid, bid);
    let reply = r
        .server(
            &json!({"jsonrpc":"2.0","id":bid,"result":["two"]}).to_string(),
            &access(),
        )
        .await
        .expect("response");
    assert_eq!(reply.clients[0].0, 2);
    assert_eq!(parse(&reply.clients[0].1)["id"], "same");
    let shutdown = r
        .client(
            1,
            packet(json!({"jsonrpc":"2.0","id":9,"method":"shutdown"})),
            &access(),
        )
        .await
        .expect("shutdown");
    assert!(shutdown.server.is_empty());
    assert_eq!(shutdown.clients[0].0, 1);
}
#[tokio::test]
async fn server_requests_are_bound_to_one_client_and_cancel_disconnect_clear_ids() {
    let mut r = initialized().await;
    let request=r.server(r#"{"jsonrpc":"2.0","id":"config","method":"workspace/configuration","params":{"items":[]}}"#,&access()).await.expect("request");
    assert_eq!(request.clients.len(), 1);
    let target = request.clients[0].0;
    let id = parse(&request.clients[0].1)["id"].clone();
    let cancel = r
        .server(
            r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":"config"}}"#,
            &access(),
        )
        .await
        .expect("server cancel");
    assert_eq!(cancel.clients.len(), 1);
    assert_eq!(cancel.clients[0].0, target);
    assert_eq!(parse(&cancel.clients[0].1)["params"]["id"], id);
    let response = packet(json!({"jsonrpc":"2.0","id":id,"result":[]}));
    assert!(
        r.client(if target == 1 { 2 } else { 1 }, response.clone(), &access())
            .await
            .is_err()
    );
    let sent = r
        .client(target, response.clone(), &access())
        .await
        .expect("reply");
    assert_eq!(parse(&sent.server[0])["id"], "config");
    assert!(r.client(target, response, &access()).await.is_err());
    let sent = r
        .client(
            1,
            packet(json!({"jsonrpc":"2.0","id":3,"method":"workspace/symbol"})),
            &access(),
        )
        .await
        .expect("request");
    let id = parse(&sent.server[0])["id"].clone();
    let cancel = r
        .client(
            1,
            packet(json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":3}})),
            &access(),
        )
        .await
        .expect("cancel");
    assert_eq!(parse(&cancel.server[0])["params"]["id"], id);
    assert_eq!(r.pending_count(), 0);
    r.client(
        2,
        packet(json!({"jsonrpc":"2.0","id":4,"method":"workspace/symbol"})),
        &access(),
    )
    .await
    .expect("request");
    assert_eq!(r.detach(2).server.len(), 1);
    assert_eq!(r.pending_count(), 0);
    assert!(
        r.client(
            1,
            packet(json!({"jsonrpc":"2.0","id":5,"method":"workspace/symbol"})),
            &access()
        )
        .await
        .is_ok()
    );
}
#[tokio::test]
async fn document_refs_readonly_and_owner_transfer_preserve_one_open_and_monotonic_versions() {
    let mut r = initialized().await;
    let owner = Arc::new(Mutex::new("L1".to_owned()));
    let authority: DocumentAccess = {
        let owner = owner.clone();
        Arc::new(move |uri, lease: Option<String>| {
            let owner = owner.clone();
            Box::pin(async move {
                if lease
                    .as_ref()
                    .is_some_and(|l| l != &*owner.lock().expect("owner"))
                {
                    return Err(LspError::LeaseLost);
                }
                Ok(DocumentGrant {
                    uri,
                    lease_id: lease,
                    disk_text: "disk".into(),
                })
            })
        })
    };
    let open = |text: &str, lease: Option<&str>, version: i32| {
        RelayMessage{json:json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///owned/a.rs","languageId":"rust","version":version,"text":text}}}).to_string(),lease_id:lease.map(str::to_owned)}
    };
    assert_eq!(
        r.client(1, open("mine", Some("L1"), 1), &authority)
            .await
            .expect("open")
            .server
            .len(),
        1
    );
    assert!(
        r.client(2, open("stale readonly", None, 1), &authority)
            .await
            .expect("readonly")
            .server
            .is_empty()
    );
    let change = |text: &str, lease: Option<&str>, version: i32| {
        RelayMessage{json:json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///owned/a.rs","version":version},"contentChanges":[{"text":text}]}}).to_string(),lease_id:lease.map(str::to_owned)}
    };
    assert!(
        r.client(2, change("bad", None, 2), &authority)
            .await
            .is_err()
    );
    assert_eq!(
        r.client(1, change("new", Some("L1"), 2), &authority)
            .await
            .expect("change")
            .server
            .len(),
        1
    );
    assert!(
        r.client(1, change("older", Some("L1"), 1), &authority)
            .await
            .expect("stale version")
            .server
            .is_empty()
    );
    *owner.lock().expect("owner") = "L2".into();
    let taken = r
        .client(2, change("new owner", Some("L2"), 1), &authority)
        .await
        .expect("take");
    let body = parse(&taken.server[0]);
    assert_eq!(body["params"]["contentChanges"][0]["text"], "new owner");
    assert_eq!(body["params"]["textDocument"]["version"], 3);
    assert!(
        r.client(1, change("stale owner", Some("L1"), 9), &authority)
            .await
            .is_err()
    );
    assert!(r.detach(1).server.is_empty());
    let last = r.detach(2);
    assert_eq!(last.server.len(), 1);
    assert_eq!(parse(&last.server[0])["method"], "textDocument/didClose");
}
#[tokio::test]
async fn server_cannot_negotiate_non_utf16_positions() {
    let mut r = Relay::new("file:///owned".into(), 1);
    r.attach(1).expect("attach");
    let out = r
        .client(
            1,
            packet(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})),
            &access(),
        )
        .await
        .expect("init");
    let id = parse(&out.server[0])["id"].clone();
    assert!(r.server(&json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{"positionEncoding":"utf-8"}}}).to_string(),&access()).await.is_err());
}

#[tokio::test]
async fn stale_diagnostic_versions_are_not_forwarded_and_empty_current_diagnostics_are_preserved() {
    let mut r = initialized().await;
    let access = access();
    let open=RelayMessage{json:json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///owned/a.rs","languageId":"rust","version":5,"text":"current"}}}).to_string(),lease_id:Some("L".into())};
    r.client(1, open, &access).await.expect("open");
    let change=RelayMessage{json:json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///owned/a.rs","version":6},"contentChanges":[{"text":"new"}]}}).to_string(),lease_id:Some("L".into())};
    r.client(1, change, &access).await.expect("change");
    let diagnostic = |version| {
        json!({"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///owned/a.rs","version":version,"diagnostics":[]}}).to_string()
    };
    let old = r.server(&diagnostic(1), &access).await.expect("stale");
    assert!(old.clients.is_empty() && old.diagnostics.is_empty());
    let current = r.server(&diagnostic(2), &access).await.expect("current");
    assert_eq!(current.diagnostics.len(), 1);
    assert!(current.diagnostics[0].items.is_empty());
}

#[tokio::test]
async fn review_did_save_preserves_the_acknowledged_saved_text() {
    let mut r = initialized().await;
    let uri = "file:///owned/a.rs";
    let open=RelayMessage{json:json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"rust","version":1,"text":"saved A"}}}).to_string(),lease_id:Some("L1".into())};
    r.client(1, open, &access()).await.expect("relay fixture");
    let change=RelayMessage{json:json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"unsaved B"}]}}).to_string(),lease_id:Some("L1".into())};
    r.client(1, change, &access()).await.expect("relay fixture");
    let save=RelayMessage{json:json!({"jsonrpc":"2.0","method":"textDocument/didSave","params":{"textDocument":{"uri":uri},"text":"saved A"}}).to_string(),lease_id:Some("L1".into())};
    let out = r.client(1, save, &access()).await.expect("relay fixture");
    assert_eq!(parse(&out.server[0])["params"]["text"], "saved A");
}

#[tokio::test]
async fn review_cancelled_or_disconnected_late_reply_has_no_authority_side_effects() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for disconnected in [false, true] {
        let mut r = initialized().await;
        let request = r
            .client(
                1,
                packet(
                    json!({"jsonrpc":"2.0","id":9,"method":"textDocument/definition","params":{}}),
                ),
                &access(),
            )
            .await
            .expect("request");
        let id = parse(&request.server[0])["id"].clone();
        if disconnected {
            r.detach(1);
        } else {
            r.client(
                1,
                packet(json!({"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":9}})),
                &access(),
            )
            .await
            .expect("cancel");
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let authority: DocumentAccess = Arc::new(move |uri, lease_id| {
            counted.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if !uri.starts_with("file:///owned/") {
                    return Err(LspError::OutsideProject);
                }
                Ok(DocumentGrant {
                    uri,
                    lease_id,
                    disk_text: String::new(),
                })
            })
        });
        let late = r
            .server(
                &json!({"jsonrpc":"2.0","id":id,"result":{"uri":"file:///dependency/lib.rs"}})
                    .to_string(),
                &authority,
            )
            .await
            .expect("unrouted reply ignored");
        assert!(
            late.clients.is_empty()
                && late.server.is_empty()
                && late.diagnostics.is_empty()
                && late.close.is_empty()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(r.generation, 1);
        assert!(r.ready());
        let live = r
            .client(
                2,
                packet(
                    json!({"jsonrpc":"2.0","id":5,"method":"textDocument/definition","params":{}}),
                ),
                &authority,
            )
            .await
            .expect("live sibling");
        let id = parse(&live.server[0])["id"].clone();
        let reply = r
            .server(
                &json!({"jsonrpc":"2.0","id":id,"result":{"uri":"file:///owned/a.rs"}}).to_string(),
                &authority,
            )
            .await
            .expect("live reply");
        assert_eq!(reply.clients[0].0, 2);
        assert_eq!(parse(&reply.clients[0].1)["id"], 5);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let live = r
            .client(
                2,
                packet(
                    json!({"jsonrpc":"2.0","id":6,"method":"textDocument/definition","params":{}}),
                ),
                &authority,
            )
            .await
            .expect("live request");
        let id = parse(&live.server[0])["id"].clone();
        assert!(
            r.server(
                &json!({"jsonrpc":"2.0","id":id,"result":{"uri":"file:///dependency/lib.rs"}})
                    .to_string(),
                &authority
            )
            .await
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
