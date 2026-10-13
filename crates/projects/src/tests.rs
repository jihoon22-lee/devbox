use super::*;
use devbox_core::{Db, Hub};
use std::sync::Arc;

async fn service(dir: &std::path::Path) -> (ProjectsService, Arc<Hub>) {
    let db = Db::open(
        &dir.join("devbox.db"),
        &dir.join("b"),
        "1.0.0-dev",
        MIGRATIONS,
    )
    .expect("open fixture database");
    let hub = Arc::new(Hub::new());
    (
        ProjectsService::new(
            db,
            hub.clone(),
            dir.to_path_buf(),
            vec!["~/projects".into()],
        ),
        hub,
    )
}

#[tokio::test]
async fn add_list_remove_and_publish() {
    let t = tempfile::tempdir().expect("create fixture directory");
    let proj = t.path().join("projects/alpha");
    std::fs::create_dir_all(proj.join(".git")).expect("create fixture directory");
    let (s, hub) = service(t.path()).await;
    let mut events = hub.subscribe("projects.changed");

    let p = s
        .add(AddProject {
            path: proj.display().to_string(),
        })
        .await
        .expect("register fixture project");
    assert_eq!(p.name, "alpha");
    assert!(matches!(
        events.recv().await,
        Some(devbox_core::HubMessage::Event { .. })
    ));

    let list = s
        .list(ListProjects {})
        .await
        .expect("list fixture projects");
    assert_eq!(list.items.len(), 1);
    assert!(list.rev.parse::<u64>().expect("parse fixture value") >= 1);

    let dup = s
        .add(AddProject {
            path: proj.display().to_string(),
        })
        .await
        .expect_err("fixture operation must fail");
    assert!(matches!(
        dup,
        Fail::Domain(ProjectsError::AlreadyExists { .. })
    ));

    s.remove(RemoveProject { id: p.id.clone() })
        .await
        .expect("remove fixture registration");
    let gone = s
        .remove(RemoveProject { id: p.id })
        .await
        .expect_err("fixture operation must fail");
    assert!(matches!(gone, Fail::Domain(ProjectsError::NotFound { .. })));
}

#[tokio::test]
async fn rejects_relative_missing_and_file_paths() {
    let t = tempfile::tempdir().expect("create fixture directory");
    let file = t.path().join("f.txt");
    std::fs::write(&file, "x").expect("write fixture file");
    let (s, _) = service(t.path()).await;
    for (path, want) in [
        ("rel/path", "projects.path_invalid"),
        (
            t.path()
                .join("missing")
                .to_str()
                .expect("fixture path is UTF-8"),
            "projects.path_invalid",
        ),
        (
            file.to_str().expect("fixture path is UTF-8"),
            "projects.not_directory",
        ),
    ] {
        let e = s
            .add(AddProject { path: path.into() })
            .await
            .expect_err("fixture operation must fail");
        let Fail::Domain(e) = e else {
            panic!("domain error expected")
        };
        assert_eq!(devbox_protocol::to_wire(&e).code, want);
    }
}

#[tokio::test]
async fn scan_lists_root_children_and_marks_registered_and_git() {
    let t = tempfile::tempdir().expect("create fixture directory");
    let root = t.path().join("projects");
    std::fs::create_dir_all(root.join("a/.git")).expect("create fixture directory");
    std::fs::create_dir_all(root.join("b")).expect("create fixture directory");
    std::fs::create_dir_all(root.join("a-agents/x")).expect("create fixture directory");
    std::fs::create_dir_all(root.join(".hidden")).expect("create fixture directory");
    let (s, _) = service(t.path()).await;
    s.add(AddProject {
        path: root.join("a").display().to_string(),
    })
    .await
    .expect("register fixture project");
    let r = s
        .scan(ScanProjects {})
        .await
        .expect("scan fixture projects");
    let names: Vec<_> = r
        .candidates
        .iter()
        .map(|c| (c.name.as_str(), c.is_git, c.registered))
        .collect();
    assert_eq!(names, vec![("a", true, true), ("b", false, false)]);
}
#[tokio::test]
async fn concurrent_duplicate_and_missing_remove_do_not_commit_or_publish() {
    let t = tempfile::tempdir().expect("create fixture directory");
    let project = t.path().join("project");
    std::fs::create_dir(&project).expect("create fixture directory");
    let (s, hub) = service(t.path()).await;
    let mut events = hub.subscribe("projects.changed");
    let path = project.to_str().expect("fixture path is UTF-8").to_owned();
    let (a, b) = tokio::join!(
        s.add(AddProject { path: path.clone() }),
        s.add(AddProject { path })
    );
    let (ok, rejected) = if a.is_ok() { (a, b) } else { (b, a) };
    let project = ok.expect("one fixture registration succeeds");
    assert!(matches!(
        rejected,
        Err(Fail::Domain(ProjectsError::AlreadyExists { .. }))
    ));
    assert_eq!(
        s.list(ListProjects {})
            .await
            .expect("list fixture projects")
            .rev,
        "1"
    );
    assert!(matches!(
        events.recv().await,
        Some(devbox_core::HubMessage::Event { rev: 1, .. })
    ));
    assert!(matches!(
        s.remove(RemoveProject {
            id: "missing".into()
        })
        .await,
        Err(Fail::Domain(ProjectsError::NotFound { .. }))
    ));
    assert_eq!(
        s.list(ListProjects {})
            .await
            .expect("list fixture projects")
            .rev,
        "1"
    );
    s.remove(RemoveProject { id: project.id })
        .await
        .expect("remove fixture registration");
    drop(s);
    drop(hub);
    assert!(matches!(
        events.recv().await,
        Some(devbox_core::HubMessage::Event { rev: 2, .. })
    ));
    assert_eq!(events.recv().await, None);
}

#[cfg(unix)]
#[tokio::test]
async fn canonical_scan_deduplicates_roots_and_aliases() {
    let t = tempfile::tempdir().expect("create fixture directory");
    let root = t.path().join("projects");
    std::fs::create_dir_all(root.join("a/.git")).expect("create fixture directory");
    std::fs::create_dir_all(root.join("b")).expect("create fixture directory");
    std::os::unix::fs::symlink(root.join("a"), root.join("alias")).expect("create fixture symlink");
    std::os::unix::fs::symlink(&root, t.path().join("root-alias")).expect("create fixture symlink");
    let (mut s, _) = service(t.path()).await;
    s.roots.extend([
        root.to_str().expect("fixture path is UTF-8").into(),
        "~/root-alias".into(),
    ]);
    s.add(AddProject {
        path: root
            .join("alias")
            .to_str()
            .expect("fixture path is UTF-8")
            .into(),
    })
    .await
    .expect("fixture path is UTF-8");
    let r = s
        .scan(ScanProjects {})
        .await
        .expect("scan fixture projects");
    assert_eq!(
        r.candidates
            .iter()
            .map(|c| (c.name.as_str(), c.is_git, c.registered))
            .collect::<Vec<_>>(),
        vec![("a", true, true), ("b", false, false)]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_non_utf8_canonical_target() {
    use std::os::unix::ffi::OsStringExt;
    let t = tempfile::tempdir().expect("create fixture directory");
    let root = t.path().join("projects");
    std::fs::create_dir(&root).expect("create fixture directory");
    let target = root.join(std::ffi::OsString::from_vec(vec![b'a', 0xff]));
    std::fs::create_dir(&target).expect("create fixture directory");
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&target, &alias).expect("create fixture symlink");
    let (s, _) = service(t.path()).await;
    assert!(matches!(
        s.add(AddProject {
            path: alias.to_str().expect("fixture path is UTF-8").into()
        })
        .await,
        Err(Fail::Domain(ProjectsError::PathInvalid { .. }))
    ));
    assert!(
        s.scan(ScanProjects {})
            .await
            .expect("scan fixture projects")
            .candidates
            .is_empty()
    );
}
#[tokio::test]
async fn dropped_remove_waiter_still_publishes_committed_change() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let t = tempfile::tempdir().expect("create fixture directory");
    let project = t.path().join("project");
    std::fs::create_dir(&project).expect("create fixture directory");
    let (s, hub) = service(t.path()).await;
    let project = s
        .add(AddProject {
            path: project.to_str().expect("fixture path is UTF-8").into(),
        })
        .await
        .expect("fixture path is UTF-8");
    let mut events = hub.subscribe("projects.changed");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let db = s.db.clone();
    let blocker = tokio::spawn(async move {
        db.write_checked::<(), (), _>(move |_| {
            ready_tx.send(()).expect("send fixture message");
            release_rx.recv().expect("receive fixture message");
            Ok(Err(()))
        })
        .await
        .expect("complete fixture transaction barrier")
    });
    tokio::task::spawn_blocking(move || {
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("receive fixture signal before deadline")
    })
    .await
    .expect("receive fixture signal before deadline");
    let mut remove = Box::pin(s.remove(RemoveProject { id: project.id }));
    assert!(matches!(
        remove
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    // Dropping the handler after enqueue cannot retract the writer's job.
    drop(remove);
    release_tx.send(()).expect("send fixture message");
    assert_eq!(
        blocker.await.expect("complete fixture transaction barrier"),
        Err(())
    );
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .expect("receive fixture message"),
        Some(devbox_core::HubMessage::Event { rev: 2, .. })
    ));
    let list = s
        .list(ListProjects {})
        .await
        .expect("list fixture projects");
    assert_eq!(list.rev, "2");
    assert!(list.items.is_empty());
}
#[tokio::test]
async fn config_recovers_for_ui_and_favorites_publish_stably() {
    let t = tempfile::tempdir().expect("fixture");
    let (s, hub) = service(t.path()).await;
    let mut registered = Vec::new();
    for name in ["alpha", "zeta"] {
        let path = t.path().join(name);
        std::fs::create_dir(&path).expect("fixture");
        registered.push(
            s.add(AddProject {
                path: path.display().to_string(),
            })
            .await
            .expect("add"),
        );
    }
    let alpha = &registered[0];
    let zeta = &registered[1];
    assert!(
        s.config(GetProjectConfig {
            id: alpha.id.clone()
        })
        .await
        .expect("config")
        .error
        .is_none()
    );
    std::fs::create_dir(std::path::Path::new(&alpha.path).join(".devbox")).expect("fixture");
    std::fs::write(
        std::path::Path::new(&alpha.path).join(".devbox/devbox.toml"),
        "[agnet]\nsetup=[]",
    )
    .expect("fixture");
    let config = s
        .config(GetProjectConfig {
            id: alpha.id.clone(),
        })
        .await
        .expect("config");
    assert!(config.error.expect("diagnostic").message.contains("agnet"));
    assert!(config.config.launch.terminals.is_empty());
    assert!(matches!(
        s.config(GetProjectConfig {
            id: "missing".into()
        })
        .await,
        Err(Fail::Domain(ProjectsError::NotFound { .. }))
    ));
    let mut events = hub.subscribe("projects.changed");
    let done = s
        .set_favorite(SetFavorite {
            id: zeta.id.clone(),
            favorite: true,
        })
        .await
        .expect("favorite");
    assert_eq!(done.rev, "3");
    assert!(matches!(
        events.recv().await,
        Some(devbox_core::HubMessage::Event { rev: 3, .. })
    ));
    let list = s.list(ListProjects {}).await.expect("list");
    assert_eq!(
        list.items
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["zeta", "alpha"]
    );
    assert!(list.items[0].favorite);
    assert!(matches!(
        s.set_favorite(SetFavorite {
            id: "missing".into(),
            favorite: true
        })
        .await,
        Err(Fail::Domain(ProjectsError::NotFound { .. }))
    ));
    assert_eq!(s.list(ListProjects {}).await.expect("list").rev, "3");
    s.set_favorite(SetFavorite {
        id: zeta.id.clone(),
        favorite: false,
    })
    .await
    .expect("favorite");
    assert_eq!(
        s.list(ListProjects {}).await.expect("list").items[0].name,
        "alpha"
    );
}

#[tokio::test]
async fn clone_registers_success_and_preserves_preexisting_and_failed_targets() {
    let t = tempfile::tempdir().expect("fixture");
    let source = t.path().join("source.git");
    std::fs::create_dir(&source).expect("fixture");
    let output = std::process::Command::new("git")
        .env("HOME", t.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", t.path().join("absent-gitconfig"))
        .args(["init", "--bare", "--template="])
        .arg(&source)
        .output()
        .expect("fixture git");
    assert!(output.status.success());
    let (s, _) = service(t.path()).await;
    let project = s
        .clone_repository(CloneProject {
            url: source.display().to_string(),
            dir: None,
        })
        .await
        .expect("clone");
    assert_eq!(project.name, "source");
    assert_eq!(
        std::path::Path::new(&project.path),
        t.path().join("projects/source")
    );
    assert!(std::path::Path::new(&project.path).join(".git").is_dir());
    assert!(matches!(
        s.clone_repository(CloneProject {
            url: source.display().to_string(),
            dir: None
        })
        .await,
        Err(Fail::Domain(ProjectsError::CloneTargetExists { .. }))
    ));
    let preexisting = t.path().join("existing");
    std::fs::create_dir(&preexisting).expect("fixture");
    std::fs::write(preexisting.join("precious"), "preserve").expect("fixture");
    assert!(
        s.clone_repository(CloneProject {
            url: source.display().to_string(),
            dir: Some(preexisting.display().to_string())
        })
        .await
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(preexisting.join("precious")).expect("read"),
        "preserve"
    );
    let failed = t.path().join("failed");
    assert!(matches!(
        s.clone_repository(CloneProject {
            url: t.path().join("absent.git").display().to_string(),
            dir: Some(failed.display().to_string())
        })
        .await,
        Err(Fail::Domain(ProjectsError::CloneFailed { .. }))
    ));
    assert!(failed.is_dir());
    assert_eq!(s.list(ListProjects {}).await.expect("list").items.len(), 1);
    let rejected = t.path().join("rejected");
    assert!(
        s.clone_repository(CloneProject {
            url: "ext::sh -c touch marker".into(),
            dir: Some(rejected.display().to_string())
        })
        .await
        .is_err()
    );
    assert!(!rejected.exists());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn clone_registration_rechecks_inode_after_waiting_for_database_writer() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let root = tempfile::tempdir().unwrap();
    let (s, hub) = service(root.path()).await;
    let mut events = hub.subscribe("projects.changed");
    let target = root.path().join("clone");
    let reservation = crate::clone_target::CloneTarget::reserve(&target).unwrap();
    std::fs::write(target.join("owned"), "preserve owned files").unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let db = s.db.clone();
    let blocker = tokio::spawn(async move {
        db.write_checked::<(), (), _>(move |_| {
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(Err(()))
        })
        .await
        .unwrap()
    });
    tokio::task::spawn_blocking(move || {
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
    })
    .await
    .unwrap();
    let mut register = Box::pin(s.register_clone(reservation));
    assert!(matches!(
        register
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    let retained = root.path().join("retained");
    std::fs::rename(&target, &retained).unwrap();
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("foreign"), "preserve foreign files").unwrap();
    release_tx.send(()).unwrap();
    assert_eq!(blocker.await.unwrap(), Err(()));
    assert!(matches!(
        register.await,
        Err(Fail::Domain(ProjectsError::CloneFailed { .. }))
    ));
    let list = s.list(ListProjects {}).await.unwrap();
    assert!(list.items.is_empty());
    assert_eq!(list.rev, "0");
    assert_eq!(
        std::fs::read_to_string(retained.join("owned")).unwrap(),
        "preserve owned files"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("foreign")).unwrap(),
        "preserve foreign files"
    );
    drop(s);
    drop(hub);
    assert!(events.recv().await.is_none());
}
