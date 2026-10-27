use super::*;
use crate::{store, tracker::Binding};

async fn seed(
    f: &Fixture,
    state: AgentState,
    exited: bool,
    launch: &str,
    marker: &str,
    terminal: Option<&str>,
) -> AgentTask {
    let task: AgentTask = serde_json::from_value(serde_json::json!({
        "id":"tool-recovery","projectId":f.project,"title":"Owned recovery","profile":"missing","baseBranch":"main",
        "branch":null,"worktreePath":null,"workdir":f.repo,"sessionId":null,"terminalId":terminal,
        "state":state,"toolExited":exited,"reason":null,"attention":null,"port":20000,"prompt":"",
        "followUps":[],"tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null
    })).expect("owned task");
    let row = task.clone();
    let launch = launch.to_owned();
    f.svc
        .db
        .write(move |tx| {
            store::put(tx, &row)?;
            let mut binding = Binding::new(0);
            binding.launch_id = Some(launch);
            store::put_binding(tx, &row.id, &binding)
        })
        .await
        .expect("owned launch");
    let phase = f.svc.phase_path(&task, "current");
    crate::prepare::private_dir(phase.parent().expect("directory")).expect("data");
    fs::write(phase, marker).expect("owned phase");
    task
}

#[tokio::test]
async fn durable_tool_marker_recovers_exact_live_preparing_launch() {
    let f = Fixture::new().await;
    fs::write(
        f.svc.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned config");
    f.svc
        .terminal
        .tmux()
        .new_session("owned-tool", &f.repo, Some("sleep 60"), &[])
        .await
        .expect("owned live terminal");
    let task = seed(
        &f,
        AgentState::Preparing,
        false,
        "current",
        "tool",
        Some("owned-tool"),
    )
    .await;
    let result = f.svc.reconcile().await;
    f.svc.terminal.tmux().kill_server().await;
    result.expect("restart recovery");
    let current = f.svc.get(&task.id).await.expect("task");
    assert_eq!(
        current.state,
        AgentState::Running,
        "durable Tool marker repairs a lost SetupOk"
    );
    assert!(!current.tool_exited);
    assert_eq!(current.terminal_id, task.terminal_id);
}

#[tokio::test]
async fn durable_tool_marker_never_resurrects_or_overrides_unproven_launches() {
    let f = Fixture::new().await;
    fs::write(
        f.svc.paths.instance_config_dir.join("tmux.conf"),
        devbox_terminal::TMUX_CONF,
    )
    .expect("owned config");
    f.svc
        .terminal
        .tmux()
        .new_session("owned-tool", &f.repo, Some("sleep 60"), &[])
        .await
        .expect("owned live terminal");
    for (state, exited, launch, marker, terminal) in [
        (AgentState::Preparing, false, "current", "tool", None),
        (
            AgentState::Preparing,
            false,
            "current",
            "tool",
            Some("other-terminal"),
        ),
        (
            AgentState::Preparing,
            false,
            "replacement",
            "tool",
            Some("owned-tool"),
        ),
        (
            AgentState::Preparing,
            false,
            "current",
            "starting",
            Some("owned-tool"),
        ),
        (
            AgentState::Preparing,
            true,
            "current",
            "tool",
            Some("owned-tool"),
        ),
        (
            AgentState::Failed,
            false,
            "current",
            "tool",
            Some("owned-tool"),
        ),
        (
            AgentState::Review,
            false,
            "current",
            "tool",
            Some("owned-tool"),
        ),
        (
            AgentState::Closed,
            false,
            "current",
            "tool",
            Some("owned-tool"),
        ),
    ] {
        let task = seed(&f, state, exited, launch, marker, terminal).await;
        let result = f
            .svc
            .recover_tool_exit(&task.id)
            .await
            .expect("phase recovery");
        let current = f.svc.get(&task.id).await.expect("task");
        assert!(!result, "{state:?}/{exited}/{launch}/{marker}/{terminal:?}");
        assert_eq!(current, task);
    }
    f.svc.terminal.tmux().kill_server().await;
}

/// A retry after the daemon began serving must not fail launches started meanwhile:
/// a task created in the window is `Preparing` without a terminal until `prepare` ends.
#[tokio::test]
async fn late_reconcile_only_touches_tasks_unchanged_since_the_first_attempt() {
    let f = Fixture::new().await;
    // No tmux server runs for this fixture, so every terminal counts as gone.
    let lost = seed(&f, AgentState::Running, false, "current", "", Some("gone")).await;
    let mut relaunched = lost.clone();
    relaunched.id = "relaunched".into();
    relaunched.terminal_id = Some("old-terminal".into());
    async fn put(f: &Fixture, task: AgentTask) {
        f.svc
            .db
            .write(move |tx| {
                store::put(tx, &task)?;
                store::put_binding(tx, &task.id, &Binding::new(0))
            })
            .await
            .expect("owned task");
    }
    put(&f, relaunched.clone()).await;
    let known = f.svc.known_tasks().await.expect("tasks before serving");
    let mut created = lost.clone();
    created.id = "created".into();
    created.state = AgentState::Preparing;
    created.terminal_id = None;
    created.branch = Some("agent/created".into());
    created.worktree_path = Some(f.root.path().join("absent").to_string_lossy().into_owned());
    put(&f, created.clone()).await;
    relaunched.terminal_id = Some("new-terminal".into());
    put(&f, relaunched.clone()).await;
    f.svc
        .reconcile_known(Some(&known))
        .await
        .expect("late reconcile");
    let current = f.svc.get(&lost.id).await.expect("lost task");
    assert_eq!(current.state, AgentState::Failed);
    assert_eq!(current.reason.as_deref(), Some("session_lost"));
    assert_eq!(f.svc.get("created").await.expect("created task"), created);
    assert_eq!(
        f.svc.get("relaunched").await.expect("relaunched task"),
        relaunched
    );
}
