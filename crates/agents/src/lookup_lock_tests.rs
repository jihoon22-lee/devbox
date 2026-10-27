//! Looking a CLI up never starts it, and never holds a lock other tasks need.
use super::*;
use crate::{
    command_path::{ShellAnswer, ShellProbe},
    tracker::Binding,
};
use devbox_protocol::Fail;
use futures::future::BoxFuture;
use std::time::Duration;

const HELD: &str = "devbox-owned-held-tool";

/// Holds the lookup of [`HELD`] until released, then finds nothing. Other programs are
/// answered by the fixture shell.
struct Held {
    inner: Arc<dyn ShellProbe>,
    entered: tokio::sync::Notify,
    gate: tokio::sync::Semaphore,
}
impl ShellProbe for Held {
    fn command_v<'a>(&'a self, program: &'a str) -> BoxFuture<'a, ShellAnswer> {
        if program != HELD {
            return self.inner.command_v(program);
        }
        Box::pin(async move {
            self.entered.notify_one();
            match self.gate.acquire().await {
                Ok(_released) => ShellAnswer::Answered(None),
                Err(_) => ShellAnswer::NoAnswer,
            }
        })
    }
}

/// A fixture whose `held` profile runs [`HELD`], and the probe holding its lookup.
async fn held_fixture() -> (Fixture, Arc<Held>) {
    let mut held = None;
    let mut f = Fixture::with_shell(|inner| {
        let probe = Arc::new(Held {
            inner,
            entered: tokio::sync::Notify::new(),
            gate: tokio::sync::Semaphore::new(0),
        });
        held = Some(probe.clone());
        probe as Arc<dyn ShellProbe>
    })
    .await;
    Arc::get_mut(&mut f.svc).expect("owned service").profiles = vec![crate::profile::ToolProfile {
        id: "held".into(),
        label: "Held".into(),
        command: format!("{HELD} --flag"),
        hooks: crate::profile::Hooks::None,
        waiting_patterns: vec![],
    }];
    (f, held.expect("held probe"))
}

/// A task of the fixture project whose current launch is at `phase`: ended, or running in a
/// terminal that no longer exists.
async fn seed(f: &Fixture, id: &str, running: bool, phase: &str) -> AgentTask {
    let task: AgentTask = serde_json::from_value(serde_json::json!({
        "id":id,"projectId":f.project,"title":"Owned","profile":"held","baseBranch":"main",
        "branch":null,"worktreePath":null,"workdir":f.repo,"sessionId":null,
        "terminalId":running.then_some("t-owned-gone"),"state":if running {"running"} else {"failed"},
        "toolExited":!running,"reason":null,"attention":null,"port":20000,"prompt":"owned",
        "followUps":[],"tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,
        "closedMs":null
    }))
    .expect("owned task");
    let row = task.clone();
    f.svc
        .db
        .write(move |tx| {
            store::put(tx, &row)?;
            let mut binding = Binding::new(0);
            binding.launch_id = Some("current".into());
            store::put_binding(tx, &row.id, &binding)
        })
        .await
        .expect("owned launch");
    let path = f.svc.phase_path(&task, "current");
    crate::prepare::private_dir(path.parent().expect("data directory")).expect("owned data");
    fs::write(path, phase).expect("owned phase");
    task
}

#[tokio::test]
async fn task_start_never_runs_the_cli_it_finds() {
    if checkout_fixture::run_child(
        "service::resource_tests::lookup_lock_tests::task_start_never_runs_the_cli_it_finds",
    ) {
        return;
    }
    let f = Fixture::new().await;
    f.svc.create(f.request()).await.expect("reserve");
    f.drain().await;
    let cli = f.svc.paths.home.join("fixture-bin/agent-cli");
    assert!(cli.exists(), "the lookup found the fixture CLI");
    assert!(
        !cli.with_extension("invoked").exists(),
        "a task start never runs the CLI it finds"
    );
}

#[tokio::test]
async fn resume_lookup_does_not_hold_the_action_lock() {
    let (f, held) = held_fixture().await;
    let ended = seed(&f, "a-ended", false, "exit:0").await;
    let running = seed(&f, "a-running", true, "tool").await;
    let svc = f.svc.clone();
    let resume = tokio::spawn(async move { svc.resume("a-ended").await });
    tokio::time::timeout(Duration::from_secs(5), held.entered.notified())
        .await
        .expect("resume reached the lookup");
    // While the shell has not answered, a send to another task goes ahead.
    let sent = tokio::time::timeout(
        Duration::from_secs(2),
        f.svc.send(SendToAgent {
            id: running.id.clone(),
            text: "owned".into(),
        }),
    )
    .await
    .expect("a send is not held up by another task's lookup");
    assert!(
        matches!(sent, Err(Fail::Domain(AgentsError::SessionMissing))),
        "{sent:?}"
    );
    held.gate.add_permits(1);
    let resumed = resume.await.expect("resume task");
    assert!(
        matches!(&resumed, Err(Fail::Domain(AgentsError::ToolMissing { command })) if command == HELD),
        "{resumed:?}"
    );
    assert_eq!(f.svc.get(&ended.id).await.expect("task"), ended);
    assert_eq!(f.svc.get(&running.id).await.expect("task"), running);
}

#[tokio::test]
async fn create_lookup_does_not_hold_the_reservation_lock() {
    if checkout_fixture::run_child(
        "service::resource_tests::lookup_lock_tests::create_lookup_does_not_hold_the_reservation_lock",
    ) {
        return;
    }
    let (f, held) = held_fixture().await;
    let svc = f.svc.clone();
    let mut request = f.request();
    request.profile = "held".into();
    let create = tokio::spawn(async move { svc.create(request).await });
    tokio::time::timeout(Duration::from_secs(5), held.entered.notified())
        .await
        .expect("create reached the lookup");
    // While the shell has not answered, another start reserves its task.
    let other = tokio::time::timeout(Duration::from_secs(2), f.svc.create(f.request()))
        .await
        .expect("a start is not held up by another start's lookup")
        .expect("reserved");
    held.gate.add_permits(1);
    let created = create.await.expect("create task");
    assert!(
        matches!(&created, Err(Fail::Domain(AgentsError::ToolMissing { command })) if command == HELD),
        "{created:?}"
    );
    f.drain().await;
    let tasks = f.svc.list(ListAgents::default()).await.expect("list").items;
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        [other.id.as_str()]
    );
}
