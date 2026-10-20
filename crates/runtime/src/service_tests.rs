use crate::*;
use devbox_core::{Db, Hub};
use devbox_projects::{AddProject, ProjectsService, TrustPreviewStatus};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    root: tempfile::TempDir,
    project: String,
    db: Db,
    hub: Arc<Hub>,
    projects: Arc<ProjectsService>,
    runs: Arc<RuntimeService>,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().expect("owned fixture");
        let repo = root.path().join("repo");
        std::fs::create_dir(&repo).expect("owned project");
        let db = Db::open(
            &root.path().join("index.db"),
            &root.path().join("backups"),
            "1.0.0",
            &[devbox_projects::MIGRATIONS, MIGRATIONS].concat(),
        )
        .expect("owned db");
        let hub = Arc::new(Hub::new());
        let projects = Arc::new(ProjectsService::new(
            db.clone(),
            hub.clone(),
            root.path().into(),
            vec![],
        ));
        let project = projects
            .add(AddProject {
                path: repo.to_string_lossy().into_owned(),
            })
            .await
            .expect("register")
            .id;
        let runs = Arc::new(RuntimeService::new(
            db.clone(),
            hub.clone(),
            projects.clone(),
        ));
        Self {
            root,
            project,
            db,
            hub,
            projects,
            runs,
        }
    }
    fn config_path(&self) -> PathBuf {
        self.root.path().join("repo/.devbox/devbox.toml")
    }
    fn write(&self, text: &str) {
        let path = self.config_path();
        std::fs::create_dir_all(path.parent().expect("parent")).expect("owned dir");
        std::fs::write(path, text).expect("owned config");
    }
    async fn define(&self, id: &str, command: &str, save_to: SaveTo) -> DefineResult {
        self.runs
            .define(DefineRun {
                project_id: self.project.clone(),
                def: task(id, command),
                save_to,
            })
            .await
            .expect("define")
    }
    async fn list(&self) -> DefinitionList {
        self.runs
            .list(ListRuns {
                project_id: self.project.clone(),
                cursor: None,
            })
            .await
            .expect("list")
    }
}
fn task(id: &str, command: &str) -> RunDef {
    RunDef::Task(TaskDef {
        id: id.into(),
        command: command.into(),
        ..Default::default()
    })
}

#[tokio::test]
async fn project_definitions_are_not_duplicated_in_runtime_db() {
    let f = Fixture::new().await;
    f.define("file", "echo file", SaveTo::Project).await;
    f.define("local", "echo local", SaveTo::ThisPc).await;
    let rows: Vec<String> =
        f.db.read(|tx| {
            tx.prepare("SELECT id FROM runtime_definitions ORDER BY id")?
                .query_map([], |row| row.get(0))?
                .collect()
        })
        .await
        .expect("rows");
    assert_eq!(rows, ["local"]);
    let list = f.list().await;
    assert_eq!(list.defs.len(), 2);
    assert!(matches!(list.trust, TrustPreviewStatus::Approved));
}

#[tokio::test]
async fn local_storage_never_approves_repository_commands_and_survives_restart() {
    let f = Fixture::new().await;
    f.write("[[task]]\nid='file'\ncommand='echo external'\n");
    f.define("local", "echo local", SaveTo::ThisPc).await;
    assert!(matches!(
        f.list().await.trust,
        TrustPreviewStatus::ApprovalRequired
    ));
    let count: i64 =
        f.db.read(|tx| {
            tx.query_row("SELECT count(*) FROM projects_command_trust", [], |r| {
                r.get(0)
            })
        })
        .await
        .expect("approvals");
    assert_eq!(count, 0);
    let restarted = RuntimeService::new(f.db.clone(), f.hub.clone(), f.projects.clone());
    let list = restarted
        .list(ListRuns {
            project_id: f.project.clone(),
            cursor: None,
        })
        .await
        .expect("restart list");
    assert_eq!(list.defs.len(), 2);
}

#[tokio::test]
async fn undefine_removes_effective_source_then_reveals_shadowed_local_definition() {
    let f = Fixture::new().await;
    f.define("a", "echo local", SaveTo::ThisPc).await;
    f.define("a", "echo file", SaveTo::Project).await;
    assert_eq!(f.list().await.shadowed.len(), 1);
    f.runs
        .undefine(UndefineRun {
            project_id: f.project.clone(),
            id: "a".into(),
        })
        .await
        .expect("delete project definition");
    assert_eq!(f.list().await.defs[0].def.command(), Some("echo local"));
    f.runs
        .undefine(UndefineRun {
            project_id: f.project.clone(),
            id: "a".into(),
        })
        .await
        .expect("delete local definition");
    assert!(f.list().await.defs.is_empty());
    f.runs
        .undefine(UndefineRun {
            project_id: f.project.clone(),
            id: "a".into(),
        })
        .await
        .expect("idempotent missing deletion");
}

#[tokio::test]
async fn invalid_definition_has_no_database_or_file_side_effects() {
    let f = Fixture::new().await;
    let rev = f.db.rev();
    for save_to in [SaveTo::Project, SaveTo::ThisPc] {
        assert!(
            f.runs
                .define(DefineRun {
                    project_id: f.project.clone(),
                    def: task("bad", ""),
                    save_to
                })
                .await
                .is_err()
        );
    }
    assert_eq!(f.db.rev(), rev);
    assert!(!f.config_path().exists());
}

#[tokio::test]
async fn watch_reloads_on_external_edit() {
    let f = Fixture::new().await;
    f.define("a", "echo initial", SaveTo::Project).await;
    f.runs.initialize().await.expect("watcher starts");
    let mut events = f.hub.subscribe("runtime.changed");
    f.write("[[task]]\nid='a'\ncommand='echo changed'\n");
    tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
        .await
        .expect("changed event");
    let list = f.list().await;
    assert_eq!(list.defs[0].def.command(), Some("echo changed"));
    assert!(matches!(list.trust, TrustPreviewStatus::ApprovalRequired));
    let rev = f.db.rev();
    f.write("[[task]]\nid='a'\ncommand='echo changed'\n");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(650), events.recv())
            .await
            .is_err()
    );
    assert_eq!(f.db.rev(), rev);
    f.runs.shutdown().await;
}

#[tokio::test]
async fn watch_observes_new_config_and_atomic_replacement_without_recursive_project_watch() {
    let f = Fixture::new().await;
    f.runs.initialize().await.expect("watcher");
    let mut events = f.hub.subscribe("runtime.changed");
    f.write("[[task]]\nid='a'\ncommand='echo created'\n");
    tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
        .await
        .expect("new config");
    let replacement = f.config_path().with_extension("tmp");
    std::fs::write(&replacement, "[[task]]\nid='a'\ncommand='echo replaced'\n")
        .expect("replacement");
    std::fs::rename(replacement, f.config_path()).expect("atomic edit");
    tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
        .await
        .expect("replaced config");
    assert_eq!(f.list().await.defs[0].def.command(), Some("echo replaced"));
    f.runs.shutdown().await;
}

#[tokio::test]
async fn lists_paginate_by_rows_and_json_bytes_without_omitting_shadowed_rows() {
    for payload in [String::new(), "\"".repeat(4096)] {
        let f = Fixture::new().await;
        let project = f.project.clone();
        f.db.write(move |tx| {
            for n in 0..300 {
                let mut value = TaskDef {
                    id: format!("task-{n:03}"),
                    command: "true".into(),
                    ..Default::default()
                };
                value.env.insert("DATA".into(), payload.clone());
                store::define(tx, &project, &RunDef::Task(value))?;
            }
            Ok(())
        })
        .await
        .expect("owned large local fixture");
        f.define("task-100", "echo project override", SaveTo::Project)
            .await;
        let mut seen = std::collections::BTreeSet::new();
        let mut cursor = None;
        let mut pages = 0;
        loop {
            let page = f
                .runs
                .list(ListRuns {
                    project_id: f.project.clone(),
                    cursor,
                })
                .await
                .expect("bounded page");
            assert!(serde_json::to_vec(&page).expect("wire page").len() <= service::PAGE_BYTES);
            assert!(page.defs.len() + page.shadowed.len() <= 256);
            for (shadowed, defs) in [(false, page.defs), (true, page.shadowed)] {
                for definition in defs {
                    assert!(
                        seen.insert((definition.def.id().to_owned(), shadowed)),
                        "duplicate page row"
                    );
                }
            }
            pages += 1;
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
            assert!(pages < 10);
        }
        assert!(pages >= 2);
        assert_eq!(seen.len(), 301);
        assert!(seen.contains(&("task-100".into(), true)));
    }
}

#[tokio::test]
async fn concurrent_project_defines_are_serialized_and_same_id_is_updated_once() {
    let f = Fixture::new().await;
    let define = |id: &str, command: &str| DefineRun {
        project_id: f.project.clone(),
        def: task(id, command),
        save_to: SaveTo::Project,
    };
    let (a, b) = tokio::join!(
        f.runs.define(define("a", "true")),
        f.runs.define(define("b", "true"))
    );
    a.expect("first serialized definition");
    b.expect("second serialized definition");
    f.define("a", "echo updated", SaveTo::Project).await;
    let list = f.list().await;
    assert_eq!(list.defs.len(), 2);
    assert_eq!(list.defs[0].def.command(), Some("echo updated"));
}

#[tokio::test]
async fn approval_of_unchanged_bytes_refreshes_runtime_trust_without_touch_events() {
    let f = Fixture::new().await;
    f.write("[[task]]\nid='a'\ncommand='true'\n");
    f.runs.initialize().await.expect("watcher");
    let mut events = f.hub.subscribe("runtime.changed");
    let bytes = std::fs::read(f.config_path()).expect("initial bytes");
    let preview = f
        .projects
        .trust_preview(
            devbox_projects::TrustPreviewParams {
                id: f.project.clone(),
                operation: devbox_projects::TrustOperation::Run,
                agent_id: None,
                worktrees: vec![],
                worktree: None,
            },
            true,
            vec![],
        )
        .await
        .expect("preview");
    f.projects
        .trust_approve(devbox_projects::TrustApprove {
            id: f.project.clone(),
            expected_canonical_path: preview.canonical_path,
            expected_digest: preview.digest.expect("digest"),
            expected_scope_version: preview.scope_version,
        })
        .await
        .expect("approve unchanged config");
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .is_ok(),
        "approval is a meaningful runtime list change even with the same file digest"
    );
    assert!(matches!(f.list().await.trust, TrustPreviewStatus::Approved));
    let rev = f.db.rev();
    std::fs::write(f.config_path(), &bytes).expect("touch same bytes");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(650), events.recv())
            .await
            .is_err()
    );
    assert_eq!(f.db.rev(), rev);
    f.runs.shutdown().await;
}

#[tokio::test]
async fn watch_reattaches_when_the_configuration_directory_is_replaced() {
    let f = Fixture::new().await;
    f.write("[[task]]\nid='a'\ncommand='echo initial'\n");
    f.runs.initialize().await.expect("watcher");
    let mut events = f.hub.subscribe("runtime.changed");
    let directory = f
        .config_path()
        .parent()
        .expect("config directory")
        .to_owned();
    std::fs::rename(&directory, directory.with_file_name(".devbox-old"))
        .expect("owned old directory");
    f.write("[[task]]\nid='a'\ncommand='echo replaced-directory'\n");
    tokio::time::timeout(std::time::Duration::from_secs(3), events.recv())
        .await
        .expect("directory replacement observed");
    // Let notifications from the fixture's own DB commit drain before the next edit.
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    f.write("[[task]]\nid='a'\ncommand='echo subsequent-edit'\n");
    let subsequent = tokio::time::timeout(std::time::Duration::from_secs(3), events.recv()).await;
    f.runs.shutdown().await;
    assert!(
        subsequent.is_ok(),
        "replacement directory must keep receiving config events"
    );
}

#[tokio::test]
async fn new_project_first_config_created_before_watch_registration_emits_changed() {
    let f = Fixture::new().await;
    let mut events = f.hub.subscribe("runtime.changed");
    let baseline_rev = f.db.rev();
    f.runs.initialize().await.expect("watcher");
    assert_eq!(f.db.rev(), baseline_rev, "startup only records a baseline");
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(25), events.recv())
            .await
            .is_err()
    );
    let repo = f.root.path().join("new-project");
    std::fs::create_dir(&repo).expect("new owned project");
    let project = f
        .projects
        .add(AddProject {
            path: repo.to_string_lossy().into_owned(),
        })
        .await
        .expect("register new project");
    let initial = f
        .runs
        .list(ListRuns {
            project_id: project.id.clone(),
            cursor: None,
        })
        .await
        .expect("early empty list");
    assert!(initial.defs.is_empty());
    std::fs::create_dir(repo.join(".devbox")).expect("first config directory");
    std::fs::write(
        repo.join(".devbox/devbox.toml"),
        "[[task]]\nid='first'\ncommand='true'\n",
    )
    .expect("first config before watcher attachment");
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv()).await;
    f.runs.shutdown().await;
    assert!(
        event.is_ok(),
        "newly registered project must announce its first discovered config"
    );
    let Some(devbox_core::HubMessage::Event { rev, payload }) = event.expect("event deadline")
    else {
        panic!("expected definition event");
    };
    assert!(rev > initial.rev.parse::<u64>().expect("initial revision"));
    let changed: RuntimeChange = serde_json::from_str(&payload).expect("typed changed event");
    assert_eq!(changed.project_id, project.id);
    let list = f
        .runs
        .list(ListRuns {
            project_id: project.id,
            cursor: None,
        })
        .await
        .expect("updated list");
    assert_eq!(list.defs[0].def.id(), "first");
}

#[tokio::test]
async fn watch_does_not_duplicate_the_project_define_commit_event() {
    let f = Fixture::new().await;
    f.runs.initialize().await.expect("watcher");
    let mut events = f.hub.subscribe("runtime.changed");
    let result = f.define("a", "echo saved", SaveTo::Project).await;
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
        .await
        .expect("saved event");
    let Some(devbox_core::HubMessage::Event { rev, .. }) = event else {
        panic!("expected saved event")
    };
    assert_eq!(rev.to_string(), result.rev);
    let duplicate =
        tokio::time::timeout(std::time::Duration::from_millis(650), events.recv()).await;
    f.runs.shutdown().await;
    assert!(
        duplicate.is_err(),
        "own definition commit is published once"
    );
    assert_eq!(
        f.db.rev().to_string(),
        result.rev,
        "watching adds no duplicate revision"
    );
}
