//! Task 6 review follow-ups: how a window's `applied` reply is checked, the note
//! size limit on the encoded bytes (appends and rename rewrites), busy notes and
//! summary input bounds.
use crate::fixture::*;
use crate::*;
use devbox_core::HubMessage;
use devbox_documents::{Claimed, Expect, NOTE_LIMIT, SaveDocument};
use std::sync::Arc;
use std::time::Duration;

const NEW: &str = "### 오늘 한 일\n- new\n";

fn inbox(text: &str) -> AppendNote {
    AppendNote {
        target: AppendTarget::Inbox,
        text: text.into(),
        format: AppendFormat::Plain,
    }
}
async fn next_request(rx: &mut devbox_core::HubReceiver) -> RewriteRequest {
    match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        Ok(Some(HubMessage::Event { payload, .. })) => {
            serde_json::from_str(&payload).expect("rewrite request payload")
        }
        other => panic!("no rewrite request: {other:?}"),
    }
}
/// The window saves `saved` (or nothing) and answers `applied: true`.
async fn answer(n: &Nx, open: &Claimed, req: &RewriteRequest, saved: Option<&str>) -> T {
    if let Some(text) = saved {
        n.docs
            .save(SaveDocument {
                lease_id: req.lease_id.clone(),
                seq: req.journal_seq + 1,
                base_rev: open.rev.clone(),
                expect: Expect::Present,
                text: text.into(),
            })
            .await
            .map_err(|e| format!("{e:?}"))?;
    }
    let reply = RewriteReply {
        op_id: req.op_id.clone(),
        key: req.key.clone(),
        applied: true,
    };
    n.svc
        .rewrite_reply(reply)
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}
/// Appends `text` to the open inbox while the window answers with `saved`.
async fn append_answered(
    n: &Arc<Nx>,
    draft: &str,
    text: &'static str,
    saved: Option<&str>,
) -> Result<R<Appended>, Box<dyn std::error::Error>> {
    let open = n.open_note("inbox.md", "w1", Some((1, draft))).await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let task = {
        let n = n.clone();
        tokio::spawn(async move { n.svc.append(inbox(text)).await })
    };
    let req = next_request(&mut rx).await;
    answer(n, &open, &req, saved).await?;
    Ok(task.await?)
}

#[tokio::test]
async fn an_applied_reply_that_saved_other_text_is_refused() -> T {
    let n = Arc::new(fixture(&[("inbox.md", "a\n")]).await);
    let out = append_answered(&n, "a\nx\n", "hi", Some("a\nother\n")).await?;
    assert_eq!(domain(out), NotesError::OpenUnsaved);
    assert_eq!(n.read("inbox.md"), "a\nother\n");
    Ok(())
}

#[tokio::test]
async fn an_applied_reply_without_a_save_is_refused_even_if_the_entry_is_on_disk() -> T {
    // The disk already ends with the entry; the window's buffer does not have it.
    let n = Arc::new(fixture(&[("inbox.md", "a\nhi\n")]).await);
    let out = append_answered(&n, "a\nx\n", "hi", None).await?;
    assert_eq!(domain(out), NotesError::OpenUnsaved);
    assert_eq!(n.read("inbox.md"), "a\nhi\n");
    Ok(())
}

#[tokio::test]
async fn an_entry_saved_with_later_typing_counts_as_appended() -> T {
    // The window applies the entry, the user keeps typing, then it saves: the
    // entry is there (not at the end), so a retry must not add it twice.
    let n = Arc::new(fixture(&[("inbox.md", "a\n")]).await);
    let out = append_answered(&n, "a\nx\n", "hi", Some("a\nx\nhi\nmore typing\n")).await?;
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(n.read("inbox.md"), "a\nx\nhi\nmore typing\n");
    Ok(())
}

#[tokio::test]
async fn an_open_daily_note_summary_is_checked_after_an_applied_reply() -> T {
    let n = Arc::new(fixture(&[("daily/2026-10-21.md", "# d\n")]).await);
    let open = n
        .open_note("daily/2026-10-21.md", "w1", Some((1, "# d\nmine\n")))
        .await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let task = {
        let n = n.clone();
        tokio::spawn(async move { n.svc.insert_summary("2026-10-21", "pc-a", NEW).await })
    };
    let req = next_request(&mut rx).await;
    // Saved, but without the block.
    answer(&n, &open, &req, Some("# d\nmine\nother\n")).await?;
    assert_eq!(domain(task.await?), NotesError::OpenUnsaved);
    assert_eq!(n.read("daily/2026-10-21.md"), "# d\nmine\nother\n");
    Ok(())
}

/// LF text of `lines` lines whose CRLF form is just above the note limit.
fn near_limit(lines: usize) -> String {
    let line = "x".repeat(NOTE_LIMIT as usize / lines - 1);
    format!("{line}\n").repeat(lines)
}

#[tokio::test]
async fn crlf_notes_are_limited_by_their_encoded_size() -> T {
    let text = near_limit(200_000);
    assert!(text.len() as u64 <= NOTE_LIMIT - 10, "LF form fits");
    let crlf = text.replace('\n', "\r\n");
    let n = fixture(&[]).await;
    // Just under the limit on disk: readable and editable.
    let disk = &crlf[..crlf.len() - (crlf.len() - NOTE_LIMIT as usize + 4)];
    let disk = &disk[..disk.rfind("\r\n").expect("line end") + 2];
    n.write("inbox.md", disk);
    let r = n.svc.append(inbox(&"y\n".repeat(10))).await;
    assert_eq!(domain(r), NotesError::TooLarge { limit: NOTE_LIMIT });
    assert_eq!(std::fs::read(n.vault().join("inbox.md"))?, disk.as_bytes());
    // An LF note of the same text length still takes a small entry.
    let lf = disk.replace("\r\n", "\n");
    n.write("inbox.md", &lf);
    let r = n.svc.append(inbox("y")).await;
    assert!(r.is_ok(), "{r:?}");
    Ok(())
}

#[tokio::test]
async fn a_rename_rewrite_never_grows_a_crlf_note_past_the_limit() -> T {
    let n = fixture(&[("a.md", "x\n")]).await;
    let body = near_limit(200_000).replace('\n', "\r\n");
    let room = NOTE_LIMIT as usize - "[[a]]\r\n".len() - 2;
    let body = &body[..body[..room].rfind("\r\n").expect("line end") + 2];
    let b = format!("[[a]]\r\n{body}");
    assert!(b.len() as u64 <= NOTE_LIMIT);
    n.write("b.md", &b);
    n.svc.rescan().await.map_err(|e| format!("{e:?}"))?;
    let status = n.renamed("a.md", "a-much-longer-name.md").await?;
    assert_eq!(failed(&status), [("b.md", RewriteFailure::TooLarge)]);
    assert_eq!(std::fs::read(n.vault().join("b.md"))?, b.as_bytes());
    Ok(())
}

#[tokio::test]
async fn a_note_a_rename_is_rewriting_is_busy() -> T {
    let n = fixture(&[("inbox.md", "a\n"), ("daily/2026-10-21.md", "# d\n")]).await;
    n.svc
        .s
        .renames
        .paths
        .lock()
        .expect("rename paths")
        .extend(["inbox.md".into(), "daily/2026-10-21.md".into()]);
    assert_eq!(domain(n.svc.append(inbox("hi")).await), NotesError::Busy);
    let r = n.svc.insert_summary("2026-10-21", "pc-a", NEW).await;
    assert_eq!(domain(r), NotesError::Busy);
    assert_eq!(n.read("inbox.md"), "a\n");
    assert_eq!(n.read("daily/2026-10-21.md"), "# d\n");
    Ok(())
}

#[tokio::test]
async fn oversized_or_nul_summaries_are_refused_before_anything_is_created() -> T {
    let n = fixture(&[]).await;
    let big = format!("- {}\n", "x".repeat(256 * 1024));
    for markdown in [big.as_str(), "- a\0b\n"] {
        let r = n.svc.insert_summary("2026-10-21", "pc-a", markdown).await;
        assert_eq!(domain(r), NotesError::InvalidInput);
    }
    assert!(!n.vault().join("daily/2026-10-21.md").exists());
    Ok(())
}
