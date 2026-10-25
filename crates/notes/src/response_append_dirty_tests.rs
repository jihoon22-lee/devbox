//! Response-specific preflight must run before a dirty holder receives any rewrite.
use crate::fixture::*;
use crate::*;
use std::time::Duration;
async fn intent(n: &Nx, path: &str) -> AppendResponseNote {
    let target = n
        .svc
        .select_response_target(SelectResponseNote {
            target: ResponseNoteTarget::Existing { path: path.into() },
        })
        .await
        .expect("selected");
    AppendResponseNote {
        history_id: "b".repeat(32),
        selection: target.selection,
        operation_id: uuid::Uuid::new_v4().simple().to_string(),
    }
}
#[tokio::test]
async fn oversized_crlf_bom_rewrite_leaves_dirty_buffer_and_disk_untouched_without_a_request() {
    let n = fixture(&[]).await;
    n.write("chosen.md", b"\xef\xbb\xbfkeep\r\n");
    let draft = "x\n".repeat(690_000);
    let _holder = n.open_note("chosen.md", "owner", Some((1, &draft))).await;
    let p = intent(&n, "chosen.md").await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let result = n.svc.append_response(p, "y\n".repeat(20_000)).await;
    assert_eq!(
        domain(result),
        NotesError::TooLarge {
            limit: devbox_documents::NOTE_LIMIT
        }
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), rx.recv())
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(n.vault().join("chosen.md")).expect("disk"),
        b"\xef\xbb\xbfkeep\r\n"
    );
    assert_eq!(
        n.svc
            .s
            .docs
            .edit_state(&n.svc.key("chosen.md"))
            .await
            .expect("state")
            .journal
            .map(|(_, text)| text),
        Some(draft)
    );
}
#[tokio::test]
async fn a_recovery_draft_without_a_holder_is_never_written_behind() {
    let mut n = fixture(&[("chosen.md", "keep\n")]).await;
    let _holder = n
        .open_note("chosen.md", "owner", Some((1, "keep\nunsaved\n")))
        .await;
    n.restart();
    let p = intent(&n, "chosen.md").await;
    assert_eq!(
        domain(n.svc.append_response(p, "response".into()).await),
        NotesError::OpenUnsaved
    );
    assert_eq!(n.read("chosen.md"), "keep\n");
}
#[tokio::test]
async fn a_replaced_vault_rejects_the_old_selection_even_at_the_same_path() {
    let n = fixture(&[("chosen.md", "keep\n")]).await;
    let p = intent(&n, "chosen.md").await;
    std::fs::rename(n.vault(), n.home.join("old-notes")).expect("owned rename");
    n.write("chosen.md", "replacement\n");
    assert_eq!(
        domain(n.svc.append_response(p, "response".into()).await),
        NotesError::ResponseTargetChanged
    );
    assert_eq!(n.read("chosen.md"), "replacement\n");
}
