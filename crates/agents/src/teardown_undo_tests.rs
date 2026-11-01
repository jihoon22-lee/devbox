//! A closed task's cleanup runs the project's teardown in the work folder, then removes the folder
//! without force. What the teardown leaves in a folder that was clean at the closed head is undone
//! first; a folder with changes of its own, or one the undo cannot account for, is refused as
//! before (Ruling 61). Folders with submodules are in `teardown_submodule_tests`.
use super::{teardown_harness::*, *};

#[test]
fn cleanup_undoes_what_its_teardown_left_in_a_clean_folder_and_removes_it() {
    cases(
        "teardown_undo_tests::cleanup_undoes_what_its_teardown_left_in_a_clean_folder_and_removes_it",
        &[
            "auto-untracked",
            "manual-untracked",
            "auto-tracked",
            "auto-ignored",
        ],
        case,
    );
}

#[test]
fn cleanup_keeps_a_folder_whose_changes_it_cannot_account_for() {
    cases(
        "teardown_undo_tests::cleanup_keeps_a_folder_whose_changes_it_cannot_account_for",
        &["user-file", "nested", "head-moved"],
        case,
    );
}

/// A teardown command that fails stops cleanup with `agents.teardown_failed`: which command of
/// `[agent] teardown` (1-based) and its exit code, none when it was killed. The command's text
/// stays out; the user reads it in the file. The folder and branch stay.
#[test]
fn a_failing_teardown_names_its_command_and_keeps_the_folder() {
    cases(
        "teardown_undo_tests::a_failing_teardown_names_its_command_and_keeps_the_folder",
        &["fails-auto", "fails-killed", "fails-manual"],
        case,
    );
}

/// A teardown that writes into a folder that was clean at the closed head and then fails: what it
/// wrote is undone after the same checks as before a removal, so the folder stays in the cleanup
/// list and a fixed teardown cleans it up. The failure is still `agents.teardown_failed` and the
/// folder stays. A teardown that also moved HEAD keeps what it left.
#[test]
fn a_failed_teardown_leaves_the_folder_as_it_was_and_listed() {
    cases(
        "teardown_undo_tests::a_failed_teardown_leaves_the_folder_as_it_was_and_listed",
        &["failed-auto", "failed-manual", "failed-head-moved"],
        case,
    );
}

/// `git status` does not show an edit to a file marked assume-unchanged or skip-worktree, so the
/// folder looks clean while it holds one, maybe the user's. Then nothing is undone: the removal
/// refuses what the teardown left, as it did before the undo existed.
#[test]
fn cleanup_never_undoes_an_edit_git_status_does_not_show() {
    cases(
        "teardown_undo_tests::cleanup_never_undoes_an_edit_git_status_does_not_show",
        &["assume-unchanged", "skip-worktree"],
        case,
    );
}

/// A sparse checkout marks every entry outside its cone skip-worktree, with no file in the folder:
/// such an entry holds no edit, so the undo still runs and the folder goes. An entry outside the
/// cone whose file is there again can hold the user's edit: Git shows it unless told to expect such
/// files, and then nothing is undone. With no entry inside the cone, `git restore` would match
/// nothing, so nothing is undone either, and the removal refuses what is left.
#[test]
fn the_undo_keeps_to_what_a_sparse_checkout_leaves_out() {
    cases(
        "teardown_undo_tests::the_undo_keeps_to_what_a_sparse_checkout_leaves_out",
        &[
            "sparse",
            "sparse-materialized",
            "sparse-materialized-shown",
            "all-skip-worktree",
        ],
        case,
    );
}

/// Commits `keep/kept.txt` and `out/outside.txt` on the task's branch, then narrows the work folder
/// to a cone of `keep` with `pattern`, as a sparse-checkout project's work folders are.
fn narrow(wt: &Path, pattern: &[&str]) {
    for name in ["keep/kept.txt", "out/outside.txt"] {
        let file = wt.join(name);
        fs::create_dir_all(file.parent().expect("owned folder")).expect("owned folder");
        fs::write(file, "base\n").expect("owned file");
    }
    git(wt, &["add", "keep", "out"]);
    git(wt, &["commit", "-qm", "sparse layout"]);
    git(wt, &[&["sparse-checkout", "set"][..], pattern].concat());
    assert!(!wt.join("out").exists());
}

async fn case(mode: String) {
    let mode = mode.as_str();
    let (mut f, task, wt) = action_fixture().await;
    match mode {
        "sparse" => {
            narrow(&wt, &["--cone", "keep"]);
            let ran = approve_teardown(&f, &task, &wt, &["printf x > teardown.log"]).await;
            let closed = close(&f, &task, MergeKind::Squash).await;
            assert!(ran.exists(), "the teardown ran");
            assert_eq!(closed.cleanup_error, None, "{closed:?}");
            assert!(!wt.exists());
            assert!(!has_branch(&f).await);
            let cleaned = receipt("clean");
            assert_eq!(cleaned.len(), 1, "{cleaned:?}");
        }
        "sparse-materialized" | "sparse-materialized-shown" => {
            set_cleanup(&mut f, false);
            narrow(&wt, &["--cone", "keep"]);
            if mode == "sparse-materialized" {
                // Git then keeps the entry skip-worktree, and status shows no edit to it.
                git(
                    &wt,
                    &[
                        "config",
                        "--worktree",
                        "sparse.expectFilesOutsideOfPatterns",
                        "true",
                    ],
                );
            }
            close(&f, &task, MergeKind::Merge).await;
            let ran = approve_teardown(&f, &task, &wt, &["printf x > teardown.log"]).await;
            fs::create_dir(wt.join("out")).expect("owned folder");
            fs::write(wt.join("out/outside.txt"), "the user's copy\n").expect("user file");
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
            // Shown, the edit made the folder dirty before cleanup, so nothing was undone either.
            let files = if mode == "sparse-materialized" {
                serde_json::json!(["teardown.log"])
            } else {
                serde_json::json!(["out/outside.txt", "teardown.log"])
            };
            assert_eq!(note["detail"]["files"], files);
            assert_eq!(
                fs::read_to_string(wt.join("out/outside.txt")).expect("kept file"),
                "the user's copy\n"
            );
            assert!(wt.join("teardown.log").exists(), "nothing was undone");
            assert!(receipt("clean").is_empty());
            assert!(has_branch(&f).await);
        }
        "all-skip-worktree" => {
            narrow(&wt, &["--no-cone", "/nothing-matches"]);
            let ran = approve_teardown(&f, &task, &wt, &["printf x > teardown.log"]).await;
            let closed = close(&f, &task, MergeKind::Squash).await;
            assert!(ran.exists(), "the teardown ran");
            // The removal's refusal, not a failed `git restore`.
            let note = note_of(&closed);
            assert_eq!(note["code"], "agents.worktree_dirty", "{note}");
            assert_eq!(note["detail"]["files"], serde_json::json!(["teardown.log"]));
            assert!(receipt("clean").is_empty(), "nothing was undone");
            assert!(has_branch(&f).await);
        }
        "auto-untracked" | "auto-tracked" | "auto-ignored" => {
            let teardown = match mode {
                "auto-tracked" => "printf edited > tracked && git add tracked",
                _ => "printf x > teardown.log",
            };
            if mode == "auto-ignored" {
                fs::write(wt.join(".gitignore"), "ignored.log\n").expect("owned ignore file");
                git(&wt, &["add", ".gitignore"]);
                git(&wt, &["commit", "-qm", "ignore"]);
                fs::write(wt.join("ignored.log"), "ignored\n").expect("ignored file");
            }
            let ran = approve_teardown(&f, &task, &wt, &[teardown]).await;
            let closed = close(&f, &task, MergeKind::Squash).await;
            assert!(ran.exists(), "the teardown ran");
            assert_eq!(closed.cleanup_error, None, "{closed:?}");
            assert!(!wt.exists());
            assert!(!has_branch(&f).await);
            assert_eq!(reservation(&f, &task).await, Resources::Planned);
            // Untracked files that are not ignored go, with one `-f` and no `-x`.
            let cleaned = receipt("clean");
            assert_eq!(cleaned.len(), 1, "{cleaned:?}");
            assert!(cleaned[0].ends_with(" clean -f -d"), "{cleaned:?}");
            if mode == "auto-ignored" {
                // The ignored file was still there when the removal started: it goes with the
                // folder, never before.
                assert_eq!(receipt("remove"), ["kept"]);
            }
        }
        "manual-untracked" => {
            set_cleanup(&mut f, false);
            close(&f, &task, MergeKind::Merge).await;
            let ran = approve_teardown(&f, &task, &wt, &["printf x > teardown.log"]).await;
            let listed = f
                .svc
                .cleanup_candidates(CleanupCandidates {
                    project_id: f.project.clone(),
                })
                .await
                .expect("candidates");
            assert_eq!(listed.worktrees, [wt.to_string_lossy().into_owned()]);
            assert_eq!(listed.branches, ["agent/review"]);
            f.svc
                .cleanup(CleanupAgents {
                    project_id: f.project.clone(),
                    worktrees: listed.worktrees,
                    branches: listed.branches,
                })
                .await
                .expect("cleanup of the listed task");
            assert!(ran.exists(), "the teardown ran");
            assert!(!wt.exists());
            assert!(!has_branch(&f).await);
            assert_eq!(reservation(&f, &task).await, Resources::Planned);
        }
        "user-file" => {
            // The user's own file was in the folder before cleanup started: nothing is undone.
            set_cleanup(&mut f, false);
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
            assert!(wt.join("teardown.log").exists(), "nothing is undone");
            assert!(has_branch(&f).await);
            assert!(receipt("clean").is_empty());
        }
        "nested" => {
            // The teardown makes a nested repository: a single `-f` leaves it, so the folder stays.
            let ran = approve_teardown(
                &f,
                &task,
                &wt,
                &["printf x > teardown.log && mkdir -p vendor/tool && git -C vendor/tool init -q && printf data > vendor/tool/data"],
            )
            .await;
            let closed = close(&f, &task, MergeKind::Squash).await;
            assert!(ran.exists(), "the teardown ran");
            let note = note_of(&closed);
            assert_eq!(note["code"], "agents.worktree_dirty", "{note}");
            assert_eq!(note["detail"]["files"], serde_json::json!(["vendor/tool/"]));
            assert!(wt.join("vendor/tool/.git").is_dir());
            assert_eq!(
                fs::read_to_string(wt.join("vendor/tool/data")).expect("nested file"),
                "data"
            );
            assert!(has_branch(&f).await);
        }
        "head-moved" => {
            let ran =
                approve_teardown(&f, &task, &wt, &["git commit --allow-empty -qm teardown"]).await;
            let closed = close(&f, &task, MergeKind::Squash).await;
            assert!(ran.exists(), "the teardown ran");
            let note = note_of(&closed);
            assert_eq!(note["code"], "agents.state_changed", "{note}");
            let head = devbox_git::run::resolve(&wt, "HEAD")
                .await
                .expect("moved head");
            assert_eq!(note["detail"]["head"], serde_json::json!(head));
            assert_ne!(Some(head), closed.closed_head_oid);
            assert!(wt.exists());
            assert!(has_branch(&f).await);
            assert!(receipt("clean").is_empty(), "nothing was undone");
            assert_eq!(reservation(&f, &task).await, Resources::Owned);
        }
        "fails-auto" | "fails-killed" => {
            // The marker command is the first; the failing one is the second.
            let (command, note) = if mode == "fails-auto" {
                (
                    "printf x > teardown.log; exit 3",
                    r#"{"code":"agents.teardown_failed","detail":{"index":2,"exit_code":3}}"#,
                )
            } else {
                (
                    "kill -KILL $$",
                    r#"{"code":"agents.teardown_failed","detail":{"index":2,"exit_code":null}}"#,
                )
            };
            let ran = approve_teardown(&f, &task, &wt, &[command]).await;
            let closed = close(&f, &task, MergeKind::Squash).await;
            assert!(ran.exists(), "the first command ran");
            // Stored as the task's note, like every other code, so its notice can name it.
            assert_eq!(closed.cleanup_error.as_deref(), Some(note));
            assert!(!note.contains(command), "the command's text stays out");
            assert!(wt.exists());
            assert!(has_branch(&f).await);
            assert_eq!(reservation(&f, &task).await, Resources::Owned);
        }
        "fails-manual" => {
            set_cleanup(&mut f, false);
            close(&f, &task, MergeKind::Merge).await;
            approve_teardown(&f, &task, &wt, &["exit 3"]).await;
            let result = f
                .svc
                .cleanup(CleanupAgents {
                    project_id: f.project.clone(),
                    worktrees: vec![wt.to_string_lossy().into_owned()],
                    branches: vec!["agent/review".into()],
                })
                .await;
            let Err(Fail::Domain(error)) = &result else {
                panic!("a domain error: {result:?}");
            };
            assert_eq!(
                serde_json::to_value(error).expect("error JSON"),
                serde_json::json!({"code":"agents.teardown_failed","detail":{"index":2,"exit_code":3}})
            );
            assert!(wt.exists());
            assert!(has_branch(&f).await);
        }
        "assume-unchanged" | "skip-worktree" => {
            let flag = format!("--{mode}");
            git(&wt, &["update-index", &flag, "tracked"]);
            fs::write(wt.join("tracked"), "the user's edit\n").expect("user edit");
            let ran = approve_teardown(&f, &task, &wt, &["printf x > teardown.log"]).await;
            let closed = close(&f, &task, MergeKind::Squash).await;
            assert!(ran.exists(), "the teardown ran");
            let note = note_of(&closed);
            assert_eq!(note["code"], "agents.worktree_dirty", "{note}");
            assert_eq!(note["detail"]["files"], serde_json::json!(["teardown.log"]));
            assert_eq!(
                fs::read_to_string(wt.join("tracked")).expect("kept edit"),
                "the user's edit\n"
            );
            assert!(wt.join("teardown.log").exists(), "nothing was undone");
            assert!(receipt("clean").is_empty());
            assert!(has_branch(&f).await);
        }
        "failed-auto" | "failed-manual" => {
            let failing = "printf x > t.log; exit 3";
            let note = serde_json::json!({"code":"agents.teardown_failed","detail":{"index":2,"exit_code":3}});
            if mode == "failed-auto" {
                approve_teardown(&f, &task, &wt, &[failing]).await;
                let closed = close(&f, &task, MergeKind::Squash).await;
                assert_eq!(note_of(&closed), note);
            } else {
                set_cleanup(&mut f, false);
                close(&f, &task, MergeKind::Merge).await;
                approve_teardown(&f, &task, &wt, &[failing]).await;
                let result = f.svc.cleanup(offered(&f, &listed(&f).await)).await;
                let Err(Fail::Domain(error)) = &result else {
                    panic!("a domain error: {result:?}");
                };
                assert_eq!(serde_json::to_value(error).expect("error JSON"), note);
            }
            // The failed command's file is undone: the folder is clean at the closed head again.
            assert!(!wt.join("t.log").exists());
            assert!(devbox_git::status(&wt).await.expect("status").clean);
            let options = listed(&f).await;
            assert_eq!(options.worktrees, [wt.to_string_lossy().into_owned()]);
            assert_eq!(options.branches, ["agent/review"]);
            // Fixed, the teardown runs again and the folder and branch go.
            let ran = approve_teardown(&f, &task, &wt, &["printf x > t.log"]).await;
            fs::remove_file(&ran).expect("marker of the failed run");
            f.svc
                .cleanup(offered(&f, &options))
                .await
                .expect("cleanup with the fixed teardown");
            assert!(ran.exists(), "the fixed teardown ran");
            assert!(!wt.exists());
            assert!(!has_branch(&f).await);
            assert_eq!(f.svc.get(&task.id).await.expect("task").cleanup_error, None);
        }
        "failed-head-moved" => {
            approve_teardown(
                &f,
                &task,
                &wt,
                &["git commit --allow-empty -qm teardown && printf x > t.log; exit 3"],
            )
            .await;
            let closed = close(&f, &task, MergeKind::Squash).await;
            assert_eq!(note_of(&closed)["code"], "agents.teardown_failed");
            // A pin moved: nothing is undone, and the folder is no longer offered.
            assert!(wt.join("t.log").exists());
            assert!(receipt("clean").is_empty());
            assert!(listed(&f).await.worktrees.is_empty());
            assert!(has_branch(&f).await);
        }
        other => panic!("unknown mode {other}"),
    }
}
