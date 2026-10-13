use super::*;
use crate::{
    hooks::HookSource,
    tracker::{self, Diagnostic},
};
use devbox_core::{Instance, Migration, paths::Env};
use std::{fs, io::Write, path::PathBuf};

struct TestEnv(PathBuf);
impl Env for TestEnv {
    fn var(&self, key: &str) -> Option<String> {
        match key {
            "HOME" => Some(self.0.to_string_lossy().into_owned()),
            "XDG_RUNTIME_DIR" => Some(self.0.join("runtime").to_string_lossy().into_owned()),
            _ => None,
        }
    }
    fn uid(&self) -> u32 {
        1000
    }
}
struct Fixture {
    svc: Arc<AgentsService>,
    root: tempfile::TempDir,
    sessions: PathBuf,
    workdir: String,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::resolve(
            Instance::parse("test-correlation").unwrap(),
            &TestEnv(root.path().into()),
        )
        .unwrap();
        paths.ensure_dirs().unwrap();
        let svc = Self::service(paths);
        let sessions = root.path().join(".codex/sessions");
        fs::create_dir_all(&sessions).unwrap();
        let workdir = root.path().join("work").to_string_lossy().into_owned();
        let fixture = Self {
            svc,
            root,
            sessions,
            workdir,
        };
        fixture.seed(Binding::new(at("2026-10-13T00:00:00Z"))).await;
        fixture
    }
    fn service(paths: Paths) -> Arc<AgentsService> {
        let migrations: Vec<Migration> = crate::MIGRATIONS
            .iter()
            .chain(devbox_projects::MIGRATIONS)
            .chain(devbox_terminal::MIGRATIONS)
            .copied()
            .collect();
        let db = Db::open(&paths.db_path, &paths.backups_dir, "test", &migrations).unwrap();
        let hub = Arc::new(Hub::new());
        let projects = Arc::new(ProjectsService::new(
            db.clone(),
            hub.clone(),
            paths.home.clone(),
            vec![],
        ));
        let terminal = Arc::new(TerminalService::new(
            db.clone(),
            hub.clone(),
            paths.clone(),
            projects.clone(),
            devbox_core::config::TerminalConfig::default(),
        ));
        Arc::new(AgentsService::new(
            db,
            hub,
            paths,
            projects,
            terminal,
            vec![],
            "/owned/devbox".into(),
            false,
            false,
        ))
    }
    async fn seed(&self, binding: Binding) {
        let task = AgentTask {
            id: "a".into(),
            project_id: "p".into(),
            title: "test".into(),
            profile: "codex".into(),
            base_branch: "main".into(),
            branch: None,
            worktree_path: None,
            workdir: self.workdir.clone(),
            session_id: binding.thread_id.clone(),
            terminal_id: None,
            state: AgentState::Running,
            reason: None,
            attention: None,
            port: 20000,
            prompt: String::new(),
            follow_ups: vec![],
            tokens: None,
            pr_url: None,
            closed_how: None,
            created_ms: binding.launch_ms,
            updated_ms: binding.launch_ms,
            closed_ms: None,
            closed_commits: None,
            closed_head_oid: None,
            cleanup_error: None,
            tokens_truncated: false,
            activity_seq: 0,
        };
        self.svc
            .db
            .write(move |tx| {
                store::put(tx, &task)?;
                store::put_binding(tx, "a", &binding)
            })
            .await
            .unwrap();
    }
    fn rollout(&self, id: &str, source: &str, created: &str, turn: &str, started: &str) -> PathBuf {
        let path = self.sessions.join(format!("rollout-{id}.jsonl"));
        let meta = serde_json::json!({"type":"session_meta","payload":{"id":id,"timestamp":created,"cwd":self.workdir,"source":"cli","thread_source":source}});
        let event = serde_json::json!({"type":"event_msg","timestamp":started,"payload":{"type":"task_started","turn_id":turn}});
        fs::write(&path, format!("{meta}\n{event}\n")).unwrap();
        path
    }
    fn next_turn(&self, path: &Path, turn: &str, timestamp: &str) {
        let event = serde_json::json!({"type":"event_msg","timestamp":timestamp,"payload":{"type":"task_started","turn_id":turn}});
        writeln!(
            fs::OpenOptions::new().append(true).open(path).unwrap(),
            "{event}"
        )
        .unwrap();
    }
    async fn complete(&self, thread: &str, turn: &str) -> Result<Done> {
        self.svc.event(AgentEventParams { agent_id:"a".into(),source:HookSource::Codex,payload:serde_json::json!({"type":"agent-turn-complete","thread-id":thread,"turn-id":turn,"cwd":self.workdir,"last-assistant-message":"done"}).to_string(),code:None }).await
    }
    async fn state(&self) -> AgentState {
        self.svc.get("a").await.unwrap().state
    }
    async fn expect_diagnostic(&self, expected: Diagnostic) {
        let before = self.svc.get("a").await.unwrap();
        let rev = self.svc.db.rev();
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            self.complete("primary", "one"),
        )
        .await
        .expect("bounded metadata retry");
        assert!(
            matches!(result,Err(Fail::Domain(AgentsError::SessionMetadata { diagnostic })) if diagnostic==expected),
            "{result:?}"
        );
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(180),
            "retry must not terminate on first failure"
        );
        assert_eq!(self.svc.db.rev(), rev);
        assert_eq!(
            serde_json::to_string(&self.svc.get("a").await.unwrap()).unwrap(),
            serde_json::to_string(&before).unwrap()
        );
    }
}
fn at(timestamp: &str) -> i64 {
    tracker::utc_ms(timestamp).unwrap()
}

#[tokio::test]
async fn completion_uses_primary_current_turn_and_stale_notify_preserves_waiting() {
    let f = Fixture::new().await;
    let primary = f.rollout(
        "primary",
        "user",
        "2026-10-13T00:00:01Z",
        "one",
        "2026-10-13T00:00:02Z",
    );
    f.rollout(
        "title",
        "title",
        "2026-10-13T00:00:01Z",
        "title-turn",
        "2026-10-13T00:00:02Z",
    );
    f.complete("title", "title-turn").await.unwrap();
    assert_eq!(f.state().await, AgentState::Running);
    f.complete("primary", "one").await.unwrap();
    assert_eq!(f.state().await, AgentState::Idle);
    f.next_turn(&primary, "two", "2026-10-13T00:00:03Z");
    let task = f.svc.get("a").await.unwrap();
    f.svc.refresh_binding(&task, false).await.unwrap();
    assert_eq!(f.state().await, AgentState::Running);
    f.svc
        .update("a", |t| {
            apply_event(t, &AgentEvent::OutputWaiting, now_ms())
        })
        .await
        .unwrap();
    let rev = f.svc.db.rev();
    f.complete("primary", "one").await.unwrap();
    assert_eq!(f.state().await, AgentState::Waiting);
    // A metadata refresh may persist its identical binding; state and attention must be retained.
    assert!(f.svc.db.rev() >= rev);
    f.complete("primary", "two").await.unwrap();
    assert_eq!(f.state().await, AgentState::Idle);
}

#[tokio::test]
async fn missing_and_ambiguous_metadata_retry_with_typed_diagnostic_without_mutation() {
    let f = Fixture::new().await;
    f.expect_diagnostic(Diagnostic::Missing).await;
    f.rollout(
        "primary",
        "user",
        "2026-10-13T00:00:01Z",
        "one",
        "2026-10-13T00:00:02Z",
    );
    f.rollout(
        "other",
        "user",
        "2026-10-13T00:00:01Z",
        "other-one",
        "2026-10-13T00:00:02Z",
    );
    f.expect_diagnostic(Diagnostic::Ambiguous).await;
}

#[tokio::test]
async fn known_resume_allows_older_creation_but_requires_turn_after_current_launch_and_survives_reopen()
 {
    let mut f = Fixture::new().await;
    let launch = at("2026-10-13T00:00:10Z");
    let mut binding = Binding::new(launch);
    binding.thread_id = Some("primary".into());
    f.seed(binding).await;
    let path = f.rollout(
        "primary",
        "user",
        "2026-10-12T00:00:00Z",
        "old",
        "2026-10-13T00:00:02Z",
    );
    f.expect_diagnostic(Diagnostic::Missing).await;
    f.next_turn(&path, "resumed", "2026-10-13T00:00:11Z");
    f.complete("primary", "resumed").await.unwrap();
    assert_eq!(f.state().await, AgentState::Idle);
    let paths = f.svc.paths.clone();
    f.svc.shutdown().await;
    f.svc = Fixture::service(paths);
    assert_eq!(
        f.svc.get("a").await.unwrap().session_id.as_deref(),
        Some("primary")
    );
    f.svc
        .update("a", |t| {
            apply_event(t, &AgentEvent::UserInstructed, now_ms())
        })
        .await
        .unwrap();
    f.next_turn(&path, "after-reopen", "2026-10-13T00:00:12Z");
    f.complete("primary", "resumed").await.unwrap();
    assert_eq!(f.state().await, AgentState::Running);
    f.complete("primary", "after-reopen").await.unwrap();
    assert_eq!(f.state().await, AgentState::Idle);
    assert!(f.root.path().exists());
}

#[tokio::test]
async fn truncated_and_overlarge_matching_rollouts_fail_closed() {
    let f = Fixture::new().await;
    let path = f.rollout(
        "primary",
        "user",
        "2026-10-13T00:00:01Z",
        "one",
        "2026-10-13T00:00:02Z",
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{truncated")
        .unwrap();
    f.expect_diagnostic(Diagnostic::Invalid).await;
    let path = f.rollout(
        "primary",
        "user",
        "2026-10-13T00:00:01Z",
        "one",
        "2026-10-13T00:00:02Z",
    );
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    f.expect_diagnostic(Diagnostic::LimitExceeded).await;
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_metadata_cannot_escape_owned_home() {
    let f = Fixture::new().await;
    let target = f.root.path().join("outside.jsonl");
    fs::write(&target, b"owned outside fixture").unwrap();
    std::os::unix::fs::symlink(&target, f.sessions.join("rollout-primary.jsonl")).unwrap();
    f.expect_diagnostic(Diagnostic::UnsafePath).await;
    assert_eq!(fs::read(&target).unwrap(), b"owned outside fixture");
}

#[cfg(unix)]
#[test]
fn review_quiesce_accepts_an_already_unloaded_owned_scope() {
    const CHILD: &str = "DEVBOX_REVIEW_MISSING_SCOPE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let mut fixture = Fixture::new().await;
            Arc::get_mut(&mut fixture.svc).unwrap().use_scope = true;
            let mut task = fixture.svc.get("a").await.unwrap();
            task.terminal_id = None;
            let result = fixture.svc.quiesce(&task).await;
            let expected = std::env::var("DEVBOX_REVIEW_SCOPE_MODE").unwrap();
            assert_eq!(
                result.is_ok(),
                expected.starts_with("absent"),
                "scope stop outcome for {expected}: {result:?}"
            );
        });
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let systemctl = root.path().join("systemctl");
    fs::write(
        &systemctl,
        r#"#!/bin/sh
case "$*" in
 *show*) case "$DEVBOX_REVIEW_SCOPE_MODE" in
    absent0) printf 'not-found\n'; exit 0;;
    absent1) printf 'not-found\n'; exit 1;;
    loaded) printf 'loaded\n'; exit 0;;
    bus) exit 1;;
 esac;;
esac
exit 5
"#,
    )
    .unwrap();
    fs::set_permissions(&systemctl, fs::Permissions::from_mode(0o700)).unwrap();
    for mode in ["absent0", "absent1", "loaded", "bus"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "service::correlation_tests::review_quiesce_accepts_an_already_unloaded_owned_scope", "--nocapture"])
        .env(CHILD, "1")
        .env("DEVBOX_REVIEW_SCOPE_MODE", mode)
        .env("PATH", root.path())
        .env("TOKIO_WORKER_THREADS", "2")
        .output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[tokio::test]
async fn terminal_observation_transaction_rejects_newer_activity_and_launch() {
    let f = Fixture::new().await;
    f.svc
        .update("a", |task| {
            task.terminal_id = Some("old-terminal".into());
            true
        })
        .await
        .unwrap();
    let task = f.svc.get("a").await.unwrap();
    let binding = f.svc.db.read(|tx| store::binding(tx, "a")).await.unwrap();
    let observation = crate::watch::Snapshot {
        task: task.clone(),
        binding: binding.clone(),
    };
    f.svc
        .update("a", move |task| {
            apply_event(task, &AgentEvent::PromptSubmitted, task.updated_ms)
        })
        .await
        .unwrap();
    let rev = f.svc.db.rev();
    assert!(
        f.svc
            .output_event(observation, true)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.svc.db.rev(), rev);
    assert_eq!(f.state().await, AgentState::Running);
    let observation = crate::watch::Snapshot {
        task: f.svc.get("a").await.unwrap(),
        binding,
    };
    f.svc
        .db
        .write(|tx| {
            let mut b = store::binding(tx, "a")?.unwrap();
            b.launch_ms += 1;
            store::put_binding(tx, "a", &b)
        })
        .await
        .unwrap();
    let rev = f.svc.db.rev();
    assert!(
        f.svc
            .output_event(observation, true)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.svc.db.rev(), rev);
    f.svc
        .update("a", |task| apply_event(task, &AgentEvent::OutputWaiting, 1))
        .await
        .unwrap();
    let observation = crate::watch::Snapshot {
        task: f.svc.get("a").await.unwrap(),
        binding: f.svc.db.read(|tx| store::binding(tx, "a")).await.unwrap(),
    };
    f.svc
        .update("a", |task| {
            apply_event(
                task,
                &AgentEvent::NeedsInput {
                    message: "permission".into(),
                },
                1,
            )
        })
        .await
        .unwrap();
    let rev = f.svc.db.rev();
    assert!(
        f.svc
            .output_event(observation, false)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.svc.db.rev(), rev);
    assert_eq!(
        f.svc.get("a").await.unwrap().attention.unwrap().kind,
        "input"
    );
}
