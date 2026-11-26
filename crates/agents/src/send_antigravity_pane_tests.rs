//! An Antigravity task through a resume and sends, with the fake `tmux` of the parent module:
//! `capture-pane` prints `$live.screen` and every call's tmux command goes to `$live.calls`. The
//! screens and payloads are real ones of agy 1.5.17, redacted
//! (`tests/fixtures/antigravity-1.5.17/README.md`).
use super::*;
use crate::tracker::Resources;

const CONVERSATION: &str = "00000000-0000-4000-8000-000000000001";
/// The fixture's content: what follows its `#` provenance lines and the `---` line.
fn fixture(text: &'static str) -> &'static str {
    text.split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}
fn pre_invocation() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-pre-invocation.json"
    ))
}
fn stop() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-stop.json"
    ))
}

macro_rules! capture {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!(
                "../tests/fixtures/antigravity-1.5.17/",
                $name,
                ".txt"
            )),
        )
    };
}

pub(super) fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(["-c", "commit.gpgSign=false"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Task `a` of the builtin Antigravity profile in a work folder of its own, whose first launch
/// ended. That launch made the hooks folder beside it, for a devbox at another path. Also the
/// work folder.
pub(super) async fn ended(session: Option<&str>) -> (Ended, PathBuf) {
    let f = Ended::new("antigravity", session).await;
    let repo = f.root.path().join("project");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "fixture"]);
    let wt = crate::plan::worktree_dir(&repo, "fix").expect("owned path");
    let path = wt.to_str().expect("UTF-8").to_owned();
    git(
        &repo,
        &["worktree", "add", "-q", "-b", "agent/fix", &path, "main"],
    );
    let folder = path.clone();
    f.svc
        .db
        .write(move |tx| {
            let mut task = crate::store::get(tx, "a")?.expect("owned task");
            task.branch = Some("agent/fix".into());
            task.worktree_path = Some(folder.clone());
            task.workdir = folder;
            crate::store::put(tx, &task)?;
            let mut binding = crate::store::binding(tx, "a")?.expect("owned binding");
            binding.resources = Resources::Owned;
            crate::store::put_binding(tx, "a", &binding)
        })
        .await
        .expect("owned work folder");
    crate::antigravity::write_hooks_dir(
        &crate::antigravity::hooks_dir(&path),
        "/old/devbox",
        "a",
        true,
    )
    .expect("first launch");
    (f, wt)
}

/// One of agy's hooks from the task's current launch.
pub(super) async fn hook(f: &Ended, payload: &str) {
    let launch = f.binding().await.launch_id;
    f.svc
        .event(AgentEventParams {
            agent_id: "a".into(),
            launch_id: launch,
            source: HookSource::Antigravity,
            payload: payload.to_owned(),
            code: None,
        })
        .await
        .expect("owned hook");
}

/// The tmux command of every call since the last `show`, in order.
pub(super) fn calls() -> Vec<String> {
    let live = std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake");
    let calls = fs::read_to_string(format!("{live}.calls")).unwrap_or_default();
    calls.lines().map(str::to_owned).collect()
}

/// The pane shows the fixture `shown` from now on, and the calls so far are forgotten.
pub(super) fn show(shown: &str) {
    let live = std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake");
    let pane = shown
        .split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1;
    fs::write(format!("{live}.screen"), pane).expect("owned screen");
    let _ = fs::remove_file(format!("{live}.calls"));
}

/// From now on, each paste leaves the pane showing the fixture `shown` (a fixture's header and
/// pane, or a pane alone), whatever it showed before.
pub(super) fn after_paste(shown: &str) {
    let live = std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake");
    let pane = shown.split_once("\n---\n").map_or(shown, |(_, pane)| pane);
    fs::write(format!("{live}.pasted"), pane).expect("owned screen after a paste");
}

/// The next paste makes the fake `tmux` fail `operation` from then on (see `fail_tmux`).
pub(super) fn fail_after_paste(operation: &str) {
    let live = std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake");
    fs::write(format!("{live}.pasted-fail"), operation).expect("owned fake failure");
}

#[test]
fn an_antigravity_resume_names_the_conversation_and_writes_its_hooks_folder_anew() {
    with_fake_tmux(
        "antigravity_pane::an_antigravity_resume_names_the_conversation_and_writes_its_hooks_folder_anew",
        || async {
            let (f, wt) = ended(Some(CONVERSATION)).await;
            let hooks = crate::antigravity::hooks_dir(wt.to_str().expect("UTF-8"));
            let resumed = f.svc.resume("a").await.expect("resume");
            assert_eq!(resumed.state, AgentState::Running);
            assert_eq!(resumed.session_id.as_deref(), Some(CONVERSATION));
            let script = f.script().await;
            let tool = format!(
                "\nagy --add-dir '{}' --conversation '{CONVERSATION}'\n",
                hooks.display()
            );
            assert!(script.contains(&tool), "{script}");
            assert!(
                script.contains(&format!("cd '{}' || ", wt.display())),
                "{script}"
            );
            assert!(
                !script.contains("$(cat") && !script.contains("--settings"),
                "{script}"
            );
            // The hooks call this daemon's devbox now.
            assert_eq!(
                fs::read_to_string(hooks.join(".agents/hooks.json")).expect("hooks"),
                crate::antigravity::hooks_json("/owned/devbox")
            );
            // agy continues the conversation without an instruction and reports nothing until one
            // is given: the launch waits for it.
            assert!(f.binding().await.resumed_session);
            // A report of the launch before is not this launch's.
            let stale = AgentEventParams {
                agent_id: "a".into(),
                launch_id: Some("first".into()),
                source: HookSource::Antigravity,
                payload: stop().into(),
                code: None,
            };
            f.svc.event(stale).await.expect("ignored");
            assert_eq!(f.task().await.state, AgentState::Running);
            // The user's first prompt in the resumed conversation, and its end.
            hook(&f, pre_invocation()).await;
            assert!(!f.binding().await.resumed_session);
            hook(&f, stop()).await;
            let idle = f.task().await;
            assert_eq!(idle.state, AgentState::Idle);
            assert_eq!(idle.session_id.as_deref(), Some(CONVERSATION));
            f.close().await;
        },
    );
}

#[test]
fn an_antigravity_resume_without_a_conversation_starts_agy_again_and_waits() {
    with_fake_tmux(
        "antigravity_pane::an_antigravity_resume_without_a_conversation_starts_agy_again_and_waits",
        || async {
            let (f, wt) = ended(None).await;
            let hooks = crate::antigravity::hooks_dir(wt.to_str().expect("UTF-8"));
            f.svc.resume("a").await.expect("resume");
            let script = f.script().await;
            assert!(
                script.contains(&format!("\nagy --add-dir '{}'\n", hooks.display())),
                "{script}"
            );
            assert!(
                !script.contains("--conversation") && !script.contains("$(cat"),
                "{script}"
            );
            // No prompt argument either way, so this launch waits for its first instruction too.
            assert!(f.binding().await.resumed_session);
            f.close().await;
        },
    );
}

#[test]
fn an_antigravity_task_in_the_projects_own_folder_is_not_resumed() {
    with_fake_tmux(
        "antigravity_pane::an_antigravity_task_in_the_projects_own_folder_is_not_resumed",
        || async {
            // A task that began under another profile of this id, without a work folder.
            let f = Ended::new("antigravity", Some(CONVERSATION)).await;
            let before = (f.task().await, f.binding().await);
            let refused = f.svc.resume("a").await;
            assert!(
                matches!(
                    &refused,
                    Err(devbox_protocol::Fail::Domain(AgentsError::WorkFolderRequired { tool }))
                        if tool == "Antigravity"
                ),
                "{refused:?}"
            );
            assert_eq!((f.task().await, f.binding().await), before);
            // Nothing was put beside the user's own checkout.
            assert!(!f.root.path().join("project.agy-hooks").exists());
        },
    );
}

#[test]
fn a_send_waits_at_every_antigravity_prompt_and_is_delivered_at_its_prompt_box() {
    with_fake_tmux(
        "antigravity_pane::a_send_waits_at_every_antigravity_prompt_and_is_delivered_at_its_prompt_box",
        || async {
            let (f, _) = ended(Some(CONVERSATION)).await;
            crate::watch::Watcher::new(&f.svc).expect("builtin waiting patterns");
            f.svc.resume("a").await.expect("resume");
            f.tool_started().await;
            // A turn ran and ended: the task is Idle, and the output watcher no longer reads its
            // pane, so the pane a send reads itself is all that keeps it out of a prompt.
            hook(&f, pre_invocation()).await;
            hook(&f, stop()).await;
            let idle = (f.task().await, f.binding().await);
            assert_eq!(idle.0.state, AgentState::Idle);
            assert!(f.svc.watched_tasks().await.expect("watched").is_empty());
            // At these a paste is dropped and Enter answers "Yes", or the paste becomes a shell
            // or slash command, or lands behind a queued prompt.
            for (name, shown) in [
                capture!("trust"),
                capture!("trust-default-statusline"),
                capture!("approval"),
                capture!("approval-six-options"),
                capture!("selection-model"),
                capture!("selection-hooks"),
                capture!("slash-menu"),
                capture!("slash-draft"),
                capture!("shell-mode"),
                capture!("shell-mode-draft"),
                capture!("queued"),
                capture!("login"),
            ] {
                show(shown);
                let refused = f.try_send().await;
                assert!(
                    matches!(
                        refused,
                        Err(devbox_protocol::Fail::Domain(AgentsError::AwaitingInput))
                    ),
                    "{name}: {refused:?}"
                );
                // The send read the pane and stopped there: nothing was pasted and no key sent.
                assert_eq!(calls(), ["list-sessions", "capture-pane"], "{name}");
                assert_eq!((f.task().await, f.binding().await), idle, "{name}");
            }
            // At the prompt box, empty or while agy works, the send goes in as a prompt.
            for (count, (name, shown)) in [
                capture!("idle"),
                capture!("idle-default-statusline"),
                capture!("idle-resumed"),
                capture!("idle-interrupted"),
                capture!("working"),
                capture!("working-tip"),
            ]
            .into_iter()
            .enumerate()
            {
                show(shown);
                let sent = f.send().await;
                assert_eq!(sent.state, AgentState::Running, "{name}");
                assert_eq!(sent.follow_ups.len(), count + 1, "{name}");
                assert_eq!(
                    sent.follow_ups[count].delivery,
                    Some(InstructionDelivery::Delivered),
                    "{name}"
                );
                assert_eq!(
                    calls(),
                    [
                        "list-sessions",
                        "capture-pane",
                        "load-buffer",
                        "paste-buffer",
                        "send-keys"
                    ],
                    "{name}"
                );
            }
            f.close().await;
        },
    );
}
