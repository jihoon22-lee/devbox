//! Owned vault/index faults and delayed holder saves exercise operation replay.
use crate::fixture::*;
use crate::*;
use devbox_core::{HubMessage, HubReceiver};
use devbox_documents::{Claimed, Expect, SaveDocument};
use devbox_protocol::Fail;
use std::time::Duration;

const NOTE: &str = "response.md";
const ORIGINAL: &str = "existing note\n";
const DRAFT: &str = "existing note\nunsaved user text\n";
const MARKDOWN: &str = "### Owned response\n\n```text\nfixture-response-payload\n```\n";
const WAIT: Duration = Duration::from_secs(5);

async fn selected(n: &Nx) -> AppendResponseNote {
    let target = n
        .svc
        .select_response_target(SelectResponseNote {
            target: ResponseNoteTarget::Existing { path: NOTE.into() },
        })
        .await
        .expect("select owned existing note");
    AppendResponseNote {
        history_id: "a".repeat(32),
        selection: target.selection,
        operation_id: uuid::Uuid::new_v4().simple().to_string(),
    }
}

async fn next_rewrite(rx: &mut HubReceiver) -> RewriteRequest {
    match tokio::time::timeout(WAIT, rx.recv()).await {
        Ok(Some(HubMessage::Event { payload, .. })) => {
            serde_json::from_str(&payload).expect("owned rewrite payload")
        }
        other => panic!("expected owned rewrite: {other:?}"),
    }
}

async fn no_rewrite(rx: &mut HubReceiver) {
    assert!(
        tokio::time::timeout(Duration::from_millis(40), rx.recv())
            .await
            .is_err(),
        "a retry must not issue another rewrite"
    );
}

async fn late_save(n: &Nx, open: &Claimed, request: &RewriteRequest) {
    assert_eq!(request.edits.len(), 1);
    let edit = &request.edits[0];
    let end = DRAFT.encode_utf16().count() as u32;
    assert_eq!((edit.from_utf16, edit.to_utf16), (end, end));
    assert!(edit.text.contains(MARKDOWN));
    n.docs
        .save(SaveDocument {
            lease_id: request.lease_id.clone(),
            seq: request.journal_seq + 1,
            base_rev: open.rev.clone(),
            expect: Expect::Present,
            text: format!("{DRAFT}{}later user typing\n", edit.text),
        })
        .await
        .expect("the original holder finishes its delayed save");
}

#[tokio::test]
async fn response_commit_survives_index_refresh_failure_and_restart_without_reappend() {
    let mut n = fixture(&[(NOTE, ORIGINAL)]).await;
    let request = selected(&n).await;
    // Only this fixture's rebuildable index is changed. The note write still succeeds.
    let index = rusqlite::Connection::open(n.data.join("index.db")).expect("owned index");
    index
        .execute_batch(
            "CREATE TRIGGER owned_response_refresh_failure BEFORE INSERT ON notes_files
             BEGIN SELECT RAISE(ABORT, 'owned refresh failure'); END;",
        )
        .expect("inject owned refresh failure");
    let first = n
        .svc
        .append_response(request.clone(), MARKDOWN.into())
        .await;
    assert!(matches!(first, Err(Fail::Internal(_))), "{first:?}");
    let saved = n.read(NOTE);
    let identity = n.identity(NOTE);
    assert!(saved.starts_with(ORIGINAL));
    assert_eq!(saved.matches(MARKDOWN).count(), 1);
    let retried = n
        .svc
        .append_response(request.clone(), MARKDOWN.into())
        .await
        .expect("committed retry succeeds despite the failed refresh");
    assert_eq!(retried.path, NOTE);
    assert_eq!(n.read(NOTE), saved);
    assert_eq!(n.identity(NOTE), identity);
    index
        .execute_batch("DROP TRIGGER owned_response_refresh_failure")
        .expect("restore owned index");
    drop(index);
    n.restart();
    let recovered = n
        .svc
        .response_append_result(&request)
        .await
        .expect("read durable completed operation")
        .expect("completed receipt survives service restart");
    assert_eq!(recovered, retried);
    assert_eq!(n.read(NOTE), saved);
    assert_eq!(n.identity(NOTE), identity);
    let mut changed = request;
    changed.history_id = "b".repeat(32);
    assert_eq!(
        domain(n.svc.response_append_result(&changed).await),
        NotesError::AppendOperationMismatch
    );
}

#[tokio::test]
async fn response_timeout_late_save_and_restart_reconcile_once_without_another_rewrite() {
    let mut n = fixture(&[(NOTE, ORIGINAL)]).await;
    let open = n.open_note(NOTE, "owned-window", Some((7, DRAFT))).await;
    let request = selected(&n).await;
    let mut rewrites = n.hub.subscribe("notes.rewrite_request");
    let svc = n.svc.clone();
    let pending = request.clone();
    let task = tokio::spawn(async move { svc.append_response(pending, MARKDOWN.into()).await });
    let rewrite = next_rewrite(&mut rewrites).await;
    assert_eq!(n.read(NOTE), ORIGINAL, "never write behind dirty holder");
    n.advance(31_000);
    let first = tokio::time::timeout(WAIT, task)
        .await
        .expect("bounded holder wait")
        .expect("append task");
    assert_eq!(domain(first), NotesError::OpenUnsaved);
    assert_eq!(
        domain(n.svc.response_append_result(&request).await),
        NotesError::AppendUncertain
    );
    no_rewrite(&mut rewrites).await;
    late_save(&n, &open, &rewrite).await;
    assert_eq!(
        domain(
            n.svc
                .rewrite_reply(RewriteReply {
                    op_id: rewrite.op_id,
                    key: rewrite.key,
                    applied: true,
                })
                .await
        ),
        NotesError::NotFound,
        "late ACK cannot revive the expired reply slot"
    );
    let saved = n.read(NOTE);
    let identity = n.identity(NOTE);
    assert!(saved.contains("unsaved user text"));
    assert!(saved.contains("later user typing"));
    assert_eq!(saved.matches(MARKDOWN).count(), 1);
    // The receipt is still Attempted; the new service must inspect the original save.
    n.restart();
    let result = n
        .svc
        .append_response(request.clone(), MARKDOWN.into())
        .await
        .expect("recover late success from durable note and operation");
    assert_eq!(result.operation_id, request.operation_id);
    assert_eq!(n.read(NOTE), saved);
    assert_eq!(n.identity(NOTE), identity);
    no_rewrite(&mut rewrites).await;
    // A new explicit click is a new intent, even for the same response.
    let fresh = selected(&n).await;
    n.svc
        .append_response(fresh, MARKDOWN.into())
        .await
        .expect("new explicit append remains possible");
    assert_eq!(n.read(NOTE).matches(MARKDOWN).count(), 2);
}

#[tokio::test]
async fn response_timeout_without_save_remains_uncertain_after_restart_and_preserves_draft() {
    let mut n = fixture(&[(NOTE, ORIGINAL)]).await;
    n.open_note(NOTE, "owned-window", Some((3, DRAFT))).await;
    let request = selected(&n).await;
    let mut rewrites = n.hub.subscribe("notes.rewrite_request");
    let svc = n.svc.clone();
    let pending = request.clone();
    let task = tokio::spawn(async move { svc.append_response(pending, MARKDOWN.into()).await });
    next_rewrite(&mut rewrites).await;
    n.advance(31_000);
    let first = tokio::time::timeout(WAIT, task)
        .await
        .expect("bounded holder wait")
        .expect("append task");
    assert_eq!(domain(first), NotesError::OpenUnsaved);
    n.restart();
    let retried = n.svc.append_response(request, MARKDOWN.into()).await;
    assert_eq!(domain(retried), NotesError::AppendUncertain);
    no_rewrite(&mut rewrites).await;
    assert_eq!(n.read(NOTE), ORIGINAL);
    let reopened = n.open_note(NOTE, "new-owned-window", None).await;
    assert_eq!(
        reopened
            .journal
            .expect("recovery draft preserved")
            .text
            .as_deref(),
        Some(DRAFT)
    );
}
