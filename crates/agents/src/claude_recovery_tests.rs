use super::*;
use crate::{
    claude_recovery::{MESSAGE, REASON},
    state::CloseHow,
};

fn request(task: &AgentTask, action: ConversationAction) -> RecoverConversation {
    RecoverConversation {
        id: task.id.clone(),
        updated_ms: task.updated_ms,
        action,
    }
}

#[test]
fn archived_old_missing_conversation_keeps_files_and_rejects_stale_actions() {
    with_fake_tmux(
        "conversation::archived_old_missing_conversation_keeps_files_and_rejects_stale_actions",
        || async {
            let f = Ended::new("claude", Some("recorded-id")).await;
            let started = f
                .svc
                .resume("a")
                .await
                .expect("resume a legacy session by its bound id");
            let generation = f
                .binding()
                .await
                .launch_id
                .expect("resumed Claude task has a launch id");
            fs::write(f.svc.phase_path(&started, &generation), "exit:1\n")
                .expect("record the failed exit for the resumed launch");
            let live = std::env::var("DEVBOX_RESUME_READY_LIVE")
                .expect("fake tmux provides the live pane fixture path");
            fs::write(
                format!("{live}.screen"),
                "No conversation found with session ID: another-id\n",
            )
            .expect("show a missing-conversation error for a different session");
            f.svc
                .recover_tool_exit("a")
                .await
                .expect("recover the recorded tool exit");
            assert_eq!(f.task().await.reason.as_deref(), Some("exit:1"));
            fs::write(
                format!("{live}.screen"),
                "some unrelated output\nNo conversation found with session ID: recorded-id\n",
            )
            .expect("show the missing-session text below unrelated output");
            f.svc
                .reconcile()
                .await
                .expect("reconcile the pane with unrelated output");
            assert_eq!(f.task().await.reason.as_deref(), Some("exit:1"));
            fs::write(
                format!("{live}.screen"),
                "No conversation found with session ID: recorded-id\n[devbox] 에이전트가 끝났습니다(종료 코드 1).\n[devbox] 작업이 끝났습니다. 재개 버튼으로 다시 시작하세요.\n",
            )
            .expect("show the missing-session error and tool-exit footer");
            let legacy_record = f
                .svc
                .paths
                .home
                .join(".claude/projects/legacy/recorded-id.jsonl");
            fs::create_dir_all(
                legacy_record
                    .parent()
                    .expect("legacy transcript has a project directory"),
            )
            .expect("create the legacy transcript directory");
            fs::write(&legacy_record, "opaque").expect("write the existing Claude transcript");
            f.svc
                .reconcile()
                .await
                .expect("reconcile with the legacy transcript present");
            assert_eq!(f.task().await.reason.as_deref(), Some("exit:1"));
            fs::remove_file(legacy_record)
                .expect("remove the legacy transcript to simulate a missing conversation");
            let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
            f.svc
                .reconcile()
                .await
                .expect("reconcile after the legacy transcript disappears");
            let stopped = f.task().await;
            assert_eq!(stopped.reason.as_deref(), Some(REASON));
            assert_eq!(
                stopped
                    .attention
                    .as_ref()
                    .expect("missing conversation must request attention")
                    .message,
                MESSAGE
            );
            assert_eq!(
                notices(&mut attention),
                1,
                "announce the useful cause even after a legacy exit:1"
            );
            let mut stale = request(&stopped, ConversationAction::StartNew);
            stale.updated_ms -= 1;
            assert!(f.svc.recover_conversation(stale).await.is_err());
            let folder = PathBuf::from(&stopped.workdir);
            let sh = |args: &[&str]| {
                assert!(
                    std::process::Command::new("git")
                        .current_dir(&folder)
                        .args(args)
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .env("GIT_CONFIG_NOSYSTEM", "1")
                        .status()
                        .expect("start Git in the owned repository fixture")
                        .success()
                );
            };
            sh(&["init", "-qb", "main"]);
            sh(&[
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-qm",
                "kept",
            ]);
            let tree = f.root.path().join("project-agents/retained");
            let worktree = tree.to_str().expect("retained worktree path is UTF-8");
            sh(&["worktree", "add", "-qb", "agent/retained", worktree, "main"]);
            let mut stopped = stopped;
            stopped.branch = Some("agent/retained".into());
            stopped.worktree_path = Some(worktree.into());
            stopped.workdir = worktree.into();
            let saved = stopped.clone();
            f.svc
                .db
                .write(move |tx| {
                    let mut b = crate::store::binding(tx, "a")?
                        .expect("stopped fixture task has a launch binding");
                    b.resources = crate::tracker::Resources::Owned;
                    crate::store::put_binding(tx, "a", &b)?;
                    crate::store::put(tx, &saved)
                })
                .await
                .expect("persist the stopped task and its owned worktree binding");
            fs::write(tree.join("dirty"), "user changes")
                .expect("write uncommitted content in the retained worktree");
            let archived = f
                .svc
                .recover_conversation(request(&stopped, ConversationAction::Archive))
                .await
                .expect("archive the stopped task with its conversation missing");
            assert_eq!(archived.closed_how, Some(CloseHow::Archived));
            let remaining = f
                .svc
                .cleanup_candidates(CleanupCandidates {
                    project_id: stopped.project_id.clone(),
                })
                .await
                .expect("list cleanup candidates after archiving");
            assert!(remaining.worktrees.is_empty() && remaining.branches.is_empty());
            assert!(
                f.svc
                    .cleanup(CleanupAgents {
                        project_id: stopped.project_id.clone(),
                        worktrees: vec![worktree.into()],
                        branches: vec!["agent/retained".into()]
                    })
                    .await
                    .is_err()
            );
            assert!(archived.terminal_id.is_none());
            assert!(
                f.svc
                    .list(ListAgents::default())
                    .await
                    .expect("list open tasks after archiving")
                    .items
                    .is_empty()
            );
            assert_eq!(
                fs::read_to_string(tree.join("dirty"))
                    .expect("read the retained uncommitted file after cleanup was refused"),
                "user changes"
            );
            sh(&[
                "show-ref",
                "--verify",
                "--quiet",
                "refs/heads/agent/retained",
            ]);
            assert!(
                f.svc
                    .recover_conversation(request(&stopped, ConversationAction::StartNew))
                    .await
                    .is_err()
            );
            f.svc.shutdown().await;
        },
    );
}

#[test]
fn claude_session_start_binds_transcript_and_fresh_chat_never_replays_prompt() {
    with_fake_tmux(
        "conversation::claude_session_start_binds_transcript_and_fresh_chat_never_replays_prompt",
        || async {
            let f = Ended::new("claude", Some("recorded-id")).await;
            let active = f.svc.resume("a").await.expect("legacy exact resume");
            f.tool_started().await;
            let root = f.svc.paths.home.join(".claude/projects/owned");
            fs::create_dir_all(&root).expect("create the bound transcript directory");
            let path = root.join("recorded-id.jsonl");
            f.claude(
                serde_json::json!({"hook_event_name":"SessionStart","cwd":active.workdir,
            "transcript_path":path,"source":"resume","session_id":"recorded-id"}),
            )
            .await;
            let b = f.binding().await;
            assert_eq!(b.claude_transcript.as_ref(), Some(&path));
            fs::write(&path, "persisted opaque history\n")
                .expect("write the transcript reported by SessionStart");
            assert!(!f.svc.missing_conversation_at_exit(&active, &b, 0).await);
            fs::remove_file(path).expect("remove the bound transcript before the clean exit");
            let launch_id = b.launch_id.expect("resumed task has a current launch id");
            fs::write(f.svc.phase_path(&active, &launch_id), "exit:0\n")
                .expect("record a clean exit for the current launch");
            f.svc
                .event(AgentEventParams {
                    agent_id: "a".into(),
                    launch_id: None,
                    source: HookSource::Exit,
                    payload: launch_id,
                    code: Some(0),
                })
                .await
                .expect("apply the exit event to the resumed Claude task");
            let missing = f.task().await;
            assert_eq!(missing.reason.as_deref(), Some(REASON));
            let new = f
                .svc
                .recover_conversation(request(&missing, ConversationAction::StartNew))
                .await
                .expect("start a fresh conversation for the missing transcript");
            let script = f.script().await;
            let cmd = script
                .lines()
                .find(|s| s.contains(" --settings "))
                .expect("new launch script contains the Claude settings invocation");
            assert!(!cmd.contains("--resume") && !cmd.contains("$(cat"), "{cmd}");
            assert!(new.session_id.is_none() && new.terminal_id != missing.terminal_id);
            assert!(f.binding().await.claude_transcript.is_none());
            f.tool_started().await;
            f.claude(serde_json::json!({"hook_event_name":"SessionStart","session_id":"fresh-session","source":"startup"})).await;
            let ready = f.task().await;
            assert_eq!(ready.state, AgentState::Idle);
            assert_eq!(ready.session_id.as_deref(), Some("fresh-session"));
            f.close().await;
            f.svc.shutdown().await;
        },
    );
}

#[test]
fn a_delayed_session_start_with_a_different_id_never_rebinds_a_claude_resume() {
    with_fake_tmux(
        "conversation::a_delayed_session_start_with_a_different_id_never_rebinds_a_claude_resume",
        || async {
            let f = Ended::new("claude", Some("bound-id")).await;
            f.svc
                .resume("a")
                .await
                .expect("resume Claude with the previously bound session id");
            f.tool_started().await;
            f.claude(
                serde_json::json!({"hook_event_name":"UserPromptSubmit","session_id":"other-id"}),
            )
            .await;
            assert!(!f.binding().await.resumed_session);
            assert!(f.binding().await.unconfirmed_session);
            f.claude(serde_json::json!({"hook_event_name":"SessionStart","session_id":"other-id","source":"startup"})).await;
            let failed = f.task().await;
            assert_eq!(failed.session_id.as_deref(), Some("bound-id"));
            assert_eq!(failed.reason.as_deref(), Some("session_mismatch"));
            assert!(f.try_send().await.is_err());
            f.close().await;
            f.svc.shutdown().await;
        },
    );
}
