use super::*;
fn capture(relay: &mut Relay, request: i64, names: &[&str]) {
    let covered = names
        .iter()
        .map(|name| {
            let uri = format!("file:///owned/{name}.rs");
            (
                uri.clone(),
                crate::WorkspaceBaseline {
                    uri,
                    disk_rev: None,
                    text_hash: crate::workspace::text_hash("disk"),
                    lease_id: None,
                    version: None,
                },
            )
        })
        .collect();
    relay.proofs.insert(
        1,
        crate::workspace::captured(1, json!(request), Default::default(), covered),
    );
}
async fn request(relay: &mut Relay, request: i64) -> Value {
    let out = relay
        .client(
            1,
            packet(
                json!({"jsonrpc":"2.0","id":request,"method":"textDocument/rename","params":{}}),
            ),
            &access(),
        )
        .await
        .expect("request");
    parse(&out.server[0])["id"].clone()
}
#[tokio::test]
async fn server_cannot_forge_proof_or_turn_an_uncovered_alias_into_a_current_baseline() {
    let mut relay = initialized().await;
    capture(&mut relay, 2, &["a"]);
    let id = request(&mut relay, 2).await;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = calls.clone();
    let forbidden: DocumentAccess = Arc::new(move |_, _| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Err(LspError::OutsideProject) })
    });
    let out=relay.server(&json!({"jsonrpc":"2.0","id":id,"result":{"changes":{"file:///owned/a.rs":[]},"proof":{"token":"server-forged"}}}).to_string(),&forbidden).await.expect("known canonical target needs no current read");
    assert_ne!(
        parse(&out.clients[0].1)["result"]["proof"]["token"],
        "server-forged"
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    capture(&mut relay, 3, &["a"]);
    let id = request(&mut relay, 3).await;
    let out = relay
        .server(
            &json!({"jsonrpc":"2.0","id":id,"result":{"changes":{"file:///owned/alias.rs":[]}}})
                .to_string(),
            &forbidden,
        )
        .await
        .expect("refused alias leaves shared generation alive");
    assert_eq!(parse(&out.clients[0].1)["error"]["code"], -32801);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(relay.take_capture_hints(1), vec!["file:///owned/alias.rs"]);
    assert!(
        relay.take_capture_hints(1).is_empty(),
        "only next explicit capture uses hints"
    );
    assert!(relay.ready());
}
#[tokio::test]
async fn missing_second_target_refuses_all_and_unsolicited_unversioned_edit_cannot_borrow_proof() {
    let mut relay = initialized().await;
    capture(&mut relay, 2, &["a"]);
    let id = request(&mut relay, 2).await;
    let out=relay.server(&json!({"jsonrpc":"2.0","id":id,"result":{"changes":{"file:///owned/a.rs":[],"file:///owned/b.rs":[]}}}).to_string(),&access()).await.expect("partial refusal");
    assert!(parse(&out.clients[0].1).get("result").is_none());
    assert_eq!(
        relay.take_capture_hints(1),
        vec!["file:///owned/a.rs", "file:///owned/b.rs"]
    );
    capture(&mut relay, 3, &["a"]);
    let out=relay.server(&json!({"jsonrpc":"2.0","id":"unsolicited","method":"workspace/applyEdit","params":{"edit":{"changes":{"file:///owned/a.rs":[]}}}}).to_string(),&access()).await.expect("safe refusal");
    assert!(out.clients.is_empty());
    assert_eq!(parse(&out.server[0])["result"]["applied"], false);
    assert!(
        relay.proofs.pending(1),
        "unsolicited does not borrow or consume Rename proof"
    );
}

#[tokio::test]
async fn final_authority_stops_at_first_changed_target_and_budget_refuses_before_io() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for over_budget in [false, true] {
        let mut relay = initialized().await;
        let names = if over_budget {
            vec!["a", "b", "c", "d", "e", "f", "g"]
        } else {
            vec!["a", "b"]
        };
        let covered = names
            .iter()
            .map(|name| {
                let uri = format!("file:///owned/{name}.rs");
                (
                    uri.clone(),
                    crate::WorkspaceBaseline {
                        uri,
                        disk_rev: Some(devbox_documents::DiskRev {
                            hash: "raw".into(),
                            size: 5 * 1024 * 1024,
                            mtime_ns: "1".into(),
                            ino: "2".into(),
                        }),
                        text_hash: crate::workspace::text_hash("dirty"),
                        lease_id: None,
                        version: None,
                    },
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let changes = covered
            .keys()
            .map(|uri| (uri.clone(), json!([])))
            .collect::<serde_json::Map<_, _>>();
        relay.proofs.insert(
            1,
            crate::workspace::captured(1, json!(2), Default::default(), covered),
        );
        let id = request(&mut relay, 2).await;
        let out = relay
            .server(
                &json!({"jsonrpc":"2.0","id":id,"result":{"changes":changes}}).to_string(),
                &access(),
            )
            .await
            .expect("covered");
        let proof: crate::WorkspaceProof =
            serde_json::from_value(parse(&out.clients[0].1)["result"]["proof"].clone())
                .expect("proof");
        let mut targets = proof.targets;
        for target in &mut targets {
            target.lease_id = Some("claimed".into());
        }
        let validation = crate::WorkspaceValidation {
            identity: crate::WorkspaceIdentity {
                token: proof.token,
                request_id: proof.request_id,
                root_uri: proof.root_uri,
                generation: proof.generation,
            },
            targets,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let changed: DocumentAccess = Arc::new(move |uri, lease_id| {
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(DocumentGrant {
                    uri,
                    disk_text: "changed".into(),
                    disk_rev: None,
                    lease_id,
                })
            })
        });
        let out=relay.client(1,packet(json!({"jsonrpc":"2.0","id":3,"method":"devbox/validateWorkspaceEdit","params":validation})),&changed).await.expect("normal refusal");
        assert_eq!(parse(&out.clients[0].1)["result"], false);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            if over_budget { 0 } else { 1 }
        );
        assert!(relay.ready());
    }
}

#[tokio::test]
async fn capture_completion_rechecks_synced_open_buffer_before_dispatch() {
    use crate::{
        actor::Command,
        capture::{CaptureTask, Captures},
    };
    use tokio_util::{sync::CancellationToken, task::TaskTracker};
    let mut relay = initialized().await;
    let owned = |value: Value| RelayMessage {
        json: value.to_string(),
        lease_id: Some("L".into()),
    };
    relay.client(1,owned(json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///owned/a.rs","languageId":"rust","version":1,"text":"before"}}})),&access()).await.expect("owned buffer");
    let (commands, mut done) = tokio::sync::mpsc::channel(1);
    let tasks = TaskTracker::new();
    let stop = CancellationToken::new();
    let adapter: crate::adapters::CaptureAccess = Arc::new(|_, _| {
        Box::pin(async {
            Ok(devbox_documents::CodeCapture {
                root: "/owned".into(),
                covered: vec![],
                omitted: vec![],
                stopped: None,
                bytes_read: 0,
                entries_scanned: 0,
            })
        })
    });
    let mut captures = Captures::default();
    captures
        .start(
            1,
            1,
            packet(json!({"jsonrpc":"2.0","id":42,"method":"textDocument/rename","params":{}})),
            &mut relay,
            CaptureTask {
                access: adapter,
                commands,
                tasks: &tasks,
                stop: &stop,
            },
        )
        .expect("capture starts");
    relay.client(1,owned(json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///owned/a.rs","version":2},"contentChanges":[{"text":"after"}]}})),&access()).await.expect("edit while capture pending");
    let Command::Captured {
        client,
        job,
        result,
    } = done.recv().await.expect("completion")
    else {
        panic!("capture completion");
    };
    assert_eq!(
        captures
            .complete(client, job, 1, result, &mut relay)
            .expect_err("changed synchronized buffer must not forward Rename"),
        json!(42)
    );
    assert!(!relay.proofs.pending(1));
    tasks.close();
    tasks.wait().await;
}
