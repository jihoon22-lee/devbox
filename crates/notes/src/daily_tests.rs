//! Task 6: daily notes, the per-PC summary block and appends (inbox, daily).
use crate::daily::{SummaryError, capture_entry, single_edit};
use crate::fixture::*;
use crate::*;
use devbox_core::HubMessage;
use std::sync::Arc;
use std::time::Duration;

const TWO_PCS: &str = "# 2026-10-21\n\n## pc-a\n<!-- devbox:summary:pc-a -->\nold\n<!-- /devbox:summary -->\n\n## pc-b\n<!-- devbox:summary:pc-b -->\nkeep\n<!-- /devbox:summary -->\n메모\n";
const NEW: &str = "### 오늘 한 일\n- new\n";

fn set_today(n: &Nx, date: &str, time: &str) {
    *n.today.lock().expect("fixture today") = (date.into(), time.into());
}
fn append(target: AppendTarget, text: &str, format: AppendFormat) -> AppendNote {
    AppendNote {
        target,
        text: text.into(),
        format,
    }
}

#[test]
fn summary_block_is_replaced_in_place_and_other_pc_blocks_survive() {
    let out = replace_summary(TWO_PCS, "pc-a", NEW).expect("well-formed blocks");
    assert!(
        out.contains(
            "<!-- devbox:summary:pc-a -->\n### 오늘 한 일\n- new\n<!-- /devbox:summary -->"
        ),
        "{out}"
    );
    assert!(out.contains("keep") && out.ends_with("메모\n") && !out.contains("old"));
    assert_eq!(
        out,
        TWO_PCS.replace("pc-a -->\nold\n", "pc-a -->\n### 오늘 한 일\n- new\n"),
        "nothing else moves"
    );
}

#[test]
fn user_text_outside_markers_is_preserved() {
    let doc = "앞 글\n\n## pc-a\n내 메모\n<!-- devbox:summary:pc-a -->\nold\n<!-- /devbox:summary -->\n아래 메모\n\n끝\n";
    let out = replace_summary(doc, "pc-a", NEW).expect("well-formed block");
    assert_eq!(
        out,
        "앞 글\n\n## pc-a\n내 메모\n<!-- devbox:summary:pc-a -->\n### 오늘 한 일\n- new\n<!-- /devbox:summary -->\n아래 메모\n\n끝\n"
    );
    // A heading that exists without a block gets the block right below it; the
    // user's text under it stays as it was.
    let doc = "# d\n\n## pc-a\n내 메모\n";
    let out = replace_summary(doc, "pc-a", NEW).expect("heading");
    assert_eq!(
        out,
        "# d\n\n## pc-a\n<!-- devbox:summary:pc-a -->\n### 오늘 한 일\n- new\n<!-- /devbox:summary -->\n내 메모\n"
    );
}

#[test]
fn missing_pc_heading_is_appended_once() {
    let doc = "# 2026-10-21\n\n메모";
    let once = replace_summary(doc, "pc-a", NEW).expect("append");
    assert_eq!(
        once,
        "# 2026-10-21\n\n메모\n\n## pc-a\n<!-- devbox:summary:pc-a -->\n### 오늘 한 일\n- new\n<!-- /devbox:summary -->\n"
    );
    let twice = replace_summary(&once, "pc-a", NEW).expect("replace");
    assert_eq!(twice, once, "a second run replaces, never adds");
    assert_eq!(twice.matches("## pc-a").count(), 1);
    let empty = replace_summary("", "pc-a", "- x").expect("empty note");
    assert_eq!(
        empty,
        "## pc-a\n<!-- devbox:summary:pc-a -->\n- x\n<!-- /devbox:summary -->\n"
    );
}

#[test]
fn a_pc_name_prefix_never_matches_another_pc() {
    let doc = "## pc-a\n<!-- devbox:summary:pc-a -->\nA\n<!-- /devbox:summary -->\n";
    let out = replace_summary(doc, "pc", NEW).expect("different pc");
    assert!(out.starts_with(doc), "pc-a's block is untouched: {out}");
    assert!(out.ends_with(
        "## pc\n<!-- devbox:summary:pc -->\n### 오늘 한 일\n- new\n<!-- /devbox:summary -->\n"
    ));
}

#[test]
fn broken_or_nested_markers_are_refused_instead_of_eating_text() {
    // No end marker: replacing "to the end" would delete the user's text.
    let open = "## pc-a\n<!-- devbox:summary:pc-a -->\nold\n\n사용자 글\n";
    assert_eq!(
        replace_summary(open, "pc-a", NEW),
        Err(SummaryError::Broken)
    );
    // The next end marker belongs to another PC's block.
    let nested = "<!-- devbox:summary:pc-a -->\nold\n<!-- devbox:summary:pc-b -->\nB\n<!-- /devbox:summary -->\n";
    assert_eq!(
        replace_summary(nested, "pc-a", NEW),
        Err(SummaryError::Broken)
    );
}

#[test]
fn block_text_and_pc_names_cannot_forge_markers() {
    for content in [
        "- x <!-- /devbox:summary -->\n",
        "<!-- devbox:summary:pc-b -->\n",
        "- a --> b\n",
    ] {
        assert_eq!(
            replace_summary(TWO_PCS, "pc-a", content),
            Err(SummaryError::InvalidContent),
            "{content}"
        );
    }
    for pc in ["", " ", "a -->", "a<!--b", "two\nlines", &"p".repeat(129)] {
        assert_eq!(
            replace_summary(TWO_PCS, pc, NEW),
            Err(SummaryError::InvalidPc),
            "{pc:?}"
        );
    }
}

#[test]
fn capture_entries_and_single_edits() {
    assert_eq!(capture_entry("09:05", "a\nb"), "- 09:05 a\n  b\n");
    assert_eq!(
        capture_entry("09:05", "a\r\n\r\nb\n\n"),
        "- 09:05 a\n\n  b\n"
    );
    let edit = single_edit("가a\nb\n", "가a\nb\n- x\n");
    assert_eq!(
        (edit.from_utf16, edit.to_utf16, edit.text.as_str()),
        (5, 5, "- x\n")
    );
    let edit = single_edit("x\nold\ny\n", "x\nnew\ny\n");
    assert_eq!(
        (edit.from_utf16, edit.to_utf16, edit.text.as_str()),
        (2, 5, "new")
    );
}

#[tokio::test]
async fn daily_creates_once_from_the_template_or_the_default() -> T {
    let n = fixture(&[]).await;
    let first = n.svc.daily(DailyParams { date: None }).await;
    assert_eq!(
        first.as_ref().ok(),
        Some(&DailyNote {
            path: "daily/2026-10-22.md".into(),
            created: true
        })
    );
    let body = "---\ntitle: \"\"\n---\n# 2026-10-22\n";
    assert_eq!(n.read("daily/2026-10-22.md"), body);
    // AR-F10: the empty title falls back to the file name.
    assert_eq!(n.index_title("daily/2026-10-22.md").await?, "2026-10-22");
    n.write("daily/2026-10-22.md", "mine\n");
    let again = n.svc.daily(DailyParams { date: None }).await;
    assert_eq!(again.ok().map(|d| d.created), Some(false));
    assert_eq!(n.read("daily/2026-10-22.md"), "mine\n", "never rewritten");

    n.write("templates/daily.md", "# {{date}} @ {{pc}}\n");
    let past = n.svc.daily(DailyParams {
        date: Some("2026-10-19".into()),
    });
    assert_eq!(past.await.ok().map(|d| d.created), Some(true));
    assert_eq!(n.read("daily/2026-10-19.md"), "# 2026-10-19 @ desk\n");
    for bad in ["2026-02-30", "2026-1-05", "../x", ""] {
        let r = n.svc.daily(DailyParams {
            date: Some(bad.into()),
        });
        assert_eq!(domain(r.await), NotesError::InvalidInput, "{bad}");
    }
    Ok(())
}

#[tokio::test]
async fn append_capture_formats_time_and_rejects_nul() -> T {
    let n = fixture(&[("inbox.md", "# inbox\n- old")]).await;
    set_today(&n, "2026-10-22", "09:05");
    let out = n
        .svc
        .append(append(AppendTarget::Inbox, "a\nb", AppendFormat::Capture))
        .await;
    assert_eq!(out.as_ref().ok().map(|a| a.path.as_str()), Some("inbox.md"));
    assert_eq!(n.read("inbox.md"), "# inbox\n- old\n- 09:05 a\n  b\n");
    for text in ["a\0b", "", " \n", &"x".repeat(64 * 1024 + 1)] {
        let r = n
            .svc
            .append(append(AppendTarget::Inbox, text, AppendFormat::Capture));
        assert_eq!(domain(r.await), NotesError::InvalidInput);
    }
    assert_eq!(n.read("inbox.md"), "# inbox\n- old\n- 09:05 a\n  b\n");
    // A missing inbox is created with just the entry.
    std::fs::remove_file(n.vault().join("inbox.md"))?;
    let out = n
        .svc
        .append(append(AppendTarget::Inbox, "hi", AppendFormat::Plain))
        .await;
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(n.read("inbox.md"), "hi\n");
    Ok(())
}

#[tokio::test]
async fn append_to_daily_creates_today_first_and_keeps_crlf() -> T {
    let n = fixture(&[]).await;
    let out = n
        .svc
        .append(append(AppendTarget::Daily, "hi", AppendFormat::Plain))
        .await;
    assert_eq!(
        out.as_ref().ok().map(|a| a.path.as_str()),
        Some("daily/2026-10-22.md")
    );
    assert_eq!(
        n.read("daily/2026-10-22.md"),
        "---\ntitle: \"\"\n---\n# 2026-10-22\nhi\n"
    );
    n.write("daily/2026-10-22.md", "# d\r\n메모\r\n");
    let out = n
        .svc
        .append(append(AppendTarget::Daily, "x\ny", AppendFormat::Plain))
        .await;
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(n.read("daily/2026-10-22.md"), "# d\r\n메모\r\nx\r\ny\r\n");
    Ok(())
}

#[tokio::test]
async fn insert_summary_keeps_other_pcs_user_text_and_crlf() -> T {
    let n = fixture(&[]).await;
    let crlf = TWO_PCS.replace('\n', "\r\n");
    n.write("daily/2026-10-21.md", &crlf);
    let out = n.svc.insert_summary("2026-10-21", "pc-a", NEW).await;
    assert_eq!(
        out.as_ref().ok().map(|s| s.path.as_str()),
        Some("daily/2026-10-21.md")
    );
    let expected = TWO_PCS.replace("pc-a -->\nold\n", "pc-a -->\n### 오늘 한 일\n- new\n");
    assert_eq!(
        n.read("daily/2026-10-21.md"),
        expected.replace('\n', "\r\n")
    );
    // A missing daily note is created first; the block goes at the end.
    let out = n.svc.insert_summary("2026-10-20", "desk", "- x\n").await;
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(
        n.read("daily/2026-10-20.md"),
        "---\ntitle: \"\"\n---\n# 2026-10-20\n\n## desk\n<!-- devbox:summary:desk -->\n- x\n<!-- /devbox:summary -->\n"
    );
    Ok(())
}

#[tokio::test]
async fn broken_markers_and_forged_text_leave_the_note_unchanged() -> T {
    let open = "## pc-a\n<!-- devbox:summary:pc-a -->\nold\n\n사용자 글\n";
    let n = fixture(&[("daily/2026-10-21.md", open)]).await;
    let r = n.svc.insert_summary("2026-10-21", "pc-a", NEW).await;
    assert_eq!(domain(r), NotesError::SummaryBroken);
    let r = n
        .svc
        .insert_summary("2026-10-21", "pc-b", "- <!-- /devbox:summary -->\n")
        .await;
    assert_eq!(domain(r), NotesError::InvalidInput);
    let r = n.svc.insert_summary("2026-10-21", "a -->", NEW).await;
    assert_eq!(domain(r), NotesError::InvalidInput);
    assert_eq!(n.read("daily/2026-10-21.md"), open);
    // A refused summary creates no daily note either.
    let r = n.svc.insert_summary("2026-10-23", "pc-a", "<!--\n").await;
    assert_eq!(domain(r), NotesError::InvalidInput);
    assert!(!n.vault().join("daily/2026-10-23.md").exists());
    Ok(())
}

#[tokio::test]
async fn an_external_edit_between_read_and_write_is_kept() -> T {
    let n = fixture(&[("inbox.md", "a\n")]).await;
    let (vault, fired) = (
        n.vault(),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let once = fired.clone();
    *n.svc.s.after_read.lock().expect("hook") = Some(Arc::new(move || {
        if !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::fs::write(vault.join("inbox.md"), "a\nexternal\n").expect("external edit");
        }
    }));
    let out = n
        .svc
        .append(append(AppendTarget::Inbox, "mine", AppendFormat::Plain))
        .await;
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(n.read("inbox.md"), "a\nexternal\nmine\n");
    Ok(())
}

async fn next_request(rx: &mut devbox_core::HubReceiver) -> RewriteRequest {
    match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        Ok(Some(HubMessage::Event { payload, .. })) => {
            serde_json::from_str(&payload).expect("rewrite request payload")
        }
        other => panic!("no rewrite request: {other:?}"),
    }
}

#[tokio::test]
async fn dirty_open_target_gets_a_rewrite_request_instead_of_a_disk_write() -> T {
    let n = Arc::new(fixture(&[("inbox.md", "a\n")]).await);
    let open = n.open_note("inbox.md", "w1", Some((3, "a\ndraft\n"))).await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let task = {
        let n = n.clone();
        tokio::spawn(async move {
            let p = append(AppendTarget::Inbox, "hi", AppendFormat::Plain);
            n.svc.append(p).await
        })
    };
    let req = next_request(&mut rx).await;
    assert_eq!(req.key, vault_key("inbox.md"));
    assert_eq!(
        (req.lease_id.as_str(), req.journal_seq),
        (open.lease_id.as_deref().unwrap_or(""), 3)
    );
    let edit = TextEdit {
        from_utf16: 8,
        to_utf16: 8,
        text: "hi\n".into(),
    };
    assert_eq!(req.edits, [edit]);
    assert_eq!(n.read("inbox.md"), "a\n", "not written behind the window");
    n.docs
        .save(devbox_documents::SaveDocument {
            lease_id: req.lease_id.clone(),
            seq: 4,
            base_rev: open.rev.clone(),
            expect: devbox_documents::Expect::Present,
            text: "a\ndraft\nhi\n".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let reply = RewriteReply {
        op_id: req.op_id.clone(),
        key: req.key.clone(),
        applied: true,
    };
    n.svc
        .rewrite_reply(reply)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let out = task.await?;
    assert_eq!(out.ok().map(|a| a.path), Some("inbox.md".into()));
    assert_eq!(n.read("inbox.md"), "a\ndraft\nhi\n", "exactly one entry");

    // No answer within 30 s (injected clock): refused, disk untouched.
    n.open_note("inbox.md", "w1", Some((5, "a\ndraft\nhi\nmore\n")))
        .await;
    let task = {
        let n = n.clone();
        tokio::spawn(async move {
            let p = append(AppendTarget::Inbox, "late", AppendFormat::Plain);
            n.svc.append(p).await
        })
    };
    next_request(&mut rx).await;
    n.advance(31_000);
    assert_eq!(domain(task.await?), NotesError::OpenUnsaved);
    assert_eq!(n.read("inbox.md"), "a\ndraft\nhi\n");
    Ok(())
}

#[tokio::test]
async fn an_unsaved_draft_without_a_window_is_not_written_behind() -> T {
    let n = fixture(&[("inbox.md", "a\n")]).await;
    let open = n.open_note("inbox.md", "w1", Some((2, "a\ndraft\n"))).await;
    let lease_id = open.lease_id.clone().expect("lease");
    n.docs
        .release(devbox_documents::LeaseRef { lease_id })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let r = n
        .svc
        .append(append(AppendTarget::Inbox, "hi", AppendFormat::Plain))
        .await;
    assert_eq!(domain(r), NotesError::OpenUnsaved);
    let r = n.svc.insert_summary("2026-10-22", "desk", NEW).await;
    assert!(r.is_ok(), "another note is not affected: {r:?}");
    assert_eq!(n.read("inbox.md"), "a\n");
    Ok(())
}

#[tokio::test]
async fn an_applied_reply_without_the_saved_entry_is_refused() -> T {
    let n = Arc::new(fixture(&[("inbox.md", "a\n")]).await);
    n.open_note("inbox.md", "w1", Some((1, "a\ndraft\n"))).await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let task = {
        let n = n.clone();
        tokio::spawn(async move {
            let p = append(AppendTarget::Inbox, "hi", AppendFormat::Plain);
            n.svc.append(p).await
        })
    };
    let req = next_request(&mut rx).await;
    // The window says "applied" but never saved the entry.
    let reply = RewriteReply {
        op_id: req.op_id,
        key: req.key,
        applied: true,
    };
    n.svc
        .rewrite_reply(reply)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(domain(task.await?), NotesError::OpenUnsaved);
    assert_eq!(n.read("inbox.md"), "a\n");
    Ok(())
}

#[tokio::test]
async fn an_open_note_without_unsaved_edits_is_appended_on_disk() -> T {
    let n = fixture(&[("inbox.md", "a\n")]).await;
    n.open_note("inbox.md", "w1", None).await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let out = n
        .svc
        .append(append(AppendTarget::Inbox, "hi", AppendFormat::Plain))
        .await;
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(n.read("inbox.md"), "a\nhi\n");
    let asked = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(asked.is_err(), "no rewrite request for a clean open note");
    Ok(())
}
