use super::*;
use devbox_documents::DiskRev;

const ROOT: &str = "file:///owned";
const CLIENT: u32 = 7;
const GENERATION: u64 = 11;
fn baseline(name: &str, open: bool) -> WorkspaceBaseline {
    WorkspaceBaseline {
        uri: format!("{ROOT}/{name}.rs"),
        disk_rev: Some(DiskRev {
            hash: "raw CRLF/BOM hash".into(),
            size: 22,
            mtime_ns: "1".into(),
            ino: "2".into(),
        }),
        text_hash: text_hash(if open {
            "dirty open buffer\n"
        } else {
            "normalized disk\n"
        }),
        lease_id: open.then(|| "open-lease".into()),
        version: open.then_some(3),
    }
}
fn prepared() -> (Proofs, WorkspaceValidation) {
    let a = baseline("a", true);
    let b = baseline("b", false);
    let mut proofs = Proofs::default();
    proofs.insert(
        GENERATION,
        captured(
            CLIENT,
            json!("rename"),
            [(a.uri.clone(), a.clone())].into(),
            [(b.uri.clone(), b.clone())].into(),
        ),
    );
    let edit = json!({"changes":{a.uri:[], b.uri:[]}});
    let proof = proofs
        .response(CLIENT, &json!("rename"), ROOT, GENERATION, &edit)
        .expect("covered response");
    let targets = proof
        .targets
        .into_iter()
        .map(|mut target| {
            if target.lease_id.is_none() {
                target.lease_id = Some("newly-claimed-lease".into());
                target.version = Some(1);
            }
            target
        })
        .collect();
    (
        proofs,
        WorkspaceValidation {
            identity: WorkspaceIdentity {
                token: proof.token,
                request_id: proof.request_id,
                root_uri: proof.root_uri,
                generation: proof.generation,
            },
            targets,
        },
    )
}
#[test]
fn proof_retains_dirty_open_and_normalized_unopened_baselines_until_ack() {
    let (mut proofs, request) = prepared();
    let pending = proofs
        .validation(CLIENT, ROOT, GENERATION, &request)
        .expect("both targets still match");
    assert_eq!(
        pending.targets.as_ref().expect("targets")[0],
        baseline("a", true)
    );
    assert_eq!(
        pending.targets.as_ref().expect("targets")[1],
        baseline("b", false)
    );
    assert!(
        proofs.pending(CLIENT),
        "response delivery cannot dispose proof"
    );
    proofs.validated(&request.identity.token);
    assert!(proofs.pending(CLIENT), "validation is not final apply ACK");
    assert!(
        proofs
            .validation(CLIENT, ROOT, GENERATION, &request)
            .is_err(),
        "validation replay rejected"
    );
    proofs.finish(CLIENT, ROOT, GENERATION, &request.identity);
    assert!(!proofs.pending(CLIENT));
    assert!(
        proofs
            .validation(CLIENT, ROOT, GENERATION, &request)
            .is_err()
    );
}
#[test]
fn proof_rejects_wrong_stream_request_root_generation_and_token_without_consuming_owner() {
    let (mut proofs, valid) = prepared();
    assert!(
        proofs
            .validation(CLIENT + 1, ROOT, GENERATION, &valid)
            .is_err()
    );
    assert!(
        proofs
            .validation(CLIENT, "file:///other", GENERATION, &valid)
            .is_err()
    );
    assert!(
        proofs
            .validation(CLIENT, ROOT, GENERATION + 1, &valid)
            .is_err()
    );
    for field in ["token", "request", "root", "generation"] {
        let mut wrong = valid.clone();
        match field {
            "token" => wrong.identity.token.push_str("-forged"),
            "request" => wrong.identity.request_id = json!("other").to_string(),
            "root" => wrong.identity.root_uri = "file:///other".into(),
            "generation" => wrong.identity.generation = (GENERATION + 1).to_string(),
            _ => unreachable!(),
        }
        assert!(
            proofs.validation(CLIENT, ROOT, GENERATION, &wrong).is_err(),
            "wrong {field}"
        );
        proofs.finish(CLIENT, ROOT, GENERATION, &wrong.identity);
        assert!(
            proofs.validation(CLIENT, ROOT, GENERATION, &valid).is_ok(),
            "wrong ACK must not discard owner"
        );
    }
    proofs.finish(CLIENT + 1, ROOT, GENERATION, &valid.identity);
    assert!(proofs.validation(CLIENT, ROOT, GENERATION, &valid).is_ok());
}
#[test]
fn proof_rejects_changed_or_missing_target_content_revision_identity_and_lease() {
    let (proofs, valid) = prepared();
    for field in ["uri", "disk", "text", "lease", "missing", "extra"] {
        let mut wrong = valid.clone();
        match field {
            "uri" => wrong.targets[0].uri = "file:///outside/a.rs".into(),
            "disk" => wrong.targets[0].disk_rev.as_mut().expect("disk").hash = "new disk".into(),
            "text" => wrong.targets[1].text_hash = text_hash("opened and edited during rename"),
            "lease" => wrong.targets[0].lease_id = Some("other-owner".into()),
            "missing" => {
                wrong.targets.pop();
            }
            "extra" => wrong.targets.push(baseline("c", true)),
            _ => unreachable!(),
        }
        assert!(
            proofs.validation(CLIENT, ROOT, GENERATION, &wrong).is_err(),
            "changed {field}"
        );
    }
    let mut unclaimed = valid.clone();
    unclaimed.targets[1].lease_id = None;
    assert!(
        proofs
            .validation(CLIENT, ROOT, GENERATION, &unclaimed)
            .is_err()
    );
    assert!(proofs.validation(CLIENT, ROOT, GENERATION, &valid).is_ok());
}
#[test]
fn proof_expiry_rejects_validation_and_releases_admission() {
    let (mut proofs, request) = prepared();
    proofs
        .items
        .get_mut(&request.identity.token)
        .expect("proof")
        .expires = Instant::now() - Duration::from_millis(1);
    assert!(
        proofs
            .validation(CLIENT, ROOT, GENERATION, &request)
            .is_err()
    );
    assert!(!proofs.pending(CLIENT));
    assert!(proofs.items.get(&request.identity.token).is_none());
}
#[test]
fn cancel_and_detach_are_bound_to_the_owning_stream_and_request() {
    let (mut proofs, request) = prepared();
    proofs.cancel(CLIENT + 1, &json!("rename"));
    proofs.cancel(CLIENT, &json!("other"));
    proofs.detach(CLIENT + 1);
    assert!(
        proofs
            .validation(CLIENT, ROOT, GENERATION, &request)
            .is_ok()
    );
    proofs.cancel(CLIENT, &json!("rename"));
    assert!(
        proofs
            .validation(CLIENT, ROOT, GENERATION, &request)
            .is_err()
    );
    let (mut proofs, request) = prepared();
    proofs.detach(CLIENT);
    assert!(
        proofs
            .validation(CLIENT, ROOT, GENERATION, &request)
            .is_err()
    );
}
#[test]
fn uncovered_target_refuses_the_whole_response_and_does_not_publish_a_partial_proof() {
    let a = baseline("a", false);
    let mut proofs = Proofs::default();
    let token = proofs.insert(
        GENERATION,
        captured(
            CLIENT,
            json!(1),
            BTreeMap::new(),
            [(a.uri.clone(), a.clone())].into(),
        ),
    );
    assert!(
        proofs
            .response(
                CLIENT,
                &json!(1),
                ROOT,
                GENERATION,
                &json!({"changes":{a.uri.clone():[],"file:///owned/uncovered.rs":[]}})
            )
            .is_err()
    );
    assert!(proofs.items.get(&token).expect("pending").targets.is_none());
    let reply = proofs
        .response(
            CLIENT,
            &json!(1),
            ROOT,
            GENERATION,
            &json!({"changes":{a.uri:[]}}),
        )
        .expect("covered only");
    assert_eq!(reply.targets.len(), 1);
    assert!(
        proofs
            .response(CLIENT, &json!(1), ROOT, GENERATION, &json!({"changes":{}}))
            .is_err(),
        "response replay"
    );
}
#[test]
fn open_dirty_snapshot_takes_precedence_over_same_file_disk_capture() {
    let open = baseline("a", true);
    let disk = baseline("a", false);
    let mut proofs = Proofs::default();
    proofs.insert(
        GENERATION,
        captured(
            CLIENT,
            json!(1),
            [(open.uri.clone(), open.clone())].into(),
            [(disk.uri.clone(), disk)].into(),
        ),
    );
    let reply = proofs.response(CLIENT, &json!(1), ROOT, GENERATION, &json!({"documentChanges":[{"textDocument":{"uri":open.uri,"version":null},"edits":[]}]})).expect("open source wins");
    assert_eq!(reply.targets, vec![open]);
}
#[test]
fn target_admission_rejects_duplicates_and_more_than_256_before_proof() {
    let same = json!({"textDocument":{"uri":"file:///owned/a.rs","version":null},"edits":[]});
    assert!(targets(&json!({"documentChanges":[same.clone(),same]})).is_err());
    let changes: serde_json::Map<String, Value> = (0..257)
        .map(|i| (format!("file:///owned/{i}.rs"), json!([])))
        .collect();
    assert!(targets(&json!({"changes":changes})).is_err());
}
#[test]
fn validation_identity_roundtrips_flattened_wire_fields() {
    let (_, valid) = prepared();
    let wire = serde_json::to_value(&valid).expect("serialize validation");
    assert!(wire.get("identity").is_none());
    let roundtrip: WorkspaceValidation =
        serde_json::from_value(wire).expect("flattened request must decode");
    assert_eq!(roundtrip.identity.token, valid.identity.token);
    assert_eq!(roundtrip.targets, valid.targets);
}

#[tokio::test]
async fn cancelled_capture_completion_survives_a_full_actor_queue_and_releases_client_admission() {
    use crate::{
        RelayMessage,
        actor::Command,
        adapters::{CaptureAccess, DocumentAccess, DocumentGrant},
        capture::Captures,
        relay::Relay,
    };
    use std::sync::Arc;
    use tokio::sync::{Semaphore, mpsc};
    use tokio_util::{sync::CancellationToken, task::TaskTracker};
    let access: DocumentAccess = Arc::new(|uri, lease_id| {
        Box::pin(async move {
            Ok(DocumentGrant {
                uri,
                lease_id,
                disk_text: "disk".into(),
                disk_rev: None,
            })
        })
    });
    let packet = |value: Value| RelayMessage {
        json: value.to_string(),
        lease_id: None,
    };
    let mut relay = Relay::new(ROOT.into(), GENERATION);
    relay.attach(CLIENT).expect("attach");
    let init = relay
        .client(
            CLIENT,
            packet(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})),
            &access,
        )
        .await
        .expect("initialize");
    let id = serde_json::from_str::<Value>(&init.server[0]).expect("forwarded init")["id"].clone();
    relay
        .server(
            &json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{}}}).to_string(),
            &access,
        )
        .await
        .expect("initialized");
    let release = Arc::new(Semaphore::new(0));
    let (entered, mut starts) = mpsc::unbounded_channel();
    let worker_release = release.clone();
    let capture: CaptureAccess = Arc::new(move |_priority, cancel| {
        let release = worker_release.clone();
        let entered = entered.clone();
        Box::pin(async move {
            entered.send(cancel).expect("capture observed");
            release.acquire().await.expect("release capture").forget();
            Err(LspError::LimitExceeded)
        })
    });
    let tasks = TaskTracker::new();
    let stop = CancellationToken::new();
    let (commands, mut queue) = mpsc::channel(1);
    commands
        .send(Command::Detach { id: 99 })
        .await
        .expect("fill actor queue");
    let mut captures = Captures::default();
    let rename =
        packet(json!({"jsonrpc":"2.0","id":42,"method":"textDocument/rename","params":{}}));
    captures
        .start(
            CLIENT,
            GENERATION,
            rename.clone(),
            &mut relay,
            crate::capture::CaptureTask {
                access: capture.clone(),
                commands: commands.clone(),
                tasks: &tasks,
                stop: &stop,
            },
        )
        .expect("start capture");
    let token = starts.recv().await.expect("capture start");
    captures.cancel(CLIENT, Some(&json!(42)));
    assert!(token.is_cancelled());
    release.add_permits(1);
    tasks.close();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), tasks.wait())
            .await
            .is_err(),
        "cancelled completion must wait for actor queue capacity, not disappear"
    );
    assert!(matches!(
        queue.recv().await,
        Some(Command::Detach { id: 99 })
    ));
    let completed = tokio::time::timeout(Duration::from_secs(1), queue.recv())
        .await
        .expect("completion delivered")
        .expect("command");
    let Command::Captured {
        client,
        job,
        result,
    } = completed
    else {
        panic!("capture completion required")
    };
    assert!(
        captures
            .complete(client, job, GENERATION, result, &mut relay)
            .expect("cancel discarded")
            .is_none()
    );
    tasks.wait().await;
    let next_tasks = TaskTracker::new();
    captures
        .start(
            CLIENT,
            GENERATION,
            rename,
            &mut relay,
            crate::capture::CaptureTask {
                access: capture,
                commands,
                tasks: &next_tasks,
                stop: &stop,
            },
        )
        .expect("client admission recovered");
    starts.recv().await.expect("next worker starts");
    release.add_permits(1);
    let completed = tokio::time::timeout(Duration::from_secs(1), queue.recv())
        .await
        .expect("next completion")
        .expect("command");
    let Command::Captured {
        client,
        job,
        result,
    } = completed
    else {
        panic!("capture completion required")
    };
    assert!(
        captures
            .complete(client, job, GENERATION, result, &mut relay)
            .is_err()
    );
    next_tasks.close();
    next_tasks.wait().await;
}

#[tokio::test]
async fn conflicting_capture_retains_canonical_omission_and_refuses_the_whole_edit() {
    use crate::{
        RelayMessage,
        actor::Command,
        adapters::DocumentGrant,
        capture::{CaptureTask, Captures},
        relay::Relay,
    };
    use devbox_documents::{CaptureReason, CodeBaseline, CodeCapture, DocKey};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio_util::{sync::CancellationToken, task::TaskTracker};
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let access: crate::adapters::DocumentAccess = Arc::new(move |_, _| {
        counted.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(LspError::OutsideProject) })
    });
    let packet = |value: Value| RelayMessage {
        json: value.to_string(),
        lease_id: None,
    };
    let mut relay = Relay::new(ROOT.into(), GENERATION);
    relay.attach(CLIENT).expect("attach");
    let init = relay
        .client(
            CLIENT,
            packet(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})),
            &access,
        )
        .await
        .expect("initialize");
    let id = serde_json::from_str::<Value>(&init.server[0]).expect("init JSON")["id"].clone();
    relay
        .server(
            &json!({"jsonrpc":"2.0","id":id,"result":{"capabilities":{}}}).to_string(),
            &access,
        )
        .await
        .expect("initialized");
    let b = baseline("b", false);
    relay.documents.notification(CLIENT + 1, "textDocument/didOpen", &json!({"params":{"textDocument":{"languageId":"rust","version":0,"text":"another owner's dirty buffer"}}}), DocumentGrant {
        uri:b.uri.clone(), disk_text:"normalized disk\n".into(), disk_rev:b.disk_rev.clone(), lease_id:Some("other lease".into()),
    }).expect("other owner synchronized B");
    let canonical_b = DocKey {
        root: "project:p".into(),
        path: "b.rs".into(),
    };
    let disk = |name: &str| CodeBaseline {
        key: DocKey {
            root: "project:p".into(),
            path: format!("{name}.rs"),
        },
        rev: b.disk_rev.clone().expect("raw revision"),
        text_hash: b.text_hash.clone(),
    };
    let captured_disk = CodeCapture {
        root: "/owned".into(),
        covered: vec![disk("a"), disk("b")],
        omitted: vec![],
        stopped: None,
        bytes_read: 44,
        entries_scanned: 2,
    };
    let adapter: crate::adapters::CaptureAccess = Arc::new(move |_, _| {
        let result = captured_disk.clone();
        Box::pin(async move { Ok(result) })
    });
    let tasks = TaskTracker::new();
    let stop = CancellationToken::new();
    let (commands, mut done) = tokio::sync::mpsc::channel(1);
    let mut captures = Captures::default();
    captures
        .start(
            CLIENT,
            GENERATION,
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
    let Command::Captured {
        client,
        job,
        result,
    } = done.recv().await.expect("completed capture")
    else {
        panic!("capture completion");
    };
    assert!(
        captures
            .complete(client, job, GENERATION, result, &mut relay)
            .expect("capture completes")
            .is_some()
    );
    let retained = relay
        .proofs
        .items
        .values()
        .next()
        .expect("retained proof before response");
    assert!(retained.covered.contains_key("file:///owned/a.rs"));
    assert!(!retained.covered.contains_key(&b.uri));
    assert_eq!(
        retained.omitted.len(),
        1,
        "relay-side contradiction must retain its reason"
    );
    assert_eq!(retained.omitted[0].key, canonical_b);
    assert_eq!(retained.omitted[0].reason, CaptureReason::Changed);
    assert_eq!(retained.stopped, None);
    assert!(
        relay
            .proofs
            .response(
                CLIENT,
                &json!(42),
                ROOT,
                GENERATION,
                &json!({"changes":{"file:///owned/a.rs":[],b.uri:[]}})
            )
            .is_err()
    );
    assert!(
        relay
            .proofs
            .items
            .values()
            .next()
            .expect("retained refusal")
            .targets
            .is_none(),
        "a safe first target cannot publish a partial proof"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "contradiction uses synchronized hashes without another authority read"
    );
    tasks.close();
    tasks.wait().await;
}
