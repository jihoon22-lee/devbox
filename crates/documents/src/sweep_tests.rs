use crate::fixture::*;
use crate::*;
use devbox_core::{HubMessage, HubReceiver};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

fn age(path: &Path, by: Duration) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(SystemTime::now() - by))
        .expect("age fixture file");
}
async fn next_change(rx: &mut HubReceiver, within: Duration) -> Option<DocumentChanged> {
    match tokio::time::timeout(within, rx.recv()).await {
        Ok(Some(HubMessage::Event { payload, .. })) => {
            Some(serde_json::from_str(&payload).expect("changed payload"))
        }
        Ok(other) => panic!("unexpected hub message {other:?}"),
        Err(_) => None,
    }
}
fn temp_names(dir: &Path) -> Vec<String> {
    files(dir)
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .filter(|n| n.starts_with(".devbox-tmp-"))
        .collect()
}

#[test]
fn internal_paths_are_detected_per_component() {
    for path in [
        "a/.trash/b.md",
        "x/.devbox-tmp-1/y",
        ".devbox-tmp-abc",
        ".git/config",
        "a/.git",
        ".trash",
    ] {
        assert!(is_internal(path), "{path}");
    }
    for path in [
        "a/b.md",
        "trash/b.md",
        "a/.trashy.md",
        "devbox-tmp-1",
        "a/.gitignore",
    ] {
        assert!(!is_internal(path), "{path}");
    }
}

#[tokio::test]
async fn internal_temp_files_are_invisible_and_swept_on_start() -> T {
    let fail = Arc::new(AtomicBool::new(true));
    let flag = fail.clone();
    let mut fx = Fx::with(move |c| {
        let flag = flag.clone();
        c.hooks.after_tmp_written = Some(Arc::new(move || flag.load(Ordering::SeqCst)));
    });
    fx.write("a.md", "v1\n");
    std::fs::create_dir_all(fx.project.join("src"))?;
    std::fs::create_dir_all(fx.project.join("other"))?;
    std::fs::write(fx.project.join("src/p.rs"), "fn main() {}\n")?;
    fx.svc.start().await?;
    let mut changed = fx.hub.subscribe("documents.changed");
    let c = claimed(&fx.svc, "new.md", "w1").await;
    assert!(fx.svc.save(save(&c, c.rev.clone(), "v2\n")).await.is_err());
    let left = temp_names(&fx.vault);
    assert_eq!(left.len(), 1, "an interrupted save leaves its temp file");
    assert_eq!(
        fx.ledger().len(),
        1,
        "the temp file was recorded before it existed"
    );
    assert!(
        next_change(&mut changed, Duration::from_millis(800))
            .await
            .is_none()
    );

    fx.svc
        .claim(
            Default::default(),
            ClaimDocument {
                key: DocKey {
                    root: "project:p".into(),
                    path: "src/p.rs".into(),
                },
                policy: DocPolicy {
                    kind: DocKind::Code,
                },
                window_id: "w1".into(),
            },
        )
        .await
        .map_err(|e| format!("{e:?}"))?;
    let hours = Duration::from_secs(2 * 3600);
    age(&fx.vault.join(&left[0]), hours);
    for entry in fx.ledger() {
        age(&entry, hours);
    }
    std::fs::create_dir_all(fx.vault.join("sub"))?;
    for (path, by) in [
        (
            fx.vault
                .join("sub/.devbox-tmp-write-00000000000000000000000000000000"),
            hours,
        ),
        (
            fx.vault.join(".devbox-tmp-recent"),
            Duration::from_secs(600),
        ),
        (
            fx.project
                .join("src/.devbox-tmp-write-11111111111111111111111111111111"),
            hours,
        ),
        (fx.project.join("other/.devbox-tmp-w"), hours),
    ] {
        std::fs::write(&path, "stale")?;
        age(&path, by);
    }
    fail.store(false, Ordering::SeqCst);
    fx.restart();
    fx.svc.start().await?;
    assert_eq!(temp_names(&fx.vault), [".devbox-tmp-recent"]);
    assert!(temp_names(&fx.vault.join("sub")).is_empty());
    assert!(fx.ledger().is_empty());
    assert!(
        temp_names(&fx.project.join("src")).is_empty(),
        "claimed folder is swept"
    );
    assert_eq!(
        temp_names(&fx.project.join("other")),
        [".devbox-tmp-w"],
        "project trees are not walked"
    );
    assert_eq!(fx.read("a.md"), "v1\n");
    Ok(())
}

#[tokio::test]
async fn external_edit_emits_changed_with_new_rev() -> T {
    let fx = Fx::new();
    fx.write("a.md", "v1\n");
    std::fs::write(fx.project.join("p.rs"), "one\n")?;
    fx.svc.start().await?;
    let mut changed = fx.hub.subscribe("documents.changed");
    let c = claimed(&fx.svc, "a.md", "w1").await;
    std::fs::write(fx.vault.join("a.md"), "external\n")?;
    let event = next_change(&mut changed, Duration::from_secs(1))
        .await
        .expect("changed within one second");
    assert_eq!(event.key, key("a.md"));
    let rev = event.rev.expect("file still exists");
    assert_ne!(
        Some(rev.hash.clone()),
        c.rev.as_ref().map(|r| r.hash.clone())
    );
    assert_eq!(rev.size, "external\n".len() as u64);
    for _ in 0..3 {
        age(&fx.vault.join("a.md"), Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        next_change(&mut changed, Duration::from_millis(800))
            .await
            .is_none(),
        "metadata-only touches are not changes"
    );
    fx.svc
        .save(save(&c, Some(rev), "mine\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        next_change(&mut changed, Duration::from_millis(800))
            .await
            .is_none(),
        "our own save is not reported back"
    );

    let code = fx
        .svc
        .claim(
            Default::default(),
            ClaimDocument {
                key: DocKey {
                    root: "project:p".into(),
                    path: "p.rs".into(),
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
    std::fs::write(fx.project.join("p.rs"), "two\n")?;
    let event = next_change(&mut changed, Duration::from_secs(1))
        .await
        .expect("claimed project file is watched");
    assert_eq!(event.key, code.key);
    std::fs::remove_file(fx.project.join("p.rs"))?;
    let event = next_change(&mut changed, Duration::from_secs(1))
        .await
        .expect("deletion is reported");
    assert_eq!((event.key, event.rev), (code.key, None));
    Ok(())
}

#[tokio::test]
async fn a_new_document_in_a_new_folder_is_watched_after_its_first_save() -> T {
    let fx = Fx::new();
    fx.svc.start().await?;
    let mut changed = fx.hub.subscribe("documents.changed");
    let c = fx
        .svc
        .claim(
            Default::default(),
            ClaimDocument {
                key: DocKey {
                    root: "project:p".into(),
                    path: "new/p.rs".into(),
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
    assert!(c.rev.is_none());
    fx.svc
        .save(save(&c, None, "fn main() {}\n"))
        .await
        .map_err(|e| format!("{e:?}"))?;
    assert!(
        next_change(&mut changed, Duration::from_millis(600))
            .await
            .is_none()
    );
    std::fs::write(fx.project.join("new/p.rs"), "external\n")?;
    let event = next_change(&mut changed, Duration::from_secs(1))
        .await
        .expect("the saved document is now watched");
    assert_eq!(event.key, c.key);
    Ok(())
}
