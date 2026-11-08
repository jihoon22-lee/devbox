//! A finished turn refreshes the open task's token total, which the list shows, with the read
//! `agents.usage` makes. Each test reads the tool's own session the way the daemon does.
use super::*;
use crate::{
    closed_review::{ClosedReview, Closure},
    hooks::HookSource,
    tracker::Binding,
};
use devbox_core::{Db, Hub, HubReceiver, Instance, Migration, Paths, paths::Env};
use devbox_protocol::Topic;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const LAUNCH: &str = "launch";
const CLAUDE_SESSION: &str = "claude-session";
#[cfg(unix)]
const OPENCODE_SESSION: &str = "ses_taskroot";
/// How long a test waits for the service's background work.
const PATIENCE: Duration = Duration::from_secs(5);

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

/// Task `a` of the builtin profile `profile`, running on launch `launch` in `work` under the
/// owned home `root`, with nothing else in the service's background.
struct Fixture {
    svc: Arc<AgentsService>,
    root: tempfile::TempDir,
    workdir: String,
}
impl Fixture {
    async fn new(profile: &str, session: Option<&str>) -> Self {
        let root = tempfile::tempdir().expect("owned home");
        let paths = Paths::resolve(
            Instance::parse("test-token-refresh").expect("owned instance"),
            &Home(root.path().into()),
        )
        .expect("owned paths");
        paths.ensure_dirs().expect("owned directories");
        let migrations: Vec<Migration> = crate::MIGRATIONS
            .iter()
            .chain(devbox_projects::MIGRATIONS)
            .chain(devbox_terminal::MIGRATIONS)
            .copied()
            .collect();
        let db = Db::open(&paths.db_path, &paths.backups_dir, "test", &migrations)
            .expect("owned database");
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
        let svc = Arc::new(AgentsService::new(
            db,
            hub,
            paths,
            projects,
            terminal,
            vec![],
            runtime,
        ));
        let workdir = root.path().join("work");
        fs::create_dir(&workdir).expect("owned work folder");
        let workdir = workdir.to_string_lossy().into_owned();
        let task: AgentTask = serde_json::from_value(json!({
            "id":"a","projectId":"p","title":"Owned tokens","profile":profile,"baseBranch":"main",
            "branch":null,"worktreePath":null,"workdir":workdir,"sessionId":session,
            "terminalId":null,"state":"running","reason":null,"attention":null,"port":20000,
            "prompt":"owned prompt","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,
            "createdMs":0,"updatedMs":0,"closedMs":null
        }))
        .expect("owned task");
        let launched = crate::tracker::utc_ms("2026-10-13T00:00:00Z").expect("launch time");
        let mut binding = Binding::new(launched);
        binding.launch_id = Some(LAUNCH.into());
        svc.db
            .write(move |tx| {
                store::put(tx, &task)?;
                store::put_binding(tx, "a", &binding)
            })
            .await
            .expect("owned launch");
        Self { svc, root, workdir }
    }
    /// `payload` of `source` from the current launch, as `devbox agent event` in its pane sends it.
    async fn report(&self, source: HookSource, payload: serde_json::Value) {
        self.svc
            .event(AgentEventParams {
                agent_id: "a".into(),
                launch_id: Some(LAUNCH.into()),
                source,
                payload: payload.to_string(),
                code: None,
            })
            .await
            .expect("owned report");
    }
    /// Claude ends its turn.
    async fn stop(&self) {
        let stop = json!({"hook_event_name":"Stop","session_id":CLAUDE_SESSION,
            "last_assistant_message":"done"});
        self.report(HookSource::Claude, stop).await;
    }
    /// Waits until every refresh the service started has ended.
    async fn settled(&self) {
        let until = Instant::now() + PATIENCE;
        while !self.svc.background.is_empty() {
            assert!(Instant::now() < until, "a token refresh is still running");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    /// Task `a` as `agents.list` lists it.
    async fn listed(&self) -> AgentTask {
        let list = self.svc.list(ListAgents::default()).await.expect("list");
        let task = list.items.into_iter().find(|task| task.id == "a");
        task.expect("listed task")
    }
    /// Claude's transcript of the task's session, in the folder Claude keeps the task's folder in:
    /// two answers, the first written as two records the way Claude writes one per content block.
    fn transcript(&self) -> PathBuf {
        let folder: String = (self.workdir.chars())
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let dir = self.root.path().join(".claude/projects").join(folder);
        fs::create_dir_all(&dir).expect("owned Claude projects");
        let answer = |id: &str, input: u64, output: u64, read: u64, write: u64| {
            json!({"type":"assistant","sessionId":CLAUDE_SESSION,"cwd":self.workdir,"message":{
                "id":id,"usage":{"input_tokens":input,"output_tokens":output,
                "cache_read_input_tokens":read,"cache_creation_input_tokens":write}}})
        };
        let user = json!({"type":"user","sessionId":CLAUDE_SESSION,"cwd":self.workdir,
            "message":{"role":"user","content":"owned prompt"}});
        let records = [
            user,
            answer("msg_1", 1200, 30, 800, 64),
            answer("msg_1", 1200, 30, 800, 64),
            answer("msg_2", 1500, 45, 900, 0),
        ];
        let body: String = records.iter().map(|record| format!("{record}\n")).collect();
        let path = dir.join(format!("{CLAUDE_SESSION}.jsonl"));
        fs::write(&path, body).expect("owned transcript");
        path
    }
    /// Codex's rollout of thread `primary`, which began turn `one` after the launch and counts
    /// the session's tokens so far.
    fn rollout(&self) {
        let sessions = self.root.path().join(".codex/sessions");
        fs::create_dir_all(&sessions).expect("owned Codex sessions");
        let records = [
            json!({"type":"session_meta","payload":{"id":"primary",
                "timestamp":"2026-10-13T00:00:01Z","cwd":self.workdir,"source":"cli",
                "thread_source":"user"}}),
            json!({"type":"event_msg","timestamp":"2026-10-13T00:00:02Z",
                "payload":{"type":"task_started","turn_id":"one"}}),
            json!({"type":"event_msg","timestamp":"2026-10-13T00:00:03Z",
                "payload":{"type":"token_count","info":{"total_token_usage":{
                "input_tokens":1200,"cached_input_tokens":800,"output_tokens":34,
                "reasoning_output_tokens":12,"total_tokens":1234}}}}),
        ];
        let body: String = records.iter().map(|record| format!("{record}\n")).collect();
        fs::write(sessions.join("rollout-primary.jsonl"), body).expect("owned rollout");
    }
    /// Makes every lookup's `fixture-bin/agent-cli` play opencode: it notes each export in
    /// `calls` and reads `export.json` at once, but prints it only once no `hold` file exists.
    /// The hold is in place.
    #[cfg(unix)]
    fn opencode(&self) {
        use std::os::unix::fs::PermissionsExt;
        let home = self.root.path();
        let path = |name: &str| crate::plan::sh_quote(&home.join(name).to_string_lossy());
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$3\" >> {calls}\nout=$(cat {export})\n\
             while [ -e {hold} ]; do sleep 0.02; done\nprintf '%s' \"$out\"\n",
            calls = path("calls"),
            export = path("export.json"),
            hold = path("hold"),
        );
        let cli = home.join("fixture-bin/agent-cli");
        fs::write(&cli, script).expect("owned fake opencode");
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).expect("owned mode");
        fs::write(home.join("hold"), "").expect("owned hold");
    }
    /// What `opencode export` prints of the task's session: one answer of `input` tokens.
    #[cfg(unix)]
    fn export(&self, input: u64) {
        let export = json!({"info":{"id":OPENCODE_SESSION,"directory":self.workdir},"messages":[
            {"info":{"role":"user"}},
            {"info":{"role":"assistant","tokens":{"input":input,"output":7,"reasoning":2,
                "cache":{"read":30,"write":0}}}}]});
        let path = self.root.path().join("export.json");
        fs::write(path, export.to_string()).expect("owned export");
    }
    /// Lets the held exports print.
    #[cfg(unix)]
    fn release(&self) {
        fs::remove_file(self.root.path().join("hold")).expect("owned hold");
    }
    /// How many exports the fake opencode was asked for.
    #[cfg(unix)]
    fn exports(&self) -> usize {
        let calls = fs::read_to_string(self.root.path().join("calls")).unwrap_or_default();
        calls.lines().count()
    }
    /// Waits until the fake opencode has been asked for `count` exports.
    #[cfg(unix)]
    async fn exports_started(&self, count: usize) {
        let until = Instant::now() + PATIENCE;
        while self.exports() < count {
            assert!(Instant::now() < until, "no refresh read the usage");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    /// Closes the task the way a confirmed discard does, keeping `usage`.
    async fn close(&self, usage: AgentUsage) {
        let review = ClosedReview {
            changes: AgentChanges {
                base: "main".into(),
                ahead: 0,
                behind: 0,
                head_oid: "head".into(),
                base_oid: "base".into(),
                files: vec![],
            },
            commits: vec![],
        };
        self.svc
            .close_record("a", Closure::Discarded, "head".into(), usage, review)
            .await
            .expect("closed record");
    }
}

/// The tokens of the Claude transcript: each answer once, at its largest counts.
fn transcribed() -> Tokens {
    Tokens {
        input: 1200 + 1500,
        output: 30 + 45,
        cache_read: 800 + 900,
        cache_write: 64,
    }
}

/// The tokens of an opencode export of `input` tokens, reasoning counted as output.
#[cfg(unix)]
fn exported(input: u64) -> Tokens {
    Tokens {
        input,
        output: 7 + 2,
        cache_read: 30,
        cache_write: 0,
    }
}

/// How many notices were published to `changes` so far. Each is published before its write
/// replies.
fn notices(changes: &mut HubReceiver) -> usize {
    use futures::FutureExt;
    let mut count = 0;
    while let Some(Some(_)) = changes.recv().now_or_never() {
        count += 1;
    }
    count
}

#[tokio::test]
async fn turn_end_refreshes_open_task_tokens() {
    let f = Fixture::new("claude", Some(CLAUDE_SESSION)).await;
    f.transcript();
    let mut changes = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    f.stop().await;
    // The turn's own change is published before its hook replies; the refresh's comes after.
    assert_eq!(notices(&mut changes), 1);
    f.settled().await;
    assert_eq!(
        notices(&mut changes),
        1,
        "the refresh publishes its total once"
    );
    let listed = f.listed().await;
    let usage = f.svc.usage("a").await.expect("usage");
    assert_eq!(listed.tokens, usage.tokens);
    assert_eq!(listed.tokens, Some(transcribed()));
    assert!(!listed.tokens_truncated);
    // The turn's end set the state, the attention and the activity epoch; the refresh did not.
    assert_eq!(listed.state, AgentState::Idle);
    let attention = listed.attention.expect("attention");
    assert_eq!(
        (attention.kind.as_str(), attention.message.as_str()),
        ("idle", "done")
    );
    assert_eq!(listed.activity_seq, 1);
}

#[tokio::test]
async fn codex_turn_completion_refreshes_tokens() {
    let f = Fixture::new("codex", None).await;
    f.rollout();
    let mut changes = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    let done = json!({"type":"agent-turn-complete","thread-id":"primary","turn-id":"one",
        "cwd":f.workdir,"last-assistant-message":"done"});
    f.report(HookSource::Codex, done).await;
    // The binding and the turn's end are published before the report replies.
    assert!(notices(&mut changes) > 0);
    f.settled().await;
    assert_eq!(
        notices(&mut changes),
        1,
        "the refresh publishes its total once"
    );
    let listed = f.listed().await;
    let usage = f.svc.usage("a").await.expect("usage");
    assert_eq!(listed.tokens, usage.tokens);
    let total = Tokens {
        input: 1200,
        output: 34,
        cache_read: 800,
        cache_write: 0,
    };
    assert_eq!(listed.tokens, Some(total));
    assert!(!listed.tokens_truncated);
    assert_eq!(listed.state, AgentState::Idle);
}

#[cfg(unix)]
#[tokio::test]
async fn token_refresh_never_touches_a_closed_task() {
    let f = Fixture::new("opencode", Some(OPENCODE_SESSION)).await;
    f.opencode();
    f.export(5000);
    f.report(HookSource::Opencode, json!({"v":1,"event":"idle"}))
        .await;
    f.exports_started(1).await;
    // The task closes while the refresh reads its usage.
    let closing = AgentUsage {
        tokens: Some(exported(4000)),
        truncated: false,
    };
    f.close(closing).await;
    let mut changes = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    f.release();
    f.settled().await;
    assert_eq!(f.exports(), 1, "the refresh read the usage");
    let closed = f.svc.get("a").await.expect("closed task");
    assert_eq!(closed.state, AgentState::Closed);
    assert_eq!(
        (closed.tokens, closed.tokens_truncated),
        (Some(exported(4000)), false)
    );
    assert_eq!(notices(&mut changes), 0);
}

#[tokio::test]
async fn token_refresh_keeps_the_last_total_when_the_read_has_none() {
    let f = Fixture::new("claude", Some(CLAUDE_SESSION)).await;
    let transcript = f.transcript();
    f.stop().await;
    f.settled().await;
    assert_eq!(f.listed().await.tokens, Some(transcribed()));
    // The session's transcript is gone, so a read finds no tokens.
    fs::remove_file(transcript).expect("owned transcript");
    assert_eq!(f.svc.usage("a").await.expect("usage").tokens, None);
    let prompt = json!({"hook_event_name":"UserPromptSubmit","session_id":CLAUDE_SESSION});
    f.report(HookSource::Claude, prompt).await;
    let mut changes = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    f.stop().await;
    assert_eq!(notices(&mut changes), 1, "the turn's end");
    f.settled().await;
    assert_eq!(notices(&mut changes), 0, "the refresh stored nothing");
    let task = f.listed().await;
    assert_eq!(task.state, AgentState::Idle);
    assert_eq!(
        (task.tokens, task.tokens_truncated),
        (Some(transcribed()), false)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn token_refreshes_coalesce_per_task() {
    let f = Fixture::new("opencode", Some(OPENCODE_SESSION)).await;
    f.opencode();
    f.export(5000);
    let (working, idle) = (
        json!({"v":1,"event":"working"}),
        json!({"v":1,"event":"idle"}),
    );
    f.report(HookSource::Opencode, idle.clone()).await;
    f.exports_started(1).await;
    // Two more turns end while the first refresh reads.
    for _ in 0..2 {
        f.report(HookSource::Opencode, working.clone()).await;
        f.report(HookSource::Opencode, idle.clone()).await;
    }
    assert_eq!(f.svc.background.len(), 1, "one refresh runs for the task");
    // What those turns added is what the read after them finds.
    f.export(6000);
    f.release();
    f.settled().await;
    assert_eq!(
        f.exports(),
        2,
        "the turns during the read are read once more"
    );
    let task = f.listed().await;
    assert_eq!(task.state, AgentState::Idle);
    assert_eq!(
        (task.tokens, task.tokens_truncated),
        (Some(exported(6000)), false)
    );
}

#[tokio::test]
async fn close_keeps_the_refreshed_total_when_the_closing_read_fails() {
    let f = Fixture::new("claude", Some(CLAUDE_SESSION)).await;
    let transcript = f.transcript();
    f.stop().await;
    f.settled().await;
    assert_eq!(f.listed().await.tokens, Some(transcribed()));
    // A folder stands where the transcript was, which no read takes for the session's file.
    fs::remove_file(&transcript).expect("owned transcript");
    fs::create_dir(&transcript).expect("owned unreadable transcript");
    let closing = f.svc.closure_usage("a").await;
    assert_eq!((closing.tokens.clone(), closing.truncated), (None, true));
    f.close(closing).await;
    let closed = f.svc.get("a").await.expect("closed task");
    assert_eq!(closed.state, AgentState::Closed);
    // The total refreshed while the task was open stays, marked as possibly short.
    assert_eq!(
        (closed.tokens, closed.tokens_truncated),
        (Some(transcribed()), true)
    );
}
