use crate::windows::{quote_powershell, sync, windows_to_wsl};
use std::{fs, path::Path, process::Command};
fn git(path: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}
#[tokio::test]
async fn windows_sync_copies_changed_tracked_files_and_keeps_extra_target_files() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source's repo");
    let target = temp.path().join("target's repo");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(target.join("node_modules")).unwrap();
    git(&source, &["init", "-q"]);
    git(&target, &["init", "-q"]);
    fs::write(source.join("tracked.txt"), "before").unwrap();
    fs::write(source.join("deleted.txt"), "keep deleted source").unwrap();
    fs::write(source.join(".gitignore"), "ignored\n").unwrap();
    git(&source, &["add", "."]);
    fs::write(source.join("new file's.txt"), "added").unwrap();
    fs::write(source.join("ignored"), "do not copy").unwrap();
    fs::write(target.join("node_modules/keep"), "keep").unwrap();
    let first = sync(&source, &target).await.unwrap();
    assert_eq!(first.copied, 4);
    assert_eq!(first.deleted, 0);
    let again = sync(&source, &target).await.unwrap();
    assert_eq!((again.copied, again.skipped), (0, 4));
    fs::write(source.join("tracked.txt"), "changed").unwrap();
    fs::remove_file(source.join("deleted.txt")).unwrap();
    let last = sync(&source, &target).await.unwrap();
    assert_eq!(last.copied, 1);
    assert_eq!(last.deleted, 0);
    assert_eq!(
        fs::read_to_string(target.join("tracked.txt")).unwrap(),
        "changed"
    );
    assert_eq!(
        fs::metadata(source.join("tracked.txt"))
            .unwrap()
            .modified()
            .unwrap(),
        fs::metadata(target.join("tracked.txt"))
            .unwrap()
            .modified()
            .unwrap()
    );
    assert!(target.join("deleted.txt").exists());
    assert!(target.join("node_modules/keep").exists());
    assert!(target.join(".git").is_dir());
    assert!(!target.join("ignored").exists());
    fs::write(source.join("tracked.txt"), "altered").unwrap();
    let modified = fs::metadata(source.join("tracked.txt"))
        .unwrap()
        .modified()
        .unwrap()
        + std::time::Duration::from_secs(2);
    fs::File::options()
        .write(true)
        .open(source.join("tracked.txt"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert_eq!(
        sync(&source, &target).await.unwrap().copied,
        1,
        "same-size mtime change is copied"
    );
    assert_eq!(
        fs::read_to_string(target.join("tracked.txt")).unwrap(),
        "altered"
    );
}
#[test]
fn windows_command_is_quoted_for_powershell() {
    assert_eq!(quote_powershell("a'b"), "'a''b'");
    assert_eq!(
        windows_to_wsl("C:\\dev\\a'b repo")
            .unwrap()
            .to_str()
            .unwrap(),
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
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let outside = temp.path().join("outside");
        for path in [&source, &target, &outside] {
            fs::create_dir_all(path).unwrap();
        }
        git(&source, &["init", "-q"]);
        fs::create_dir(source.join("dir")).unwrap();
        fs::write(source.join("dir/tracked.txt"), "new repository bytes").unwrap();
        git(&source, &["add", "."]);
        fs::write(outside.join("tracked.txt"), "owned sentinel").unwrap();
        match kind {
            "leaf" => {
                fs::create_dir(target.join("dir")).unwrap();
                symlink(outside.join("tracked.txt"), target.join("dir/tracked.txt")).unwrap();
            }
            "parent" => symlink(&outside, target.join("dir")).unwrap(),
            "source" => {
                fs::remove_file(source.join("dir/tracked.txt")).unwrap();
                fs::remove_dir(source.join("dir")).unwrap();
                symlink(&outside, source.join("dir")).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(sync(&source, &target).await.is_err(), "{kind}");
        assert_eq!(
            fs::read_to_string(outside.join("tracked.txt")).unwrap(),
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
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).unwrap();
        git(&source, &["init", "-q"]);
        for name in names {
            let path = source.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "owned").unwrap();
        }
        assert!(sync(&source, &target).await.is_err());
        assert!(!target.exists());
    }
}
#[test]
fn windows_native_dispatch_claim_is_attempt_scoped_and_known_exit_is_full_width() {
    let temp = tempfile::tempdir().unwrap();
    let store = crate::exit_record::ExitStore::new(temp.path(), "owned").unwrap();
    store.begin("first").unwrap();
    store.claim_windows_native("first").unwrap();
    assert!(store.claim_windows_native("first").is_err());
    store.record_windows_exit("first", 0x1234).unwrap();
    assert_eq!(store.windows_exit("first").unwrap(), Some(0x1234));
    store.begin("second").unwrap();
    assert!(store.claim_windows_native("first").is_err());
    store.claim_windows_native("second").unwrap();
    assert_eq!(store.windows_exit("second").unwrap(), None);
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
        devbox_core::Instance::parse("test-windows").unwrap(),
        &E(f.root.path().display().to_string()),
    )
    .unwrap();
    fs::create_dir_all(&paths.instance_config_dir).unwrap();
    fs::write(
        paths.instance_config_dir.join("env"),
        "PATH=/usr/bin:/bin\n",
    )
    .unwrap();
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
        .unwrap();
    f.projects
        .trust_approve(TrustApprove {
            id: f.project.clone(),
            expected_canonical_path: preview.canonical_path,
            expected_digest: preview.digest.unwrap(),
            expected_scope_version: preview.scope_version,
        })
        .await
        .unwrap();
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
        .unwrap();
    let graph = f
        .runs
        .authorize_run_graph(&StartRun {
            project_id: f.project.clone(),
            id: "win".into(),
            worktree: None,
        })
        .await
        .unwrap();
    assert_eq!(graph.ids(), vec!["win"]);
    assert_eq!(graph.windows["win"].cwd, "C:\\owned-copy\\repo");
    git(&f.root.path().join("repo"), &["init", "-q"]);
    let owned_target = f.root.path().join("owned windows target");
    let report = sync(&graph.windows["win"].source, &owned_target)
        .await
        .unwrap();
    assert_eq!(report.deleted, 0);
    assert!(owned_target.join(".devbox/devbox.toml").is_file());
    assert!(
        f.runs
            .runs(RunsQuery {
                project_id: Some(f.project.clone()),
                limit: 10
            })
            .await
            .unwrap()
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
        .unwrap();
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
        .unwrap();
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
            .unwrap()
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
        .unwrap();
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
