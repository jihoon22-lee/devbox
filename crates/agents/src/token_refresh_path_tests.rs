//! Turn ends that refresh the total other than a hook's turn from running to idle: a Codex turn
//! that ends on a task still idle, and a resumed session's ready judgement. Also a refresh that
//! panics, which must not keep the task from refreshing again.
use super::*;
#[cfg(unix)]
use crate::command_path::{ShellAnswer, ShellProbe};
#[cfg(unix)]
use futures::future::BoxFuture;
use std::io::Write;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};

impl Fixture {
    /// Codex begins turn `turn` of thread `primary` at `at`; the session's tokens are `total`
    /// after it.
    fn next_turn(&self, turn: &str, at: &str, total: &Tokens) {
        let records = [
            json!({"type":"event_msg","timestamp":at,
                "payload":{"type":"task_started","turn_id":turn}}),
            json!({"type":"event_msg","timestamp":at,"payload":{"type":"token_count",
                "info":{"total_token_usage":{"input_tokens":total.input,
                "cached_input_tokens":total.cache_read,"output_tokens":total.output}}}}),
        ];
        let path = self
            .root
            .path()
            .join(".codex/sessions/rollout-primary.jsonl");
        let mut rollout = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("owned rollout");
        for record in records {
            writeln!(rollout, "{record}").expect("owned rollout");
        }
    }
    /// The current launch resumed the tool's session, which waits at its prompt for the first
    /// instruction (see `Binding::resumed_session`).
    async fn resumed(&self) {
        self.svc
            .db
            .write(|tx| {
                let mut binding =
                    store::binding(tx, "a")?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                binding.resumed_session = true;
                store::put_binding(tx, "a", &binding)
            })
            .await
            .expect("owned resumed launch");
    }
}

#[tokio::test]
async fn codex_turn_on_an_idle_task_refreshes_tokens() {
    let f = Fixture::new("codex", None).await;
    f.rollout();
    f.codex_done("one", "done").await;
    f.settled().await;
    let first = Tokens {
        input: 1200,
        output: 34,
        cache_read: 800,
        cache_write: 0,
    };
    assert_eq!(f.listed().await.tokens, Some(first));
    // A follow-up typed in Codex's pane ends before any poll sees it start, so the task is still
    // idle when its notify comes.
    let second = Tokens {
        input: 2600,
        output: 70,
        cache_read: 1700,
        cache_write: 0,
    };
    f.next_turn("two", "2026-10-13T00:00:04Z", &second);
    assert_eq!(f.listed().await.state, AgentState::Idle);
    f.codex_done("two", "done again").await;
    assert_eq!(f.svc.background.len(), 1, "the turn's end starts a refresh");
    f.settled().await;
    let listed = f.listed().await;
    let usage = f.svc.usage("a").await.expect("usage");
    assert_eq!(listed.tokens, usage.tokens);
    assert_eq!(listed.tokens, Some(second));
    // The same report again changes nothing, so nothing is read.
    let mut changes = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    f.codex_done("two", "done again").await;
    assert!(
        f.svc.background.is_empty(),
        "a repeated report reads nothing"
    );
    assert_eq!(notices(&mut changes), 0);
}

#[tokio::test]
async fn claude_resume_ready_refreshes_tokens() {
    let f = Fixture::new("claude", Some(CLAUDE_SESSION)).await;
    f.resumed().await;
    f.transcript();
    let mut changes = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    // The resumed session starts, so Claude waits for the next instruction.
    let start = json!({"hook_event_name":"SessionStart","session_id":CLAUDE_SESSION,
        "source":"resume"});
    f.report(HookSource::Claude, start).await;
    assert_eq!(notices(&mut changes), 1, "the ready judgement");
    assert_eq!(f.svc.background.len(), 1, "it starts one refresh");
    f.settled().await;
    assert_eq!(
        notices(&mut changes),
        1,
        "the refresh publishes its total once"
    );
    let listed = f.listed().await;
    assert_eq!(listed.state, AgentState::Idle);
    let usage = f.svc.usage("a").await.expect("usage");
    assert_eq!(listed.tokens, usage.tokens);
    assert_eq!(listed.tokens, Some(transcribed()));
}

/// Answers each lookup as the fixture shell does, except the first, which panics the way a bug in
/// the code a refresh runs would.
#[cfg(unix)]
struct PanicsOnce {
    shell: Arc<dyn ShellProbe>,
    asked: AtomicBool,
}
#[cfg(unix)]
impl ShellProbe for PanicsOnce {
    fn command_v<'a>(&'a self, program: &'a str) -> BoxFuture<'a, ShellAnswer> {
        if !self.asked.swap(true, Ordering::SeqCst) {
            return Box::pin(async { panic!("owned test panic in a token refresh") });
        }
        self.shell.command_v(program)
    }
}
#[cfg(unix)]
fn panics_once(home: &Path) -> Arc<dyn ShellProbe> {
    Arc::new(PanicsOnce {
        shell: crate::command_path::fixture_shell(home),
        asked: AtomicBool::new(false),
    })
}

#[cfg(unix)]
#[tokio::test]
async fn a_panicking_refresh_does_not_block_later_refreshes() {
    let f = Fixture::with_shell("opencode", Some(OPENCODE_SESSION), panics_once).await;
    f.opencode();
    f.release();
    f.export(5000);
    let (working, idle) = (
        json!({"v":1,"event":"working"}),
        json!({"v":1,"event":"idle"}),
    );
    // The first refresh panics while it looks up opencode, before it exports anything.
    f.report(HookSource::Opencode, idle.clone()).await;
    f.settled().await;
    assert_eq!(f.exports(), 0);
    assert_eq!(f.listed().await.tokens, None);
    // The next turn's end refreshes again.
    f.report(HookSource::Opencode, working).await;
    f.report(HookSource::Opencode, idle).await;
    f.settled().await;
    assert_eq!(f.exports(), 1, "the later turn's end is read");
    let listed = f.listed().await;
    assert_eq!(listed.tokens, Some(exported(5000)));
    let usage = f.svc.usage("a").await.expect("usage");
    assert_eq!(listed.tokens, usage.tokens);
}
