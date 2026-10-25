//! Response-to-note admission and target identity, using an owned vault.
use crate::fixture::*;
use crate::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}
async fn intent(n: &Nx, target: ResponseNoteTarget) -> AppendResponseNote {
    let selected = n
        .svc
        .select_response_target(SelectResponseNote { target })
        .await
        .expect("select");
    AppendResponseNote {
        history_id: id(),
        selection: selected.selection,
        operation_id: id(),
    }
}
fn existing(path: &str) -> ResponseNoteTarget {
    ResponseNoteTarget::Existing { path: path.into() }
}

#[tokio::test]
async fn response_note_admission_accepts_more_than_the_command_limit() {
    let n = fixture(&[]).await;
    let text = "a".repeat(64 * 1024 + 1);
    assert!(devbox_core::input::validate_command_text(&text).is_err());
    let p = intent(&n, ResponseNoteTarget::Daily).await;
    let result = n
        .svc
        .append_response(p, text.clone())
        .await
        .expect("note admission");
    assert!(n.read(&result.path).contains(&text));
    assert_eq!(
        domain(
            n.svc
                .append(AppendNote {
                    target: AppendTarget::Inbox,
                    text,
                    format: AppendFormat::Plain
                })
                .await
        ),
        NotesError::InvalidInput
    );
}

#[tokio::test]
async fn response_note_target_can_identify_an_existing_note() {
    let n = fixture(&[("chosen.md", "keep\n"), ("other.md", "other\n")]).await;
    let target: ResponseNoteTarget =
        serde_json::from_str(r#"{"kind":"existing","path":"chosen.md"}"#).expect("typed target");
    let p = intent(&n, target).await;
    let result = n
        .svc
        .append_response(p, "response body".into())
        .await
        .expect("append");
    assert_eq!(result.path, "chosen.md");
    assert!(n.read("chosen.md").starts_with("keep\n"));
    assert_eq!(n.read("other.md"), "other\n");
}

#[tokio::test]
async fn selected_missing_or_replaced_note_is_not_created_or_adopted() {
    let n = fixture(&[("chosen.md", "original\n")]).await;
    let p = intent(&n, existing("chosen.md")).await;
    std::fs::rename(n.vault().join("chosen.md"), n.vault().join("moved.md")).expect("rename");
    assert_eq!(
        domain(n.svc.append_response(p.clone(), "response".into()).await),
        NotesError::ResponseTargetChanged
    );
    assert!(!n.vault().join("chosen.md").exists());
    n.write("chosen.md", "replacement\n");
    assert_eq!(
        domain(n.svc.append_response(p, "response".into()).await),
        NotesError::ResponseTargetChanged
    );
    assert_eq!(n.read("chosen.md"), "replacement\n");
}

#[tokio::test]
async fn target_deleted_between_cas_read_and_write_is_not_recreated() {
    let n = fixture(&[("chosen.md", "keep\n")]).await;
    let p = intent(&n, existing("chosen.md")).await;
    let path = n.vault().join("chosen.md");
    *n.svc.s.after_read.lock().expect("hook") = Some(Arc::new(move || {
        let _ = std::fs::remove_file(&path);
    }));
    assert_eq!(
        domain(n.svc.append_response(p, "response".into()).await),
        NotesError::ResponseTargetChanged
    );
    assert!(!n.vault().join("chosen.md").exists());
}

#[tokio::test]
async fn external_content_change_is_preserved_by_the_existing_cas_retry() {
    let n = fixture(&[("chosen.md", "keep\n")]).await;
    let p = intent(&n, existing("chosen.md")).await;
    let path = n.vault().join("chosen.md");
    let fired = AtomicBool::new(false);
    *n.svc.s.after_read.lock().expect("hook") = Some(Arc::new(move || {
        if !fired.swap(true, Ordering::SeqCst) {
            std::fs::write(&path, "keep\nexternal\n").expect("edit");
        }
    }));
    n.svc
        .append_response(p, "response".into())
        .await
        .expect("CAS retry");
    let text = n.read("chosen.md");
    assert!(text.starts_with("keep\nexternal\n"));
    assert_eq!(text.matches("devbox:api-response:").count(), 1);
}

#[tokio::test]
async fn forged_outside_alias_and_expired_selections_refuse_before_writing() {
    let n = fixture(&[("chosen.md", "keep\n")]).await;
    for path in [
        "../outside.md",
        "/tmp/outside.md",
        "source.rs",
        ".trash/note.md",
    ] {
        assert!(
            n.svc
                .select_response_target(SelectResponseNote {
                    target: existing(path)
                })
                .await
                .is_err()
        );
    }
    std::os::unix::fs::symlink(n.vault().join("chosen.md"), n.vault().join("alias.md"))
        .expect("owned alias");
    assert!(
        n.svc
            .select_response_target(SelectResponseNote {
                target: existing("alias.md")
            })
            .await
            .is_err()
    );
    let mut p = intent(&n, existing("chosen.md")).await;
    p.selection = id();
    assert_eq!(
        domain(n.svc.append_response(p, "response".into()).await),
        NotesError::ResponseTargetChanged
    );
    let p = intent(&n, existing("chosen.md")).await;
    n.advance(600_001);
    assert_eq!(
        domain(n.svc.append_response(p, "response".into()).await),
        NotesError::ResponseTargetChanged
    );
    assert_eq!(n.read("chosen.md"), "keep\n");
}

#[tokio::test]
async fn response_admission_and_serialized_note_limits_preserve_bom_crlf() {
    let n = fixture(&[]).await;
    n.write("chosen.md", b"\xef\xbb\xbfkeep\r\n");
    let p = intent(&n, existing("chosen.md")).await;
    n.svc
        .append_response(p, "한글\n본문".into())
        .await
        .expect("append");
    let bytes = std::fs::read(n.vault().join("chosen.md")).expect("bytes");
    assert!(bytes.starts_with(b"\xef\xbb\xbfkeep\r\n"));
    assert!(
        !String::from_utf8(bytes)
            .expect("utf8")
            .replace("\r\n", "")
            .contains('\n')
    );
    let p = intent(&n, existing("chosen.md")).await;
    assert_eq!(
        domain(
            n.svc
                .append_response(p, "x".repeat(crate::response_append::RESPONSE_LIMIT + 1))
                .await
        ),
        NotesError::InvalidInput
    );
    let p = intent(&n, existing("chosen.md")).await;
    assert_eq!(
        domain(n.svc.append_response(p, "bad\0body".into()).await),
        NotesError::InvalidInput
    );
    n.write("maximum.md", "");
    let p = intent(&n, existing("maximum.md")).await;
    let maximum = "x".repeat(crate::response_append::RESPONSE_LIMIT);
    n.svc
        .append_response(p, maximum.clone())
        .await
        .expect("full bounded admission");
    assert!(n.read("maximum.md").contains(&maximum));
    n.write(
        "full.md",
        "x".repeat(devbox_documents::NOTE_LIMIT as usize - 10),
    );
    let p = intent(&n, existing("full.md")).await;
    assert_eq!(
        domain(n.svc.append_response(p, "response".into()).await),
        NotesError::TooLarge {
            limit: devbox_documents::NOTE_LIMIT
        }
    );
    assert_eq!(
        n.read("full.md").len(),
        devbox_documents::NOTE_LIMIT as usize - 10
    );
}

#[tokio::test]
async fn repeated_intent_is_once_and_fresh_intent_can_append_the_same_response_again() {
    let n = fixture(&[("chosen.md", "keep\n")]).await;
    let p = intent(&n, existing("chosen.md")).await;
    let one = n
        .svc
        .append_response(p.clone(), "response".into())
        .await
        .expect("first");
    assert_eq!(
        n.svc
            .append_response(p.clone(), "response".into())
            .await
            .expect("retry"),
        one
    );
    let mut forged = p.clone();
    forged.history_id = id();
    assert_eq!(
        domain(n.svc.response_append_result(&forged).await),
        NotesError::AppendOperationMismatch
    );
    let mut fresh = intent(&n, existing("chosen.md")).await;
    fresh.history_id = p.history_id;
    n.svc
        .append_response(fresh, "response".into())
        .await
        .expect("new intent");
    assert_eq!(
        n.read("chosen.md").matches("devbox:api-response:").count(),
        2
    );
}

#[tokio::test]
async fn a_replaced_receipt_fifo_is_refused_without_waiting_or_reappending() {
    let n = fixture(&[("chosen.md", "keep\n")]).await;
    let p = intent(&n, existing("chosen.md")).await;
    n.svc
        .append_response(p.clone(), "response".into())
        .await
        .expect("first");
    let path = n
        .svc
        .s
        .rename_dir
        .join("response-appends")
        .join(format!("{}.json", p.operation_id));
    std::fs::remove_file(&path).expect("owned receipt");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("owned FIFO")
            .success()
    );
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(300),
        n.svc.response_append_result(&p),
    )
    .await
    .expect("nonblocking receipt read");
    assert!(result.is_err());
    assert_eq!(
        n.read("chosen.md").matches("devbox:api-response:").count(),
        1
    );
}
