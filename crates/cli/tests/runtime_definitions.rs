mod support;
use devbox_cli::client::{Client, ClientError};
use devbox_runtime::*;
use serde_json::json;
use std::{path::Path, time::Duration};
use support::TestDaemon;

fn task(id: &str, command: &str) -> RunDef {
    RunDef::Task(TaskDef {
        id: id.into(),
        command: command.into(),
        ..Default::default()
    })
}

async fn list(client: &Client, id: &str) -> DefinitionList {
    client
        .call::<RuntimeList>(&ListRuns {
            project_id: id.into(),
            cursor: None,
        })
        .await
        .expect("list execution definitions")
}

#[tokio::test]
async fn runtime_definitions_round_trip_shadow_delete_and_restart() {
    let mut daemon = TestDaemon::start();
    let client = daemon.client().await;
    let repo = daemon.git_repo("projects/runtime");
    let project = daemon.add_project(&client, &repo).await;
    let mut events = client
        .subscribe("runtime.changed")
        .await
        .expect("subscribe");
    for (save_to, command) in [
        (SaveTo::ThisPc, "echo local"),
        (SaveTo::Project, "echo file"),
    ] {
        let result = client
            .call::<RuntimeDefine>(&DefineRun {
                project_id: project.clone(),
                def: task("build", command),
                save_to,
            })
            .await
            .expect("save definition");
        let event = tokio::time::timeout(Duration::from_secs(5), events.next_event())
            .await
            .expect("change deadline")
            .expect("change event");
        assert_eq!(event.rev, result.rev);
    }
    let result = list(&client, &project).await;
    assert_eq!(result.defs[0].def.command(), Some("echo file"));
    assert_eq!(result.shadowed[0].def.command(), Some("echo local"));
    assert!(matches!(
        result.trust,
        devbox_projects::TrustPreviewStatus::Approved
    ));
    daemon.restart();
    let client = daemon.client().await;
    assert_eq!(list(&client, &project).await.shadowed.len(), 1);
    client
        .call::<RuntimeUndefine>(&UndefineRun {
            project_id: project.clone(),
            id: "build".into(),
        })
        .await
        .expect("remove project definition");
    assert_eq!(
        list(&client, &project).await.defs[0].def.command(),
        Some("echo local")
    );
    client
        .call::<RuntimeUndefine>(&UndefineRun {
            project_id: project.clone(),
            id: "build".into(),
        })
        .await
        .expect("remove local definition");
    assert!(list(&client, &project).await.defs.is_empty());
}

#[tokio::test]
async fn run_trust_preview_needs_no_agent_and_lists_approved_snapshot_inputs() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let repo = daemon.git_repo("projects/runtime-trust");
    let project = daemon.add_project(&client, &repo).await;
    std::fs::create_dir(Path::new(&repo).join(".devbox")).expect("config directory");
    std::fs::write(Path::new(&repo).join(".devbox/devbox.toml"), "[[task]]\nid='build'\ncommand='echo repository'\ncwd='sub'\nenv={MODE='test'}\ntarget='linux'\nsecrets=['TOKEN']\n").expect("repository definition");
    let preview = client
        .call::<devbox_projects::ProjectsTrustPreview>(&devbox_projects::TrustPreviewParams {
            id: project.clone(),
            operation: devbox_projects::TrustOperation::Run,
            agent_id: None,
            worktrees: vec![],
            worktree: None,
        })
        .await
        .expect("run preview without agent");
    assert!(preview.requires_approval);
    assert_eq!(preview.scope_version, 2);
    let json = serde_json::to_string(&preview).expect("typed preview");
    assert!(json.contains("echo repository") && json.contains("TOKEN") && json.contains("MODE"));
    client
        .call::<RuntimeDefine>(&DefineRun {
            project_id: project.clone(),
            def: task("local", "true"),
            save_to: SaveTo::ThisPc,
        })
        .await
        .expect("local definition needs no approval");
    assert!(matches!(
        list(&client, &project).await.trust,
        devbox_projects::TrustPreviewStatus::ApprovalRequired
    ));
}

#[tokio::test]
async fn external_edit_emits_runtime_changed_and_stales_trust() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let repo = daemon.git_repo("projects/runtime-watch");
    let project = daemon.add_project(&client, &repo).await;
    client
        .call::<RuntimeDefine>(&DefineRun {
            project_id: project.clone(),
            def: task("build", "echo approved"),
            save_to: SaveTo::Project,
        })
        .await
        .expect("first definition");
    // Wait for the notify service to install the newly added project's watches.
    tokio::time::sleep(Duration::from_millis(450)).await;
    let mut events = client
        .subscribe("runtime.changed")
        .await
        .expect("subscribe external edits");
    let file = Path::new(&repo).join(".devbox/devbox.toml");
    std::fs::write(
        &file,
        "[[task]]\nid='build'\ncommand='echo externally-edited'\n",
    )
    .expect("external edit");
    tokio::time::timeout(Duration::from_secs(5), events.next_event())
        .await
        .expect("external changed deadline")
        .expect("external event");
    let result = list(&client, &project).await;
    assert_eq!(result.defs[0].def.command(), Some("echo externally-edited"));
    assert!(matches!(
        result.trust,
        devbox_projects::TrustPreviewStatus::ApprovalRequired
    ));
}

#[tokio::test]
async fn invalid_definitions_fail_without_files_and_rpc_retries_keep_the_same_id() {
    let daemon = TestDaemon::start();
    let client = daemon.client().await;
    let repo = daemon.git_repo("projects/runtime-errors");
    let project = daemon.add_project(&client, &repo).await;
    let error = client
        .call::<RuntimeDefine>(&DefineRun {
            project_id: project.clone(),
            def: task("bad", ""),
            save_to: SaveTo::Project,
        })
        .await
        .expect_err("empty command");
    assert!(matches!(error,ClientError::Rpc {code,..} if code=="runtime.def_invalid"));
    assert!(!Path::new(&repo).join(".devbox").exists());
    let params = json!({"projectId":project,"def":{"kind":"task","id":"stable","command":"echo stable"},"saveTo":"thisPc"});
    let first = client
        .raw_call(
            "runtime.define",
            params.clone(),
            Some("runtime-same-request".into()),
        )
        .await
        .expect("first define");
    let retry = client
        .raw_call(
            "runtime.define",
            params,
            Some("runtime-same-request".into()),
        )
        .await
        .expect("retry define");
    assert_eq!(first, retry);
    assert_eq!(list(&client, &project).await.defs.len(), 1);
}

/// Restores a folder's mode on drop, so a failing test never leaves it unreadable.
struct ModeGuard(std::path::PathBuf);

impl ModeGuard {
    fn restrict(path: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o111))
            .expect("restrict folder");
        Self(path.into())
    }
    fn restore(&self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

impl Drop for ModeGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

async fn changed_for(events: &mut devbox_cli::client::Subscription, project: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let event = tokio::time::timeout_at(deadline, events.next_event())
            .await
            .expect("runtime.changed deadline")
            .expect("runtime.changed event");
        if event.payload.contains(project) {
            return;
        }
    }
}

#[tokio::test]
async fn daemon_starts_when_a_project_config_directory_cannot_be_watched() {
    if nix::unistd::geteuid().is_root() {
        eprintln!("skipped: root ignores mode bits");
        return;
    }
    let mut daemon = TestDaemon::start();
    let client = daemon.client().await;
    let locked = daemon.home.path().join("locked");
    let a = daemon.git_repo("locked/a");
    let b = daemon.git_repo("projects/b");
    let config = Path::new(&a).join(".devbox/devbox.toml");
    std::fs::create_dir_all(config.parent().expect("config folder")).expect("config folder");
    std::fs::write(&config, "[[task]]\nid='first'\ncommand='echo first'\n").expect("config");
    // B starts with an empty .devbox so its watch exists at startup.
    let b_config = Path::new(&b).join(".devbox/devbox.toml");
    std::fs::create_dir_all(b_config.parent().expect("config folder")).expect("config folder");
    let project_a = daemon.add_project(&client, &a).await;
    let project_b = daemon.add_project(&client, &b).await;
    drop(client);
    daemon.stop();
    // Execute-only: A's config stays readable, but inotify cannot watch these folders.
    let guards = [
        ModeGuard::restrict(&locked),
        ModeGuard::restrict(config.parent().expect("config folder")),
    ];
    if std::fs::read_dir(&locked).is_ok() {
        eprintln!("skipped: mode bits are not enforced in this environment");
        return;
    }
    daemon.launch();
    let client = daemon.client().await;
    client
        .call::<devbox_protocol::system::Ping>(&devbox_protocol::system::PingParams {})
        .await
        .expect("ping after degraded start");
    assert!(list(&client, &project_b).await.defs.is_empty());
    assert_eq!(list(&client, &project_a).await.defs[0].def.id(), "first");
    let mut events = client
        .subscribe("runtime.changed")
        .await
        .expect("subscribe");
    // One unwatchable project must not disable the watches of the others.
    std::fs::write(&b_config, "[[task]]\nid='b'\ncommand='echo b'\n").expect("edit B");
    changed_for(&mut events, &project_b).await;
    for guard in &guards {
        guard.restore();
    }
    let other = daemon.git_repo("projects/c");
    let project_c = daemon.add_project(&client, &other).await;
    // C is always published after the refresh that re-attaches A's failed watches.
    changed_for(&mut events, &project_c).await;
    std::fs::write(&config, "[[task]]\nid='second'\ncommand='echo second'\n").expect("edit A");
    changed_for(&mut events, &project_a).await;
    assert_eq!(list(&client, &project_a).await.defs[0].def.id(), "second");
}
