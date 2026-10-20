use crate::{SecretsError, SecretsService, SetSecret, crypto::Key, keystore::DpapiKeyStore};
use devbox_protocol::{Fail, secrets::Bootstrap};
use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};

fn keystore(root: &std::path::Path, paths: &devbox_core::Paths) -> DpapiKeyStore {
    let executable = root.join("fake-dpapi");
    std::fs::write(&executable, "#!/usr/bin/python3\nimport sys\nb=sys.stdin.buffer.read()\nsys.stdout.buffer.write(b'owned-sealed:'+b if '::Protect(' in sys.argv[-1] else b.removeprefix(b'owned-sealed:'))\n").expect("owned executable");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("owned executable permissions");
    DpapiKeyStore::new(&paths.data_dir).with_executable(executable)
}
fn secret() -> SetSecret {
    SetSecret {
        name: "TOKEN".into(),
        value: "owned-value".into(),
    }
}

#[tokio::test]
async fn first_key_database_failure_is_retryable_without_touching_other_key_material() {
    for bootstrap in [false, true] {
        for restart in [false, true] {
            let (root, db, paths) = crate::store_tests::fixture();
            let store = keystore(root.path(), &paths);
            let unrelated = store.for_key(&Key::from_bytes([91; 32]).id());
            unrelated
                .publish(b"unrelated-sealed-key", false)
                .expect("owned sentinel");
            let service =
                SecretsService::new(db.clone(), paths.clone()).with_keystore(store.clone());
            let supplied = Key::from_bytes([92; 32]);
            let blob = store.seal(&supplied).await.expect("fixture seal");
            db.write(|tx| tx.execute_batch("CREATE TRIGGER deny_init BEFORE INSERT ON secrets_key BEGIN SELECT RAISE(ABORT,'owned initialization fault'); END;")).await.expect("inject database failure");
            let result = if bootstrap {
                service
                    .bootstrap(Bootstrap {
                        key_id: supplied.id(),
                        blob: blob.clone(),
                    })
                    .await
            } else {
                service.set(secret()).await
            };
            assert!(
                matches!(result, Err(Fail::Internal(_))),
                "database failure must surface"
            );
            let status = service
                .status()
                .await
                .expect("failed initialization status");
            assert!(status.locked);
            assert_eq!(status.key_id, None);
            assert_eq!(status.blob, None, "uncommitted publication must roll back");
            assert!(
                !status.key_mismatch,
                "failed initialization is not a foreign key"
            );
            assert!(service.list().await.expect("no secret saved").is_empty());
            assert_eq!(
                unrelated.blob().expect("sentinel preserved"),
                Some(b"unrelated-sealed-key".to_vec())
            );
            assert_eq!(
                std::fs::read_dir(paths.data_dir.join("keys"))
                    .expect("keys")
                    .count(),
                1
            );
            db.write(|tx| tx.execute_batch("DROP TRIGGER deny_init"))
                .await
                .expect("remove owned fault");
            let service = if restart {
                drop(service);
                let restarted =
                    SecretsService::new(db.clone(), paths.clone()).with_keystore(store.clone());
                restarted
                    .initialize()
                    .await
                    .expect("restart after failed initialization");
                restarted
            } else {
                service
            };
            if bootstrap {
                service
                    .bootstrap(Bootstrap {
                        key_id: supplied.id(),
                        blob: blob.clone(),
                    })
                    .await
                    .expect("retry native bootstrap");
                service
                    .unlock(devbox_protocol::secrets::Unlock { key: vec![92; 32] })
                    .await
                    .expect("matching native key");
            }
            service.set(secret()).await.expect("retry first set");
            let committed = service.status().await.expect("committed status");
            assert!(!committed.locked && !committed.key_mismatch);
            assert_eq!(
                service
                    .resolve(&["TOKEN".into()])
                    .await
                    .expect("saved secret")["TOKEN"]
                    .as_slice(),
                b"owned-value"
            );
            drop(service);
            let restarted = SecretsService::new(db, paths).with_keystore(store);
            restarted.initialize().await.expect("load committed key");
            assert_eq!(
                restarted.status().await.expect("restart status").key_id,
                committed.key_id
            );
            assert_eq!(
                restarted
                    .resolve(&["TOKEN".into()])
                    .await
                    .expect("persisted secret")["TOKEN"]
                    .as_slice(),
                b"owned-value"
            );
        }
    }
}

#[tokio::test]
async fn initial_key_commit_survives_request_cancellation() {
    let (root, db, paths) = crate::store_tests::fixture();
    let store = keystore(root.path(), &paths);
    let service =
        Arc::new(SecretsService::new(db.clone(), paths.clone()).with_keystore(store.clone()));
    let (entered, entered_wait) = tokio::sync::oneshot::channel();
    let (release, release_wait) = std::sync::mpsc::channel();
    let writer = db.clone();
    let blocker = tokio::spawn(async move {
        writer
            .write(move |_| {
                let _ = entered.send(());
                release_wait
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release writer");
                Ok(())
            })
            .await
    });
    entered_wait.await.expect("writer blocked");
    let setting = service.clone();
    let task = tokio::spawn(async move { setting.set(secret()).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !std::fs::read_dir(paths.data_dir.join("keys"))
            .is_ok_and(|entries| entries.count() == 1)
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("sealed key published before identity write");
    task.abort();
    assert!(task.await.expect_err("cancelled request").is_cancelled());
    release.send(()).expect("release writer");
    blocker
        .await
        .expect("join writer")
        .expect("writer completed");
    db.write(|_| Ok(()))
        .await
        .expect("queued initialization drained");
    let status = service.status().await.expect("committed key status");
    assert!(!status.locked && !status.key_mismatch);
    assert!(status.key_id.is_some() && status.blob.is_some());
    service
        .set(secret())
        .await
        .expect("same daemon remains usable");
    drop(service);
    let restarted = SecretsService::new(db, paths).with_keystore(store);
    restarted.initialize().await.expect("reload committed key");
    assert_eq!(
        restarted
            .resolve(&["TOKEN".into()])
            .await
            .expect("persisted secret")["TOKEN"]
            .as_slice(),
        b"owned-value"
    );
}

#[tokio::test]
async fn initialization_does_not_adopt_or_overwrite_unidentified_legacy_key() {
    let (root, db, paths) = crate::store_tests::fixture();
    let store = keystore(root.path(), &paths);
    let old_blob = store
        .seal(&Key::from_bytes([93; 32]))
        .await
        .expect("fixture seal");
    store
        .publish(&old_blob, false)
        .expect("existing unidentified key");
    let service = SecretsService::new(db, paths).with_keystore(store.clone());
    assert!(
        service
            .status()
            .await
            .expect("mismatch status")
            .key_mismatch
    );
    assert!(matches!(
        service.set(secret()).await,
        Err(Fail::Domain(SecretsError::KeyMismatch))
    ));
    assert!(matches!(
        service
            .bootstrap(Bootstrap {
                key_id: Key::from_bytes([94; 32]).id(),
                blob: b"new-key".to_vec()
            })
            .await,
        Err(Fail::Domain(SecretsError::KeyMismatch))
    ));
    assert_eq!(
        store.blob().expect("original key preserved"),
        Some(old_blob)
    );
    assert_eq!(
        service.status().await.expect("no identity adopted").key_id,
        None
    );
}
