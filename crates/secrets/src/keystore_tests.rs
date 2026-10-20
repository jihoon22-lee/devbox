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
