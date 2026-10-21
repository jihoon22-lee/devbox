use crate::fixture::*;
use crate::*;
use devbox_core::HubMessage;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

fn rename(lease: Option<String>, from: &str, to: &str) -> RenameDocument {
    RenameDocument {
        lease_id: lease,
        key: key(from),
        to: key(to),
    }
}

#[tokio::test]
async fn rename_noreplace_refuses_existing_target() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    fx.write("b.md", "b\n");
    let err = domain(fx.svc.rename(rename(None, "a.md", "b.md")).await);
    assert_eq!(err, DocumentsError::Exists);
    assert_eq!(
        (fx.read("a.md"), fx.read("b.md")),
        ("a\n".into(), "b\n".into())
    );
    let err = domain(
        fx.svc
            .rename(RenameDocument {
                lease_id: None,
                key: key("a.md"),
                to: DocKey {
                    root: "project:p".into(),
                    path: "a.md".into(),
                },
            })
            .await,
    );
    assert_eq!(
        err,
        DocumentsError::InvalidInput,
        "moves stay inside one root"
    );
    Ok(())
}

#[tokio::test]
async fn rename_moves_the_journal_to_the_new_key_and_drops_the_old_one() -> T {
    let mut fx = Fx::new();
    fx.write("a.md", "a\n");
    let mut moved = fx.hub.subscribe("documents.moved");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    let lease = c.lease_id.clone().expect("lease");
    fx.svc
        .journal(WriteJournal {
            lease_id: lease.clone(),
            seq: 3,
            base_rev: c.rev.clone(),
            text: "dirty\n".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let renamed = fx
        .svc
        .rename(rename(Some(lease.clone()), "a.md", "sub/b.md"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(renamed.key, key("sub/b.md"));
    assert_eq!(fx.read("sub/b.md"), "a\n");
    assert!(!fx.vault.join("a.md").exists());
    assert_eq!(fx.journals().len(), 1);
    let Some(HubMessage::Event { payload, .. }) =
        tokio::time::timeout(Duration::from_secs(1), moved.recv()).await?
    else {
        panic!("documents.moved is published");
    };
    let event: DocumentMoved = serde_json::from_str(&payload)?;
    assert_eq!((event.from, event.to), (key("a.md"), key("sub/b.md")));
    // The same lease now edits the new key.
    let again = claimed(&fx.svc, "sub/b.md", "w1").await;
    assert_eq!(again.lease_id.as_deref(), Some(lease.as_str()));
    assert_eq!(again.journal.expect("moved journal").seq, 3);
    fx.restart();
    assert!(claimed(&fx.svc, "a.md", "w2").await.journal.is_none());
    let new = claimed(&fx.svc, "sub/b.md", "w2").await;
    assert_eq!(new.journal.and_then(|j| j.text).as_deref(), Some("dirty\n"));
    Ok(())
}

fn gate(reached: Arc<Notify>, release: Arc<Notify>) -> Hook {
    Arc::new(move || {
        let (reached, release) = (reached.clone(), release.clone());
        Box::pin(async move {
            reached.notify_one();
            release.notified().await;
        })
    })
}

#[tokio::test]
async fn dirty_rename_then_save_leaves_no_old_key_journal() -> T {
    let (reached, release) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
    let hook = gate(reached.clone(), release.clone());
    let mut fx = Fx::with(move |c| c.hooks.before_rename_commit = Some(hook.clone()));
    fx.write("a.md", "v0\n");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    let lease = c.lease_id.clone().expect("lease");
    let journal = |seq: u64, text: &str| WriteJournal {
        lease_id: lease.clone(),
        seq,
        base_rev: c.rev.clone(),
        text: text.into(),
    };
    fx.svc
        .journal(journal(1, "one\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let svc = fx.svc.clone();
    let renaming = tokio::spawn(async move { svc.rename(rename(None, "a.md", "b.md")).await });
    tokio::time::timeout(Duration::from_secs(5), reached.notified()).await?;
    let svc = fx.svc.clone();
    let second = journal(2, "two\n");
    let writing = tokio::spawn(async move { svc.journal(second).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !writing.is_finished(),
        "the journal waits for the document lock"
    );
    release.notify_one();
    let renamed = renaming.await?.map_err(|e| format!("{e:?}"))?;
    assert_eq!(renamed.key, key("b.md"));
    writing.await?.map_err(|e| format!("{e:?}"))?;
    let mut s = save(&c, c.rev.clone(), "two\n");
    s.seq = 2;
    fx.svc.save(s).await.map_err(|e| format!("{e:?}"))?;
    fx.restart();
    assert!(claimed(&fx.svc, "a.md", "w1").await.journal.is_none());
    assert!(claimed(&fx.svc, "b.md", "w1").await.journal.is_none());
    assert!(fx.journals().is_empty(), "{:?}", fx.journals());
    assert_eq!(fx.read("b.md"), "two\n");
    Ok(())
}

#[tokio::test]
async fn rename_refuses_a_target_with_a_lease_or_journal() -> T {
    let fx = Fx::new();
    fx.write("a.md", "a\n");
    let draft = claimed(&fx.svc, "b.md", "w2").await;
    assert!(draft.rev.is_none() && draft.lease_id.is_some());
    fx.svc
        .journal(WriteJournal {
            lease_id: draft.lease_id.clone().expect("lease"),
            seq: 1,
            base_rev: None,
            text: "unsaved new note\n".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let leased = claimed(&fx.svc, "c.md", "w3").await;
    for to in ["b.md", "c.md"] {
        let err = domain(fx.svc.rename(rename(None, "a.md", to)).await);
        assert_eq!(err, DocumentsError::Exists, "{to}");
    }
    assert_eq!(fx.read("a.md"), "a\n");
    let again = claimed(&fx.svc, "b.md", "w2").await;
    assert_eq!(
        again.lease_id, draft.lease_id,
        "the target lease is untouched"
    );
    let journal = again.journal.expect("the target journal is preserved");
    assert_eq!(journal.text.as_deref(), Some("unsaved new note\n"));
    let c = claimed(&fx.svc, "c.md", "w3").await;
    assert_eq!(c.lease_id, leased.lease_id);
    Ok(())
}

#[tokio::test]
async fn startup_keeps_only_the_journal_whose_file_exists() -> T {
    let mut fx = Fx::new();
    fx.write("a.md", "a\n");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .journal(WriteJournal {
            lease_id: c.lease_id.clone().expect("lease"),
            seq: 2,
            base_rev: c.rev.clone(),
            text: "dirty\n".into(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    // A crash between the move and the old-key journal removal leaves both copies.
    let store = crate::journal::JournalStore::new(fx.data.join("journals"));
    let mut copy = store.load(&key("a.md"))?.expect("journal");
    copy.key = key("b.md");
    store.write(&copy)?;
    fs::rename(fx.vault.join("a.md"), fx.vault.join("b.md"))?;
    assert_eq!(fx.journals().len(), 2);
    fx.restart();
    fx.svc.start().await?;
    assert_eq!(fx.journals().len(), 1);
    assert!(store.load(&key("b.md"))?.is_some());
    Ok(())
}

#[test]
fn moves_files_and_directories_but_preserves_existing_destinations() -> T {
    // v0.9.0 knowledge-vault-engine document_publish.rs, adapted to dirfd-relative rustix.
    use crate::sys::{SysError, move_no_replace, open_dir};
    let root = tempfile::tempdir()?;
    let dir = open_dir(root.path())?;
    let (source, target) = (root.path().join("source.md"), root.path().join("target.md"));
    fs::write(&source, "reviewed source")?;
    fs::write(&target, "external destination")?;
    assert!(matches!(
        move_no_replace(&dir, "source.md", &dir, "target.md"),
        Err(SysError::Exists)
    ));
    assert_eq!(fs::read_to_string(&source)?, "reviewed source");
    assert_eq!(fs::read_to_string(&target)?, "external destination");
    fs::remove_file(&target)?;
    move_no_replace(&dir, "source.md", &dir, "target.md")?;
    assert!(!source.exists());
    assert_eq!(fs::read_to_string(&target)?, "reviewed source");

    let (source_dir, target_dir) = (
        root.path().join("source-dir"),
        root.path().join("target-dir"),
    );
    fs::create_dir(&source_dir)?;
    fs::write(source_dir.join("note.md"), "nested source")?;
    fs::create_dir(&target_dir)?;
    assert!(move_no_replace(&dir, "source-dir", &dir, "target-dir").is_err());
    assert_eq!(
        fs::read_to_string(source_dir.join("note.md"))?,
        "nested source"
    );
    assert!(target_dir.is_dir());
    fs::remove_dir(&target_dir)?;
    move_no_replace(&dir, "source-dir", &dir, "target-dir")?;
    assert!(!source_dir.exists());
    assert_eq!(
        fs::read_to_string(target_dir.join("note.md"))?,
        "nested source"
    );
    Ok(())
}
