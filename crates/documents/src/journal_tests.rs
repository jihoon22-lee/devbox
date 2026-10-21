use crate::fixture::*;
use crate::*;

fn entry(c: &Claimed, seq: u64, text: &str) -> WriteJournal {
    WriteJournal {
        lease_id: c.lease_id.clone().expect("lease"),
        seq,
        base_rev: c.rev.clone(),
        text: text.into(),
    }
}

#[tokio::test]
async fn journal_rejects_older_seq_and_drop_is_cas() -> T {
    let fx = Fx::new();
    fx.write("a.md", "disk\n");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .journal(entry(&c, 5, "five\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let err = domain(fx.svc.journal(entry(&c, 4, "four\n")).await);
    assert_eq!(err, DocumentsError::StaleJournal);
    let err = domain(fx.svc.journal(entry(&c, 5, "again\n")).await);
    assert_eq!(err, DocumentsError::StaleJournal);
    let drop = |upto_seq| DropJournal {
        key: key("a.md"),
        upto_seq,
        lease_id: c.lease_id.clone(),
    };
    fx.svc
        .journal_drop(drop(4))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        fx.journals().len(),
        1,
        "a newer journal survives an older drop"
    );
    let kept = claimed(&fx.svc, "a.md", "w1")
        .await
        .journal
        .expect("journal");
    assert_eq!((kept.seq, kept.text.as_deref()), (5, Some("five\n")));
    fx.svc
        .journal_drop(drop(5))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(fx.journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn journal_survives_restart_and_dropped_journal_is_not_recovered() -> T {
    let mut fx = Fx::new();
    fx.write("a.md", "disk\n");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .journal(entry(&c, 3, "draft\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    fx.restart();
    let recovered = claimed(&fx.svc, "a.md", "w1").await;
    let journal = recovered.journal.clone().expect("recovery candidate");
    assert_eq!((journal.seq, journal.base_rev), (3, c.rev.clone()));
    assert_eq!(journal.text.as_deref(), Some("draft\n"));
    // Conflict -> "disk version": the screen drops up to its journal seq (AR-F13).
    fx.svc
        .journal_drop(DropJournal {
            key: recovered.key.clone(),
            upto_seq: 3,
            lease_id: recovered.lease_id.clone(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    fx.restart();
    assert!(claimed(&fx.svc, "a.md", "w1").await.journal.is_none());
    Ok(())
}

#[tokio::test]
async fn save_drops_only_journals_it_covers() -> T {
    let fx = Fx::new();
    fx.write("a.md", "disk\n");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .journal(entry(&c, 4, "four\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let mut s = save(&c, c.rev.clone(), "three\n");
    s.seq = 3;
    let saved = fx.svc.save(s).await.map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.journals().len(), 1, "seq 4 is newer than the save");
    let err = domain(fx.svc.journal(entry(&c, 3, "three\n")).await);
    assert_eq!(err, DocumentsError::StaleJournal, "saved seq is stale too");
    let mut s = save(&c, Some(saved.rev), "four\n");
    s.seq = 4;
    fx.svc.save(s).await.map_err(|e| format!("{e:?}"))?;
    assert!(fx.journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn journal_text_over_the_policy_limit_is_refused() -> T {
    let fx = Fx::new();
    let c = claimed(&fx.svc, "a.md", "w1").await;
    let big = "a".repeat(NOTE_LIMIT as usize + 1);
    let err = domain(fx.svc.journal(entry(&c, 1, &big)).await);
    assert_eq!(err, DocumentsError::TooLarge { limit: NOTE_LIMIT });
    let err = domain(fx.svc.save(save(&c, None, &big)).await);
    assert_eq!(err, DocumentsError::TooLarge { limit: NOTE_LIMIT });
    assert!(fx.journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn journal_and_ledger_files_are_private() -> T {
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &std::path::Path| {
        std::fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o777)
            .expect("fixture metadata")
    };
    let fx = Fx::new();
    let c = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .journal(entry(&c, 1, "secret draft\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let journal = fx.journals().pop().expect("journal");
    assert_eq!(mode(&journal), 0o600);
    assert_eq!(mode(&fx.data.join("journals")), 0o700);
    let ledger = crate::ledger::TmpLedger::new(fx.data.join("documents/tmp"));
    let entry = ledger.record(&fx.vault.join(".devbox-tmp-x"))?;
    assert_eq!(mode(&fx.ledger()[0]), 0o600);
    assert_eq!(mode(&fx.data.join("documents")), 0o700);
    assert_eq!(mode(&fx.data.join("documents/tmp")), 0o700);
    ledger.remove(entry)?;
    Ok(())
}

#[tokio::test]
async fn journal_drop_is_refused_unless_the_caller_holds_the_lease() -> T {
    let fx = Fx::new();
    fx.write("a.md", "disk\n");
    let w1 = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .journal(entry(&w1, 1, "w1 draft\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let w2 = fx
        .svc
        .take(
            tokio_util::sync::CancellationToken::new(),
            TakeDocument {
                key: key("a.md"),
                window_id: "w2".into(),
                policy: None,
            },
        )
        .await
        .map_err(|e| format!("{e:?}"))?
        .expect("live owner");
    fx.svc
        .journal(entry(&w2, 2, "w2 draft\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let drop = |lease_id: Option<String>| DropJournal {
        key: key("a.md"),
        upto_seq: 9,
        lease_id,
    };
    // The read-only window (stale lease) and a lease-less caller cannot remove the holder's journal.
    for lease in [w1.lease_id.clone(), None] {
        let err = domain(fx.svc.journal_drop(drop(lease)).await);
        assert_eq!(
            err,
            DocumentsError::LeaseLost {
                reason: LeaseLoss::Taken
            }
        );
        assert_eq!(fx.journals().len(), 1, "the holder's journal stays");
    }
    let id = w2.lease_id.clone().expect("lease");
    fx.svc
        .release(LeaseRef {
            lease_id: id.clone(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    // Nobody holds it any more: a retried drop (offline tombstone) goes through.
    fx.svc
        .journal_drop(drop(None))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(fx.journals().is_empty());
    Ok(())
}

#[tokio::test]
async fn journal_drop_by_the_holder_rejects_a_late_older_journal() -> T {
    let fx = Fx::new();
    fx.write("a.md", "disk\n");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    fx.svc
        .journal_drop(DropJournal {
            key: key("a.md"),
            upto_seq: 5,
            lease_id: c.lease_id.clone(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    // A journal write that was in flight before the drop must not bring the draft back.
    let err = domain(fx.svc.journal(entry(&c, 5, "late\n")).await);
    assert_eq!(err, DocumentsError::StaleJournal);
    assert!(fx.journals().is_empty());
    fx.svc
        .journal(entry(&c, 6, "newer\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.journals().len(), 1);
    Ok(())
}
