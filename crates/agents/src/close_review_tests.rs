use super::*;
use std::process::{Child, Stdio};

#[path = "rebase_review_tests.rs"]
mod rebase_review_tests;

#[test]
fn review_close_without_git_cleanup_stops_owned_terminal_and_scope() {
    cases(&["merge", "pr"]);
}

#[test]
fn review_close_stop_failure_prevents_merge_or_pr() {
    cases(&["merge-stop-fails", "pr-stop-fails"]);
}

#[test]
fn review_close_recovers_already_ended_owned_terminal_and_scope() {
    cases(&["merge-ended", "pr-ended"]);
}

#[test]
fn review_close_rechecks_confirmed_state_after_quiesce() {
    cases(&[
        "merge-late-head",
        "pr-late-head",
        "merge-late-dirty",
        "pr-late-dirty",
    ]);
}

async fn close_case(mode: &str) {
    if mode.starts_with("rebase") {
        rebase_review_tests::case(mode).await;
        return;
    }
    let (mut fixture, task, wt) = action_fixture().await;
    let svc = Arc::get_mut(&mut fixture.svc).expect("owned service");
    svc.cleanup_after_close = false;
    svc.use_scope = true;
    fs::write(
        fixture.svc.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned tmux configuration");
    fs::write(
        std::env::var_os("DEVBOX_CLOSE_TMUX_OWNER").expect("owner path"),
        fixture.svc.terminal.tmux().socket(),
    )
    .expect("owned socket receipt");
    let terminal = fixture
        .svc
        .terminal
        .create_internal(
            devbox_terminal::SessionKind::Agent,
            Some(fixture.project.clone()),
            wt.clone(),
            "Owned close fixture".into(),
            "cat".into(),
            &[],
        )
        .await
        .expect("owned agent terminal");
    let terminal_id = terminal.id.clone();
    fixture
        .svc
        .update(&task.id, move |task| {
            task.terminal_id = Some(terminal_id);
            true
        })
        .await
        .expect("attach owned terminal");
    if mode.ends_with("ended") {
        fixture
            .svc
            .terminal
            .close_internal(&terminal.id)
            .await
            .expect("owned session already ended");
    }
    let before_base = devbox_git::run::resolve(&fixture.repo, "main")
        .await
        .expect("before base");
    fs::write(
        std::env::var_os("DEVBOX_CLOSE_WORKTREE_RECEIPT").expect("worktree receipt"),
        wt.to_str().expect("owned worktree"),
    )
    .expect("owned worktree receipt");
    let remote = fixture.root.path().join("remote.git");
    git(
        &fixture.repo,
        &["init", "--bare", remote.to_str().expect("remote")],
    );
    git(
        &fixture.repo,
        &["remote", "add", "origin", remote.to_str().expect("remote")],
    );
    let confirmed = expect(&fixture, &task).await;
    let result = if mode.starts_with("merge") {
        fixture
            .svc
            .merge(MergeAgent {
                id: task.id.clone(),
                strategy: MergeKind::Squash,
                message: Some("owned squash".into()),
                expect: confirmed,
            })
            .await
    } else {
        fixture
            .svc
            .pr(PrAgent {
                id: task.id.clone(),
                title: None,
                body: None,
                expect: confirmed,
            })
            .await
    };
    if mode.ends_with("fails") || mode.contains("-late-") {
        assert!(
            result.is_err(),
            "failed owned scope stop must block closure: {result:?}"
        );
        assert_ne!(
            fixture.svc.get(&task.id).await.expect("task").state,
            AgentState::Closed
        );
        assert_eq!(
            devbox_git::run::resolve(&fixture.repo, "main")
                .await
                .expect("base"),
            before_base,
            "quiesce failure may not first mutate the merge target"
        );
        assert!(
            devbox_git::branches(&remote)
                .await
                .expect("remote branches")
                .is_empty(),
            "quiesce failure may not first push"
        );
        assert!(
            !Path::new(&std::env::var_os("DEVBOX_CLOSE_GH_MARKER").expect("marker")).exists(),
            "quiesce failure may not first create a PR"
        );
        if mode.ends_with("head") {
            assert!(
                matches!(result, Err(Fail::Domain(AgentsError::StateChanged { .. }))),
                "late head must require renewed review"
            );
        } else if mode.ends_with("dirty") {
            assert!(
                matches!(result, Err(Fail::Domain(AgentsError::WorktreeDirty { .. }))),
                "late uncommitted content must be retained"
            );
        }
    } else {
        let closed = result.expect("successful close");
        assert_eq!(closed.state, AgentState::Closed);
        assert!(wt.exists(), "cleanup=false preserves owned worktree");
        assert!(
            devbox_git::branches(&fixture.repo)
                .await
                .expect("branches")
                .contains(&"agent/review".into())
        );
        assert_eq!(reservation(&fixture, &task).await, Resources::Owned);
        assert!(
            !fixture
                .svc
                .terminal
                .tmux()
                .list()
                .await
                .expect("sessions")
                .iter()
                .any(|item| item.name == terminal.id),
            "closed task may not keep its live agent terminal"
        );
        assert!(
            !fixture
                .svc
                .terminal
                .list(devbox_terminal::ListSessions {
                    project_id: Some(fixture.project.clone()),
                    after_id: None,
                })
                .await
                .expect("terminal records")
                .items
                .iter()
                .any(|item| item.id == terminal.id),
            "closed task releases its terminal record even with Git cleanup disabled"
        );
    }
    let stopped =
        fs::read_to_string(std::env::var_os("DEVBOX_CLOSE_SCOPE_MARKER").expect("scope marker"))
            .expect("scope stop invoked");
    assert_eq!(
        stopped.lines().collect::<Vec<_>>(),
        vec![format!(
            "devbox-{}-agent-{}.scope",
            fixture.svc.paths.instance.as_str(),
            task.id
        )],
        "close stops only its exact scope once"
    );
}

struct OwnedResources {
    scope: Child,
    tmux_owner: PathBuf,
}
impl Drop for OwnedResources {
    fn drop(&mut self) {
        let _ = self.scope.kill();
        let _ = self.scope.wait();
        if let Ok(socket) = fs::read_to_string(&self.tmux_owner) {
            assert!(socket.starts_with("devbox-test-resource-"));
            let _ = Command::new("tmux")
                .args(["-L", socket.trim(), "kill-server"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}
fn cases(modes: &[&str]) {
    const CHILD: &str = "DEVBOX_CLOSE_CASE";
    if let Ok(mode) = std::env::var(CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(close_case(&mode));
        return;
    }
    let root = tempfile::tempdir().expect("owned executables");
    fs::write(
        root.path().join("systemctl"),
        r#"#!/bin/sh
case "$2" in
 stop) printf '%s\n' "$3" >> "$DEVBOX_CLOSE_SCOPE_MARKER"
   case "$DEVBOX_CLOSE_CASE" in *fails|*ended) exit 23;; esac
   kill -TERM "$DEVBOX_CLOSE_SCOPE_PID" || exit $?
   case "$DEVBOX_CLOSE_CASE" in
    *late-head) wt=$(cat "$DEVBOX_CLOSE_WORKTREE_RECEIPT")
      printf 'late committed edit\n' > "$wt/tracked"
      git -C "$wt" add tracked && git -C "$wt" commit -qm 'late fixture commit';;
    *late-dirty) wt=$(cat "$DEVBOX_CLOSE_WORKTREE_RECEIPT")
      printf 'late uncommitted edit\n' > "$wt/tracked";;
   esac;;
 show) case "$DEVBOX_CLOSE_CASE" in *ended) printf 'not-found\n';; *) printf 'loaded\n';; esac;;
 *) exit 24;;
esac
"#,
    )
    .expect("owned systemctl wrapper");
    fs::write(
        root.path().join("gh"),
        r#"#!/bin/sh
case "$2" in
 list) exit 0;;
 create) : > "$DEVBOX_CLOSE_GH_MARKER"; printf 'https://example.invalid/pr/1\n';;
 *) exit 25;;
esac
"#,
    )
    .expect("owned gh wrapper");
    for name in ["systemctl", "gh"] {
        fs::set_permissions(root.path().join(name), fs::Permissions::from_mode(0o700))
            .expect("wrapper mode");
    }
    let path = std::env::join_paths(std::iter::once(root.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH")),
    ))
    .expect("owned PATH");
    let test = if modes[0].starts_with("rebase") {
        if modes[0].ends_with("fails") {
            "rebase_review_tests::rebase_stop_failure_blocks_git_mutation"
        } else if modes[0].contains("-late-") {
            "rebase_review_tests::rebase_revalidates_committed_head_and_dirty_content_after_stop"
        } else {
            "rebase_review_tests::rebase_stops_owned_scope_and_retains_review_for_success_and_conflict"
        }
    } else if modes[0].ends_with("ended") {
        "review_close_recovers_already_ended_owned_terminal_and_scope"
    } else if modes[0].contains("-late-") {
        "review_close_rechecks_confirmed_state_after_quiesce"
    } else if modes[0].ends_with("fails") {
        "review_close_stop_failure_prevents_merge_or_pr"
    } else {
        "review_close_without_git_cleanup_stops_owned_terminal_and_scope"
    };
    let mut failures = vec![];
    for mode in modes {
        let ready = root.path().join(format!("ready-{mode}"));
        let scope = Command::new("bash")
            .args([
                "-c",
                "trap '' HUP\nprintf ready > \"$1\"\nexec sleep 600",
                "--",
            ])
            .arg(&ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("owned SIGHUP-ignoring process");
        let pid = scope.id();
        let mut owner = OwnedResources {
            scope,
            tmux_owner: root.path().join(format!("tmux-{mode}")),
        };
        let started = std::time::Instant::now();
        while !ready.exists() {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(2),
                "scope readiness"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if mode.ends_with("ended") {
            owner.scope.kill().expect("owned scope already ended");
            owner.scope.wait().expect("reap already ended owned scope");
        }
        let marker = root.path().join(format!("scope-{mode}"));
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                &format!(
                    "service::resource_tests::action_review_tests::close_review_tests::{test}"
                ),
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("PATH", &path)
            .env("DEVBOX_CLOSE_SCOPE_PID", pid.to_string())
            .env("DEVBOX_CLOSE_SCOPE_MARKER", marker)
            .env("DEVBOX_CLOSE_TMUX_OWNER", &owner.tmux_owner)
            .env(
                "DEVBOX_CLOSE_WORKTREE_RECEIPT",
                root.path().join(format!("worktree-{mode}")),
            )
            .env(
                "DEVBOX_CLOSE_GH_MARKER",
                root.path().join(format!("gh-{mode}")),
            )
            .output()
            .expect("owned child");
        if !output.status.success() {
            failures.push(format!(
                "{mode}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        } else if !mode.ends_with("fails") {
            assert!(
                owner.scope.try_wait().expect("scope state").is_some(),
                "successful close must stop its SIGHUP-ignoring process"
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
