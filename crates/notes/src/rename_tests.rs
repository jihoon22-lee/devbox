//! Task 5 rename flows: link rewrites, conflicts, crash resume (AR-D01) and open
//! documents. Refusals and guards are in `rename_guard_tests.rs`.
use crate::fixture::*;
use crate::*;
use devbox_core::HubMessage;
use std::time::Duration;

fn srcs(b: &Backlinks) -> Vec<&str> {
    let mut v: Vec<&str> = b.items.iter().map(|i| i.src.as_str()).collect();
    v.dedup();
    v
}

#[tokio::test]
async fn rename_rewrites_wiki_and_markdown_links_in_all_backlinking_notes() -> T {
    let mut n = fixture(&[
        ("a.md", "x\n[b](b.md) [[b]]\n"),
        ("b.md", "[[a]] [[a|별칭]] [[a#h]] [[ a | 공백 ]] [[A]]\n"),
        (
            "sub/c.md",
            "[t](../a.md) [t](./../a.md#h) [w](https://e/a.md)\n",
        ),
        ("d.md", "[t](a.md) [t](./sub/../a.md)\r\n"),
        ("e.md", "---\ntitle: Stable\n---\n[[Stable]]\n"),
        ("f.md", "[[b]]\n"),
    ])
    .await;
    let status = n.renamed("a.md", "z/a2.md").await?;
    assert_eq!((status.state, status.failed.len()), (RenameState::Done, 0));
    assert!(!n.vault().join("a.md").exists());
    // The moved note's own relative link follows it; its wiki link still resolves.
    assert_eq!(n.read("z/a2.md"), "x\n[b](../b.md) [[b]]\n");
    // Alias, heading and inner spaces are kept; only the note part changes.
    assert_eq!(
        n.read("b.md"),
        "[[a2]] [[a2|별칭]] [[a2#h]] [[ a2 | 공백 ]] [[a2]]\n"
    );
    assert_eq!(
        n.read("sub/c.md"),
        "[t](../z/a2.md) [t](../z/a2.md#h) [w](https://e/a.md)\n"
    );
    assert_eq!(n.read("d.md"), "[t](z/a2.md) [t](z/a2.md)\r\n", "CRLF kept");
    assert_eq!(n.read("e.md"), "---\ntitle: Stable\n---\n[[Stable]]\n");
    assert_eq!(n.read("f.md"), "[[b]]\n");
    assert!(n.rename_journals().is_empty(), "{:?}", n.rename_journals());
    // The index follows without waiting for a rescan.
    let back = n
        .svc
        .backlinks(NotePath {
            path: "z/a2.md".into(),
        })
        .await;
    let back = back.map_err(|e| format!("{e:?}"))?;
    assert_eq!(srcs(&back), ["b.md", "d.md", "sub/c.md"]);
    n.reopen().await?;
    assert_eq!(
        n.read("b.md"),
        "[[a2]] [[a2|별칭]] [[a2#h]] [[ a2 | 공백 ]] [[a2]]\n"
    );
    Ok(())
}

#[tokio::test]
async fn title_links_stay_and_an_ambiguous_new_name_is_written_as_a_vault_path() -> T {
    let n = fixture(&[
        ("a.md", "---\ntitle: Alpha\n---\nx\n"),
        ("other/a2.md", "o\n"),
        ("b.md", "[[a]] [[Alpha]]\n"),
        ("other/n.md", "[[a2]]\n"),
    ])
    .await;
    let status = n.renamed("a.md", "z/a2.md").await?;
    assert_eq!(status.state, RenameState::Done);
    // `a2` would be ambiguous from the vault root, so the path form is written.
    assert_eq!(n.read("b.md"), "[[z/a2]] [[Alpha]]\n");
    // Same-folder rule keeps this one on other/a2.md.
    assert_eq!(n.read("other/n.md"), "[[a2]]\n");
    Ok(())
}

#[tokio::test]
async fn links_the_new_name_would_capture_keep_their_target() -> T {
    let n = fixture(&[
        ("a.md", "x\n"),
        ("other/a2.md", "o\n"),
        ("other/n.md", "[[a2]]\n"),
        ("b.md", "[[a]]\n"),
    ])
    .await;
    let status = n.renamed("a.md", "a2.md").await?;
    assert_eq!(status.state, RenameState::Done);
    // The new root `a2.md` would win by exact path key; the link keeps other/a2.md.
    assert_eq!(n.read("other/n.md"), "[[other/a2]]\n");
    assert_eq!(n.read("b.md"), "[[a2]]\n");
    let r = n.resolve_from("other/a2", Some("other/n.md")).await?;
    assert_eq!(r.paths, ["other/a2.md"]);
    Ok(())
}

#[tokio::test]
async fn markdown_destinations_are_relative_and_reference_links_are_listed() -> T {
    let n = fixture(&[
        ("a.md", "x\n"),
        ("d/n.md", "[t](../a.md) [u](<../a.md>)\n"),
        ("r.md", "[t][ref]\n\n[ref]: a.md\n"),
    ])
    .await;
    let status = n.renamed("a.md", "new dir/a (b).md").await?;
    assert_eq!(status.state, RenameState::Incomplete);
    assert_eq!(failed(&status), [("r.md", RewriteFailure::Unsupported)]);
    assert_eq!(
        n.read("d/n.md"),
        "[t](../new%20dir/a%20%28b%29.md) [u](<../new dir/a (b).md>)\n"
    );
    assert_eq!(n.read("r.md"), "[t][ref]\n\n[ref]: a.md\n");
    let md = n
        .svc
        .resolve(ResolveLink {
            target: "../new%20dir/a%20%28b%29.md".into(),
            kind: LinkKind::Md,
            from: Some("d/n.md".into()),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(md.paths, ["new dir/a (b).md"]);
    Ok(())
}

#[tokio::test]
async fn conflicting_file_is_reported_not_overwritten() -> T {
    let n = fixture(&[("a.md", "x\n"), ("b.md", "[[a]]\n"), ("c.md", "[[a]]\n")]).await;
    let vault = n.vault();
    n.svc.on_planned(move || {
        std::fs::write(vault.join("b.md"), "[[a]] external\n").expect("external edit");
    });
    let status = n.renamed("a.md", "a2.md").await?;
    assert_eq!(status.state, RenameState::Incomplete);
    assert_eq!(failed(&status), [("b.md", RewriteFailure::Conflict)]);
    assert_eq!(n.read("b.md"), "[[a]] external\n");
    assert_eq!(n.read("c.md"), "[[a2]]\n");
    assert!(n.rename_journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn crash_right_after_the_move_resumes_link_rewrites() -> T {
    let mut n = fixture(&[("a.md", "x"), ("b.md", "[[a]]"), ("c.md", "[a](a.md)")]).await;
    let op = n
        .svc
        .rename_with(rename("a.md", "z/a2.md"), Some(StopAt::AfterMove))
        .await
        .map_err(|e| format!("{e:?}"))?;
    // Simulated exit right after the move: the note moved, no link is fixed yet.
    assert!(n.vault().join("z/a2.md").exists() && !n.vault().join("a.md").exists());
    assert_eq!(n.read("b.md"), "[[a]]");
    let journal = n.data.join(format!("notes-rename/{}.json", op.op_id));
    assert!(journal.exists());
    n.reopen().await?;
    let status = n.settled(&op.op_id).await?;
    assert_eq!(status.state, RenameState::Done);
    assert_eq!(n.read("b.md"), "[[a2]]");
    assert_eq!(n.read("c.md"), "[a](z/a2.md)");
    assert!(!journal.exists());
    Ok(())
}

#[tokio::test]
async fn journal_is_durable_before_the_move() -> T {
    let mut n = fixture(&[("a.md", "x"), ("b.md", "[[a]]")]).await;
    let op = n
        .svc
        .rename_with(rename("a.md", "z/a2.md"), Some(StopAt::BeforeMove))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let journal = n.data.join(format!("notes-rename/{}.json", op.op_id));
    let text = std::fs::read_to_string(&journal)?;
    assert!(text.contains("\"moved\":false"), "{text}");
    assert!(n.vault().join("a.md").exists() && !n.vault().join("z").exists());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&journal)?.permissions().mode() & 0o777,
        0o600
    );
    let dir = n.data.join("notes-rename");
    assert_eq!(std::fs::metadata(dir)?.permissions().mode() & 0o777, 0o700);
    // The next start finishes what the journal promised.
    n.reopen().await?;
    assert_eq!(n.settled(&op.op_id).await?.state, RenameState::Done);
    assert_eq!(n.read("z/a2.md"), "x");
    assert_eq!(n.read("b.md"), "[[a2]]");
    assert!(!journal.exists());
    Ok(())
}

#[tokio::test]
async fn crash_after_the_first_rewrite_resumes_the_rest() -> T {
    let notes = [
        ("a.md", "x"),
        ("b.md", "[[a]]"),
        ("c.md", "[[a]]"),
        ("d.md", "[[a]]"),
    ];
    let mut n = fixture(&notes).await;
    let op = n
        .svc
        .rename_with(rename("a.md", "a2.md"), Some(StopAt::AfterFile(1)))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        [n.read("b.md"), n.read("c.md"), n.read("d.md")],
        ["[[a2]]", "[[a]]", "[[a]]"]
    );
    let first = n.identity("b.md");
    n.reopen().await?;
    assert_eq!(n.settled(&op.op_id).await?.state, RenameState::Done);
    assert_eq!(
        [n.read("b.md"), n.read("c.md"), n.read("d.md")],
        ["[[a2]]", "[[a2]]", "[[a2]]"]
    );
    assert_eq!(
        n.identity("b.md"),
        first,
        "a done file is not written again"
    );
    assert!(n.rename_journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn crash_midway_resumes_from_the_progress_journal() -> T {
    let notes = [
        ("a.md", "x"),
        ("b.md", "[[a]]"),
        ("c.md", "[[a]]"),
        ("d.md", "[[a]]"),
        ("e.md", "[[a]] [[a]]"),
    ];
    let mut n = fixture(&notes).await;
    let op = n
        .svc
        .rename_with(rename("a.md", "a2.md"), Some(StopAt::AfterFile(2)))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let done = [n.identity("b.md"), n.identity("c.md")];
    assert_eq!(n.read("d.md"), "[[a]]");
    // Edited by someone else between the exit and the next start.
    std::fs::write(n.vault().join("d.md"), "[[a]] edited")?;
    n.reopen().await?;
    let status = n.settled(&op.op_id).await?;
    assert_eq!((status.state, status.failed.len()), (RenameState::Done, 0));
    assert_eq!([n.identity("b.md"), n.identity("c.md")], done);
    assert_eq!(n.read("b.md"), "[[a2]]");
    assert_eq!(n.read("c.md"), "[[a2]]");
    assert_eq!(
        n.read("d.md"),
        "[[a2]] edited",
        "fixed on the new rev, edit kept"
    );
    assert_eq!(n.read("e.md"), "[[a2]] [[a2]]");
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
async fn dirty_open_document_receives_a_rewrite_request_instead_of_disk_write() -> T {
    let n = fixture(&[("a.md", "x"), ("b.md", "[[a]]\n"), ("c.md", "[[a]]\n")]).await;
    let open = n.open_note("b.md", "w1", Some((3, "[[a]] draft\n"))).await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let op = n
        .svc
        .rename(rename("a.md", "a2.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let req = next_request(&mut rx).await;
    assert_eq!(
        (req.op_id.as_str(), req.key.clone()),
        (op.op_id.as_str(), vault_key("b.md"))
    );
    assert_eq!(
        (req.lease_id.as_str(), req.journal_seq),
        (open.lease_id.as_deref().unwrap_or(""), 3)
    );
    let edit = TextEdit {
        from_utf16: 2,
        to_utf16: 3,
        text: "a2".into(),
    };
    assert_eq!(req.edits, [edit]);
    assert_eq!(
        n.read("b.md"),
        "[[a]]\n",
        "the open note is not written behind the window"
    );
    // The window applies the edits to its buffer and saves.
    n.docs
        .save(devbox_documents::SaveDocument {
            lease_id: req.lease_id.clone(),
            seq: 4,
            base_rev: open.rev.clone(),
            expect: devbox_documents::Expect::Present,
            text: "[[a2]] draft\n".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let reply = RewriteReply {
        op_id: op.op_id.clone(),
        key: vault_key("b.md"),
        applied: true,
    };
    n.svc
        .rewrite_reply(reply)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let status = n.settled(&op.op_id).await?;
    assert_eq!((status.state, status.failed.len()), (RenameState::Done, 0));
    assert_eq!(n.read("b.md"), "[[a2]] draft\n");
    assert_eq!(n.read("c.md"), "[[a2]]\n");

    // No answer within 30 s (injected clock): listed as open, disk untouched.
    n.open_note("b.md", "w1", Some((5, "[[a2]] draft 2\n")))
        .await;
    let op = n
        .svc
        .rename(rename("a2.md", "a3.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    next_request(&mut rx).await;
    n.advance(31_000);
    let status = n.settled(&op.op_id).await?;
    assert_eq!(status.state, RenameState::Incomplete);
    assert_eq!(failed(&status), [("b.md", RewriteFailure::Open)]);
    assert_eq!(n.read("b.md"), "[[a2]] draft\n");
    assert_eq!(n.read("c.md"), "[[a3]]\n");
    Ok(())
}

#[tokio::test]
async fn an_applied_reply_without_a_fixed_note_counts_as_open() -> T {
    let n = fixture(&[("a.md", "x"), ("b.md", "[[a]]\n")]).await;
    n.open_note("b.md", "w1", Some((1, "[[a]] draft\n"))).await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let op = n
        .svc
        .rename(rename("a.md", "a2.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let req = next_request(&mut rx).await;
    let reply = RewriteReply {
        op_id: op.op_id.clone(),
        key: req.key,
        applied: true,
    };
    n.svc
        .rewrite_reply(reply)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let status = n.settled(&op.op_id).await?;
    assert_eq!(failed(&status), [("b.md", RewriteFailure::Open)]);
    assert_eq!(n.read("b.md"), "[[a]]\n");
    Ok(())
}

#[tokio::test]
async fn an_unsaved_draft_without_a_window_is_not_written_on_disk() -> T {
    let n = fixture(&[("a.md", "x"), ("b.md", "[[a]]\n")]).await;
    let open = n.open_note("b.md", "w1", Some((2, "[[a]] draft\n"))).await;
    let lease_id = open.lease_id.clone().expect("lease");
    n.docs
        .release(devbox_documents::LeaseRef { lease_id })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let status = n.renamed("a.md", "a2.md").await?;
    assert_eq!(failed(&status), [("b.md", RewriteFailure::Open)]);
    assert_eq!(n.read("b.md"), "[[a]]\n");
    let state = n
        .docs
        .edit_state(&vault_key("b.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(state.journal, Some((2, "[[a]] draft\n".to_string())));
    Ok(())
}

#[tokio::test]
async fn trash_and_temp_files_are_not_rewritten() -> T {
    let n = fixture(&[
        ("a.md", "x"),
        ("b.md", "[[a]]"),
        (".trash/2026-10-01/old.md", "[[a]]"),
        (".devbox-tmp-1", "[[a]]"),
        (".hidden/h.md", "[[a]]"),
    ])
    .await;
    let status = n.renamed("a.md", "a2.md").await?;
    assert_eq!((status.state, status.failed.len()), (RenameState::Done, 0));
    assert_eq!(n.read("b.md"), "[[a2]]");
    for rel in [".trash/2026-10-01/old.md", ".devbox-tmp-1", ".hidden/h.md"] {
        assert_eq!(n.read(rel), "[[a]]", "{rel}");
    }
    Ok(())
}

#[tokio::test]
async fn incomplete_rename_raises_one_notice_after_restart() -> T {
    let mut n = fixture(&[("a.md", "x"), ("b.md", "[[a]]"), ("c.md", "[[a]]")]).await;
    let op = n
        .svc
        .rename_with(rename("a.md", "a2.md"), Some(StopAt::AfterMove))
        .await
        .map_err(|e| format!("{e:?}"))?;
    // c.md has an unsaved draft nobody holds when the daemon comes back.
    let open = n.open_note("c.md", "w1", Some((1, "[[a]] draft"))).await;
    let lease_id = open.lease_id.expect("lease");
    n.docs
        .release(devbox_documents::LeaseRef { lease_id })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let mut notices = n.hub.subscribe("notes.rename_incomplete");
    n.reopen().await?;
    let status = n.settled(&op.op_id).await?;
    assert_eq!(failed(&status), [("c.md", RewriteFailure::Open)]);
    assert_eq!(n.read("b.md"), "[[a2]]");
    n.reopen().await?;
    let mut seen = Vec::new();
    while let Ok(Some(HubMessage::Event { payload, .. })) =
        tokio::time::timeout(Duration::from_millis(200), notices.recv()).await
    {
        seen.push(serde_json::from_str::<RenameIncomplete>(&payload)?);
    }
    let expected = RenameIncomplete {
        op_id: op.op_id,
        count: 1,
        reason: IncompleteReason::Failed,
    };
    assert_eq!(seen, [expected]);
    assert!(n.rename_journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn links_in_indented_code_and_html_comments_are_not_rewritten() -> T {
    let body = "[[노트 a]]\n\n    [[노트 a]]\n\n<!-- [[노트 a]] -->\n\n말 <!-- [[노트 a]] --> 끝\n\n문단\n    [[노트 a]] 이어짐\n";
    let n = fixture(&[("가/노트 a.md", "x\n"), ("b.md", body)]).await;
    // The index counts the same links the rename rewrites: the two visible ones.
    let back = n
        .svc
        .backlinks(NotePath {
            path: "가/노트 a.md".into(),
        })
        .await;
    assert_eq!(back.map_err(|e| format!("{e:?}"))?.items.len(), 2);
    let status = n.renamed("가/노트 a.md", "나/새 노트.md").await?;
    assert_eq!((status.state, status.failed.len()), (RenameState::Done, 0));
    assert_eq!(
        n.read("b.md"),
        "[[새 노트]]\n\n    [[노트 a]]\n\n<!-- [[노트 a]] -->\n\n말 <!-- [[노트 a]] --> 끝\n\n문단\n    [[새 노트]] 이어짐\n"
    );
    Ok(())
}
