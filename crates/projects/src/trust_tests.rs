use super::*;
use std::path::Path;

fn service(root: &Path) -> ProjectsService {
    let db = Db::open(
        &root.join("trust.db"),
        &root.join("backups"),
        "1.0.0-dev",
        MIGRATIONS,
    )
    .expect("owned database");
    ProjectsService::new(db, Arc::new(Hub::new()), root.into(), vec![])
}
fn config(root: &Path, text: &str) {
    std::fs::create_dir_all(root.join("project/.devbox")).expect("owned project");
    std::fs::write(root.join("project/.devbox/devbox.toml"), text).expect("owned config");
}
async fn preview(s: &ProjectsService, id: &str) -> TrustPreview {
    s.trust_preview(
        TrustPreviewParams {
            id: id.into(),
            operation: TrustOperation::Create,
            agent_id: None,
            worktrees: vec![],
            worktree: Some(true),
        },
        true,
        vec![],
    )
    .await
    .expect("preview")
}
async fn approve(s: &ProjectsService, p: &TrustPreview) {
    s.trust_approve(TrustApprove {
        id: p.project_id.clone(),
        expected_canonical_path: p.canonical_path.clone(),
        expected_digest: p.digest.clone().expect("snapshot digest"),
        expected_scope_version: p.scope_version,
    })
    .await
    .expect("explicit approval");
}

#[tokio::test]
async fn trust_is_absent_for_existing_registration_and_persists_across_restart() {
    let t = tempfile::tempdir().expect("owned fixture");
    let old = Db::open(
        &t.path().join("trust.db"),
        &t.path().join("backups"),
        "1.0.0-dev",
        &MIGRATIONS[..1],
    )
    .expect("old database");
    config(
        t.path(),
        "[agent]\nsetup=['echo exact']\ntest='echo test'\nteardown=['echo cleanup']\n[launch]\nterminals=[{name='shell',command='echo launch'}]\nagents=[{tool='claude',prompt='hello',worktree=true}]\n",
    );
    let old_service = ProjectsService::new(old, Arc::new(Hub::new()), t.path().into(), vec![]);
    let p = old_service
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("old registration");
    drop(old_service);
    let s = service(t.path());
    assert!(matches!(
        s.authorized_config(&p.id).await,
        Err(Fail::Domain(ProjectsError::TrustRequired { .. }))
    ));
    let view = preview(&s, &p.id).await;
    assert!(view.requires_approval);
    assert_eq!(
        view.config.as_ref().expect("snapshot config").agent.setup,
        ["echo exact"]
    );
    assert_eq!(
        view.config.as_ref().expect("snapshot config").launch.agents[0]
            .prompt
            .as_deref(),
        Some("hello")
    );
    approve(&s, &view).await;
    drop(s);
    let restarted = service(t.path());
    let frozen = restarted
        .authorized_config(&p.id)
        .await
        .expect("persistent approval");
    assert_eq!(frozen.config().agent.setup, ["echo exact"]);
    config(t.path(), "# bytes changed\n[agent]\nsetup=['echo exact']\n");
    assert!(matches!(
        restarted.authorized_config(&p.id).await,
        Err(Fail::Domain(ProjectsError::TrustRequired { .. }))
    ));
    assert_eq!(
        frozen.config().agent.setup,
        ["echo exact"],
        "immutable snapshot survives file edits"
    );
    assert!(matches!(
        restarted
            .trust_approve(TrustApprove {
                id: p.id,
                expected_canonical_path: view.canonical_path,
                expected_digest: view.digest.expect("snapshot digest"),
                expected_scope_version: view.scope_version
            })
            .await,
        Err(Fail::Domain(ProjectsError::TrustPreviewStale { .. }))
    ));
}

#[tokio::test]
async fn approval_requires_exact_path_digest_and_scope() {
    let t = tempfile::tempdir().expect("owned fixture");
    config(t.path(), "[agent]\nsetup=['true']\n");
    let s = service(t.path());
    let p = s
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("registration");
    let view = preview(&s, &p.id).await;
    for (path, digest) in [
        (
            "/different".into(),
            view.digest.clone().expect("snapshot digest"),
        ),
        (view.canonical_path.clone(), "00".into()),
    ] {
        assert!(matches!(
            s.trust_approve(TrustApprove {
                id: p.id.clone(),
                expected_canonical_path: path,
                expected_digest: digest,
                expected_scope_version: view.scope_version
            })
            .await,
            Err(Fail::Domain(ProjectsError::TrustPreviewStale { .. }))
        ));
    }
    approve(&s, &view).await;
    s.db.write(|tx| {
        tx.execute("UPDATE projects_command_trust SET scope_version=3", [])?;
        Ok(())
    })
    .await
    .expect("future scope fixture");
    assert!(s.authorized_config(&p.id).await.is_ok());
}

#[tokio::test]
async fn command_free_operations_never_issue_a_whole_config_capability() {
    let t = tempfile::tempdir().expect("owned fixture");
    config(t.path(), "[agent]\ntest='unapproved test'\n");
    let s = service(t.path());
    let p = s
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("registration");
    assert!(
        s.setup_snapshot(&p.id)
            .await
            .expect("no setup")
            .setup()
            .is_empty()
    );
    assert!(
        s.teardown_snapshot(&p.id)
            .await
            .expect("no teardown")
            .is_none()
    );
    assert!(s.authorized_config(&p.id).await.is_err());
    assert!(s.test_snapshot(&p.id).await.is_err());
    config(t.path(), "[[task]]\nid='future'\ncommand='deferred'\n");
    assert!(
        s.authorized_config(&p.id).await.is_err(),
        "S2 definitions require scope two"
    );
    std::fs::remove_file(t.path().join("project/.devbox/devbox.toml")).expect("owned file");
    s.authorized_config(&p.id)
        .await
        .expect("missing configuration");
}

#[tokio::test]
async fn trust_read_is_bounded_and_rejects_symlinked_configuration() {
    let t = tempfile::tempdir().expect("owned fixture");
    config(
        t.path(),
        &format!("#{}", "x".repeat(config::CONFIG_MAX_BYTES)),
    );
    let s = service(t.path());
    let p = s
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("registration");
    assert!(matches!(
        s.authorized_config(&p.id).await,
        Err(Fail::Domain(ProjectsError::ConfigurationInvalid { .. }))
    ));
    #[cfg(unix)]
    {
        std::fs::remove_file(t.path().join("project/.devbox/devbox.toml")).expect("owned file");
        std::fs::write(t.path().join("outside"), "[agent]\nsetup=['outside']").expect("owned file");
        std::os::unix::fs::symlink(
            t.path().join("outside"),
            t.path().join("project/.devbox/devbox.toml"),
        )
        .expect("owned symlink");
        assert!(s.authorized_config(&p.id).await.is_err());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn changed_canonical_target_invalidates_the_same_byte_approval() {
    let t = tempfile::tempdir().expect("owned fixture");
    config(t.path(), "[agent]\nsetup=['true']\n");
    let s = service(t.path());
    let p = s
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("registration");
    let old = preview(&s, &p.id).await;
    approve(&s, &old).await;
    let moved = t.path().join("relocated");
    std::fs::rename(t.path().join("project"), &moved).expect("owned directory rename");
    std::os::unix::fs::symlink(&moved, t.path().join("project")).expect("owned project alias");
    let current = preview(&s, &p.id).await;
    assert_eq!(old.digest, current.digest);
    assert_ne!(old.canonical_path, current.canonical_path);
    assert!(current.requires_approval);
    assert!(matches!(
        s.authorized_config(&p.id).await,
        Err(Fail::Domain(ProjectsError::TrustRequired { .. }))
    ));
    assert!(matches!(
        s.trust_approve(TrustApprove {
            id: p.id,
            expected_canonical_path: old.canonical_path,
            expected_digest: old.digest.expect("snapshot digest"),
            expected_scope_version: old.scope_version
        })
        .await,
        Err(Fail::Domain(ProjectsError::TrustPreviewStale { .. }))
    ));
}

#[tokio::test]
async fn run_definitions_require_scope_two_approval() {
    let t = tempfile::tempdir().expect("owned fixture");
    config(t.path(), "[[task]]\nid='build'\ncommand='echo build'\n");
    let s = service(t.path());
    let p = s
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("register");
    let view = preview(&s, &p.id).await;
    approve(&s, &view).await;
    s.db.write(|tx| {
        tx.execute("UPDATE projects_command_trust SET scope_version=1", [])?;
        Ok(())
    })
    .await
    .expect("old approval");
    assert!(matches!(
        s.authorized_config(&p.id).await,
        Err(Fail::Domain(ProjectsError::TrustRequired { .. }))
    ));
}

#[tokio::test]
async fn scope_one_approval_stays_valid_without_run_definitions() {
    let t = tempfile::tempdir().expect("owned fixture");
    config(t.path(), "[agent]\nsetup=['echo setup']\n");
    let s = service(t.path());
    let p = s
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("register");
    let view = preview(&s, &p.id).await;
    approve(&s, &view).await;
    s.db.write(|tx| {
        tx.execute("UPDATE projects_command_trust SET scope_version=1", [])?;
        Ok(())
    })
    .await
    .expect("old approval");
    s.authorized_config(&p.id)
        .await
        .expect("scope one remains valid");
}

#[tokio::test]
async fn run_preview_and_launch_snapshot_share_typed_approved_definitions() {
    let t = tempfile::tempdir().expect("owned fixture");
    config(
        t.path(),
        "[[service]]\nid='web'\ncommand='serve'\ncwd='frontend'\nenv={PORT='3000'}\nsecrets=['TOKEN']\n[launch]\nservices=['web']\n",
    );
    let s = service(t.path());
    let p = s
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("register");
    assert!(matches!(
        s.launch_snapshot(&p.id).await,
        Err(Fail::Domain(ProjectsError::TrustRequired { .. }))
    ));
    let v = s
        .trust_preview(
            TrustPreviewParams {
                id: p.id.clone(),
                operation: TrustOperation::Run,
                agent_id: None,
                worktrees: vec![],
                worktree: None,
            },
            true,
            vec![],
        )
        .await
        .expect("preview");
    assert!(v.requires_approval);
    let crate::run_config::RunDef::Service(service) = &v.run_definitions[0] else {
        panic!("service preview")
    };
    assert_eq!(service.env["PORT"], "3000");
    assert_eq!(service.secrets, ["TOKEN"]);
    approve(&s, &v).await;
    let frozen = s.launch_snapshot(&p.id).await.expect("approved launch");
    config(t.path(), "[[service]]\nid='web'\ncommand='external'\n");
    assert_eq!(frozen.run_config().services[0].command, "serve");
    assert!(
        s.launch_snapshot(&p.id)
            .await
            .expect("command-free launch")
            .run_config()
            .is_empty()
    );
}

#[tokio::test]
async fn trust_approval_publishes_the_committed_project_revision() {
    use std::future::Future;
    let t = tempfile::tempdir().expect("owned fixture");
    config(t.path(), "[[task]]\nid='build'\ncommand='echo build'\n");
    let s = service(t.path());
    let p = s
        .add(AddProject {
            path: t.path().join("project").display().to_string(),
        })
        .await
        .expect("register");
    let view = preview(&s, &p.id).await;
    let mut events = s.hub.subscribe("projects.changed");
    let done = s
        .trust_approve(TrustApprove {
            id: p.id,
            expected_canonical_path: view.canonical_path,
            expected_digest: view.digest.expect("digest"),
            expected_scope_version: view.scope_version,
        })
        .await
        .expect("approve");
    // The commit callback must publish before the mutation response, without a timer.
    let next = std::future::poll_fn(|cx| {
        let mut future = std::pin::pin!(events.recv());
        std::task::Poll::Ready(future.as_mut().poll(cx))
    })
    .await;
    assert!(
        matches!(next,std::task::Poll::Ready(Some(devbox_core::HubMessage::Event{rev,..})) if rev.to_string()==done.rev)
    );
}
