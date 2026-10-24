//! Task 5 refusals: targets that exist or hold unsaved edits, paths outside the
//! visible vault, folders, concurrent renames and client-sent operation ids.
use crate::fixture::*;
use crate::*;
use std::time::Duration;

#[tokio::test]
async fn rename_refuses_existing_unsaved_and_invalid_targets() -> T {
    let n = fixture(&[("a.md", "a\n"), ("b.md", "[[a]]\n")]).await;
    let refused = |e: NotesError, to: &'static str| (e, to);
    let mut cases = vec![refused(NotesError::Exists, "b.md")];
    // A new note open in a window (not saved yet) and a draft nobody holds.
    n.open_note("new.md", "w1", None).await;
    let draft = n.open_note("draft.md", "w2", Some((1, "unsaved\n"))).await;
    let lease_id = draft.lease_id.expect("lease");
    n.docs
        .release(devbox_documents::LeaseRef { lease_id })
        .await
        .map_err(|e| format!("{e:?}"))?;
    cases.push(refused(NotesError::TargetUnsaved, "new.md"));
    cases.push(refused(NotesError::TargetUnsaved, "draft.md"));
    for to in [
        "../x.md",
        "/abs.md",
        ".trash/x.md",
        ".git/x.md",
        "d/.devbox-tmp-1.md",
        "x.txt",
        "a.md",
        "d//x.md",
        "x[1].md",
    ] {
        cases.push(refused(NotesError::InvalidInput, to));
    }
    for (expected, to) in cases {
        let err = domain(n.svc.rename(rename("a.md", to)).await);
        assert_eq!(err, expected, "{to}");
        assert_eq!(n.read("a.md"), "a\n", "{to}");
        assert_eq!(n.read("b.md"), "[[a]]\n", "{to}");
        assert!(n.rename_journals().is_empty(), "{to}");
    }
    assert!(!n.home.join("x.md").exists() && !n.vault().join(".trash").exists());
    let draft = n.docs.edit_state(&vault_key("draft.md")).await;
    assert!(draft.map_err(|e| format!("{e:?}"))?.journal.is_some());
    Ok(())
}

#[tokio::test]
async fn rename_and_delete_refuse_sources_that_are_not_visible_note_files() -> T {
    let n = fixture(&[("x.md/inner.md", "inner\n"), (".trash/d/t.md", "t\n")]).await;
    for from in ["x.md", ".trash/d/t.md", "../a.md", "missing.md"] {
        let err = domain(n.svc.rename(rename(from, "y.md")).await);
        let expected = if from == "missing.md" {
            NotesError::NotFound
        } else {
            NotesError::InvalidInput
        };
        assert_eq!(err, expected, "{from}");
    }
    let err = domain(
        n.svc
            .delete(NotePath {
                path: "x.md".into(),
            })
            .await,
    );
    assert_eq!(err, NotesError::InvalidInput);
    assert_eq!(n.read("x.md/inner.md"), "inner\n");
    assert!(!n.vault().join("y.md").exists());
    assert!(!n.vault().join(".trash/2026-10-22").exists());
    assert!(n.rename_journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn a_second_rename_or_a_delete_of_its_notes_waits_while_one_runs() -> T {
    let n = fixture(&[("a.md", "x"), ("b.md", "[[a]]\n"), ("c.md", "c\n")]).await;
    n.open_note("b.md", "w1", Some((1, "[[a]] draft\n"))).await;
    let mut rx = n.hub.subscribe("notes.rewrite_request");
    let op = n
        .svc
        .rename(rename("a.md", "a2.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    tokio::time::timeout(Duration::from_secs(5), rx.recv()).await?;
    let err = domain(n.svc.rename(rename("c.md", "c2.md")).await);
    assert_eq!(err, NotesError::Busy);
    for path in ["b.md", "a2.md"] {
        let err = domain(n.svc.delete(NotePath { path: path.into() }).await);
        assert_eq!(err, NotesError::Busy, "{path}");
    }
    assert_eq!(n.read("c.md"), "c\n");
    n.advance(31_000);
    n.settled(&op.op_id).await?;
    let status = n.renamed("c.md", "c2.md").await?;
    assert_eq!(status.state, RenameState::Done);
    n.svc
        .delete(NotePath {
            path: "b.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[tokio::test]
async fn client_operation_ids_are_checked_before_any_file_access() -> T {
    let n = fixture(&[("a.md", "x")]).await;
    // A journal-shaped file outside the rename folder must never be read.
    std::fs::write(n.data.join("x.json"), "{}")?;
    for op in ["../x", "", "x/../../y", "ABCDEF0123456789ABCDEF0123456789"] {
        let p = RenameOp { op_id: op.into() };
        assert_eq!(
            domain(n.svc.rename_status(p).await),
            NotesError::InvalidInput,
            "{op}"
        );
        let reply = RewriteReply {
            op_id: op.into(),
            key: vault_key("a.md"),
            applied: true,
        };
        assert_eq!(
            domain(n.svc.rewrite_reply(reply).await),
            NotesError::InvalidInput,
            "{op}"
        );
    }
    let unknown = "0123456789abcdef0123456789abcdef";
    let p = RenameOp {
        op_id: unknown.into(),
    };
    assert_eq!(domain(n.svc.rename_status(p).await), NotesError::NotFound);
    Ok(())
}

#[tokio::test]
async fn rename_without_rewriting_only_moves_the_note() -> T {
    let n = fixture(&[("a.md", "x"), ("b.md", "[[a]]")]).await;
    let mut p = rename("a.md", "z/a2.md");
    p.rewrite_links = false;
    let op = n.svc.rename(p).await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(n.settled(&op.op_id).await?.state, RenameState::Done);
    assert_eq!(n.read("z/a2.md"), "x");
    assert_eq!(n.read("b.md"), "[[a]]");
    assert!(n.rename_journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn an_interrupted_rename_whose_target_appeared_is_abandoned() -> T {
    let mut n = fixture(&[("a.md", "x"), ("b.md", "[[a]]")]).await;
    let op = n
        .svc
        .rename_with(rename("a.md", "a2.md"), Some(StopAt::BeforeMove))
        .await
        .map_err(|e| format!("{e:?}"))?;
    // While the daemon was down, someone else created the new name.
    n.write("a2.md", "someone else\n");
    n.reopen().await?;
    let status = n.settled(&op.op_id).await?;
    assert_eq!(status.state, RenameState::Incomplete);
    assert_eq!(status.reason, Some(IncompleteReason::TargetExists));
    assert_eq!(n.read("a.md"), "x");
    assert_eq!(n.read("a2.md"), "someone else\n");
    assert_eq!(
        n.read("b.md"),
        "[[a]]",
        "links are not pointed at the other note"
    );
    assert!(n.rename_journals().is_empty());
    Ok(())
}

async fn notices(rx: &mut devbox_core::HubReceiver) -> Vec<RenameIncomplete> {
    let mut seen = Vec::new();
    while let Ok(Some(devbox_core::HubMessage::Event { payload, .. })) =
        tokio::time::timeout(Duration::from_millis(200), rx.recv()).await
    {
        seen.push(serde_json::from_str(&payload).expect("notice payload"));
    }
    seen
}

#[tokio::test]
async fn an_interrupted_rename_whose_note_vanished_says_so() -> T {
    let mut n = fixture(&[("a.md", "x"), ("b.md", "[[a]]")]).await;
    let op = n
        .svc
        .rename_with(rename("a.md", "a2.md"), Some(StopAt::BeforeMove))
        .await
        .map_err(|e| format!("{e:?}"))?;
    std::fs::remove_file(n.vault().join("a.md"))?;
    let mut rx = n.hub.subscribe("notes.rename_incomplete");
    n.reopen().await?;
    let status = n.settled(&op.op_id).await?;
    assert_eq!(status.reason, Some(IncompleteReason::SourceMissing));
    let expected = RenameIncomplete {
        op_id: op.op_id,
        count: 0,
        reason: IncompleteReason::SourceMissing,
    };
    assert_eq!(notices(&mut rx).await, [expected]);
    assert_eq!(n.read("b.md"), "[[a]]");
    Ok(())
}

#[tokio::test]
async fn an_unreadable_journal_is_set_aside_and_reported_once() -> T {
    let mut n = fixture(&[("a.md", "x")]).await;
    let dir = n.data.join("notes-rename");
    std::fs::create_dir_all(&dir)?;
    let op = "0123456789abcdef0123456789abcdef";
    std::fs::write(dir.join(format!("{op}.json")), "{not json")?;
    let mut rx = n.hub.subscribe("notes.rename_incomplete");
    n.reopen().await?;
    let status = n.settled(op).await?;
    assert_eq!(
        (status.state, status.reason),
        (RenameState::Incomplete, Some(IncompleteReason::Unreadable))
    );
    assert_eq!(
        std::fs::read_to_string(dir.join(format!("{op}.json.corrupt")))?,
        "{not json",
        "kept for inspection"
    );
    assert!(!dir.join(format!("{op}.json")).exists());
    n.reopen().await?;
    let expected = RenameIncomplete {
        op_id: op.into(),
        count: 0,
        reason: IncompleteReason::Unreadable,
    };
    assert_eq!(notices(&mut rx).await, [expected]);
    Ok(())
}

#[tokio::test]
async fn a_journal_folder_that_breaks_after_the_move_does_not_leave_the_rename_running() -> T {
    use std::os::unix::fs::PermissionsExt;
    let n = fixture(&[("a.md", "x"), ("b.md", "[[a]]")]).await;
    let dir = n.data.join("notes-rename");
    let locked = dir.clone();
    n.svc.on_moved(move || {
        let ro = std::fs::Permissions::from_mode(0o500);
        std::fs::set_permissions(&locked, ro).expect("lock the journal folder");
    });
    let result = n.svc.rename(rename("a.md", "a2.md")).await;
    let started = result.map_err(|e| format!("{e:?}"));
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let op = started?;
    let status = n.settled(&op.op_id).await?;
    assert_eq!(status.state, RenameState::Done);
    assert_eq!(n.read("b.md"), "[[a2]]");
    // Nothing still counts as held by the rename.
    n.svc
        .delete(NotePath {
            path: "b.md".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[tokio::test]
async fn symlinked_sources_and_targets_are_refused_before_the_journal() -> T {
    let n = fixture(&[("a.md", "x"), ("real/keep.md", "k")]).await;
    std::os::unix::fs::symlink("a.md", n.vault().join("l.md"))?;
    std::os::unix::fs::symlink("real", n.vault().join("lnk"))?;
    std::os::unix::fs::symlink("missing.md", n.vault().join("t.md"))?;
    let stop = Some(StopAt::BeforeMove);
    let err = domain(n.svc.rename_with(rename("l.md", "x.md"), stop).await);
    assert_eq!(
        err,
        NotesError::InvalidInput,
        "source key differs from the asked one"
    );
    for to in ["lnk/x.md", "t.md"] {
        let err = domain(n.svc.rename_with(rename("a.md", to), stop).await);
        assert_eq!(
            err,
            NotesError::Exists,
            "{to}: target key differs from the asked one"
        );
    }
    assert!(n.rename_journals().is_empty());
    assert_eq!(n.read("a.md"), "x");
    assert!(!n.vault().join("real/x.md").exists() && !n.vault().join("missing.md").exists());
    Ok(())
}
