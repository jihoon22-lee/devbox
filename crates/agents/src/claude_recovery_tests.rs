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
            let generation = f.binding().await.launch_id.unwrap();
            fs::write(f.svc.phase_path(&started, &generation), "exit:1\n").unwrap();
            let live = std::env::var("DEVBOX_RESUME_READY_LIVE").unwrap();
            fs::write(
                format!("{live}.screen"),
                "No conversation found with session ID: another-id\n",
            )
            .unwrap();
            f.svc.recover_tool_exit("a").await.unwrap();
            assert_eq!(f.task().await.reason.as_deref(), Some("exit:1"));
            fs::write(
                format!("{live}.screen"),
                "some unrelated output\nNo conversation found with session ID: recorded-id\n",
            )
            .unwrap();
            f.svc.reconcile().await.unwrap();
            assert_eq!(f.task().await.reason.as_deref(), Some("exit:1"));
            fs::write(
                format!("{live}.screen"),
                "No conversation found with session ID: recorded-id\n[devbox] 에이전트가 끝났습니다(종료 코드 1).\n[devbox] 작업이 끝났습니다. 재개 버튼으로 다시 시작하세요.\n",
            )
            .unwrap();
            let legacy_record = f
                .svc
                .paths
                .home
                .join(".claude/projects/legacy/recorded-id.jsonl");
            fs::create_dir_all(legacy_record.parent().unwrap()).unwrap();
            fs::write(&legacy_record, "opaque").unwrap();
            f.svc.reconcile().await.unwrap();
            assert_eq!(f.task().await.reason.as_deref(), Some("exit:1"));
            fs::remove_file(legacy_record).unwrap();
            let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
            f.svc.reconcile().await.unwrap();
            let stopped = f.task().await;
            assert_eq!(stopped.reason.as_deref(), Some(REASON));
            assert_eq!(stopped.attention.as_ref().unwrap().message, MESSAGE);
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
                        .unwrap()
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
            sh(&[
                "worktree",
                "add",
                "-qb",
                "agent/retained",
                tree.to_str().unwrap(),
                "main",
            ]);
            let mut stopped = stopped;
            stopped.branch = Some("agent/retained".into());
            stopped.worktree_path = Some(tree.to_str().unwrap().into());
            stopped.workdir = tree.to_str().unwrap().into();
            let saved = stopped.clone();
            f.svc
                .db
                .write(move |tx| {
                    let mut b = crate::store::binding(tx, "a")?.unwrap();
                    b.resources = crate::tracker::Resources::Owned;
                    crate::store::put_binding(tx, "a", &b)?;
                    crate::store::put(tx, &saved)
                })
                .await
                .unwrap();
            fs::write(tree.join("dirty"), "user changes").unwrap();
            let archived = f
                .svc
                .recover_conversation(request(&stopped, ConversationAction::Archive))
                .await
                .unwrap();
            assert_eq!(archived.closed_how, Some(CloseHow::Archived));
            let remaining = f
                .svc
                .cleanup_candidates(CleanupCandidates {
                    project_id: stopped.project_id.clone(),
                })
                .await
                .unwrap();
            assert!(remaining.worktrees.is_empty() && remaining.branches.is_empty());
            assert!(
                f.svc
                    .cleanup(CleanupAgents {
                        project_id: stopped.project_id.clone(),
                        worktrees: vec![tree.to_str().unwrap().into()],
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
                    .unwrap()
                    .items
                    .is_empty()
            );
            assert_eq!(
                fs::read_to_string(tree.join("dirty")).unwrap(),
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
            fs::create_dir_all(&root).unwrap();
            let path = root.join("recorded-id.jsonl");
            f.claude(
                serde_json::json!({"hook_event_name":"SessionStart","cwd":active.workdir,
            "transcript_path":path,"source":"resume","session_id":"recorded-id"}),
            )
            .await;
            let b = f.binding().await;
            assert_eq!(b.claude_transcript.as_ref(), Some(&path));
            fs::write(&path, "persisted opaque history\n").unwrap();
            assert!(!f.svc.missing_conversation_at_exit(&active, &b, 0).await);
            fs::remove_file(path).unwrap();
            fs::write(
                f.svc.phase_path(&active, b.launch_id.as_deref().unwrap()),
                "exit:0\n",
            )
            .unwrap();
            f.svc
                .event(AgentEventParams {
                    agent_id: "a".into(),
                    launch_id: None,
                    source: HookSource::Exit,
                    payload: b.launch_id.unwrap(),
                    code: Some(0),
                })
                .await
                .unwrap();
            let missing = f.task().await;
            assert_eq!(missing.reason.as_deref(), Some(REASON));
            let new = f
                .svc
                .recover_conversation(request(&missing, ConversationAction::StartNew))
                .await
                .unwrap();
            let script = f.script().await;
            let cmd = script.lines().find(|s| s.contains(" --settings ")).unwrap();
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
            f.svc.resume("a").await.unwrap();
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
