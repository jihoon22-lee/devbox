use crate::fixture::*;
use crate::*;
use devbox_protocol::Fail;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn save_with_stale_base_rev_returns_conflict() -> T {
    let (fx, root) = fixture_with_note("a.md", "v1\n");
    let svc = &fx.svc;
    let c = claimed(svc, "a.md", "w1").await;
    let mut journal = WriteJournal {
        lease_id: c.lease_id.clone().expect("lease"),
        seq: 1,
        base_rev: c.rev.clone(),
        text: "mine\n".into(),
    };
    svc.journal(journal.clone())
        .await
        .map_err(|e| format!("{e:?}"))?;
    std::fs::write(root.join("a.md"), "external\n")?;
    let err = svc
        .save(save(&c, c.rev.clone(), "mine\n"))
        .await
        .expect_err("a stale base revision conflicts");
    assert!(matches!(err, Fail::Domain(DocumentsError::Conflict { .. })));
    let Fail::Domain(DocumentsError::Conflict { rev: Some(disk) }) = err else {
        panic!("conflict carries the disk revision");
    };
    assert_ne!(Some(disk), c.rev);
    assert_eq!(std::fs::read_to_string(root.join("a.md"))?, "external\n");
    assert_eq!(fx.journals().len(), 1, "the journal survives a conflict");
    journal.seq = 2;
    svc.journal(journal).await.map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[tokio::test]
async fn save_preserves_mode_crlf_and_bom() -> T {
    let fx = Fx::new();
    let path = fx.vault.join("a.md");
    std::fs::write(&path, "\u{feff}old\r\nline\r\n")?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))?;
    let c = claimed(&fx.svc, "a.md", "w1").await;
    assert_eq!(c.text.as_deref(), Some("old\nline\n"));
    assert_eq!((c.eol, c.bom), (Eol::Crlf, true));
    let saved = fx
        .svc
        .save(save(&c, c.rev.clone(), "a\nb\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(std::fs::read(&path)?, "\u{feff}a\r\nb\r\n".as_bytes());
    assert_eq!(
        std::fs::metadata(&path)?.permissions().mode() & 0o777,
        0o640
    );
    let disk = fx
        .svc
        .read(ReadDocument {
            key: key("a.md"),
            policy: None,
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(disk.rev, Some(saved.rev));
    assert_eq!(disk.text.as_deref(), Some("a\nb\n"));
    assert!(
        files(&fx.vault)
            .iter()
            .all(|p| !is_internal(&p.file_name().unwrap_or_default().to_string_lossy())),
        "no temporary file is left next to the note"
    );
    assert!(fx.ledger().is_empty());
    Ok(())
}

#[tokio::test]
async fn save_through_symlink_writes_the_target() -> T {
    let fx = Fx::new();
    fx.write("real/a.md", "v1\n");
    std::os::unix::fs::symlink("real/a.md", fx.vault.join("link.md"))?;
    let c = claimed(&fx.svc, "link.md", "w1").await;
    assert_eq!(c.key, key("real/a.md"));
    fx.svc
        .save(save(&c, c.rev.clone(), "v2\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read("real/a.md"), "v2\n");
    assert!(
        std::fs::symlink_metadata(fx.vault.join("link.md"))?
            .file_type()
            .is_symlink()
    );
    Ok(())
}

#[tokio::test]
async fn binary_non_utf8_and_oversized_files_open_read_only() -> T {
    let fx = Fx::with(|c| c.max_wire_text = 1024);
    std::fs::write(fx.vault.join("bin.md"), b"a\0b")?;
    std::fs::write(fx.vault.join("latin.md"), b"caf\xff\n")?;
    std::fs::write(fx.vault.join("big.md"), vec![b'a'; NOTE_LIMIT as usize + 1])?;
    std::fs::write(fx.project.join("quoted.rs"), "\"".repeat(600))?;
    for (path, reason) in [
        ("bin.md", ReadOnlyReason::Binary),
        ("latin.md", ReadOnlyReason::Encoding),
        ("big.md", ReadOnlyReason::TooLarge),
    ] {
        let c = claimed(&fx.svc, path, "w1").await;
        assert_eq!(c.read_only, Some(ReadOnly { reason }), "{path}");
        assert_eq!((c.lease_id, c.text), (None, None), "{path}");
        assert!(c.rev.is_some(), "{path}");
    }
    let code = fx
        .svc
        .claim(
            tokio_util::sync::CancellationToken::new(),
            ClaimDocument {
                key: DocKey {
                    root: "project:p".into(),
                    path: "quoted.rs".into(),
                },
                policy: DocPolicy {
                    kind: DocKind::Code,
                },
                window_id: "w1".into(),
            },
        )
        .await
        .map_err(|e| format!("{e:?}"))?
        .expect("live owner");
    assert_eq!(
        code.read_only,
        Some(ReadOnly {
            reason: ReadOnlyReason::TooLarge
        }),
        "600 quotes escape to 1200 bytes over the 1024 budget"
    );
    Ok(())
}

#[tokio::test]
async fn write_outside_registered_roots_is_refused() -> T {
    let fx = Fx::new();
    std::fs::write(fx.outside.join("x.md"), "outside\n")?;
    std::os::unix::fs::symlink(fx.outside.join("x.md"), fx.vault.join("out.md"))?;
    std::os::unix::fs::symlink(&fx.outside, fx.vault.join("dir"))?;
    use DocumentsError::{InvalidInput, OutOfScope};
    for (path, want) in [
        ("../outside/x.md", OutOfScope),
        ("out.md", OutOfScope),
        ("dir/x.md", OutOfScope),
        ("/etc/passwd", OutOfScope),
        ("a/./b.md", InvalidInput),
        ("", InvalidInput),
    ] {
        let err = domain(fx.svc.claim(Default::default(), claim(path, "w1")).await);
        assert_eq!(err, want, "{path}");
        let wire = serde_json::to_string(&err)?;
        assert!(
            !wire.contains("x.md") && !wire.contains("outside"),
            "{wire}"
        );
    }
    let unknown = DocKey {
        root: "project:missing".into(),
        path: "a.md".into(),
    };
    let err = domain(
        fx.svc
            .read(ReadDocument {
                key: unknown,
                policy: None,
            })
            .await,
    );
    assert_eq!(err, DocumentsError::OutOfScope);
    let err = domain(
        fx.svc
            .write_disk(&key("dir/x.md"), None, Expect::Present, "pwned\n")
            .await,
    );
    assert_eq!(err, DocumentsError::OutOfScope);
    assert_eq!(
        std::fs::read_to_string(fx.outside.join("x.md"))?,
        "outside\n"
    );
    assert_eq!(fix_names(&fx.outside), ["x.md"]);
    Ok(())
}

fn fix_names(dir: &std::path::Path) -> Vec<String> {
    files(dir)
        .iter()
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[tokio::test]
async fn unrelated_paths_and_symlinks_are_rejected() -> T {
    // v0.9.0 wsl-helper document.rs, adapted: the helper's path checks became WriteScope.
    let fx = Fx::new();
    std::fs::write(fx.outside.join("outside.md"), "external")?;
    std::os::unix::fs::symlink(fx.outside.join("outside.md"), fx.vault.join("link.md"))?;
    let scope = fx.svc.scope().await.map_err(|e| format!("{e:?}"))?;
    assert!(scope.resolve(&key("../outside/outside.md"), false).is_err());
    assert!(scope.resolve(&key("link.md"), false).is_err());
    let err = domain(
        fx.svc
            .write_disk(&key("link.md"), None, Expect::Present, "ours")
            .await,
    );
    assert_eq!(err, DocumentsError::OutOfScope);
    assert_eq!(
        std::fs::read_to_string(fx.outside.join("outside.md"))?,
        "external"
    );
    Ok(())
}

#[tokio::test]
async fn new_document_saves_with_expect_absent_and_refuses_existing() -> T {
    let fx = Fx::new();
    let c = claimed(&fx.svc, "new/n.md", "w1").await;
    assert_eq!((c.rev.clone(), c.text.as_deref()), (None, Some("")));
    assert!(c.read_only.is_none());
    fx.svc
        .save(save(&c, None, "hello\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(fx.read("new/n.md"), "hello\n");
    let again = domain(fx.svc.save(save(&c, None, "again\n")).await);
    assert_eq!(again, DocumentsError::Exists);
    assert_eq!(fx.read("new/n.md"), "hello\n");
    Ok(())
}

#[tokio::test]
async fn oversized_claim_defers_the_journal() -> T {
    let fx = Fx::with(|c| c.max_wire_text = 4 * 1024 * 1024);
    let body: String = "\"\\".repeat(1_900 * 1024 / 2);
    fx.write("big.md", &body);
    let c = claimed(&fx.svc, "big.md", "w1").await;
    let lease = c.lease_id.clone().expect("lease");
    let mut journal_text = body.clone();
    journal_text.replace_range(0..2, "xx");
    fx.svc
        .journal(WriteJournal {
            lease_id: lease.clone(),
            seq: 7,
            base_rev: c.rev.clone(),
            text: journal_text.clone(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let again = claimed(&fx.svc, "big.md", "w1").await;
    assert_eq!(again.lease_id.as_deref(), Some(lease.as_str()));
    assert_eq!(again.text.as_deref(), Some(body.as_str()));
    let journal = again.journal.expect("journal is reported");
    assert!(journal.deferred && journal.text.is_none());
    assert_eq!(journal.seq, 7);
    let full = fx
        .svc
        .journal_read(LeaseRef {
            lease_id: lease.clone(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!((full.seq, full.text), (7, journal_text.clone()));
    fx.svc
        .take(
            Default::default(),
            TakeDocument {
                key: key("big.md"),
                window_id: "w2".into(),
                policy: None,
            },
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
    let err = domain(fx.svc.journal_read(LeaseRef { lease_id: lease }).await);
    assert_eq!(
        err,
        DocumentsError::LeaseLost {
            reason: LeaseLoss::Taken
        }
    );

    let roomy = Fx::new();
    roomy.write("big.md", &body);
    let c = claimed(&roomy.svc, "big.md", "w1").await;
    roomy
        .svc
        .journal(WriteJournal {
            lease_id: c.lease_id.clone().expect("lease"),
            seq: 7,
            base_rev: c.rev.clone(),
            text: journal_text.clone(),
        })
        .await
        .map_err(|e| format!("{e:?}"))?;
    let one = claimed(&roomy.svc, "big.md", "w1").await;
    let journal = one.journal.expect("journal");
    assert!(!journal.deferred);
    assert_eq!(journal.text, Some(journal_text));
    Ok(())
}

#[tokio::test]
async fn daemon_private_folders_are_refused_through_any_root() -> T {
    let fx = Fx::new();
    std::fs::create_dir_all(fx.data.join("keys"))?;
    for (rel, text) in [("devbox.db", "db"), (".daemon.lock", ""), ("keys/k", "key")] {
        std::fs::write(fx.data.join(rel), text)?;
    }
    std::os::unix::fs::symlink(fx.data.join("devbox.db"), fx.vault.join("db.md"))?;
    // A project registered at the parent of the data folder (like `~`).
    let base = fx.vault.parent().expect("fixture base").to_path_buf();
    let roots = vec![
        Root {
            id: "vault".into(),
            path: fx.vault.clone(),
        },
        Root {
            id: "project:home".into(),
            path: base,
        },
    ];
    let roots: Roots = std::sync::Arc::new(move || {
        let roots = roots.clone();
        Box::pin(async move { roots })
    });
    let mut config = DocumentsConfig::new(fx.data.clone(), roots, fx.hub.clone());
    config.private = vec![fx.data.clone()];
    let svc = DocumentsService::new(config);
    let home = |path: &str| DocKey {
        root: "project:home".into(),
        path: path.into(),
    };
    let data = |path: &str| DocKey {
        root: "data".into(),
        path: path.into(),
    };
    for key in [
        home("data/devbox.db"),
        home("data/.daemon.lock"),
        home("data/keys/k"),
        data("devbox.db"),
        key("db.md"),
    ] {
        let claim = ClaimDocument {
            key: key.clone(),
            policy: note(),
            window_id: "w1".into(),
        };
        let err = domain(svc.claim(Default::default(), claim).await);
        assert_eq!(err, DocumentsError::OutOfScope, "claim {key:?}");
        let err = domain(svc.write_disk(&key, None, Expect::Absent, "x").await);
        assert_eq!(err, DocumentsError::OutOfScope, "write {key:?}");
    }
    fx.write("a.md", "a\n");
    for (from, to) in [
        (home("data/devbox.db"), home("moved.db")),
        (home("data"), home("data2")),
        (home("vault/a.md"), home("data/a.md")),
    ] {
        let err = domain(
            svc.rename(RenameDocument {
                lease_id: None,
                key: from.clone(),
                to,
            })
            .await,
        );
        assert_eq!(err, DocumentsError::OutOfScope, "rename {from:?}");
    }
    assert_eq!(std::fs::read_to_string(fx.data.join("devbox.db"))?, "db");
    assert_eq!(std::fs::read_to_string(fx.data.join("keys/k"))?, "key");
    assert_eq!(fx.read("a.md"), "a\n");
    Ok(())
}
