use super::*;
use crate::*;
use std::sync::Arc;

async fn fixture(text: &str) -> (tempfile::TempDir, ProjectsService, Project) {
    let root = tempfile::tempdir().expect("owned fixture");
    let project = root.path().join("project");
    std::fs::create_dir_all(project.join(".devbox")).expect("owned directory");
    std::fs::write(project.join(".devbox/devbox.toml"), text).expect("owned configuration");
    let db = Db::open(
        &root.path().join("db"),
        &root.path().join("backups"),
        "1.0.0-dev",
        MIGRATIONS,
    )
    .expect("owned DB");
    let service = ProjectsService::new(db, Arc::new(Hub::new()), root.path().into(), vec![]);
    let p = service
        .add(AddProject {
            path: project.display().to_string(),
        })
        .await
        .expect("register");
    (root, service, p)
}
async fn define(s: &ProjectsService, id: &str, name: &str) -> R<UpdatedRunConfig> {
    let state = s.run_config_status(id).await?;
    s.update_run_config(
        id,
        RunConfigEdit {
            expected_digest: state.digest,
            change: RunConfigChange::Define(RunDef::Task(TaskDef {
                id: name.into(),
                command: format!("echo {name}"),
                ..Default::default()
            })),
        },
    )
    .await
}
async fn approve(s: &ProjectsService, id: &str) {
    let v = s
        .trust_preview(
            TrustPreviewParams {
                id: id.into(),
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
    s.trust_approve(TrustApprove {
        id: id.into(),
        expected_canonical_path: v.canonical_path,
        expected_digest: v.digest.expect("digest"),
        expected_scope_version: v.scope_version,
    })
    .await
    .expect("approve");
}
#[tokio::test]
async fn define_into_project_file_preserves_comments() {
    let (_root,s,p)=fixture("# top\n[agent] # agent\ncopy = ['.env'] # retain\n\n[[task]] # existing\nid='other'\ncommand='echo other' # command\n").await;
    let updated = define(&s, &p.id, "build").await.expect("define");
    assert_eq!(updated.config.tasks.len(), 2);
    let text =
        std::fs::read_to_string(Path::new(&p.path).join(".devbox/devbox.toml")).expect("read");
    for comment in [
        "# top",
        "[agent] # agent",
        "copy = ['.env'] # retain",
        "[[task]] # existing",
        "command='echo other' # command",
    ] {
        assert!(text.contains(comment), "{text}");
    }
    assert!(matches!(
        s.run_config_status(&p.id).await.expect("state").trust,
        TrustPreviewStatus::ApprovalRequired
    ));
}
#[tokio::test]
async fn first_define_into_file_without_run_definitions_needs_no_prompt() {
    let (_root, s, p) = fixture("# initial\n[agent]\ncopy=[]\n").await;
    define(&s, &p.id, "build").await.expect("define");
    let snapshot = s.run_snapshot(&p.id).await.expect("auto approval");
    assert_eq!(snapshot.config().raw_tasks[0].id, "build");
    let scope =
        s.db.read(|tx| {
            tx.query_row(
                "SELECT scope_version FROM projects_command_trust",
                [],
                |r| r.get::<_, u32>(0),
            )
        })
        .await
        .expect("scope");
    assert_eq!(scope, 2);
}
#[tokio::test]
async fn define_into_approved_file_refreshes_the_approval() {
    let (_root, s, p) = fixture("[[task]]\nid='old'\ncommand='echo old'\n").await;
    approve(&s, &p.id).await;
    let frozen = s.run_snapshot(&p.id).await.expect("snapshot");
    let updated = define(&s, &p.id, "new").await.expect("define");
    assert_eq!(updated.config.tasks.len(), 2);
    assert!(s.run_snapshot(&p.id).await.is_ok());
    assert_eq!(frozen.config().raw_tasks.len(), 1);
}
#[tokio::test]
async fn define_after_external_edit_keeps_the_approval_stale() {
    let (_root, s, p) = fixture("[[task]]\nid='old'\ncommand='echo old'\n").await;
    approve(&s, &p.id).await;
    std::fs::write(
        Path::new(&p.path).join(".devbox/devbox.toml"),
        "# external\n[[task]]\nid='old'\ncommand='echo external'\n",
    )
    .expect("external edit");
    define(&s, &p.id, "new").await.expect("save allowed");
    assert!(matches!(
        s.run_snapshot(&p.id).await,
        Err(Fail::Domain(ProjectsError::TrustRequired { .. }))
    ));
}
#[tokio::test]
async fn project_definition_write_is_serialized_and_rejects_changed_bytes() {
    let (_root, s, p) = fixture("").await;
    let digest = s.run_config_status(&p.id).await.expect("state").digest;
    let edit = |id: &str| RunConfigEdit {
        expected_digest: digest.clone(),
        change: RunConfigChange::Define(RunDef::Task(TaskDef {
            id: id.into(),
            command: "echo safe".into(),
            ..Default::default()
        })),
    };
    let (a, b) = tokio::join!(
        s.update_run_config(&p.id, edit("a")),
        s.update_run_config(&p.id, edit("b"))
    );
    assert_eq!(u8::from(a.is_ok()) + u8::from(b.is_ok()), 1);
    let failure = if a.is_err() { a } else { b };
    assert!(matches!(
        failure,
        Err(Fail::Domain(ProjectsError::TrustPreviewStale { .. }))
    ));
    assert_eq!(
        s.run_config_status(&p.id)
            .await
            .expect("state")
            .config
            .tasks
            .len(),
        1
    );
    let path = Path::new(&p.path);
    let original = std::fs::read(path.join(".devbox/devbox.toml")).expect("old");
    std::fs::write(path.join(".devbox/devbox.toml"), "# external").expect("edit");
    assert!(matches!(
        checked_write(path, &original, b"# proposed", &p.id),
        Err(Fail::Domain(ProjectsError::TrustPreviewStale { .. }))
    ));
    assert_eq!(
        std::fs::read_to_string(path.join(".devbox/devbox.toml")).expect("read"),
        "# external"
    );
}
#[tokio::test]
async fn approval_db_failure_does_not_report_save_success() {
    let (_root, s, p) = fixture("").await;
    s.db.write(|tx|{tx.execute_batch("CREATE TRIGGER fail_approval BEFORE INSERT ON projects_command_trust BEGIN SELECT RAISE(ABORT, 'injected approval failure'); END;")?;Ok(())}).await.expect("failure injection");
    let before = s.list(ListProjects {}).await.expect("revision").rev;
    assert!(define(&s, &p.id, "build").await.is_err());
    assert_eq!(s.list(ListProjects {}).await.expect("revision").rev, before);
    assert!(matches!(
        s.run_snapshot(&p.id).await,
        Err(Fail::Domain(ProjectsError::TrustRequired { .. }))
    ));
    assert_eq!(
        s.run_config_status(&p.id)
            .await
            .expect("saved file")
            .config
            .tasks
            .len(),
        1
    );
}
#[tokio::test]
async fn project_file_write_failure_creates_no_approval() {
    let (_root, s, p) = fixture("").await;
    let digest = s.run_config_status(&p.id).await.expect("state").digest;
    let before = s.list(ListProjects {}).await.expect("revision").rev;
    s.fail_run_write
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(
        s.update_run_config(
            &p.id,
            RunConfigEdit {
                expected_digest: digest,
                change: RunConfigChange::Define(RunDef::Task(TaskDef {
                    id: "build".into(),
                    command: "echo safe".into(),
                    ..Default::default()
                }))
            }
        )
        .await
        .is_err()
    );
    let count =
        s.db.read(|tx| {
            tx.query_row("SELECT COUNT(*) FROM projects_command_trust", [], |row| {
                row.get::<_, u32>(0)
            })
        })
        .await
        .expect("approval count");
    assert_eq!(count, 0);
    assert_eq!(s.list(ListProjects {}).await.expect("revision").rev, before);
    assert!(
        std::fs::read(Path::new(&p.path).join(".devbox/devbox.toml"))
            .expect("unchanged file")
            .is_empty()
    );
}
#[test]
fn typed_run_fields_accept_toml_snake_case_and_wire_camel_case() {
    let config=config::parse("[[task]]\nid='build'\ncommand='echo build'\ndepends_on=[]\n[[service]]\nid='web'\ncommand='serve'\nrestart='on-failure'\nrestart_sec=3\nports_hint=[8080]\nready_timeout_sec=7\nstart_limit={burst=2,interval_sec=10}\n[[schedule]]\nid='daily'\ntask='build'\ncron='0 1 * * *'\nmissed='run-once'\n").expect("typed TOML");
    let defs = config.run_config().definitions();
    let value = serde_json::to_value(&defs[1]).expect("wire");
    assert_eq!(value["restartSec"], 3);
    assert_eq!(value["startLimit"]["intervalSec"], 10);
    assert_eq!(value["kind"], "service");
    assert_eq!(
        serde_json::from_value::<RunDef>(value).expect("wire roundtrip"),
        defs[1]
    );
    assert!(config::parse("[[service]]\nid='missing-command'").is_err());
}

#[test]
fn redefine_and_undefine_preserve_other_tables_and_field_comments() {
    for env in [
        "env={ KEY='old' } # inline env\n",
        "[task.env] # env section\nKEY='old' # environment value\n",
    ] {
        let initial = format!(
            "# top\n[[task]] # task\nid='build' # identity\ncommand='echo old' # command\ndepends_on=[] # dependencies\n{env}\n[[task]] # untouched\nid='other'\ncommand='echo other'\n[agent] # preserved\ncopy=[]\n"
        );
        let def = RunDef::Task(TaskDef {
            id: "build".into(),
            command: "echo new".into(),
            env: [("KEY".into(), "new".into())].into(),
            ..Default::default()
        });
        let updated = edited(initial.as_bytes(), &RunConfigChange::Define(def)).expect("edit");
        let text = std::str::from_utf8(&updated).expect("utf8");
        for comment in [
            "# task",
            "# identity",
            "# command",
            "# dependencies",
            "# untouched",
            "[agent] # preserved",
        ] {
            assert!(text.contains(comment), "{text}");
        }
        if env.starts_with('[') {
            assert!(
                text.contains("# env section") && text.contains("# environment value"),
                "{text}"
            );
        } else {
            assert!(text.contains("# inline env"));
        }
        let config = config::parse(text).expect("valid edited TOML");
        assert_eq!(config.raw_tasks[0].command, "echo new");
        assert_eq!(config.raw_tasks[0].env["KEY"], "new");
        assert_eq!(config.raw_tasks[1].command, "echo other");
        assert!(
            text.find("id=").expect("id order") < text.find("command=").expect("command order")
        );
        let removed =
            edited(&updated, &RunConfigChange::Undefine { id: "build".into() }).expect("remove");
        let text = std::str::from_utf8(&removed).expect("utf8");
        assert!(
            text.contains("# untouched") && text.contains("[agent] # preserved"),
            "{text}"
        );
        assert_eq!(
            config::parse(text).expect("parse removed").raw_tasks.len(),
            1
        );
    }
}
#[test]
fn inline_definition_arrays_can_be_edited_and_removed() {
    let original = b"task=[{id='build', command='old'}] # array\n[agent]\ncopy=[]\n";
    let new = RunDef::Task(TaskDef {
        id: "build".into(),
        command: "new".into(),
        ..Default::default()
    });
    let bytes = edited(original, &RunConfigChange::Define(new)).expect("define");
    assert_eq!(
        config::parse(std::str::from_utf8(&bytes).expect("utf8"))
            .expect("config")
            .raw_tasks[0]
            .command,
        "new"
    );
    let bytes =
        edited(&bytes, &RunConfigChange::Undefine { id: "build".into() }).expect("undefine");
    assert!(
        config::parse(std::str::from_utf8(&bytes).expect("utf8"))
            .expect("config")
            .raw_tasks
            .is_empty()
    );
}

#[tokio::test]
async fn updated_run_config_reports_trust_for_exact_written_bytes() {
    let (_root, s, p) = fixture("").await;
    let first = define(&s, &p.id, "a").await.expect("initial definition");
    assert!(matches!(first.trust, TrustPreviewStatus::Approved));
    let path = Path::new(&p.path).join(".devbox/devbox.toml");
    let old = std::fs::read_to_string(&path).expect("read file");
    std::fs::write(&path, format!("# external\n{old}")).expect("external edit");
    let second = define(&s, &p.id, "b").await.expect("save stale file");
    assert!(matches!(second.trust, TrustPreviewStatus::ApprovalRequired));
    let third = s
        .update_run_config(
            &p.id,
            RunConfigEdit {
                expected_digest: second.digest,
                change: RunConfigChange::Undefine { id: "a".into() },
            },
        )
        .await
        .expect("remove first definition");
    assert!(matches!(third.trust, TrustPreviewStatus::ApprovalRequired));
    let last = s
        .update_run_config(
            &p.id,
            RunConfigEdit {
                expected_digest: third.digest,
                change: RunConfigChange::Undefine { id: "b".into() },
            },
        )
        .await
        .expect("remove last definition");
    assert!(matches!(last.trust, TrustPreviewStatus::NotRequired));
    assert!(last.config.is_empty());
}

#[tokio::test]
async fn restoring_exact_approved_bytes_reports_existing_approval_without_refreshing_it() {
    let (_root, s, p) = fixture("").await;
    let first = define(&s, &p.id, "build")
        .await
        .expect("create approved file");
    let path = Path::new(&p.path).join(".devbox/devbox.toml");
    let approved_bytes = std::fs::read(&path).expect("approved bytes");
    let approval_row = || {
        s.db.read(|tx| {
            tx.query_row(
                "SELECT digest, scope_version, approved_ms FROM projects_command_trust",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
        })
    };
    let original_approval = approval_row().await.expect("original approval");
    std::fs::write(
        &path,
        String::from_utf8(approved_bytes.clone())
            .expect("UTF-8")
            .replace("echo build", "echo external"),
    )
    .expect("external edit");
    assert!(matches!(
        s.run_config_status(&p.id).await.expect("stale state").trust,
        TrustPreviewStatus::ApprovalRequired
    ));
    let restored = define(&s, &p.id, "build")
        .await
        .expect("restore approved bytes");
    assert_eq!(
        std::fs::read(&path).expect("restored bytes"),
        approved_bytes
    );
    assert_eq!(restored.digest, first.digest);
    assert_eq!(
        approval_row().await.expect("preserved approval"),
        original_approval
    );
    assert!(matches!(
        s.run_config_status(&p.id)
            .await
            .expect("actual state")
            .trust,
        TrustPreviewStatus::Approved
    ));
    assert!(
        matches!(restored.trust, TrustPreviewStatus::Approved),
        "helper must report the existing exact-byte approval"
    );
}

#[test]
fn service_start_limit_aliases_preserve_nested_comments_and_key_order() {
    for (start_limit, order) in [
        (
            "[service.start_limit] # keep table\nburst=2\ninterval_sec=10 # keep interval\n",
            ["burst=", "interval_sec="],
        ),
        (
            "start_limit={interval_sec=10,burst=2} # keep inline\n",
            ["interval_sec=", "burst="],
        ),
    ] {
        let initial = format!("[[service]]\nid='web'\ncommand='serve'\n{start_limit}");
        let mut definition = config::parse(&initial)
            .expect("original config")
            .raw_services
            .remove(0);
        definition.start_limit.interval_sec = 20;
        let bytes = edited(
            initial.as_bytes(),
            &RunConfigChange::Define(RunDef::Service(definition)),
        )
        .expect("redefine service");
        let text = std::str::from_utf8(&bytes).expect("UTF-8");
        if start_limit.starts_with('[') {
            assert!(text.contains("# keep table"), "{text}");
            assert!(text.contains("# keep interval"), "{text}");
        } else {
            assert!(text.contains("# keep inline"), "{text}");
        }
        assert!(text.contains("interval_sec=20"), "{text}");
        assert!(
            text.find(order[0]).expect("first key") < text.find(order[1]).expect("second key"),
            "{text}"
        );
        assert_eq!(
            config::parse(text).expect("updated config").raw_services[0]
                .start_limit
                .interval_sec,
            20
        );
    }
}

#[test]
fn run_environment_keys_keep_exact_case_and_underscore_identity() {
    for environment in [
        "env={FOO_BAR='upper',fooBar='camel'} # env\n",
        "[service.env] # env\nFOO_BAR='upper'\nfooBar='camel' # keep camel\n",
    ] {
        let initial = format!("[[service]]\nid='web'\ncommand='serve'\n{environment}");
        let mut definition = config::parse(&initial)
            .expect("original config")
            .raw_services
            .remove(0);
        definition.env.remove("FOO_BAR");
        definition.env.insert("fooBar".into(), "updated".into());
        let bytes = edited(
            initial.as_bytes(),
            &RunConfigChange::Define(RunDef::Service(definition)),
        )
        .expect("redefine service");
        let text = std::str::from_utf8(&bytes).expect("UTF-8");
        assert!(
            text.contains("fooBar=") && !text.contains("FOO_BAR"),
            "{text}"
        );
        assert!(text.contains("# env"), "{text}");
        if environment.starts_with('[') {
            assert!(text.contains("# keep camel"), "{text}");
        }
        assert_eq!(
            config::parse(text).expect("updated config").raw_services[0].env,
            [("fooBar".into(), "updated".into())].into()
        );
    }
}
