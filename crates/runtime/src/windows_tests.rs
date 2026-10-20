use crate::windows::{quote_powershell, sync, windows_to_wsl};
use std::{fs, path::Path, process::Command};
fn git(path: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .expect("fixture result")
            .success()
    );
}
#[tokio::test]
async fn windows_sync_copies_changed_tracked_files_and_keeps_extra_target_files() {
    let temp = tempfile::tempdir().expect("fixture result");
    let source = temp.path().join("source's repo");
    let target = temp.path().join("target's repo");
    fs::create_dir_all(&source).expect("fixture result");
    fs::create_dir_all(target.join("node_modules")).expect("fixture result");
    git(&source, &["init", "-q"]);
    git(&target, &["init", "-q"]);
    fs::write(source.join("tracked.txt"), "before").expect("fixture result");
    fs::write(source.join("deleted.txt"), "keep deleted source").expect("fixture result");
    fs::write(source.join(".gitignore"), "ignored\n").expect("fixture result");
    git(&source, &["add", "."]);
    fs::write(source.join("new file's.txt"), "added").expect("fixture result");
    fs::write(source.join("ignored"), "do not copy").expect("fixture result");
    fs::write(target.join("node_modules/keep"), "keep").expect("fixture result");
    let first = sync(&source, &target).await.expect("fixture result");
    assert_eq!(first.copied, 4);
    assert_eq!(first.deleted, 0);
    let again = sync(&source, &target).await.expect("fixture result");
    assert_eq!((again.copied, again.skipped), (0, 4));
    fs::write(source.join("tracked.txt"), "changed").expect("fixture result");
    fs::remove_file(source.join("deleted.txt")).expect("fixture result");
    let last = sync(&source, &target).await.expect("fixture result");
    assert_eq!(last.copied, 1);
    assert_eq!(last.deleted, 0);
    assert_eq!(
        fs::read_to_string(target.join("tracked.txt")).expect("fixture result"),
        "changed"
    );
    assert_eq!(
        fs::metadata(source.join("tracked.txt"))
            .expect("fixture result")
            .modified()
            .expect("fixture result"),
        fs::metadata(target.join("tracked.txt"))
            .expect("fixture result")
            .modified()
            .expect("fixture result")
    );
    assert!(target.join("deleted.txt").exists());
    assert!(target.join("node_modules/keep").exists());
    assert!(target.join(".git").is_dir());
    assert!(!target.join("ignored").exists());
    fs::write(source.join("tracked.txt"), "altered").expect("fixture result");
    let modified = fs::metadata(source.join("tracked.txt"))
        .expect("fixture result")
        .modified()
        .expect("fixture result")
        + std::time::Duration::from_secs(2);
    fs::File::options()
        .write(true)
        .open(source.join("tracked.txt"))
        .expect("fixture result")
        .set_times(fs::FileTimes::new().set_modified(modified))
        .expect("fixture result");
    assert_eq!(
        sync(&source, &target).await.expect("fixture result").copied,
        1,
        "same-size mtime change is copied"
    );
    assert_eq!(
        fs::read_to_string(target.join("tracked.txt")).expect("fixture result"),
        "altered"
    );
}
#[test]
fn windows_command_is_quoted_for_powershell() {
    assert_eq!(quote_powershell("a'b"), "'a''b'");
    assert_eq!(
        windows_to_wsl("C:\\dev\\a'b repo")
            .expect("fixture result")
            .to_str()
            .expect("fixture value"),
        "/mnt/c/dev/a'b repo"
    );
    for invalid in [
        "C:relative",
        "\\\\server\\share",
        "C:\\dev\\..\\outside",
        "C:\\dev\n",
    ] {
        assert!(windows_to_wsl(invalid).is_err(), "{invalid:?}");
    }
}
#[tokio::test]
async fn windows_sync_rejects_destination_and_source_symlinks_without_touching_external_files() {
    use std::os::unix::fs::symlink;
    for kind in ["leaf", "parent", "source"] {
        let temp = tempfile::tempdir().expect("fixture result");
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let outside = temp.path().join("outside");
        for path in [&source, &target, &outside] {
            fs::create_dir_all(path).expect("fixture result");
        }
        git(&source, &["init", "-q"]);
        fs::create_dir(source.join("dir")).expect("fixture result");
        fs::write(source.join("dir/tracked.txt"), "new repository bytes").expect("fixture result");
        git(&source, &["add", "."]);
        fs::write(outside.join("tracked.txt"), "owned sentinel").expect("fixture result");
        match kind {
            "leaf" => {
                fs::create_dir(target.join("dir")).expect("fixture result");
                symlink(outside.join("tracked.txt"), target.join("dir/tracked.txt"))
                    .expect("fixture result");
            }
            "parent" => symlink(&outside, target.join("dir")).expect("fixture result"),
            "source" => {
                fs::remove_file(source.join("dir/tracked.txt")).expect("fixture result");
                fs::remove_dir(source.join("dir")).expect("fixture result");
                symlink(&outside, source.join("dir")).expect("fixture result");
            }
            _ => unreachable!(),
        }
        assert!(sync(&source, &target).await.is_err(), "{kind}");
        assert_eq!(
            fs::read_to_string(outside.join("tracked.txt")).expect("fixture result"),
            "owned sentinel"
        );
    }
}
#[tokio::test]
async fn windows_sync_rejects_names_and_case_collisions_before_copying() {
    for names in [
        vec!["bad:name"],
        vec!["trailing."],
        vec!["trailing "],
        vec!["CON.txt"],
        vec!["A.txt", "a.txt"],
        vec!["A/first", "a/second"],
    ] {
        let temp = tempfile::tempdir().expect("fixture result");
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).expect("fixture result");
        git(&source, &["init", "-q"]);
        for name in names {
            let path = source.join(name);
            fs::create_dir_all(path.parent().expect("fixture value")).expect("fixture result");
            fs::write(path, "owned").expect("fixture result");
        }
        assert!(sync(&source, &target).await.is_err());
        assert!(!target.exists());
    }
}
#[test]
fn windows_native_dispatch_claim_is_attempt_scoped_and_known_exit_is_full_width() {
    let temp = tempfile::tempdir().expect("fixture result");
    let store = crate::exit_record::ExitStore::new(temp.path(), "owned").expect("fixture result");
    store.begin("first").expect("fixture result");
    store.claim_windows_native("first").expect("fixture result");
    assert!(store.claim_windows_native("first").is_err());
    store
        .record_windows_exit("first", 0x1234)
        .expect("fixture result");
    assert_eq!(
        store.windows_exit("first").expect("fixture result"),
        Some(0x1234)
    );
    store.begin("second").expect("fixture result");
    assert!(store.claim_windows_native("first").is_err());
    store
        .claim_windows_native("second")
        .expect("fixture result");
    assert_eq!(store.windows_exit("second").expect("fixture result"), None);
}
fn configure(f: &mut crate::service_tests::Fixture) {
    struct E(String);
    impl devbox_core::Env for E {
        fn var(&self, k: &str) -> Option<String> {
            matches!(k, "HOME" | "XDG_RUNTIME_DIR").then(|| self.0.clone())
        }
        fn uid(&self) -> u32 {
            1000
        }
    }
    let paths = devbox_core::Paths::resolve(
        devbox_core::Instance::parse("test-windows").expect("fixture result"),
        &E(f.root.path().display().to_string()),
    )
    .expect("fixture result");
    fs::create_dir_all(&paths.instance_config_dir).expect("fixture result");
    fs::write(
        paths.instance_config_dir.join("env"),
        "PATH=/usr/bin:/bin\n",
    )
    .expect("fixture result");
    let mut execution = crate::ExecutionConfig::new(paths);
    execution.windows_root = Some("C:\\owned-copy".into());
    f.runs = std::sync::Arc::new(
        crate::RuntimeService::new(f.db.clone(), f.hub.clone(), f.projects.clone())
            .with_execution(execution),
    );
}
async fn approve(f: &crate::service_tests::Fixture) {
    use devbox_projects::*;
    let preview = f
        .projects
        .trust_preview(
            TrustPreviewParams {
                id: f.project.clone(),
                operation: TrustOperation::Run,
                agent_id: None,
                worktrees: vec![],
                worktree: None,
            },
            true,
            vec![],
        )
        .await
        .expect("fixture result");
    f.projects
        .trust_approve(TrustApprove {
            id: f.project.clone(),
            expected_canonical_path: preview.canonical_path,
            expected_digest: preview.digest.expect("fixture value"),
            expected_scope_version: preview.scope_version,
        })
        .await
        .expect("fixture result");
}
#[tokio::test]
async fn this_pc_only_windows_run_ignores_stale_project_trust() {
    use crate::*;
    let mut f = crate::service_tests::Fixture::new().await;
    configure(&mut f);
    f.write("[[task]]\nid='unused'\ncommand='approved first'\n");
    approve(&f).await;
    f.write("[[task]]\nid='unused'\ncommand='unapproved replacement'\n");
    f.runs
        .define(DefineRun {
            project_id: f.project.clone(),
            save_to: SaveTo::ThisPc,
            def: RunDef::Task(TaskDef {
                id: "win".into(),
                command: "Write-Output 'owned'".into(),
                target: RunTarget::Windows,
                ..Default::default()
            }),
        })
        .await
        .expect("fixture result");
    let graph = f
        .runs
        .authorize_run_graph(&StartRun {
            project_id: f.project.clone(),
            id: "win".into(),
            worktree: None,
        })
        .await
        .expect("fixture result");
    assert_eq!(graph.ids(), vec!["win"]);
    assert_eq!(graph.windows["win"].cwd, "C:\\owned-copy\\repo");
    git(&f.root.path().join("repo"), &["init", "-q"]);
    let owned_target = f.root.path().join("owned windows target");
    let report = sync(&graph.windows["win"].source, &owned_target)
        .await
        .expect("fixture result");
    assert_eq!(report.deleted, 0);
    assert!(owned_target.join(".devbox/devbox.toml").is_file());
    assert!(
        f.runs
            .runs(RunsQuery {
                project_id: Some(f.project.clone()),
                limit: 10
            })
            .await
            .expect("fixture result")
            .items
            .is_empty()
    );
}
#[tokio::test]
async fn windows_run_with_project_dependency_requires_current_snapshot_before_sync() {
    use crate::*;
    let mut f = crate::service_tests::Fixture::new().await;
    configure(&mut f);
    f.write("[[task]]\nid='dependency'\ncommand='unapproved'\n");
    f.runs
        .define(DefineRun {
            project_id: f.project.clone(),
            save_to: SaveTo::ThisPc,
            def: RunDef::Task(TaskDef {
                id: "win".into(),
                command: "Write-Output owned".into(),
                target: RunTarget::Windows,
                depends_on: vec!["dependency".into()],
                ..Default::default()
            }),
        })
        .await
        .expect("fixture result");
    let result = f
        .runs
        .start(StartRun {
            project_id: f.project.clone(),
            id: "win".into(),
            worktree: None,
        })
        .await;
    assert!(matches!(
        result,
        Err(devbox_protocol::Fail::Domain(
            RuntimeError::ProjectTrustRequired { .. }
        ))
    ));
    approve(&f).await;
    let prepared = f
        .runs
        .authorize_run_graph(&StartRun {
            project_id: f.project.clone(),
            id: "win".into(),
            worktree: None,
        })
        .await
        .expect("fixture result");
    f.write("[[task]]\nid='dependency'\ncommand='changed after approval'\n");
    assert_eq!(
        prepared.nodes[0].command(),
        Some("unapproved"),
        "prepared graph retains the approved command snapshot"
    );
    assert!(
        f.runs
            .runs(RunsQuery {
                project_id: Some(f.project.clone()),
                limit: 10
            })
            .await
            .expect("fixture result")
            .items
            .is_empty()
    );
}
#[tokio::test]
async fn windows_cwd_must_remain_under_its_resolved_source() {
    use crate::*;
    let mut f = crate::service_tests::Fixture::new().await;
    configure(&mut f);
    f.runs
        .define(DefineRun {
            project_id: f.project.clone(),
            save_to: SaveTo::ThisPc,
            def: RunDef::Task(TaskDef {
                id: "win".into(),
                command: "exit 0".into(),
                cwd: Some("..".into()),
                target: RunTarget::Windows,
                ..Default::default()
            }),
        })
        .await
        .expect("fixture result");
    assert!(matches!(
        f.runs
            .authorize_run_graph(&StartRun {
                project_id: f.project.clone(),
                id: "win".into(),
                worktree: None
            })
            .await,
        Err(devbox_protocol::Fail::Domain(
            RuntimeError::DefInvalid { .. }
        ))
    ));
}

#[tokio::test]
async fn windows_preview_exposes_copy_directory_and_termination_notice() {
    use crate::*;
    let mut f = crate::service_tests::Fixture::new().await;
    configure(&mut f);
    let def = RunDef::Task(TaskDef {
        id: "win".into(),
        command: "exit 0".into(),
        cwd: Some("sub dir's".into()),
        target: RunTarget::Windows,
        ..Default::default()
    });
    let preview = f.runs.windows_preview("repo", "/owned/repo", &[def]);
    assert_eq!(
        preview,
        vec![format!(
            "win · C:\\owned-copy\\repo\\sub dir's · {}",
            crate::windows::EXECUTION_NOTICE
        )]
    );
}
