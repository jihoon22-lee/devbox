use super::*;

#[test]
fn teardown_lists_reject_before_discard_merge_or_pr_side_effects() {
    cases(&[
        "teardown-discard-large-fails",
        "teardown-discard-nul-fails",
        "teardown-merge-large-fails",
        "teardown-merge-nul-fails",
        "teardown-pr-large-fails",
        "teardown-pr-nul-fails",
    ]);
}
#[test]
fn teardown_lists_preflight_runner_and_entire_cleanup_selection() {
    cases(&[
        "teardown-cleanup-large-fails",
        "teardown-cleanup-nul-fails",
        "teardown-runner-large-fails",
        "teardown-runner-nul-fails",
    ]);
}
/// What a discard's own teardown writes into the work folder goes with the folder: a lock file
/// made and removed again, a tracked edit or a new file.
#[test]
fn discard_drops_what_its_own_teardown_wrote() {
    cases(&[
        "teardown-discard-scratch",
        "teardown-discard-edited",
        "teardown-discard-wrote",
    ]);
}
/// A change made after the user confirmed the discard and before its teardown runs (here, while
/// the agent stops) is still refused, before the teardown: an edit, a commit that changes no file
/// and so leaves the folder's listing and stamp as they were, or a submodule store made outside
/// the folder, which the confirmation warned of or not. So is a teardown that moves HEAD, or the
/// base branch, which the UI's confirmation pins only through the snapshot.
#[test]
fn discard_still_refuses_a_change_before_its_teardown_and_a_moved_head() {
    cases(&[
        "teardown-discard-late-dirty",
        "teardown-discard-late-commit",
        "teardown-discard-late-submodules",
        "teardown-discard-moved",
        "teardown-discard-base-moved",
    ]);
}
#[test]
fn teardown_bounds_preserve_snapshot_bytes_and_unused_paths() {
    cases(&[
        "teardown-discard-ascii",
        "teardown-discard-unicode",
        "teardown-merge-unused",
        "teardown-pr-unused",
        "teardown-cleanup-branch-unused",
    ]);
}

pub(super) async fn case(mode: &str) {
    let (mut f, task, wt) = action_fixture().await;
    let svc = Arc::get_mut(&mut f.svc).expect("owned service");
    svc.use_scope = true;
    svc.cleanup_after_close = !mode.ends_with("unused");
    let first = f.root.path().join("first-ran");
    let second = f.root.path().join("second-ran");
    let sentinel = format!(
        "printf first > {}",
        crate::plan::sh_quote(first.to_str().expect("owned sentinel"))
    );
    let command = if mode.ends_with("nul-fails") {
        "true\0bad".into()
    } else if [
        "scratch",
        "edited",
        "wrote",
        "late-dirty",
        "late-commit",
        "late-submodules",
        "moved",
    ]
    .iter()
    .any(|end| mode.ends_with(end))
    {
        // Teardown runs in the worktree: a lock file made and removed again, a tracked edit, a new
        // file, nothing (the change is made while the agent stops), a commit on the base branch
        // with the same files, or a commit.
        let change = if mode.ends_with("scratch") {
            "touch teardown.lock && rm teardown.lock"
        } else if mode.ends_with("edited") {
            "printf edited > tracked"
        } else if mode.ends_with("wrote") {
            "printf x > teardown.log"
        } else if mode.contains("-late-") {
            "true"
        } else if mode.ends_with("base-moved") {
            "git update-ref refs/heads/main \"$(git commit-tree -p main -m moved 'main^{tree}')\""
        } else {
            "git commit --allow-empty -qm moved"
        };
        format!(
            "{change} && printf second > {}",
            crate::plan::sh_quote(second.to_str().expect("owned marker"))
        )
    } else if mode.ends_with("ascii") || mode.ends_with("unicode") {
        let prefix = format!(
            "printf second > {}; #",
            crate::plan::sh_quote(second.to_str().expect("owned marker"))
        );
        let mut command = prefix.clone();
        if mode.ends_with("unicode") {
            command.push_str(&"😀".repeat((65536 - prefix.len()) / 4));
        }
        command.push_str(&"x".repeat(65536 - command.len()));
        command
    } else {
        format!("#{}", "x".repeat(140 * 1024))
    };
    fs::create_dir(f.repo.join(".devbox")).expect("owned config directory");
    fs::write(
        f.repo.join(".devbox/devbox.toml"),
        format!(
            "[agent]\nteardown=[{},{}]\n",
            serde_json::to_string(&sentinel).expect("quoted sentinel"),
            serde_json::to_string(&command).expect("quoted command")
        ),
    )
    .expect("owned config");
    git(&f.repo, &["add", ".devbox/devbox.toml"]);
    git(&f.repo, &["commit", "-qm", "owned teardown configuration"]);
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
    if !mode.ends_with("unused") {
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
    }
    if mode.starts_with("teardown-cleanup-") {
        let confirmed = expect(&f, &task).await;
        let review = f
            .svc
            .capture_review(
                &task,
                &f.repo,
                &confirmed.head_oid,
                confirmed.base_oid.as_deref().expect("base"),
            )
            .await
            .expect("review");
        git(&f.repo, &["merge", "--no-ff", "--no-edit", "agent/review"]);
        let merged = devbox_git::run::resolve(&f.repo, "main")
            .await
            .expect("merge commit");
        f.svc
            .close_record(
                &task.id,
                crate::closed_review::Closure::Merged { result: merged },
                confirmed.head_oid,
                f.svc.closure_usage(&task.id).await,
                review,
            )
            .await
            .expect("closed owned task");
        let mut absent = f.svc.get(&task.id).await.expect("closed");
        absent.id = "a-absent".into();
        absent.branch = Some("agent/absent".into());
        absent.worktree_path = Some(
            crate::plan::worktree_dir(&f.repo, "absent")
                .expect("owned absent path")
                .to_string_lossy()
                .into_owned(),
        );
        absent.workdir = absent.worktree_path.clone().expect("path");
        absent.terminal_id = None;
        absent.closed_head_oid = Some(
            devbox_git::run::resolve(&f.repo, "main")
                .await
                .expect("absent branch head"),
        );
        absent.updated_ms += 1;
        git(&f.repo, &["branch", "agent/absent", "main"]);
        f.svc
            .db
            .write(move |tx| {
                crate::store::put(tx, &absent)?;
                let mut binding = Binding::new(0);
                binding.resources = Resources::Owned;
                crate::store::put_binding(tx, &absent.id, &binding)
            })
            .await
            .expect("earlier branch-only resource");
    }
    fs::write(
        std::env::var_os("DEVBOX_CLOSE_WORKTREE_RECEIPT").expect("worktree receipt"),
        wt.to_str().expect("owned worktree"),
    )
    .expect("owned worktree receipt");
    fs::write(
        f.svc.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned configuration");
    fs::write(
        std::env::var_os("DEVBOX_CLOSE_TMUX_OWNER").expect("owned socket receipt"),
        f.svc.terminal.tmux().socket(),
    )
    .expect("socket receipt");
    let terminal = f
        .svc
        .terminal
        .create_internal(
            devbox_terminal::SessionKind::Agent,
            Some(f.project.clone()),
            wt.clone(),
            "Owned teardown fixture".into(),
            "cat".into(),
            &[],
        )
        .await
        .expect("owned terminal");
    let terminal_id = terminal.id.clone();
    f.svc
        .update(&task.id, move |t| {
            t.terminal_id = Some(terminal_id);
            true
        })
        .await
        .expect("owned holder");
    let before = f.svc.get(&task.id).await.expect("task");
    let runtime = f.svc.launch_binding(&task.id).await.expect("runtime");
    let base = devbox_git::run::resolve(&f.repo, "main")
        .await
        .expect("base");
    let head = devbox_git::run::resolve(&wt, "HEAD").await.expect("head");
    let index = fs::read(f.repo.join(".git/index")).expect("index");
    let content = fs::read(wt.join("tracked")).expect("content");
    let remote = f.root.path().join("remote.git");
    git(
        &f.repo,
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    git(
        &f.repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );
    let confirmation = expect(&f, &task).await;
    let check = f
        .svc
        .discard_check(&task.id)
        .await
        .expect("discard confirmation");
    let before_rev = f.svc.db.rev();
    let result: crate::service::Result<()> = if mode.starts_with("teardown-cleanup-") {
        f.svc
            .cleanup(CleanupAgents {
                project_id: f.project.clone(),
                worktrees: if mode.ends_with("unused") {
                    vec![]
                } else {
                    vec![wt.to_string_lossy().into_owned()]
                },
                branches: if mode.ends_with("unused") {
                    vec!["agent/absent".into()]
                } else {
                    vec!["agent/absent".into(), "agent/review".into()]
                },
            })
            .await
            .map(|_| ())
    } else if mode.starts_with("teardown-runner-") {
        let snapshot = f
            .svc
            .projects
            .teardown_snapshot(&f.project)
            .await
            .expect("approved immutable capability");
        f.svc.teardown_commands(&before, &snapshot).await
    } else if mode.starts_with("teardown-discard-") {
        f.svc
            .discard(DiscardAgent {
                id: task.id.clone(),
                expect: ReviewExpect {
                    head_oid: check.head_oid,
                    // As the UI's discard confirmation sends it: no base.
                    base_oid: if mode.ends_with("base-moved") {
                        None
                    } else {
                        confirmation.base_oid
                    },
                    unmerged_commits: Some(check.unmerged_commits),
                    discard_fingerprint: check.fingerprint,
                },
            })
            .await
            .map(|_| ())
    } else if mode.starts_with("teardown-merge-") {
        f.svc
            .merge(MergeAgent {
                id: task.id.clone(),
                strategy: MergeKind::Squash,
                message: None,
                expect: confirmation,
            })
            .await
            .map(|_| ())
    } else {
        f.svc
            .pr(PrAgent {
                id: task.id.clone(),
                title: None,
                body: None,
                expect: confirmation,
            })
            .await
            .map(|_| ())
    };
    if mode.starts_with("teardown-discard-late-") || mode.ends_with("moved") {
        assert!(
            matches!(result, Err(Fail::Domain(AgentsError::StateChanged { .. }))),
            "{result:?}"
        );
        assert!(wt.exists());
        assert_ne!(
            f.svc.get(&task.id).await.expect("task").state,
            AgentState::Closed
        );
        if mode.starts_with("teardown-discard-late-") {
            // Refused right after the agent stopped: the teardown never ran.
            assert!(!first.exists() && !second.exists());
        } else {
            assert!(first.exists() && second.exists());
        }
        if mode.ends_with("late-dirty") {
            assert_eq!(
                fs::read(wt.join("tracked")).expect("kept edit"),
                b"late uncommitted edit\n"
            );
        }
        return;
    }
    if mode.ends_with("fails") {
        assert!(
            !first.exists(),
            "all commands must be checked before the first sentinel; result={result:?}"
        );
        if mode.ends_with("nul-fails") {
            assert!(
                matches!(
                    result,
                    Err(Fail::Domain(AgentsError::TeardownCommandInvalid))
                ),
                "typed NUL rejection: {result:?}"
            );
        } else {
            assert!(
                matches!(
                    result,
                    Err(Fail::Domain(AgentsError::TeardownCommandTooLong {
                        limit_bytes: 65536
                    }))
                ),
                "typed UTF8 byte limit: {result:?}"
            );
        }
        assert_eq!(f.svc.db.rev(), before_rev);
        assert_eq!(
            serde_json::to_value(f.svc.get(&task.id).await.expect("task")).expect("after"),
            serde_json::to_value(before).expect("before")
        );
        assert_eq!(
            f.svc.launch_binding(&task.id).await.expect("runtime"),
            runtime
        );
        assert!(
            f.svc
                .terminal
                .tmux()
                .list()
                .await
                .expect("sessions")
                .iter()
                .any(|row| row.name == terminal.id)
        );
        assert_eq!(
            devbox_git::run::resolve(&f.repo, "main")
                .await
                .expect("base"),
            base
        );
        assert_eq!(
            devbox_git::run::resolve(&wt, "HEAD").await.expect("head"),
            head
        );
        assert_eq!(fs::read(f.repo.join(".git/index")).expect("index"), index);
        assert_eq!(fs::read(wt.join("tracked")).expect("content"), content);
        assert_eq!(reservation(&f, &task).await, Resources::Owned);
        assert!(
            devbox_git::branches(&remote)
                .await
                .expect("remote")
                .is_empty()
        );
        if mode.starts_with("teardown-cleanup-") {
            assert!(
                devbox_git::branches(&f.repo)
                    .await
                    .expect("refs")
                    .contains(&"agent/absent".into()),
                "whole-selection preflight must preserve earlier branch-only resource"
            );
        }
        assert!(
            !Path::new(&std::env::var_os("DEVBOX_CLOSE_SCOPE_MARKER").expect("scope marker"))
                .exists()
        );
        assert!(
            !Path::new(&std::env::var_os("DEVBOX_TEARDOWN_RECEIPT").expect("argv receipt"))
                .exists()
        );
    } else {
        result.expect("valid bounded command or command-free operation");
        if mode.ends_with("unused") {
            assert!(!first.exists());
            assert!(
                !Path::new(&std::env::var_os("DEVBOX_TEARDOWN_RECEIPT").expect("receipt")).exists()
            );
            if mode == "teardown-cleanup-branch-unused" {
                assert!(wt.exists());
                assert!(
                    !devbox_git::branches(&f.repo)
                        .await
                        .expect("refs")
                        .contains(&"agent/absent".into())
                );
                assert_eq!(
                    f.svc.get(&task.id).await.expect("unselected").terminal_id,
                    Some(terminal.id)
                );
            } else {
                assert_eq!(
                    f.svc.get(&task.id).await.expect("closed").state,
                    AgentState::Closed
                );
                assert!(wt.exists());
            }
        } else {
            assert!(first.exists());
            assert!(second.exists());
            assert!(!wt.exists());
            let receipt: Vec<String> = serde_json::from_str(
                &fs::read_to_string(std::env::var_os("DEVBOX_TEARDOWN_RECEIPT").expect("receipt"))
                    .expect("argv receipt"),
            )
            .expect("commands");
            assert_eq!(
                receipt,
                vec![sentinel, command],
                "approved command bytes remain exact"
            );
        }
    }
}
