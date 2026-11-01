//! A closed task's cleanup runs the project's teardown in the work folder, then removes the folder
//! without force. What the teardown leaves in a folder that was clean at the closed head is undone
//! first; a folder with changes of its own, or one the undo cannot account for, is refused as
//! before (Ruling 61).
use super::*;

const CHILD: &str = "DEVBOX_UNDO_CASE";

#[test]
fn cleanup_undoes_what_its_teardown_left_in_a_clean_folder_and_removes_it() {
    undo_cases(
        "cleanup_undoes_what_its_teardown_left_in_a_clean_folder_and_removes_it",
        &[
            "auto-untracked",
            "manual-untracked",
            "auto-tracked",
            "auto-ignored",
        ],
    );
}

#[test]
fn cleanup_keeps_a_folder_whose_changes_it_cannot_account_for() {
    undo_cases(
        "cleanup_keeps_a_folder_whose_changes_it_cannot_account_for",
        &[
            "user-file",
            "submodule",
            "submodule-edit",
            "nested",
            "head-moved",
        ],
    );
}

/// Each mode runs alone in a child process. Its PATH starts with owned wrappers: `bash` without
/// profile files (the teardown runs `bash -lc`, which would read the user's), and a transparent
/// `git` that notes the arguments of each `git clean` and, when a removal starts, whether the
/// fixture's ignored file is still in the folder. HOME is an owned folder, so no user Git
/// configuration applies.
fn undo_cases(test: &str, modes: &[&str]) {
    if let Ok(mode) = std::env::var(CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("owned runtime")
            .block_on(case(&mode));
        return;
    }
    let root = tempfile::tempdir().expect("owned wrappers");
    let real_git = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("git path");
    assert!(real_git.status.success());
    let real_git = String::from_utf8(real_git.stdout)
        .expect("Git path")
        .trim()
        .to_owned();
    fs::write(
        root.path().join("bash"),
        "#!/bin/sh\nexec /bin/bash --noprofile --norc \"$@\"\n",
    )
    .expect("owned bash wrapper");
    fs::write(
        root.path().join("git"),
        r#"#!/bin/sh
case " $* " in
 *" clean "*) printf '%s\n' "$*" >> "$DEVBOX_UNDO_RECEIPT.clean";;
 *" worktree remove "*)
   for last do :; done
   if [ -e "$last/ignored.log" ]; then state=kept; else state=gone; fi
   printf '%s\n' "$state" >> "$DEVBOX_UNDO_RECEIPT.remove";;
esac
exec "$DEVBOX_UNDO_REAL_GIT" "$@"
"#,
    )
    .expect("owned git wrapper");
    for name in ["bash", "git"] {
        fs::set_permissions(root.path().join(name), fs::Permissions::from_mode(0o700))
            .expect("wrapper mode");
    }
    let path = std::env::join_paths(std::iter::once(root.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH")),
    ))
    .expect("owned PATH");
    let mut failures = vec![];
    for mode in modes {
        let home = root.path().join(format!("home-{mode}"));
        fs::create_dir(&home).expect("owned home");
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                &format!(
                    "service::resource_tests::action_review_tests::teardown_undo_tests::{test}"
                ),
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("PATH", &path)
            .env("HOME", &home)
            .env("DEVBOX_UNDO_REAL_GIT", &real_git)
            .env(
                "DEVBOX_UNDO_RECEIPT",
                root.path().join(format!("receipt-{mode}")),
            )
            .output()
            .expect("owned child");
        if !output.status.success() {
            failures.push(format!(
                "{mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What a wrapper noted for this child, one line per call; empty when it was never called.
fn receipt(kind: &str) -> Vec<String> {
    let mut path = std::env::var_os("DEVBOX_UNDO_RECEIPT").expect("receipt path");
    path.push(format!(".{kind}"));
    fs::read_to_string(path)
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Writes the project's teardown, commits it on the base branch and approves it, as a user would.
/// The first command leaves a marker outside the work folder, so a test can tell it ran.
async fn approve_teardown(f: &Fixture, task: &AgentTask, wt: &Path, commands: &[&str]) -> PathBuf {
    let marker = f.root.path().join("teardown-ran");
    let mut list = vec![format!(
        "printf ran > {}",
        crate::plan::sh_quote(marker.to_str().expect("owned marker"))
    )];
    list.extend(commands.iter().map(|command| (*command).to_owned()));
    let list: Vec<_> = list
        .iter()
        .map(|command| serde_json::to_string(command).expect("TOML string"))
        .collect();
    fs::create_dir_all(f.repo.join(".devbox")).expect("owned config directory");
    fs::write(
        f.repo.join(".devbox/devbox.toml"),
        format!("[agent]\nteardown=[{}]\n", list.join(",")),
    )
    .expect("owned config");
    git(&f.repo, &["add", ".devbox/devbox.toml"]);
    git(&f.repo, &["commit", "-qm", "owned teardown"]);
    let preview = f
        .svc
        .projects
        .trust_preview(
            devbox_projects::TrustPreviewParams {
                id: f.project.clone(),
                operation: devbox_projects::TrustOperation::Cleanup,
                agent_id: Some(task.id.clone()),
                worktrees: vec![],
                worktree: None,
            },
            true,
            vec![wt.to_string_lossy().into_owned()],
        )
        .await
        .expect("owned preview");
    f.svc
        .projects
        .trust_approve(devbox_projects::TrustApprove {
            id: f.project.clone(),
            expected_canonical_path: preview.canonical_path,
            expected_digest: preview.digest.expect("config digest"),
            expected_scope_version: preview.scope_version,
        })
        .await
        .expect("explicit owned command consent");
    marker
}

/// Closes the task by merge. With cleanup on, automatic cleanup runs before this returns.
async fn close(f: &Fixture, task: &AgentTask, strategy: MergeKind) -> AgentTask {
    let review = expect(f, task).await;
    f.svc
        .merge(MergeAgent {
            id: task.id.clone(),
            strategy,
            message: Some("owned change".into()),
            expect: review,
        })
        .await
        .expect("closed by merge")
}

async fn has_branch(f: &Fixture) -> bool {
    devbox_git::branches(&f.repo)
        .await
        .expect("branches")
        .contains(&"agent/review".into())
}

/// The code of the note a closed task keeps of what cleanup left, and its detail.
fn note_of(task: &AgentTask) -> serde_json::Value {
    serde_json::from_str(task.cleanup_error.as_deref().expect("a cleanup note"))
        .expect("the note is an error's JSON")
}

fn set_cleanup(f: &mut Fixture, on: bool) {
    Arc::get_mut(&mut f.svc)
        .expect("owned service")
        .cleanup_after_close = on;
}

async fn case(mode: &str) {
    let (mut f, task, wt) = action_fixture().await;
    match mode {
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
        "submodule" | "submodule-edit" => {
            set_cleanup(&mut f, false);
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
                &wt,
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
            git(&wt, &["commit", "-qm", "submodule"]);
            // A commit that exists only in the work folder's own submodule store
            // (.git/worktrees/<name>/modules/lib), which a forced removal would lose.
            let sub = wt.join("lib");
            git(&sub, &["config", "user.name", "fixture"]);
            git(&sub, &["config", "user.email", "fixture@example.invalid"]);
            git(&sub, &["config", "commit.gpgSign", "false"]);
            fs::write(sub.join("file.txt"), "unpushed\n").expect("submodule edit");
            git(&sub, &["commit", "-qam", "unpushed"]);
            git(&wt, &["add", "lib"]);
            git(&wt, &["commit", "-qm", "bump lib"]);
            let unpushed = devbox_git::run::resolve(&sub, "HEAD")
                .await
                .expect("submodule head");
            close(&f, &task, MergeKind::Merge).await;
            // `git restore` would reset the submodule and detach its HEAD under this setting
            // unless it is told not to recurse.
            git(&f.repo, &["config", "submodule.recurse", "true"]);
            let teardown = if mode == "submodule" {
                "printf x > teardown.log"
            } else {
                "printf x > teardown.log && printf teardown >> lib/file.txt"
            };
            let ran = approve_teardown(&f, &task, &wt, &[teardown]).await;
            let result = f
                .svc
                .cleanup(CleanupAgents {
                    project_id: f.project.clone(),
                    worktrees: vec![wt.to_string_lossy().into_owned()],
                    branches: vec!["agent/review".into()],
                })
                .await;
            assert!(ran.exists(), "the teardown ran");
            if mode == "submodule" {
                // Git itself refuses a folder with an initialized submodule without force.
                assert!(result.is_err(), "{result:?}");
            } else {
                assert!(
                    matches!(&result, Err(Fail::Domain(AgentsError::WorktreeDirty { files })) if files == &["lib"]),
                    "{result:?}"
                );
            }
            assert!(wt.exists());
            assert!(has_branch(&f).await);
            let expected = if mode == "submodule" {
                "unpushed\n"
            } else {
                "unpushed\nteardown"
            };
            assert_eq!(
                fs::read_to_string(sub.join("file.txt")).expect("submodule file"),
                expected
            );
            assert_eq!(
                devbox_git::run::resolve(&sub, "HEAD")
                    .await
                    .expect("submodule head"),
                unpushed
            );
            assert_eq!(
                devbox_git::current_branch(&sub)
                    .await
                    .expect("submodule branch")
                    .as_deref(),
                Some("main")
            );
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
        other => panic!("unknown mode {other}"),
    }
}
