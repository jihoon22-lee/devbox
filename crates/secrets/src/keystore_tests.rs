use crate::{Recovery, SecretsError, SecretsService, crypto::Key, keystore::DpapiKeyStore};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};
fn executable(dir: &Path, body: &str) -> std::path::PathBuf {
    let path = dir.join("fake-dpapi");
    std::fs::write(&path, format!("#!/usr/bin/python3\n{body}\n")).expect("owned fake");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("mode");
    path
}
#[tokio::test]
async fn dpapi_fixed_arguments_binary_stdio_and_recovery_reseal() {
    let (t, db, paths) = crate::store_tests::fixture();
    let argv = t.path().join("args");
    let body = format!(
        "import sys,json\nopen({:?},'w').write(json.dumps(sys.argv[1:]))\nb=sys.stdin.buffer.read()\nif '::Protect(' in sys.argv[-1]: r=b'fixture-sealed:'+bytes(x^170 for x in b)\nelse: r=bytes(x^170 for x in b.removeprefix(b'fixture-sealed:'))\nsys.stdout.buffer.write(r)",
        argv.display().to_string()
    );
    let store = DpapiKeyStore::new(&paths.data_dir).with_executable(executable(t.path(), &body));
    let key = Key::from_bytes([b'K'; 32]);
    let blob = store.seal(&key).await.expect("fake seal");
    assert!(!blob.windows(32).any(|w| w == [b'K'; 32]));
    store.publish(&blob, false).expect("publish sealed only");
    assert_eq!(store.unseal(&blob).await.expect("unseal").id(), key.id());
    let args = std::fs::read_to_string(argv).expect("args");
    assert!(args.contains("-NoProfile"));
    assert!(args.contains("-NonInteractive"));
    assert!(args.contains("CurrentUser"));
    assert!(!args.contains(&"K".repeat(32)));
    let service = SecretsService::with_key(db, paths.clone(), key)
        .await
        .expect("key")
        .with_keystore(store);
    let recovery = service.export_key().await.expect("explicit export");
    let before = service.status().await.expect("status").key_id;
    service
        .import_key(Recovery {
            recovery: recovery.recovery.clone(),
        })
        .await
        .expect("same recovery");
    assert_eq!(service.status().await.expect("status").key_id, before);
    assert_eq!(
        std::fs::read(paths.data_dir.join("keys/data.key.dpapi")).expect("sealed file"),
        blob
    );
}
#[tokio::test]
async fn dpapi_cancel_timeout_and_output_limit_reap_owned_interop_children() {
    for mode in 0..3 {
        let overflow = mode == 1;
        let t = tempfile::tempdir().expect("owned fake");
        let pid = t.path().join("pid");
        let body = format!(
            "import os,sys,time\nopen({:?},'w').write(str(os.getpid()))\n{}\ntime.sleep(30)",
            pid.display().to_string(),
            if overflow {
                "sys.stdout.buffer.write(b'x'*70000);sys.stdout.buffer.flush()"
            } else {
                ""
            }
        );
        let store = DpapiKeyStore::new(t.path()).with_executable(executable(t.path(), &body));
        let task = tokio::spawn(async move { store.seal(&Key::from_bytes([8; 32])).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !pid.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("child started");
        let id = std::fs::read_to_string(pid).expect("child pid");
        if overflow {
            assert!(matches!(
                task.await.expect("join"),
                Err(SecretsError::KeyMismatch)
            ));
        } else if mode == 2 {
            assert!(matches!(
                task.await.expect("timeout join"),
                Err(SecretsError::Unavailable)
            ));
        } else {
            task.abort();
            assert!(task.await.expect_err("cancelled").is_cancelled());
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while Path::new(&format!("/proc/{id}")).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("owned child reaped");
    }
}
#[tokio::test]
async fn unavailable_stays_locked_but_wrong_key_reports_mismatch_until_unlock() {
    let (t, db, paths) = crate::store_tests::fixture();
    let original = SecretsService::with_key(db.clone(), paths.clone(), Key::from_bytes([3; 32]))
        .await
        .expect("original key");
    drop(original);
    let store = DpapiKeyStore::new(&paths.data_dir);
    store
        .publish(b"owned-sealed-fixture", false)
        .expect("sealed fixture");
    let unavailable = SecretsService::new(db.clone(), paths.clone())
        .with_keystore(store.with_executable(t.path().join("absent")));
    unavailable
        .initialize()
        .await
        .expect("daemon stays available");
    let status = unavailable.status().await.expect("status");
    assert!(status.locked);
    assert!(!status.key_mismatch);
    assert!(matches!(
        unavailable.export_key().await,
        Err(devbox_protocol::Fail::Domain(SecretsError::Locked))
    ));
    let fake = executable(
        t.path(),
        "import sys\nsys.stdout.buffer.write(bytes([4])*32)",
    );
    let wrong = SecretsService::new(db, paths.clone())
        .with_keystore(DpapiKeyStore::new(&paths.data_dir).with_executable(fake));
    wrong.initialize().await.expect("daemon stays available");
    assert!(wrong.status().await.expect("status").key_mismatch);
    assert!(matches!(
        wrong.export_key().await,
        Err(devbox_protocol::Fail::Domain(SecretsError::KeyMismatch))
    ));
    wrong
        .unlock(devbox_protocol::secrets::Unlock { key: vec![3; 32] })
        .await
        .expect("original key unlock");
    let status = wrong.status().await.expect("status");
    assert!(!status.locked);
    assert!(!status.key_mismatch);
    assert_eq!(
        std::fs::read(paths.data_dir.join("keys/data.key.dpapi")).expect("blob preserved"),
        b"owned-sealed-fixture"
    );
}

#[tokio::test]
async fn reset_rekeys_locally_preserves_names_and_restarts_with_the_committed_blob() {
    let (t, db, paths) = crate::store_tests::fixture();
    let fake = executable(
        t.path(),
        "import sys\nb=sys.stdin.buffer.read()\nsys.stdout.buffer.write(b'owned-sealed:'+b if '::Protect(' in sys.argv[-1] else b.removeprefix(b'owned-sealed:'))",
    );
    let store = DpapiKeyStore::new(&paths.data_dir).with_executable(fake);
    let original = Key::from_bytes([19; 32]);
    let original_id = original.id();
    store
        .publish(&store.seal(&original).await.expect("seal"), false)
        .expect("publish");
    let service = SecretsService::with_key(db.clone(), paths.clone(), original)
        .await
        .expect("fixture")
        .with_keystore(store.clone());
    service
        .set(crate::SetSecret {
            name: "TOKEN".into(),
            value: "owned-old-value".into(),
        })
        .await
        .expect("set");
    let config = std::fs::read(paths.config_dir.join("config.toml")).expect("shared names");
    store
        .publish(
            &store
                .seal(&Key::from_bytes([20; 32]))
                .await
                .expect("seal wrong account"),
            true,
        )
        .expect("mismatched fixture blob");
    drop(service);
    let service = SecretsService::new(db.clone(), paths.clone()).with_keystore(store.clone());
    service.initialize().await.expect("detect mismatch");
    assert!(
        service
            .public_status()
            .await
            .expect("mismatch state")
            .key_mismatch
    );
    service.reset().await.expect("confirmed reset");
    let status = service.status().await.expect("status");
    assert!(!status.locked && !status.key_mismatch);
    assert_ne!(status.key_id.as_deref(), Some(original_id.as_str()));
    let reset_id = status.key_id.expect("new native key id");
    let reset_blob = status.blob.expect("new native fallback blob");
    assert_eq!(
        store
            .unseal(&reset_blob)
            .await
            .expect("native fallback unwrap")
            .id(),
        reset_id
    );
    assert_eq!(
        store.for_key(&reset_id).blob().expect("committed blob"),
        Some(reset_blob.clone())
    );
    assert_eq!(
        std::fs::read(paths.config_dir.join("config.toml")).expect("names unchanged"),
        config
    );
    let names = service.list().await.expect("names");
    assert_eq!(names[0].name, "TOKEN");
    assert!(!names[0].has_value);
    let public =
        serde_json::to_value(service.public_status().await.expect("public")).expect("serialize");
    assert_eq!(
        public,
        serde_json::json!({"locked":false,"keyMismatch":false})
    );
    service
        .set(crate::SetSecret {
            name: "TOKEN".into(),
            value: "owned-new-value".into(),
        })
        .await
        .expect("reinput");
    drop(service);
    let restarted = SecretsService::new(db, paths).with_keystore(store);
    restarted.initialize().await.expect("restart");
    let restarted_status = restarted
        .status()
        .await
        .expect("native status after restart");
    assert_eq!(restarted_status.key_id, Some(reset_id));
    assert_eq!(restarted_status.blob, Some(reset_blob));
    assert_eq!(
        restarted
            .resolve(&["TOKEN".into()])
            .await
            .expect("reinput persists")["TOKEN"]
            .as_slice(),
        b"owned-new-value"
    );
    let recovery = restarted.export_key().await.expect("export new key");
    restarted
        .import_key(recovery)
        .await
        .expect("reimport selected key");
}

#[tokio::test]
async fn reset_failures_preserve_committed_key_values_blob_and_shared_settings() {
    for failure in ["dpapi", "file", "database"] {
        let (t, db, paths) = crate::store_tests::fixture();
        let fake = executable(
            t.path(),
            "import sys\nb=sys.stdin.buffer.read()\nsys.stdout.buffer.write(b'owned-sealed:'+b)",
        );
        let store = DpapiKeyStore::new(&paths.data_dir).with_executable(fake);
        let original = Key::from_bytes([21; 32]);
        let old_id = original.id();
        let old_blob = store.seal(&original).await.expect("seal original");
        store.publish(&old_blob, false).expect("publish original");
        let service = SecretsService::with_key(db.clone(), paths.clone(), original)
            .await
            .expect("fixture")
            .with_keystore(store.clone());
        std::fs::write(
            paths.config_dir.join("config.toml"),
            "[secrets]\nnames=['TOKEN']\n[mcp]\nwrite=false\n",
        )
        .expect("owned config");
        service
            .set(crate::SetSecret {
                name: "TOKEN".into(),
                value: "owned-value".into(),
            })
            .await
            .expect("set");
        let config = std::fs::read(paths.config_dir.join("config.toml")).expect("config");
        let service = if failure == "dpapi" {
            service.with_keystore(store.with_executable(t.path().join("absent")))
        } else {
            service
        };
        if failure == "database" {
            db.write(|tx| tx.execute_batch("CREATE TRIGGER deny_reset BEFORE DELETE ON secrets_values BEGIN SELECT RAISE(ABORT,'owned reset fault'); END;")).await.expect("fault injection");
        }
        if failure == "file" {
            // Inject a forbidden parent-directory symlink at publication time;
            // restore the owned directory before checking the original data.
            std::fs::rename(
                paths.data_dir.join("keys"),
                paths.data_dir.join("saved-keys"),
            )
            .expect("owned rename");
            std::os::unix::fs::symlink(
                paths.data_dir.join("saved-keys"),
                paths.data_dir.join("keys"),
            )
            .expect("owned symlink");
        }
        assert!(service.reset().await.is_err(), "{failure}");
        if failure == "file" {
            std::fs::remove_file(paths.data_dir.join("keys")).expect("owned link");
            std::fs::rename(
                paths.data_dir.join("saved-keys"),
                paths.data_dir.join("keys"),
            )
            .expect("restore");
        }
        assert_eq!(
            std::fs::read_dir(paths.data_dir.join("keys"))
                .expect("keys")
                .count(),
            1,
            "prepared blob must be removed on {failure}"
        );
        assert_eq!(
            service.status().await.expect("status").key_id.as_deref(),
            Some(old_id.as_str())
        );
        assert_eq!(
            std::fs::read(paths.data_dir.join("keys/data.key.dpapi")).expect("old blob"),
            old_blob
        );
        assert_eq!(
            service.resolve(&["TOKEN".into()]).await.expect("preserved")["TOKEN"].as_slice(),
            b"owned-value"
        );
        assert_eq!(
            std::fs::read(paths.config_dir.join("config.toml")).expect("config preserved"),
            config
        );
    }
}

#[tokio::test]
async fn reset_commit_updates_memory_even_if_the_request_is_cancelled() {
    use std::sync::Arc;
    let (t, db, paths) = crate::store_tests::fixture();
    let fake = executable(
        t.path(),
        "import sys\nsys.stdout.buffer.write(b'owned-sealed:'+sys.stdin.buffer.read())",
    );
    let service = Arc::new(
        SecretsService::with_key(db.clone(), paths.clone(), Key::from_bytes([23; 32]))
            .await
            .expect("fixture")
            .with_keystore(DpapiKeyStore::new(&paths.data_dir).with_executable(fake)),
    );
    let old_id = service.status().await.expect("old status").key_id;
    let (entered, entered_wait) = tokio::sync::oneshot::channel();
    let (release, release_wait) = std::sync::mpsc::channel();
    let writer_db = db.clone();
    let blocker = tokio::spawn(async move {
        writer_db
            .write(move |_| {
                let _ = entered.send(());
                release_wait
                    .recv_timeout(Duration::from_secs(5))
                    .expect("release owned blocker");
                Ok(())
            })
            .await
    });
    entered_wait.await.expect("writer blocked");
    let resetting = service.clone();
    let task = tokio::spawn(async move { resetting.reset().await });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if std::fs::read_dir(paths.data_dir.join("keys"))
                .is_ok_and(|entries| entries.count() == 1)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("new sealed file prepared before database write");
    task.abort();
    assert!(task.await.expect_err("cancelled caller").is_cancelled());
    release.send(()).expect("release writer");
    blocker.await.expect("writer joined").expect("write done");
    // Queue after the reset request; reaching this read confirms its write finished.
    db.write(|_| Ok(())).await.expect("writer drained");
    let status = service.status().await.expect("committed in-memory key");
    assert!(!status.locked);
    assert_ne!(status.key_id, old_id);
    service
        .set(crate::SetSecret {
            name: "TOKEN".into(),
            value: "owned-after-cancel".into(),
        })
        .await
        .expect("new key remains usable");
    assert_eq!(
        service.resolve(&["TOKEN".into()]).await.expect("resolve")["TOKEN"].as_slice(),
        b"owned-after-cancel"
    );
}
