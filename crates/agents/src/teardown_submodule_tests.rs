//! Git removes a work folder with initialized submodules only with force, which devbox never uses:
//! their commits can exist only under the folder's own `.git/worktrees/<name>/modules`. Cleanup
//! still runs the teardown, then refuses the folder with `agents.worktree_has_submodules`, which
//! names it, keeps that as the task's note and undoes nothing (Ruling 64 (c)).
use super::{teardown_harness::*, *};

/// Automatic and manual cleanup both stop with the code and store it, and the folder, its
/// submodule and the submodule's own commit stay as they were. The list names such a folder apart
/// while it is clean at the closed head, and offers neither it nor its branch.
#[test]
fn a_folder_with_submodules_is_kept_and_cleanup_says_why() {
    cases(
        "teardown_submodule_tests::a_folder_with_submodules_is_kept_and_cleanup_says_why",
        &["submodule-auto", "submodule-manual", "submodule-edit"],
        case,
    );
}

/// The submodule code advises removing the folder with force, so it is never given for a folder
/// whose changes that would lose, and never widens what the list offers.
#[test]
fn the_submodule_code_never_covers_a_folder_cleanup_must_not_take() {
    cases(
        "teardown_submodule_tests::the_submodule_code_never_covers_a_folder_cleanup_must_not_take",
        &["submodule-dirty", "submodule-unreleased"],
        case,
    );
}

/// Adds an initialized submodule `lib` to the work folder, then commits in it a change that exists
/// only in the folder's own submodule store, which a forced removal would lose. Returns the
/// submodule's folder and that commit.
async fn add_submodule(f: &Fixture, wt: &Path) -> (PathBuf, String) {
    let lib = f.root.path().join("lib");
    fs::create_dir(&lib).expect("owned submodule source");
    git(&lib, &["init", "-qb", "main"]);
    git(&lib, &["config", "user.name", "fixture"]);
    git(&lib, &["config", "user.email", "fixture@example.invalid"]);
    git(&lib, &["config", "commit.gpgSign", "false"]);
    fs::write(lib.join("file.txt"), "one\n").expect("submodule file");
    git(&lib, &["add", "file.txt"]);
    git(&lib, &["commit", "-qm", "lib"]);
    git(
        wt,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            lib.to_str().expect("owned source"),
            "lib",
        ],
    );
    git(wt, &["commit", "-qm", "submodule"]);
    let sub = wt.join("lib");
    git(&sub, &["config", "user.name", "fixture"]);
    git(&sub, &["config", "user.email", "fixture@example.invalid"]);
    git(&sub, &["config", "commit.gpgSign", "false"]);
    fs::write(sub.join("file.txt"), "unpushed\n").expect("submodule edit");
    git(&sub, &["commit", "-qam", "unpushed"]);
    git(wt, &["add", "lib"]);
    git(wt, &["commit", "-qm", "bump lib"]);
    let unpushed = devbox_git::run::resolve(&sub, "HEAD")
        .await
        .expect("submodule head");
    (sub, unpushed)
}

/// The folder, its submodule store and the submodule's commit, branch and file are as they were,
/// and Git was never asked to remove or clean anything.
async fn untouched(f: &Fixture, wt: &Path, sub: &Path, unpushed: &str, file: &str) {
    assert!(wt.exists());
    let modules = devbox_git::run::git(
        wt,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "modules",
        ],
    )
    .await
    .expect("submodule store");
    assert!(Path::new(modules.trim_end()).join("lib").is_dir());
    assert_eq!(
        fs::read_to_string(sub.join("file.txt")).expect("submodule file"),
        file
    );
    assert_eq!(
        devbox_git::run::resolve(sub, "HEAD")
            .await
            .expect("submodule head"),
        unpushed
    );
    assert_eq!(
        devbox_git::current_branch(sub)
            .await
            .expect("submodule branch")
            .as_deref(),
        Some("main")
    );
    assert!(receipt("remove").is_empty(), "Git's removal never ran");
    assert!(receipt("clean").is_empty(), "nothing was undone");
    assert!(has_branch(f).await);
}

fn refused(wt: &Path) -> serde_json::Value {
    serde_json::json!({
        "code": "agents.worktree_has_submodules",
        "detail": {"path": wt.to_string_lossy()},
    })
}

async fn case(mode: String) {
    let mode = mode.as_str();
    let (mut f, task, wt) = action_fixture().await;
    // The folder with its branch, as the list offered them before the submodule was there.
    let request = CleanupAgents {
        project_id: f.project.clone(),
        worktrees: vec![wt.to_string_lossy().into_owned()],
        branches: vec!["agent/review".into()],
    };
    match mode {
        "submodule-auto" => {
            let (sub, unpushed) = add_submodule(&f, &wt).await;
            let ran = approve_teardown(&f, &task, &wt, &["printf x > teardown.log"]).await;
            let closed = close(&f, &task, MergeKind::Merge).await;
            assert!(ran.exists(), "the teardown still runs");
            assert_eq!(note_of(&closed), refused(&wt));
            assert!(wt.join("teardown.log").exists(), "nothing is undone");
            untouched(&f, &wt, &sub, &unpushed, "unpushed\n").await;
            assert_eq!(reservation(&f, &task).await, Resources::Owned);
            // What the teardown wrote stays, so the folder is in no list: only the task's notice
            // says how to remove it, and the advice to force never stands next to a changed folder.
            let options = listed(&f).await;
            assert!(
                options.worktrees.is_empty()
                    && options.submodules.is_empty()
                    && options.branches.is_empty(),
                "{options:?}"
            );
        }
        "submodule-manual" | "submodule-edit" => {
            set_cleanup(&mut f, false);
            let (sub, unpushed) = add_submodule(&f, &wt).await;
            close(&f, &task, MergeKind::Merge).await;
            // `git restore` would reset the submodule and detach its HEAD under this setting
            // unless it is told not to recurse; nothing may get that far here.
            git(&f.repo, &["config", "submodule.recurse", "true"]);
            let teardown = if mode == "submodule-manual" {
                "printf x > teardown.log"
            } else {
                "printf x > teardown.log && printf teardown >> lib/file.txt"
            };
            let ran = approve_teardown(&f, &task, &wt, &[teardown]).await;
            if mode == "submodule-manual" {
                // Before any teardown, the folder is clean at the closed head: the list names it
                // apart and offers neither it nor its branch.
                let options = listed(&f).await;
                assert_eq!(options.submodules, [wt.to_string_lossy().into_owned()]);
                assert!(
                    options.worktrees.is_empty()
                        && options.branches.is_empty()
                        && options.off_base.is_empty(),
                    "{options:?}"
                );
                // Its branch alone is refused while the folder is there.
                let alone = f
                    .svc
                    .cleanup(CleanupAgents {
                        worktrees: vec![],
                        ..request.clone()
                    })
                    .await;
                assert!(
                    matches!(alone, Err(Fail::Domain(AgentsError::UnsafeWorktree))),
                    "{alone:?}"
                );
                assert!(!ran.exists());
            }
            // Asked for anyway with its branch, as a list read before the submodule was there
            // would offer them: the teardown runs, then cleanup refuses with the code before the
            // undo and keeps it as the task's note.
            let result = f.svc.cleanup(request).await;
            let Err(Fail::Domain(error)) = &result else {
                panic!("a domain error: {result:?}");
            };
            assert_eq!(
                serde_json::to_value(error).expect("error JSON"),
                refused(&wt)
            );
            assert!(ran.exists(), "the teardown still runs");
            assert_eq!(
                note_of(&f.svc.get(&task.id).await.expect("task")),
                refused(&wt)
            );
            assert!(wt.join("teardown.log").exists(), "nothing is undone");
            let file = if mode == "submodule-manual" {
                "unpushed\n"
            } else {
                "unpushed\nteardown"
            };
            untouched(&f, &wt, &sub, &unpushed, file).await;
        }
        "submodule-dirty" => {
            // The user's own file was there before cleanup: those changes are named, since the
            // force the submodule code advises would lose them.
            set_cleanup(&mut f, false);
            let (sub, unpushed) = add_submodule(&f, &wt).await;
            close(&f, &task, MergeKind::Merge).await;
            let ran = approve_teardown(&f, &task, &wt, &["printf x > teardown.log"]).await;
            fs::write(wt.join("notes.txt"), "the user's notes\n").expect("user file");
            let snapshot = f
                .svc
                .projects
                .teardown_snapshot(&f.project)
                .await
                .expect("approved teardown");
            f.svc
                .cleanup_closed_after_quiesce(&task.id, &snapshot)
                .await;
            assert!(ran.exists(), "the teardown still runs");
            let note = note_of(&f.svc.get(&task.id).await.expect("task"));
            assert_eq!(note["code"], "agents.worktree_dirty", "{note}");
            assert_eq!(
                note["detail"]["files"],
                serde_json::json!(["notes.txt", "teardown.log"])
            );
            assert_eq!(
                fs::read_to_string(wt.join("notes.txt")).expect("kept file"),
                "the user's notes\n"
            );
            untouched(&f, &wt, &sub, &unpushed, "unpushed\n").await;
        }
        "submodule-unreleased" => {
            // Closed by squash with cleanup off: no cleanup stopped, so the branch's own commits
            // not being on the base branch keeps the strict rule. The folder is in no list, a
            // cleanup asking for it is refused before anything runs, and no note is written.
            set_cleanup(&mut f, false);
            let (sub, unpushed) = add_submodule(&f, &wt).await;
            close(&f, &task, MergeKind::Squash).await;
            let ran = approve_teardown(&f, &task, &wt, &["printf x > teardown.log"]).await;
            let options = listed(&f).await;
            assert!(
                options.worktrees.is_empty()
                    && options.submodules.is_empty()
                    && options.branches.is_empty(),
                "{options:?}"
            );
            let result = f.svc.cleanup(request).await;
            assert!(
                matches!(result, Err(Fail::Domain(AgentsError::UnsafeWorktree))),
                "{result:?}"
            );
            assert!(!ran.exists());
            assert_eq!(f.svc.get(&task.id).await.expect("task").cleanup_error, None);
            untouched(&f, &wt, &sub, &unpushed, "unpushed\n").await;
        }
        other => panic!("unknown mode {other}"),
    }
}
