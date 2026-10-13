mod support;
use devbox_protocol::Fail;
use devbox_terminal::*;
use support::Harness;
async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(5), future)
        .await
        .expect("test deadline")
}
fn create(h: &Harness) -> CreateSession {
    CreateSession {
        project_id: None,
        cwd: Some(h.home.path().display().to_string()),
        title: None,
        command: Some("bash --norc --noprofile".into()),
        attach_zellij: None,
    }
}
#[tokio::test]
async fn service_create_rename_list_close_and_missing_errors_preserve_revisions() {
    let h = Harness::new();
    let mut events = h.hub.subscribe("terminal.changed");
    let session = h
        .terminal
        .create(create(&h))
        .await
        .expect("create terminal");
    assert!(session.alive);
    assert!(matches!(
        bounded(events.recv()).await,
        Some(devbox_core::HubMessage::Event { rev: 1, .. })
    ));
    let list = h
        .terminal
        .list(ListSessions {
            project_id: None,
            after_id: None,
        })
        .await
        .expect("list terminal");
    assert_eq!(list.items.len(), 1);
    assert!(list.items[0].alive);
    h.terminal
        .rename(RenameSession {
            id: session.id.clone(),
            title: "새 제목".into(),
        })
        .await
        .expect("rename");
    assert!(matches!(
        bounded(events.recv()).await,
        Some(devbox_core::HubMessage::Event { rev: 2, .. })
    ));
    h.terminal
        .close(CloseSession {
            id: session.id.clone(),
        })
        .await
        .expect("close terminal");
    assert!(matches!(
        bounded(events.recv()).await,
        Some(devbox_core::HubMessage::Event { rev: 3, .. })
    ));
    assert!(matches!(
        h.terminal.close(CloseSession { id: session.id }).await,
        Err(Fail::Domain(TerminalError::NotFound { .. }))
    ));
    let list = h
        .terminal
        .list(ListSessions {
            project_id: None,
            after_id: None,
        })
        .await
        .expect("empty list");
    assert!(list.items.is_empty());
    assert_eq!(list.rev, "3");
}
#[tokio::test]
async fn invalid_cwd_and_project_do_not_create_sessions_or_change_revision() {
    let h = Harness::new();
    let mut params = create(&h);
    params.cwd = Some(h.home.path().join("missing").display().to_string());
    assert!(matches!(
        h.terminal.create(params).await,
        Err(Fail::Domain(TerminalError::CwdInvalid { .. }))
    ));
    let mut params = create(&h);
    params.project_id = Some("missing".into());
    assert!(matches!(
        h.terminal.create(params).await,
        Err(Fail::Domain(TerminalError::ProjectNotFound { .. }))
    ));
    assert_eq!(h.db.rev(), 0);
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("no sessions")
            .is_empty()
    );
}

#[tokio::test]
async fn project_cwd_profiles_dead_rows_and_untracked_tmux_are_kept_separate() {
    let mut h = Harness::new();
    let project_dir = h.home.path().join("project");
    std::fs::create_dir(&project_dir).expect("project directory");
    let project = h
        .projects
        .add(devbox_projects::AddProject {
            path: project_dir.display().to_string(),
        })
        .await
        .expect("register project");
    assert!(
        h.projects
            .get(&project.id)
            .await
            .expect("project lookup")
            .is_some()
    );
    assert!(
        h.projects
            .get("missing")
            .await
            .expect("missing project lookup")
            .is_none()
    );
    h.terminal = TerminalService::new(
        h.db.clone(),
        h.hub.clone(),
        h.paths.clone(),
        h.projects.clone(),
        devbox_core::config::TerminalConfig {
            profiles: vec![TerminalProfile {
                name: "Nu".into(),
                command: "nu".into(),
            }],
        },
    );
    assert_eq!(
        h.terminal
            .profiles()
            .items
            .iter()
            .map(|p| p.command.as_str())
            .collect::<Vec<_>>(),
        ["bash -l", "nu"]
    );
    let session = h
        .terminal
        .create(CreateSession {
            project_id: Some(project.id.clone()),
            cwd: None,
            title: None,
            command: Some("bash --norc".into()),
            attach_zellij: None,
        })
        .await
        .expect("project terminal");
    assert_eq!(session.cwd, project_dir.display().to_string());
    assert_eq!(session.title, "project");
    h.terminal
        .tmux()
        .new_session("untracked", h.home.path(), Some("bash --norc"), &[])
        .await
        .expect("untracked fixture session");
    assert!(matches!(
        h.terminal
            .close(CloseSession {
                id: "untracked".into()
            })
            .await,
        Err(Fail::Domain(TerminalError::NotFound { .. }))
    ));
    assert!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("sessions")
            .iter()
            .any(|s| s.name == "untracked")
    );
    h.terminal
        .tmux()
        .kill_session(&session.id)
        .await
        .expect("simulate shell exit");
    let list = h
        .terminal
        .list(ListSessions {
            project_id: Some(project.id),
            after_id: None,
        })
        .await
        .expect("dead metadata");
    assert_eq!(list.items.len(), 1);
    assert!(!list.items[0].alive);
    h.terminal
        .close(CloseSession { id: session.id })
        .await
        .expect("close already-dead terminal");
}
#[tokio::test]
async fn failed_metadata_insert_cleans_only_new_session_and_does_not_publish() {
    let h = Harness::new();
    h.terminal.tmux().start_direct().await.expect("server");
    h.terminal
        .tmux()
        .new_session("keeper", h.home.path(), Some("bash --norc"), &[])
        .await
        .expect("sibling");
    h.db.write(|tx| tx.execute_batch("CREATE TRIGGER reject_terminal BEFORE INSERT ON terminal_sessions BEGIN SELECT RAISE(FAIL, 'fixture insertion failure'); END")).await.expect("fixture failure trigger");
    let before = h.db.rev();
    let mut events = h.hub.subscribe("terminal.changed");
    assert!(matches!(
        h.terminal.create(create(&h)).await,
        Err(Fail::Internal(_))
    ));
    assert_eq!(h.db.rev(), before);
    assert_eq!(
        h.terminal
            .tmux()
            .list()
            .await
            .expect("remaining sessions")
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["keeper"]
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), events.recv())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn dropped_rename_waiter_still_publishes_the_committed_revision() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let h = Harness::new();
    let session = h.terminal.create(create(&h)).await.expect("terminal");
    let mut events = h.hub.subscribe("terminal.changed");
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let db = h.db.clone();
    let blocker = tokio::spawn(async move {
        db.write_checked::<(), (), _>(move |_| {
            ready_tx.send(()).expect("writer barrier");
            release_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("release barrier");
            Ok(Err(()))
        })
        .await
        .expect("writer result")
    });
    tokio::task::spawn_blocking(move || {
        ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("barrier ready")
    })
    .await
    .expect("barrier task");
    let mut rename = Box::pin(h.terminal.rename(RenameSession {
        id: session.id,
        title: "committed".into(),
    }));
    assert!(matches!(
        rename
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    drop(rename);
    release_tx.send(()).expect("release writer");
    assert_eq!(bounded(blocker).await.expect("writer task"), Err(()));
    assert!(matches!(
        bounded(events.recv()).await,
        Some(devbox_core::HubMessage::Event { rev: 2, .. })
    ));
    assert_eq!(
        h.terminal
            .list(ListSessions {
                project_id: None,
                after_id: None
            })
            .await
            .expect("committed list")
            .items[0]
            .title,
        "committed"
    );
}
