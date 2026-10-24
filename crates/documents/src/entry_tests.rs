use crate::{fixture::*, *};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;

fn paused() -> (Fx, Arc<Notify>, Arc<Notify>) {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (a, b) = (entered.clone(), release.clone());
    let fx = Fx::with(move |config| {
        let (a, b) = (a.clone(), b.clone());
        config.hooks.before_entry_remove = Some(Arc::new(move || {
            let (a, b) = (a.clone(), b.clone());
            Box::pin(async move {
                a.notify_one();
                b.notified().await;
            })
        }));
    });
    (fx, entered, release)
}
#[tokio::test]
async fn entry_remove_serializes_with_save_and_never_recreates_deleted_bytes() {
    let (fx, entered, release) = paused();
    fx.write("code.ts", "before");
    let claimed = claimed(&fx.svc, "code.ts", "source").await;
    let stamp = fx.svc.stat_entry(&key("code.ts")).await.expect("stat").rev;
    let service = fx.svc.clone();
    let remove = tokio::spawn(async move { service.remove_entry(&key("code.ts"), stamp).await });
    entered.notified().await;
    let request = save(&claimed, claimed.rev.clone(), "unsaved replacement");
    let mut saving = Box::pin(fx.svc.save(request));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut saving)
            .await
            .is_err(),
        "save must wait for the namespace lock"
    );
    assert_eq!(fx.read("code.ts"), "before");
    release.notify_one();
    remove.await.expect("join").expect("remove");
    assert!(
        saving.await.is_err(),
        "an old Present save cannot recreate a deleted file"
    );
    assert!(!fx.vault.join("code.ts").exists());
}
#[tokio::test]
async fn entry_remove_serializes_with_rename_and_keeps_a_stale_external_edit() {
    let (fx, entered, release) = paused();
    fx.write("nested/code.ts", "before");
    let stamp = fx
        .svc
        .stat_entry(&key("nested/code.ts"))
        .await
        .expect("stat")
        .rev;
    let service = fx.svc.clone();
    let remove =
        tokio::spawn(async move { service.remove_entry(&key("nested/code.ts"), stamp).await });
    entered.notified().await;
    let from = key("nested");
    let to = key("moved");
    let mut rename = Box::pin(fx.svc.move_entry(&from, &to));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut rename)
            .await
            .is_err(),
        "rename must wait for the namespace lock"
    );
    fx.write("nested/code.ts", "outside");
    release.notify_one();
    assert!(remove.await.expect("join").is_err(), "stale delete refused");
    rename.await.expect("rename the preserved file");
    assert_eq!(fx.read("moved/code.ts"), "outside");
}
#[tokio::test]
async fn entry_symlink_delete_preserves_target_and_create_never_follows_a_dangling_link() {
    let fx = Fx::new();
    fx.write("target.ts", "keep");
    std::os::unix::fs::symlink("target.ts", fx.vault.join("link.ts")).expect("owned symlink");
    let entry = fx
        .svc
        .stat_entry(&key("link.ts"))
        .await
        .expect("symlink stat");
    assert_eq!(entry.kind, EntryKind::Symlink);
    fx.svc
        .remove_entry(&entry.key, entry.rev)
        .await
        .expect("delete link only");
    assert_eq!(fx.read("target.ts"), "keep");
    std::os::unix::fs::symlink("absent.ts", fx.vault.join("dangling.ts")).expect("dangling link");
    assert!(fx.svc.create_empty(&key("dangling.ts")).await.is_err());
    assert!(!fx.vault.join("absent.ts").exists());
}

#[tokio::test]
async fn code_claim_rejects_alias_to_a_different_registered_root() {
    let fx = Fx::new();
    fx.write("outside-project.ts", "other registered root");
    std::os::unix::fs::symlink(
        fx.vault.join("outside-project.ts"),
        fx.project.join("alias.ts"),
    )
    .expect("owned link");
    let claim = ClaimDocument {
        key: DocKey {
            root: "project:p".into(),
            path: "alias.ts".into(),
        },
        policy: DocPolicy {
            kind: DocKind::Code,
        },
        window_id: "code".into(),
    };
    assert!(
        fx.svc
            .claim(tokio_util::sync::CancellationToken::new(), claim)
            .await
            .is_err(),
        "code project aliases must not change the selected root"
    );
}

#[tokio::test]
async fn code_claim_read_and_move_exclude_nested_agent_roots_but_keep_in_project_aliases() {
    let fx = Fx::with(|config| {
        let old = config.roots.clone();
        config.roots = Arc::new(move || {
            let old = old.clone();
            Box::pin(async move {
                let mut roots = old().await;
                let project = roots
                    .iter()
                    .find(|root| root.id == "project:p")
                    .expect("fixture project")
                    .path
                    .clone();
                roots.push(Root {
                    id: "agents:p".into(),
                    path: project.join("owned-agents"),
                });
                roots
            })
        });
    });
    std::fs::create_dir(fx.project.join("owned-agents")).expect("agent fixture");
    std::fs::write(fx.project.join("owned-agents/private.ts"), b"keep").expect("agent file");
    std::fs::write(fx.project.join("a.ts"), b"owned").expect("project file");
    let key = DocKey {
        root: "project:p".into(),
        path: "owned-agents/private.ts".into(),
    };
    assert!(fx.svc.read_disk(&key, DocKind::Code).await.is_err());
    assert!(
        fx.svc
            .claim(
                tokio_util::sync::CancellationToken::new(),
                ClaimDocument {
                    key: key.clone(),
                    policy: DocPolicy {
                        kind: DocKind::Code
                    },
                    window_id: "code".into()
                }
            )
            .await
            .is_err()
    );
    let from = DocKey {
        root: key.root.clone(),
        path: "a.ts".into(),
    };
    let to = DocKey {
        root: key.root.clone(),
        path: "owned-agents/new.ts".into(),
    };
    assert!(fx.svc.move_entry(&from, &to).await.is_err());
    assert_eq!(
        std::fs::read(fx.project.join("a.ts")).expect("preserved"),
        b"owned"
    );
    std::os::unix::fs::symlink("a.ts", fx.project.join("alias.ts")).expect("local alias");
    let read = fx
        .svc
        .read_disk(
            &DocKey {
                root: key.root,
                path: "alias.ts".into(),
            },
            DocKind::Code,
        )
        .await
        .expect("in-project alias");
    assert_eq!(read.text.as_deref(), Some("owned"));
}
