//! Owned regression for distinct Windows copy identities; no Windows/systemd/network execution.
use crate::*;
use std::path::Path;
fn git_init(path: &Path) {
    let result = std::process::Command::new("git")
        .args(["init", "-q"])
        .arg(path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("initialize owned Git fixture");
    assert!(result.status.success());
}
#[tokio::test]
async fn separate_same_named_projects_must_not_overwrite_one_windows_copy() {
    let f = crate::service_tests::Fixture::new().await;
    let a = f.root.path().join("repo");
    let b = f.root.path().join("other/repo");
    std::fs::create_dir_all(&b).expect("owned second repo");
    for path in [&a, &b] {
        git_init(path);
    }
    std::fs::write(a.join("entry.txt"), "project A\n").expect("owned A file");
    std::fs::write(b.join("entry.txt"), "project B different bytes\n").expect("owned B file");
    let second = f
        .projects
        .add(devbox_projects::AddProject {
            path: b.to_string_lossy().into_owned(),
        })
        .await
        .expect("register distinct second project");
    let first = f
        .projects
        .get(&f.project)
        .await
        .expect("read first project")
        .expect("first project exists");
    assert_ne!(first.id, second.id);
    assert_eq!(first.name, second.name);
    struct Env(String);
    impl devbox_core::Env for Env {
        fn var(&self, key: &str) -> Option<String> {
            matches!(key, "HOME" | "XDG_RUNTIME_DIR").then(|| self.0.clone())
        }
        fn uid(&self) -> u32 {
            1000
        }
    }
    let paths = devbox_core::Paths::resolve(
        devbox_core::Instance::parse("test-copy-review").expect("owned instance"),
        &Env(f.root.path().display().to_string()),
    )
    .expect("owned paths");
    std::fs::create_dir_all(&paths.instance_config_dir).expect("owned instance config");
    std::fs::write(
        paths.instance_config_dir.join("env"),
        "PATH=/usr/bin:/bin\n",
    )
    .expect("owned captured environment");
    let mut execution = ExecutionConfig::new(paths);
    execution.windows_root = Some("C:\\owned-review-only".into());
    let service = RuntimeService::new(f.db.clone(), f.hub.clone(), f.projects.clone())
        .with_execution(execution);
    for project in [&first.id, &second.id] {
        service
            .define(DefineRun {
                project_id: project.clone(),
                save_to: SaveTo::ThisPc,
                def: RunDef::Task(TaskDef {
                    id: "win".into(),
                    command: "Write-Output owned".into(),
                    target: RunTarget::Windows,
                    ..Default::default()
                }),
            })
            .await
            .expect("owned local Windows definition");
    }
    let ga = service
        .authorize_run_graph(&StartRun {
            project_id: first.id,
            id: "win".into(),
            worktree: None,
        })
        .await
        .expect("prepare A");
    let gb = service
        .authorize_run_graph(&StartRun {
            project_id: second.id,
            id: "win".into(),
            worktree: None,
        })
        .await
        .expect("prepare B");
    let ca = &ga.windows["win"];
    let cb = &gb.windows["win"];
    assert_ne!(ca.source, cb.source);
    assert_ne!(
        ca.target, cb.target,
        "different registered projects must not alias"
    );
    // Use an ordinary owned Linux directory, never the prepared /mnt/c target.
    let copies = f.root.path().join("owned-copy-simulation");
    let copy = copies.join(&ca.identity.key);
    let other_copy = copies.join(&cb.identity.key);
    assert_ne!(copy, other_copy);
    crate::windows::sync(&ca.source, &copy)
        .await
        .expect("copy A");
    assert_eq!(
        std::fs::read_to_string(copy.join("entry.txt")).expect("A copied bytes"),
        "project A\n"
    );
    crate::windows::sync(&cb.source, &other_copy)
        .await
        .expect("copy B");
    let observed = std::fs::read_to_string(copy.join("entry.txt")).expect("post B copied bytes");
    assert_eq!(
        observed, "project A\n",
        "starting B must not overwrite A's independently selected project copy"
    );
}

#[tokio::test]
async fn same_size_and_mtime_source_change_updates_the_copy_revision() {
    let temp = tempfile::tempdir().expect("owned copy");
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    std::fs::create_dir(&source).expect("source");
    git_init(&source);
    let file = source.join("same.txt");
    std::fs::write(&file, "before").expect("before");
    let stamp = std::fs::metadata(&file)
        .expect("metadata")
        .modified()
        .expect("mtime");
    let first = crate::windows_copy::capture(&source, tokio_util::sync::CancellationToken::new())
        .await
        .expect("first revision");
    crate::windows::sync(&source, &target)
        .await
        .expect("first copy");
    std::fs::write(&file, "after!").expect("after");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .expect("owned file")
        .set_times(std::fs::FileTimes::new().set_modified(stamp))
        .expect("same mtime");
    let second = crate::windows_copy::capture(&source, tokio_util::sync::CancellationToken::new())
        .await
        .expect("second revision");
    assert_ne!(first.revision, second.revision);
    assert_eq!(
        crate::windows::sync(&source, &target)
            .await
            .expect("content copy")
            .copied,
        1
    );
    assert_eq!(
        std::fs::read_to_string(target.join("same.txt")).expect("copied"),
        "after!"
    );
}
