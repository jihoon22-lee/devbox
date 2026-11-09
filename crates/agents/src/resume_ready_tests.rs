//! A resume that continues the tool's session hands the tool no instruction, so the tool waits
//! at its prompt from the start (`Binding::resumed_session`). Each test runs in a child process
//! whose `tmux` is an owned fake, so the resume and the send go through the service.
use super::*;
use crate::{hooks::HookSource, tracker::Binding};
use devbox_core::{Db, Hub, HubReceiver, Instance, Migration, Paths, paths::Env};
use devbox_protocol::Topic;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

struct Home(PathBuf);
impl Env for Home {
    fn var(&self, key: &str) -> Option<String> {
        let path = match key {
            "HOME" => self.0.clone(),
            "XDG_RUNTIME_DIR" => self.0.join("runtime"),
            _ => return None,
        };
        Some(path.to_string_lossy().into_owned())
    }
    fn uid(&self) -> u32 {
        1000
    }
}

/// The service of a daemon on the owned home `root`, with the builtin profiles.
fn service(root: &Path) -> Arc<AgentsService> {
    let paths = Paths::resolve(
        Instance::parse("test-resume-ready").expect("owned instance"),
        &Home(root.into()),
    )
    .expect("owned paths");
    paths.ensure_dirs().expect("owned directories");
    let migrations: Vec<Migration> = crate::MIGRATIONS
        .iter()
        .chain(devbox_projects::MIGRATIONS)
        .chain(devbox_terminal::MIGRATIONS)
        .copied()
        .collect();
    let db =
        Db::open(&paths.db_path, &paths.backups_dir, "test", &migrations).expect("owned database");
    let hub = Arc::new(Hub::new());
    let projects = Arc::new(devbox_projects::ProjectsService::new(
        db.clone(),
        hub.clone(),
        paths.home.clone(),
        vec![],
    ));
    let terminal = Arc::new(devbox_terminal::TerminalService::new(
        db.clone(),
        hub.clone(),
        paths.clone(),
        projects.clone(),
        Default::default(),
    ));
    let shell = crate::command_path::fixture_shell(&paths.home);
    let runtime = crate::AgentRuntime {
        devbox_bin: "/owned/devbox".into(),
        use_scope: false,
        cleanup_after_close: false,
        shell,
        lookups: Arc::new(crate::command_path::LookupCache::stopped()),
    };
    Arc::new(AgentsService::new(
        db,
        hub,
        paths,
        projects,
        terminal,
        vec![],
        runtime,
    ))
}

/// Task `a` in its project's own folder, whose first launch, `first`, ended.
struct Ended {
    svc: Arc<AgentsService>,
    root: tempfile::TempDir,
}
impl Ended {
    /// Task `a` of the builtin profile `profile`. Its tool reported `session` during the first
    /// launch: for Codex, a thread whose rollout names the task's folder.
    async fn new(profile: &str, session: Option<&str>) -> Self {
        let root = tempfile::tempdir().expect("owned home");
        let svc = service(root.path());
        let workdir = root.path().join("project");
        fs::create_dir(&workdir).expect("owned project folder");
        let workdir = workdir.to_string_lossy().into_owned();
        let mut binding = Binding::new(1);
        binding.launch_id = Some("first".into());
        if let (Some(thread), "codex") = (session, profile) {
            let sessions = root.path().join(".codex/sessions");
            fs::create_dir_all(&sessions).expect("owned Codex sessions");
            let rollout = sessions.join(format!("rollout-{thread}.jsonl"));
            let meta = serde_json::json!({"type":"session_meta","payload":{
                "id":thread,"timestamp":"2026-10-13T00:00:01Z","cwd":workdir,
                "source":"cli","thread_source":"user"
            }});
            fs::write(&rollout, format!("{meta}\n")).expect("owned rollout");
            binding.thread_id = Some(thread.into());
            binding.rollout = Some(rollout);
        }
        let task: AgentTask = serde_json::from_value(serde_json::json!({
            "id":"a","projectId":"p","title":"Owned resume","profile":profile,"baseBranch":"main",
            "branch":null,"worktreePath":null,"workdir":workdir,"sessionId":session,
            "terminalId":null,"state":"review","toolExited":true,"reason":null,"attention":null,
            "port":20000,
            "prompt":"owned prompt","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,
            "createdMs":0,"updatedMs":0,"closedMs":null
        }))
        .expect("owned task");
        let phase = svc.phase_path(&task, "first");
        svc.db
            .write(move |tx| {
                tx.execute(
                    "INSERT INTO projects_projects(id,name,path,favorite,added_ms) \
                     VALUES('p','Owned',?1,0,0)",
                    rusqlite::params![task.workdir],
                )?;
                crate::store::put(tx, &task)?;
                crate::store::put_binding(tx, &task.id, &binding)
            })
            .await
            .expect("owned ended launch");
        crate::prepare::private_dir(phase.parent().expect("data directory")).expect("owned data");
        fs::write(&phase, "exit:0\n").expect("the first launch's tool ended");
        Self { svc, root }
    }
    /// The daemon restarts on the same data.
    async fn restart(&mut self) {
        self.svc.shutdown().await;
        self.svc = service(self.root.path());
    }
    async fn task(&self) -> AgentTask {
        self.svc.get("a").await.expect("owned task")
    }
    async fn binding(&self) -> Binding {
        self.svc.launch_binding("a").await.expect("owned binding")
    }
    /// The start script the resume wrote for the current launch.
    async fn script(&self) -> String {
        let launch = self.binding().await.launch_id.expect("launch");
        let data = self.svc.paths.data_dir.join("agents/a");
        fs::read_to_string(data.join(format!("resume-{launch}.sh"))).expect("resume script")
    }
    /// The current launch's tool started, as its start script records it.
    async fn tool_started(&self) {
        let launch = self.binding().await.launch_id.expect("launch");
        let phase = self.svc.phase_path(&self.task().await, &launch);
        fs::write(phase, "tool\n").expect("tool phase");
    }
    /// Claude's hook `payload` from launch `launch`, as `devbox agent event` in its pane sends it.
    async fn claude_from(&self, launch: Option<String>, payload: serde_json::Value) {
        self.svc
            .event(AgentEventParams {
                agent_id: "a".into(),
                launch_id: launch,
                source: HookSource::Claude,
                payload: payload.to_string(),
                code: None,
            })
            .await
            .expect("owned hook");
    }
    /// Claude's hook `payload` from the current launch.
    async fn claude(&self, payload: serde_json::Value) {
        let launch = self.binding().await.launch_id;
        self.claude_from(launch, payload).await;
    }
    async fn try_send(&self) -> Result<AgentTask> {
        self.svc
            .send(SendToAgent {
                id: "a".into(),
                text: "owned next instruction".into(),
            })
            .await
    }
    async fn send(&self) -> AgentTask {
        self.try_send().await.expect("owned send")
    }
    /// The output watcher judges Codex ready at its prompt, from a pass that read the task now.
    async fn ready(&self) {
        let seen = crate::watch::Snapshot {
            task: self.task().await,
            binding: Some(self.binding().await),
        };
        let end = crate::service::CodexTurnEnd::Ready(Box::new(seen));
        self.svc.complete_codex("a", end).await.expect("judged");
    }
    /// Removes the owned fake terminal.
    async fn close(&self) {
        let terminal = self.task().await.terminal_id.expect("owned terminal");
        self.svc
            .terminal
            .close_internal(&terminal)
            .await
            .expect("remove owned fake terminal");
    }
}

fn session_start(source: &str) -> serde_json::Value {
    serde_json::json!({
        "hook_event_name": "SessionStart",
        "session_id": "claude-session",
        "source": source,
    })
}

/// How many notices were published to `attention` so far. Each is published before its write
/// replies.
fn notices(attention: &mut HubReceiver) -> usize {
    use futures::FutureExt;
    let mut count = 0;
    while let Some(Some(_)) = attention.recv().now_or_never() {
        count += 1;
    }
    count
}

#[test]
fn claude_resume_without_prompt_becomes_idle_on_session_start() {
    with_fake_tmux(
        "claude_resume_without_prompt_becomes_idle_on_session_start",
        || async {
            let mut f = Ended::new("claude", Some("claude-session")).await;
            let resumed = f.svc.resume("a").await.expect("resume");
            assert_eq!(resumed.state, AgentState::Running);
            assert!(f.binding().await.resumed_session);
            let script = f.script().await;
            assert!(script.contains(" --resume 'claude-session'"), "{script}");
            // The daemon restarts before Claude starts the session: the marker is stored.
            f.restart().await;
            let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
            // A start reported by the launch before is not this launch's.
            f.claude_from(Some("first".into()), session_start("resume"))
                .await;
            assert_eq!(f.task().await.state, AgentState::Running);
            f.claude(session_start("resume")).await;
            let idle = f.task().await;
            assert_eq!(idle.state, AgentState::Idle);
            let shown = idle.attention.clone().expect("attention");
            assert_eq!(
                (shown.kind.as_str(), shown.message.as_str()),
                ("idle", "다음 지시를 기다립니다")
            );
            assert!(!f.binding().await.resumed_session);
            // Judged once: a later start of the session changes nothing.
            f.claude(session_start("resume")).await;
            assert_eq!(f.task().await, idle);
            assert_eq!(notices(&mut attention), 1);
            f.close().await;
        },
    );
}

#[test]
fn claude_session_start_after_a_prompt_keeps_running() {
    with_fake_tmux(
        "claude_session_start_after_a_prompt_keeps_running",
        || async {
            let f = Ended::new("claude", Some("claude-session")).await;
            f.svc.resume("a").await.expect("resume");
            assert!(f.binding().await.resumed_session);
            let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
            // The user types the launch's first prompt in the terminal.
            let prompt = serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "session_id": "claude-session",
            });
            f.claude(prompt).await;
            assert!(!f.binding().await.resumed_session);
            f.claude(session_start("resume")).await;
            let task = f.task().await;
            assert_eq!((task.state, task.attention), (AgentState::Running, None));
            assert_eq!(notices(&mut attention), 0);
            f.close().await;
        },
    );
}

#[test]
fn claude_resume_without_session_id_never_marks_ready() {
    with_fake_tmux(
        "claude_resume_without_session_id_never_marks_ready",
        || async {
            let f = Ended::new("claude", None).await;
            f.svc.resume("a").await.expect("resume");
            // Without a session Claude starts again with the task's instruction: a real turn.
            let script = f.script().await;
            assert!(
                script.contains(" -- \"$(cat ") && !script.contains("--resume"),
                "{script}"
            );
            assert!(!f.binding().await.resumed_session);
            let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
            f.claude(session_start("resume")).await;
            let task = f.task().await;
            assert_eq!((task.state, task.attention), (AgentState::Running, None));
            assert_eq!(notices(&mut attention), 0);
            f.close().await;
        },
    );
}

#[test]
fn send_right_after_resume_clears_the_ready_marker() {
    with_fake_tmux(
        "send_right_after_resume_clears_the_ready_marker",
        || async {
            let f = Ended::new("claude", Some("claude-session")).await;
            f.svc.resume("a").await.expect("resume");
            assert!(f.binding().await.resumed_session);
            f.tool_started().await;
            let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
            // The instruction is sent while Claude is still starting.
            assert_eq!(f.send().await.state, AgentState::Running);
            assert!(!f.binding().await.resumed_session);
            f.claude(session_start("resume")).await;
            let task = f.task().await;
            assert_eq!((task.state, task.attention), (AgentState::Running, None));
            assert_eq!(notices(&mut attention), 0);
            f.close().await;
        },
    );
}

#[test]
fn tracker_save_keeps_a_cleared_ready_marker() {
    with_fake_tmux("tracker_save_keeps_a_cleared_ready_marker", || async {
        let f = Ended::new("codex", Some("primary")).await;
        f.svc.resume("a").await.expect("resume");
        let script = f.script().await;
        assert!(script.contains(" resume 'primary'"), "{script}");
        // The tracker reads the binding while the launch still waits for its first instruction,
        let mut stale = f.binding().await;
        assert!(stale.resumed_session);
        f.tool_started().await;
        f.send().await;
        assert!(!f.binding().await.resumed_session);
        // and saves the turn that instruction began only after the send.
        stale.turn_id = Some("after-send".into());
        stale.turn_started_ms = Some(stale.launch_ms + 1);
        f.svc
            .save_binding("a", stale, false)
            .await
            .expect("tracker save");
        let saved = f.binding().await;
        assert_eq!(saved.turn_id.as_deref(), Some("after-send"));
        assert!(!saved.resumed_session);
        f.close().await;
    });
}

#[test]
fn unsent_send_after_resume_restores_the_ready_marker() {
    with_fake_tmux(
        "unsent_send_after_resume_restores_the_ready_marker",
        || async {
            let f = Ended::new("codex", Some("primary")).await;
            f.svc.resume("a").await.expect("resume");
            let resumed = f.binding().await;
            assert!(resumed.resumed_session);
            f.tool_started().await;
            let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
            // tmux takes none of the instruction, so Codex never gets it.
            fail_tmux(Some("load-buffer"));
            let error = f.try_send().await.expect_err("unsent");
            assert!(
                matches!(error, devbox_protocol::Fail::Internal(_)),
                "{error:?}"
            );
            let task = f.task().await;
            assert!(task.follow_ups.is_empty());
            assert_eq!((task.state, task.attention), (AgentState::Running, None));
            assert_eq!(
                f.binding().await,
                resumed,
                "still waits for its first instruction"
            );
            // Codex sits at its prompt: the watcher's judgement ends the wait, once.
            f.ready().await;
            let idle = f.task().await;
            assert_eq!(idle.state, AgentState::Idle);
            let shown = idle.attention.clone().expect("attention");
            assert_eq!(
                (shown.kind.as_str(), shown.message.as_str()),
                ("idle", "다음 지시를 기다립니다")
            );
            assert!(!f.binding().await.resumed_session);
            f.ready().await;
            assert_eq!(f.task().await, idle);
            assert_eq!(notices(&mut attention), 1);
            fail_tmux(None);
            f.close().await;
        },
    );
}

#[test]
fn delivered_send_after_resume_keeps_the_marker_cleared() {
    with_fake_tmux(
        "delivered_send_after_resume_keeps_the_marker_cleared",
        || async {
            // Delivered, and pasted with its delivery unknown: Codex may have the instruction.
            for failing in [None, Some("paste-buffer")] {
                let f = Ended::new("codex", Some("primary")).await;
                f.svc.resume("a").await.expect("resume");
                assert!(f.binding().await.resumed_session);
                f.tool_started().await;
                let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
                if let Some(operation) = failing {
                    fail_tmux(Some(operation));
                }
                let sent = f.try_send().await;
                match failing {
                    None => assert_eq!(sent.expect("delivered").state, AgentState::Running),
                    Some(_) => assert!(
                        matches!(
                            sent,
                            Err(devbox_protocol::Fail::Domain(AgentsError::DeliveryUnknown))
                        ),
                        "{sent:?}"
                    ),
                }
                assert!(!f.binding().await.resumed_session, "{failing:?}");
                // An unsent send next cleared no marker, so it puts none back.
                fail_tmux(Some("load-buffer"));
                f.try_send().await.expect_err("unsent");
                fail_tmux(None);
                assert!(!f.binding().await.resumed_session, "{failing:?}");
                f.ready().await;
                let task = f.task().await;
                assert_eq!(task.state, AgentState::Running, "{failing:?}");
                assert_eq!(notices(&mut attention), 0, "{failing:?}");
                f.close().await;
                f.svc.shutdown().await;
            }
        },
    );
}

#[test]
fn claude_session_start_while_waiting_keeps_waiting() {
    with_fake_tmux(
        "claude_session_start_while_waiting_keeps_waiting",
        || async {
            let f = Ended::new("claude", Some("claude-session")).await;
            f.svc.resume("a").await.expect("resume");
            // The watcher saw a prompt sit still on Claude's screen, its folder trust say.
            let seen = crate::watch::Snapshot {
                task: f.task().await,
                binding: Some(f.binding().await),
            };
            f.svc.output_event(seen, true).await.expect("watched");
            let waiting = f.task().await;
            assert_eq!(waiting.state, AgentState::Waiting);
            assert_eq!(
                waiting.attention.as_ref().expect("attention").kind,
                "output"
            );
            let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
            f.claude(session_start("resume")).await;
            assert_eq!(f.task().await, waiting);
            assert!(f.binding().await.resumed_session);
            assert_eq!(notices(&mut attention), 0);
            f.close().await;
        },
    );
}

const FAKE_TMUX: &str = r#"#!/bin/sh
live=$DEVBOX_RESUME_READY_LIVE
if [ -f "$live.fail" ] && [ "$(cat "$live.fail")" = "$5" ]; then
  if [ "$5" = load-buffer ]; then cat > /dev/null; fi
  exit 1
fi
case "$5" in
 has-session)
   if [ -f "$live" ] && [ "$(cat "$live")" = "${7#=}" ]; then exit 0; fi
   printf '%s\n' "can't find session" >&2; exit 1;;
 new-session)
   previous=
   for argument do
     if [ "$previous" = -s ]; then printf '%s' "$argument" > "$live"; fi
     previous=$argument
   done;;
 list-sessions) if [ -f "$live" ]; then printf '%s\t0\tbash\t/owned\n' "$(cat "$live")"; fi;;
 kill-session) rm -f "$live";;
 load-buffer) cat > /dev/null;;
esac
exit 0
"#;

/// The fake `tmux` fails `operation` (a tmux command) from now on, or nothing.
fn fail_tmux(operation: Option<&str>) {
    let fail = format!(
        "{}.fail",
        std::env::var("DEVBOX_RESUME_READY_LIVE").expect("fake")
    );
    match operation {
        Some(operation) => fs::write(fail, operation).expect("owned fake failure"),
        None => fs::remove_file(fail).expect("owned fake failure"),
    }
}

/// Runs `body` in a child test process whose `tmux` is an owned fake holding one session, as the
/// resume rollback tests do. `test` is the test's name in this module.
fn with_fake_tmux<F: std::future::Future<Output = ()>>(test: &str, body: impl FnOnce() -> F) {
    const LIVE: &str = "DEVBOX_RESUME_READY_LIVE";
    if std::env::var_os(LIVE).is_some() {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("owned runtime")
            .block_on(body());
        return;
    }
    let root = tempfile::tempdir().expect("owned fake tmux");
    let tmux = root.path().join("tmux");
    fs::write(&tmux, FAKE_TMUX).expect("owned fake executable");
    fs::set_permissions(&tmux, fs::Permissions::from_mode(0o700)).expect("mode");
    let path = std::env::join_paths(std::iter::once(root.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH")),
    ))
    .expect("owned PATH");
    let output = std::process::Command::new(std::env::current_exe().expect("owned executable"))
        .args([
            "--exact",
            &format!("resume::ready_tests::{test}"),
            "--nocapture",
        ])
        .env(LIVE, root.path().join("live"))
        .env("PATH", &path)
        .output()
        .expect("owned child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // A filter that matches nothing exits 0 after running no test.
    assert!(
        output.status.success() && stdout.contains("running 1 test\n"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
