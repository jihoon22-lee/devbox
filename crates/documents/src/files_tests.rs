use crate::fixture::*;
use crate::*;
use std::fs;

#[tokio::test]
async fn trash_moves_only_into_and_out_of_the_trash_folder() -> T {
    let fx = Fx::new();
    fx.write("a/n.md", "n\n");
    fx.write("b.md", "b\n");
    let to = fx
        .svc
        .trash_entry(&key("a/n.md"), &key(".trash/2026-10-22/a/n.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(to, key(".trash/2026-10-22/a/n.md"));
    assert!(!fx.vault.join("a/n.md").exists());
    assert_eq!(fx.read(".trash/2026-10-22/a/n.md"), "n\n");
    // Never replaces an earlier trashed copy.
    fx.write("a/n.md", "again\n");
    let err = domain(
        fx.svc
            .trash_entry(&key("a/n.md"), &key(".trash/2026-10-22/a/n.md"))
            .await,
    );
    assert_eq!(err, DocumentsError::Exists);
    // Only `.trash/` may be the internal side, and only on the right side.
    for (from, to) in [
        ("b.md", "c.md"),
        ("b.md", ".git/b.md"),
        ("b.md", ".trash/x/.devbox-tmp-1"),
        (".trash/2026-10-22/a/n.md", "z.md"),
    ] {
        let err = domain(fx.svc.trash_entry(&key(from), &key(to)).await);
        assert!(
            matches!(
                err,
                DocumentsError::InvalidInput | DocumentsError::OutOfScope
            ),
            "{from} -> {to}: {err:?}"
        );
    }
    let err = domain(
        fx.svc
            .restore_entry(&key(".trash/2026-10-22/a/n.md"), &key("a/n.md"))
            .await,
    );
    assert_eq!(err, DocumentsError::Exists, "restore never replaces");
    fs::remove_file(fx.vault.join("a/n.md"))?;
    fx.svc
        .restore_entry(&key(".trash/2026-10-22/a/n.md"), &key("a/n.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read("a/n.md"), "n\n");
    let err = domain(fx.svc.restore_entry(&key("b.md"), &key("c.md")).await);
    assert_eq!(err, DocumentsError::InvalidInput);
    Ok(())
}

#[tokio::test]
async fn trashing_an_open_note_keeps_its_lease_and_journal_on_the_old_key() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    let lease = c.lease_id.clone().expect("lease");
    fx.svc
        .journal(WriteJournal {
            lease_id: lease.clone(),
            seq: 1,
            base_rev: c.rev.clone(),
            text: "dirty\n".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    fx.svc
        .trash_entry(&key("a.md"), &key(".trash/d/a.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.journals().len(), 1, "the unsaved edit is not lost");
    // Like an external delete: the editor sees a conflict, then may recreate.
    let mut s = save(&c, c.rev.clone(), "mine\n");
    s.seq = 2;
    assert_eq!(
        domain(fx.svc.save(s.clone()).await),
        DocumentsError::Conflict { rev: None }
    );
    s.expect = Expect::Absent;
    s.base_rev = None;
    fx.svc.save(s).await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read("a.md"), "mine\n");
    assert_eq!(fx.read(".trash/d/a.md"), "a\n");
    Ok(())
}

#[tokio::test]
async fn bytes_are_created_without_replacing_and_read_within_scope() -> T {
    let fx = Fx::new();
    let png = b"\x89PNG\r\n\x1a\n\0binary\0".to_vec();
    fx.svc
        .create_bytes(&key("attachments/x.png"), &png)
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fs::read(fx.vault.join("attachments/x.png"))?, png);
    let err = domain(
        fx.svc
            .create_bytes(&key("attachments/x.png"), b"other")
            .await,
    );
    assert_eq!(err, DocumentsError::Exists);
    assert_eq!(fs::read(fx.vault.join("attachments/x.png"))?, png);
    let (k, bytes) = fx
        .svc
        .read_bytes(&key("attachments/x.png"), 1024)
        .await
        .map_err(|e| format!("{e:?}"))?
        .expect("present");
    assert_eq!((k, bytes), (key("attachments/x.png"), png));
    assert!(
        fx.svc
            .read_bytes(&key("missing.png"), 1024)
            .await
            .map_err(|e| format!("{e:?}"))?
            .is_none()
    );
    assert_eq!(
        domain(fx.svc.read_bytes(&key("attachments/x.png"), 4).await),
        DocumentsError::TooLarge { limit: 4 }
    );
    fs::write(fx.outside.join("secret.png"), b"secret")?;
    std::os::unix::fs::symlink(fx.outside.join("secret.png"), fx.vault.join("out.png"))?;
    assert_eq!(
        domain(fx.svc.read_bytes(&key("out.png"), 1024).await),
        DocumentsError::OutOfScope
    );
    fx.write(".trash/x.png", "trashed");
    assert_eq!(
        domain(fx.svc.read_bytes(&key(".trash/x.png"), 1024).await),
        DocumentsError::OutOfScope
    );
    Ok(())
}

#[tokio::test]
async fn a_symlinked_trash_folder_never_redirects_the_move() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    fs::create_dir_all(fx.vault.join("visible"))?;
    std::os::unix::fs::symlink(fx.vault.join("visible"), fx.vault.join(".trash"))?;
    let err = domain(
        fx.svc
            .trash_entry(&key("a.md"), &key(".trash/d/a.md"))
            .await,
    );
    assert!(
        matches!(
            err,
            DocumentsError::InvalidInput | DocumentsError::OutOfScope
        ),
        "{err:?}"
    );
    assert_eq!(fx.read("a.md"), "a\n");
    assert!(!fx.vault.join("visible/d").exists());
    Ok(())
}

#[tokio::test]
async fn undoing_the_delete_of_an_open_note_lets_its_editor_save_again() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .journal(WriteJournal {
            lease_id: c.lease_id.clone().expect("lease"),
            seq: 1,
            base_rev: c.rev.clone(),
            text: "dirty\n".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    fx.svc
        .trash_entry(&key("a.md"), &key(".trash/d/a.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    fx.svc
        .restore_entry(&key(".trash/d/a.md"), &key("a.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read("a.md"), "a\n");
    let mut s = save(&c, c.rev.clone(), "mine\n");
    s.seq = 2;
    fx.svc.save(s).await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read("a.md"), "mine\n");
    assert!(
        fx.journals().is_empty(),
        "the saved draft's journal is gone"
    );
    Ok(())
}

#[tokio::test]
async fn moves_through_a_symlinked_folder_inside_the_vault_are_refused() -> T {
    let fx = Fx::new();
    fx.write("real/a.md", "a\n");
    fx.write(".trash/d/n.md", "n\n");
    std::os::unix::fs::symlink(fx.vault.join("real"), fx.vault.join("lnk"))?;
    // Source: `lnk/a.md` resolves to `real/a.md`, which is not what was asked.
    let err = domain(
        fx.svc
            .trash_entry(&key("lnk/a.md"), &key(".trash/d/a.md"))
            .await,
    );
    assert_eq!(err, DocumentsError::InvalidInput);
    assert_eq!(fx.read("real/a.md"), "a\n");
    assert!(!fx.vault.join(".trash/d/a.md").exists());
    // Destination: restoring into `lnk/` would land in `real/`.
    let err = domain(
        fx.svc
            .restore_entry(&key(".trash/d/n.md"), &key("lnk/n.md"))
            .await,
    );
    assert_eq!(err, DocumentsError::InvalidInput);
    assert_eq!(fx.read(".trash/d/n.md"), "n\n");
    assert!(!fx.vault.join("real/n.md").exists());
    Ok(())
}

#[tokio::test]
async fn in_trash_refuses_other_internal_names() -> T {
    assert!(in_trash(".trash/2026-10-22/a.md"));
    for path in [
        ".trash",
        ".trash/",
        ".trash/.git/x",
        ".trash/a/.git/b.md",
        ".trash/a/.devbox-tmp-1",
        ".trash/.trash/a.md",
        "a/.trash/b.md",
    ] {
        assert!(!in_trash(path), "{path}");
    }
    let fx = Fx::new();
    fx.write("b.md", "b\n");
    let err = domain(
        fx.svc
            .trash_entry(&key("b.md"), &key(".trash/.git/b.md"))
            .await,
    );
    assert_eq!(err, DocumentsError::InvalidInput);
    assert_eq!(fx.read("b.md"), "b\n");
    assert!(!fx.vault.join(".trash").exists());
    Ok(())
}

#[tokio::test]
async fn restore_moves_only_regular_files() -> T {
    let fx = Fx::new();
    fx.write(".trash/2026-10-22/a.md", "a\n");
    fs::create_dir_all(fx.vault.join(".trash/2026-10-22/dir.md"))?;
    for from in [".trash/2026-10-22", ".trash/2026-10-22/dir.md"] {
        let err = domain(fx.svc.restore_entry(&key(from), &key("x.md")).await);
        assert_eq!(err, DocumentsError::InvalidInput, "{from}");
    }
    assert!(!fx.vault.join("x.md").exists());
    assert!(fx.vault.join(".trash/2026-10-22/a.md").exists());
    Ok(())
}

#[tokio::test]
async fn edit_state_reports_the_live_lease_and_its_unsaved_journal() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    assert_eq!(
        fx.svc
            .edit_state(&key("a.md"))
            .await
            .map_err(|e| format!("{e:?}"))?,
        EditState::default()
    );
    let c = claimed(&fx.svc, "a.md", "w1").await;
    let lease = c.lease_id.clone().expect("lease");
    let state = fx
        .svc
        .edit_state(&key("a.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (state.lease_id.as_deref(), state.journal),
        (Some(lease.as_str()), None)
    );
    fx.svc
        .journal(WriteJournal {
            lease_id: lease.clone(),
            seq: 2,
            base_rev: c.rev.clone(),
            text: "unsaved\n".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let state = fx
        .svc
        .edit_state(&key("a.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(state.journal, Some((2, "unsaved\n".to_string())));
    // The window closes without saving: the draft stays, nobody holds it.
    fx.svc
        .release(LeaseRef { lease_id: lease })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let state = fx
        .svc
        .edit_state(&key("a.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        (state.lease_id, state.journal),
        (None, Some((2, "unsaved\n".to_string())))
    );
    Ok(())
}

#[tokio::test]
async fn move_file_and_trash_file_refuse_a_folder_named_like_a_note() -> T {
    let fx = Fx::new();
    fx.write("x.md/inner.md", "inner\n");
    let err = domain(fx.svc.move_file(&key("x.md"), &key("y.md")).await);
    assert_eq!(err, DocumentsError::InvalidInput);
    let err = domain(fx.svc.trash_file(&key("x.md"), &key(".trash/d/x.md")).await);
    assert_eq!(err, DocumentsError::InvalidInput);
    assert_eq!(fx.read("x.md/inner.md"), "inner\n");
    assert!(!fx.vault.join("y.md").exists() && !fx.vault.join(".trash").exists());
    // Regular files still move.
    fx.write("a.md", "a\n");
    fx.svc
        .move_file(&key("a.md"), &key("z/b.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read("z/b.md"), "a\n");
    fx.svc
        .trash_file(&key("z/b.md"), &key(".trash/d/b.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read(".trash/d/b.md"), "a\n");
    Ok(())
}
