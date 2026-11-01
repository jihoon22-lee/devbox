//! Cases of a closed task's cleanup, each run alone in a child process of the test executable.
//! The child's PATH starts with owned wrappers:
//! - `bash` without profile files, since the teardown runs `bash -lc`, which would read the user's;
//! - a transparent `git` that notes the arguments of each `git clean` and, when a removal starts,
//!   whether the fixture's ignored file is still in the folder. In a `ref-lock` mode it also fails
//!   the first deletion of `refs/heads/agent/review`, as a lock another Git process holds would,
//!   whatever the ref backend.
//!
//! The child's Git reads no user or system configuration: HOME is an owned folder,
//! `GIT_CONFIG_GLOBAL` is empty and `GIT_CONFIG_NOSYSTEM` is set. Its repositories use the `files`
//! ref backend, or `reftable` in a mode whose name says so. A `reftable` mode is skipped, with the
//! reason printed, where the installed Git cannot make such repositories; every Git makes `files`
//! ones, so those modes always run, and a run in which no mode ran fails.
use super::*;

const CHILD: &str = "DEVBOX_UNDO_CASE";

/// Runs `case` with each mode, each in its own child. `test` is the path of the test below
/// `action_review_tests`, so the child runs only that test.
pub(super) fn cases<F: std::future::Future<Output = ()>>(
    test: &str,
    modes: &[&str],
    case: impl FnOnce(String) -> F,
) {
    if let Ok(mode) = std::env::var(CHILD) {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("owned runtime")
            .block_on(case(mode));
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
 *" update-ref --no-deref -d refs/heads/agent/review "*)
   case "$DEVBOX_UNDO_CASE" in
    ref-lock*) if [ ! -e "$DEVBOX_UNDO_RECEIPT.locked" ]; then
        printf 'refused\n' > "$DEVBOX_UNDO_RECEIPT.locked"
        printf '%s\n' "fatal: cannot lock ref 'refs/heads/agent/review'" >&2
        exit 128
      fi;;
   esac;;
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
    let mut ran = 0;
    for mode in modes {
        let format = if mode.contains("reftable") {
            "reftable"
        } else {
            "files"
        };
        if format == "reftable"
            && let Err(reason) = reftable(&real_git, root.path())
        {
            println!(
                "{mode}: skipped, the installed Git cannot make reftable repositories: {reason}"
            );
            continue;
        }
        ran += 1;
        let home = root.path().join(format!("home-{mode}"));
        fs::create_dir(&home).expect("owned home");
        let output = Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                &format!("service::resource_tests::action_review_tests::{test}"),
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("PATH", &path)
            .env("HOME", &home)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_DEFAULT_REF_FORMAT", format)
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
    assert!(ran > 0, "no mode of {test} ran");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Whether the installed Git makes a repository with the `reftable` ref backend.
fn reftable(git: &str, root: &Path) -> std::result::Result<(), String> {
    let probe = root.join("probe-reftable");
    let output = Command::new(git)
        .args(["init", "-q", "--ref-format=reftable"])
        .arg(&probe)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// The ref backend this child's repositories use.
pub(super) fn ref_format() -> String {
    std::env::var("GIT_DEFAULT_REF_FORMAT").expect("the harness sets the backend")
}

/// What a wrapper noted for this child, one line per call; empty when it was never called.
pub(super) fn receipt(kind: &str) -> Vec<String> {
    let mut path = std::env::var_os("DEVBOX_UNDO_RECEIPT").expect("receipt path");
    path.push(format!(".{kind}"));
    fs::read_to_string(path)
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Writes the project's teardown, commits it on the base branch and approves it, as a user would.
/// The first command leaves a marker outside the work folder, so a test can tell it ran.
pub(super) async fn approve_teardown(
    f: &Fixture,
    task: &AgentTask,
    wt: &Path,
    commands: &[&str],
) -> PathBuf {
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
pub(super) async fn close(f: &Fixture, task: &AgentTask, strategy: MergeKind) -> AgentTask {
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

pub(super) async fn has_branch(f: &Fixture) -> bool {
    devbox_git::branches(&f.repo)
        .await
        .expect("branches")
        .contains(&"agent/review".into())
}

/// The code of the note a closed task keeps of what cleanup left, and its detail.
pub(super) fn note_of(task: &AgentTask) -> serde_json::Value {
    serde_json::from_str(task.cleanup_error.as_deref().expect("a cleanup note"))
        .expect("the note is an error's JSON")
}

pub(super) fn set_cleanup(f: &mut Fixture, on: bool) {
    Arc::get_mut(&mut f.svc)
        .expect("owned service")
        .cleanup_after_close = on;
}

pub(super) async fn listed(f: &Fixture) -> CleanupOptions {
    f.svc
        .cleanup_candidates(CleanupCandidates {
            project_id: f.project.clone(),
        })
        .await
        .expect("candidates")
}

/// Everything the list offers, selected.
pub(super) fn offered(f: &Fixture, options: &CleanupOptions) -> CleanupAgents {
    CleanupAgents {
        project_id: f.project.clone(),
        worktrees: options.worktrees.clone(),
        branches: options.branches.clone(),
    }
}
