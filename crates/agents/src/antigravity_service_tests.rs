//! The Antigravity family through the service: a start needs a work folder, a first launch writes
//! the hooks folder, and the reports of agy's hooks. The payloads are real ones of agy 1.5.17,
//! redacted (`tests/fixtures/antigravity-1.5.17/README.md`).
use super::*;
use crate::{hooks::HookSource, tracker::Binding};
use devbox_protocol::Topic;

/// The fixture's content: what follows its `#` provenance lines and the `---` line.
fn fixture(text: &'static str) -> &'static str {
    text.split_once("\n---\n")
        .expect("fixture header ends with a --- line")
        .1
}
fn session_start() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-session-start.json"
    ))
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
fn post_invocation() -> &'static str {
    fixture(include_str!(
        "../tests/fixtures/antigravity-1.5.17/hook-post-invocation.json"
    ))
}
const CONVERSATION: &str = "00000000-0000-4000-8000-000000000001";
const OTHER: &str = "11111111-1111-4111-8111-111111111111";

fn agy_request(f: &Fixture, worktree: bool) -> CreateAgentTask {
    CreateAgentTask {
        profile: "antigravity".into(),
        worktree,
        prompt: "fix the login bug".into(),
        ..f.request()
    }
}

#[tokio::test]
async fn a_start_without_a_work_folder_is_refused_before_anything_exists() {
    let f = Fixture::new().await;
    let refused = f.svc.create(agy_request(&f, false)).await;
    assert!(
        matches!(
            &refused,
            Err(Fail::Domain(AgentsError::WorkFolderRequired { tool })) if tool == "Antigravity"
        ),
        "{refused:?}"
    );
    assert_eq!(
        devbox_protocol::to_wire(&AgentsError::WorkFolderRequired { tool: "x".into() }).code,
        "agents.work_folder_required"
    );
    f.drain().await;
    // No task, no branch, no folder beside the project, and nothing beside the checkout.
    assert!(f.svc.all_tasks(None).await.expect("tasks").is_empty());
    assert_eq!(
        devbox_git::branches(&f.repo).await.expect("branches"),
        ["main"]
    );
    let beside: Vec<_> = fs::read_dir(f.root.path())
        .expect("owned root")
        .map(|entry| entry.expect("entry").file_name())
        .filter(|name| name.to_string_lossy().contains("agy-hooks") || name == "project-agents")
        .collect();
    assert!(beside.is_empty(), "{beside:?}");
    // The check a composite launch makes for each of its agents before it starts anything.
    assert!(f.svc.work_folder_required("antigravity", true).is_ok());
    assert!(matches!(
        f.svc.work_folder_required("antigravity", false),
        Err(AgentsError::WorkFolderRequired { .. })
    ));
    for other in ["claude", "codex", "opencode", "pi", "no-such-profile"] {
        assert!(f.svc.work_folder_required(other, false).is_ok(), "{other}");
    }
}

/// A task of `profile` as a start reserves it, in the work folder `workdir` (or without one).
fn reserved(id: &str, profile: &str, workdir: &Path, worktree: bool) -> AgentTask {
    serde_json::from_value(serde_json::json!({
        "id":id,"projectId":"p","title":"Owned","profile":profile,"baseBranch":"main",
        "branch":worktree.then_some("agent/fix"),"worktreePath":worktree.then_some(workdir),
        "workdir":workdir,"sessionId":null,"terminalId":null,"state":"preparing","reason":null,
        "attention":null,"port":20000,"prompt":"owned prompt","followUps":[],"tokens":null,
        "prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null
    }))
    .expect("owned task")
}

#[tokio::test]
async fn a_launch_loads_the_hooks_folder_beside_the_work_folder() {
    let f = Fixture::new().await;
    let agy = crate::profile::resolve(&[], "antigravity").expect("builtin profile");
    let workdir = f.root.path().join("project-agents/fix");
    fs::create_dir_all(&workdir).expect("owned work folder");
    let task = reserved("a1", "antigravity", &workdir, true);
    let loaded = f
        .svc
        .connection(&task, &agy, true)
        .await
        .expect("first launch")
        .expect("hooks folder");
    let hooks = f.root.path().join("project-agents/fix.agy-hooks");
    assert_eq!(Path::new(&loaded), hooks);
    assert_eq!(
        fs::read_to_string(hooks.join(".agents/hooks.json")).expect("hooks"),
        crate::antigravity::hooks_json("/owned/devbox")
    );
    assert_eq!(
        fs::metadata(&hooks).expect("folder").permissions().mode() & 0o777,
        0o700
    );
    // Nothing of devbox's went into the work folder.
    assert_eq!(fs::read_dir(&workdir).expect("work folder").count(), 0);
    // A resume takes the same folder; a first launch never takes one that is already there.
    let again = f.svc.connection(&task, &agy, false).await.expect("resume");
    assert_eq!(again.as_deref(), Some(loaded.as_str()));
    let taken = f.svc.connection(&task, &agy, true).await;
    assert!(
        matches!(
            &taken,
            Err(Fail::Domain(AgentsError::HooksFolderExists { path })) if Path::new(path) == hooks
        ),
        "{taken:?}"
    );
    // Another task's resume does not take it either: the error names the folder.
    let other = reserved("a9", "antigravity", &workdir, true);
    let foreign = f.svc.connection(&other, &agy, false).await;
    assert!(
        matches!(
            &foreign,
            Err(Fail::Domain(AgentsError::HooksFolderExists { path })) if Path::new(path) == hooks
        ),
        "{foreign:?}"
    );
    // A task in the project's own folder has no sibling to put it in, whatever its profile said
    // when it started.
    let inline = reserved("a2", "antigravity", &f.repo, false);
    for fresh in [true, false] {
        let refused = f.svc.connection(&inline, &agy, fresh).await;
        assert!(
            matches!(
                &refused,
                Err(Fail::Domain(AgentsError::WorkFolderRequired { tool })) if tool == "Antigravity"
            ),
            "{refused:?}"
        );
    }
    assert!(!f.root.path().join("project.agy-hooks").exists());
    // The other families load their own file, or nothing.
    let claude = crate::profile::resolve(&[], "claude").expect("builtin profile");
    let file = f
        .svc
        .connection(&inline, &claude, true)
        .await
        .expect("claude")
        .expect("settings file");
    assert!(file.contains("/agent-hooks/claude-"), "{file}");
    let codex = crate::profile::resolve(&[], "codex").expect("builtin profile");
    assert_eq!(
        f.svc
            .connection(&inline, &codex, true)
            .await
            .expect("codex"),
        None
    );
}

#[tokio::test]
async fn a_folder_of_the_hooks_folders_name_refuses_the_start_before_anything_exists() {
    let f = Fixture::new().await;
    // Beside where the work folder would go: another task's, or not devbox's at all.
    let foreign = f.root.path().join("project-agents/owned.agy-hooks");
    fs::create_dir_all(&foreign).expect("foreign folder");
    fs::write(foreign.join("keep.txt"), "user file").expect("owned file");
    let refused = f.svc.create(agy_request(&f, true)).await;
    // The error names the folder to remove or rename.
    assert!(
        matches!(
            &refused,
            Err(Fail::Domain(AgentsError::HooksFolderExists { path }))
                if Path::new(path) == foreign
        ),
        "{refused:?}"
    );
    assert_eq!(
        devbox_protocol::to_wire(&AgentsError::HooksFolderExists { path: "x".into() }).code,
        "agents.hooks_folder_exists"
    );
    f.drain().await;
    assert!(f.svc.all_tasks(None).await.expect("tasks").is_empty());
    assert_eq!(
        devbox_git::branches(&f.repo).await.expect("branches"),
        ["main"]
    );
    assert!(!f.root.path().join("project-agents/owned").exists());
    assert_eq!(
        fs::read_to_string(foreign.join("keep.txt")).expect("kept"),
        "user file"
    );
    // The other families do not use that folder, and a task without a work folder has none.
    let builtin = |id: &str| crate::profile::resolve(&[], id).expect("builtin profile");
    let beside = f.root.path().join("project-agents/owned");
    let beside = beside.to_str();
    use crate::antigravity_service::hooks_folder_free;
    assert!(hooks_folder_free(&builtin("antigravity"), beside).is_err());
    assert!(hooks_folder_free(&builtin("claude"), beside).is_ok());
    assert!(hooks_folder_free(&builtin("antigravity"), None).is_ok());
    assert!(hooks_folder_free(&builtin("antigravity"), f.repo.to_str()).is_ok());
}

#[tokio::test]
async fn a_prompt_agy_would_run_as_a_command_is_refused_before_anything_exists() {
    let f = Fixture::new().await;
    for prompt in ["!rm -rf build", "/model", " \n /help", "esc \u{1b}[201~"] {
        let refused = f
            .svc
            .create(CreateAgentTask {
                prompt: prompt.into(),
                ..agy_request(&f, true)
            })
            .await;
        let command = matches!(
            &refused,
            Err(Fail::Domain(AgentsError::FirstInstructionCommand { tool })) if tool == "Antigravity"
        );
        let control = matches!(&refused, Err(Fail::Domain(AgentsError::ControlCharacters)));
        assert!(
            if prompt.starts_with("esc") {
                control
            } else {
                command
            },
            "{prompt:?}: {refused:?}"
        );
    }
    assert_eq!(
        devbox_protocol::to_wire(&AgentsError::FirstInstructionCommand { tool: "x".into() }).code,
        "agents.first_instruction_command"
    );
    f.drain().await;
    assert!(f.svc.all_tasks(None).await.expect("tasks").is_empty());
    assert_eq!(
        devbox_git::branches(&f.repo).await.expect("branches"),
        ["main"]
    );
    assert!(!f.root.path().join("project-agents").exists());
    // What a composite launch asks for each of its agents before it starts anything.
    assert!(
        f.svc
            .admits_prompt("antigravity", "fix /src/main.rs")
            .is_ok()
    );
    assert!(f.svc.admits_prompt("antigravity", "/model").is_err());
    assert!(f.svc.admits_prompt("claude", "/model").is_ok());
    assert!(f.svc.admits_prompt("no-such-profile", "/model").is_ok());
}

#[tokio::test]
async fn a_first_launch_queues_its_prompt_and_stops_at_a_folder_that_appeared_meanwhile() {
    let f = Fixture::new().await;
    let reserved = f.svc.create(agy_request(&f, true)).await.expect("reserve");
    // Before the preparation gets to it (nothing above yields to it): a folder of the hooks
    // folder's name that is not the task's appears beside the work folder.
    let foreign = f.root.path().join("project-agents/owned.agy-hooks");
    fs::create_dir_all(&foreign).expect("foreign folder");
    fs::write(foreign.join("keep.txt"), "user file").expect("owned file");
    assert_eq!(
        reserved.worktree_path.as_deref(),
        f.root.path().join("project-agents/owned").to_str()
    );
    // agy gets no prompt argument: the launch waits at agy's prompt for its first instruction,
    // and the task's prompt is queued to be sent there.
    assert_eq!(
        reserved.first_instruction.map(|first| first.delivery),
        Some(InstructionDelivery::Queued)
    );
    let binding = f.svc.launch_binding(&reserved.id).await.expect("binding");
    assert!(binding.resumed_session);
    f.drain().await;
    // The preparation stopped at the folder, before any terminal or tool was started.
    let failed = f.svc.get(&reserved.id).await.expect("task");
    assert_eq!(
        (failed.state, failed.reason.as_deref(), failed.terminal_id),
        (AgentState::Failed, Some("preparation_failed"), None)
    );
    let left: Vec<_> = fs::read_dir(&foreign)
        .expect("foreign folder")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(left, ["keep.txt"]);
    let data = f.svc.paths.data_dir.join("agents").join(&reserved.id);
    assert!(!data.join("start.sh").exists());
    // The tool never started, so the queued prompt was never sent and never will be.
    assert_eq!(
        failed.first_instruction.map(|first| first.delivery),
        Some(InstructionDelivery::NotSent)
    );
}

/// Task `agy` of the builtin Antigravity profile, running on launch `launch` in a work folder.
/// `session` is the conversation the task follows, and `resumed` marks a launch that still waits
/// for its first instruction.
async fn running(f: &Fixture, session: Option<&str>, resumed: bool) {
    let task: AgentTask = serde_json::from_value(serde_json::json!({
        "id":"agy","projectId":f.project,"title":"Owned","profile":"antigravity","baseBranch":"main",
        "branch":"agent/fix","worktreePath":"/owned/project-agents/fix","workdir":"/owned/project-agents/fix",
        "sessionId":session,"terminalId":"term","state":"running","reason":null,"attention":null,
        "port":20000,"prompt":"owned prompt","followUps":[],"tokens":null,"prUrl":null,
        "closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null
    }))
    .expect("owned task");
    let mut binding = Binding::new(1);
    binding.launch_id = Some("launch".into());
    binding.resumed_session = resumed;
    // A resume of a known conversation waits for a hook to confirm it.
    binding.unconfirmed_session = resumed && session.is_some();
    f.svc
        .db
        .write(move |tx| {
            crate::store::put(tx, &task)?;
            crate::store::put_binding(tx, &task.id, &binding)
        })
        .await
        .expect("owned launch");
}

async fn hook_from(
    f: &Fixture,
    launch: Option<&str>,
    payload: &str,
) -> std::result::Result<Done, Fail<AgentsError>> {
    f.svc
        .event(AgentEventParams {
            agent_id: "agy".into(),
            launch_id: launch.map(str::to_owned),
            source: HookSource::Antigravity,
            payload: payload.to_owned(),
            code: None,
        })
        .await
}

/// A hook of the task's current launch.
async fn hook(f: &Fixture, payload: &str) {
    hook_from(f, Some("launch"), payload).await.expect("hook");
}

async fn task(f: &Fixture) -> AgentTask {
    f.svc.get("agy").await.expect("owned task")
}

#[tokio::test]
async fn hooks_record_the_conversation_and_follow_the_turn() {
    let f = Fixture::new().await;
    running(&f, None, true).await;
    let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    // The first prompt of a new conversation.
    hook(&f, session_start()).await;
    let started = task(&f).await;
    assert_eq!(started.session_id.as_deref(), Some(CONVERSATION));
    assert_eq!(started.state, AgentState::Running);
    assert!(
        f.svc
            .launch_binding("agy")
            .await
            .expect("binding")
            .resumed_session
    );
    hook(&f, pre_invocation()).await;
    assert_eq!(task(&f).await.state, AgentState::Running);
    // The launch got its first instruction.
    assert!(
        !f.svc
            .launch_binding("agy")
            .await
            .expect("binding")
            .resumed_session
    );
    hook(&f, stop()).await;
    let idle = task(&f).await;
    assert_eq!(idle.state, AgentState::Idle);
    let shown = idle.attention.clone().expect("attention");
    assert_eq!(
        (shown.kind.as_str(), shown.message.as_str()),
        ("idle", "다음 지시를 기다립니다")
    );
    // The next turn: a model call, a tool call, another model call, the end.
    hook(&f, pre_invocation()).await;
    assert_eq!(task(&f).await.state, AgentState::Running);
    hook(&f, pre_invocation()).await;
    hook(&f, stop()).await;
    assert_eq!(task(&f).await.state, AgentState::Idle);
    assert_eq!(task(&f).await.session_id.as_deref(), Some(CONVERSATION));
    // What the prompt said never reaches the task.
    let stored = serde_json::to_string(&task(&f).await).expect("task");
    assert!(!stored.contains("Reply with OK"), "{stored}");
    let mut notices = 0;
    use futures::FutureExt;
    while let Some(Some(_)) = attention.recv().now_or_never() {
        notices += 1;
    }
    assert_eq!(notices, 2, "one notice for each turn that ended");
}

#[tokio::test]
async fn an_approval_wait_found_on_screen_ends_with_the_next_model_call() {
    let f = Fixture::new().await;
    running(&f, Some(CONVERSATION), false).await;
    // The output watcher saw the approval screen.
    f.svc
        .update("agy", |task| {
            apply_event(task, &AgentEvent::OutputWaiting, now_ms())
        })
        .await
        .expect("waiting");
    assert_eq!(task(&f).await.state, AgentState::Waiting);
    // The user approves in the terminal and the turn goes on.
    hook(&f, pre_invocation()).await;
    let resumed = task(&f).await;
    assert_eq!(
        (resumed.state, resumed.attention),
        (AgentState::Running, None)
    );
}

#[tokio::test]
async fn a_report_of_another_launch_or_none_changes_nothing() {
    let f = Fixture::new().await;
    running(&f, None, true).await;
    let before = (
        task(&f).await,
        f.svc.launch_binding("agy").await.expect("b"),
    );
    for launch in [None, Some(""), Some("first"), Some("launch-of-before")] {
        for payload in [session_start(), pre_invocation(), stop()] {
            hook_from(&f, launch, payload).await.expect("ignored");
        }
    }
    // Events devbox does not register, and payloads that are not agy's.
    for payload in [
        post_invocation(),
        "",
        "not json",
        "{}",
        r#"{"hookEventName":"Stop"}"#,
        r#"{"hook_event_name":"Stop","session_id":"claude-shaped"}"#,
        r#"{"v":1,"event":"idle"}"#,
    ] {
        hook(&f, payload).await;
    }
    assert_eq!(
        (
            task(&f).await,
            f.svc.launch_binding("agy").await.expect("b")
        ),
        before
    );
    // A conversation id that is not an id is refused, as the other families' session ids are.
    for (id, limit) in [("x".repeat(129), true), ("../../etc".to_owned(), false)] {
        let payload = serde_json::json!({ "hookEventName": "Stop", "conversationId": id });
        let refused = hook_from(&f, Some("launch"), &payload.to_string()).await;
        match refused {
            Err(Fail::Domain(AgentsError::InputTooLarge { .. })) if limit => {}
            Err(Fail::Domain(AgentsError::InvalidInput)) if !limit => {}
            other => panic!("{id}: {other:?}"),
        }
    }
    assert_eq!(task(&f).await, before.0);
}

#[path = "antigravity_session_tests.rs"]
mod session;
