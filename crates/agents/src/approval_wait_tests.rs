use super::*;
use devbox_protocol::Topic;
use serde_json::{Value, json};

async fn launched(f: &Fixture) -> String {
    let launch = uuid::Uuid::new_v4().simple().to_string();
    let mut binding = Binding::new(1);
    binding.launch_id = Some(launch.clone());
    f.seed(binding).await;
    launch
}
/// One Claude hook for the fixture task's current launch.
async fn deliver(f: &Fixture, launch: &str, payload: Value) -> Done {
    let params = AgentEventParams {
        agent_id: "a".into(),
        launch_id: Some(launch.into()),
        source: HookSource::Claude,
        payload: payload.to_string(),
        code: None,
    };
    f.svc.event(params).await.expect("owned Claude hook")
}
fn call(event: &str, tool: &str, input: &Value) -> Value {
    json!({"hook_event_name":event,"tool_name":tool,"tool_input":input})
}

#[tokio::test]
async fn another_tool_ending_neither_writes_nor_publishes_during_an_approval_wait() {
    let f = Fixture::new().await;
    let launch = launched(&f).await;
    let make = json!({"command":"make"});
    let mut changed = f.svc.hub.subscribe(crate::AgentsChangedTopic::NAME);
    let mut attention = f.svc.hub.subscribe(crate::AgentsAttentionTopic::NAME);
    deliver(&f, &launch, call("PermissionRequest", "Bash", &make)).await;
    assert_eq!(f.state().await, AgentState::Waiting);
    assert!(changed.recv().await.is_some() && attention.recv().await.is_some());
    let before = f.svc.get("a").await.expect("waiting task");
    let revision = f.svc.db.rev();
    let other = json!({"file_path":"/owned/a"});
    for event in ["PostToolUse", "PostToolUseFailure", "PermissionDenied"] {
        let done = deliver(&f, &launch, call(event, "Read", &other)).await;
        assert_eq!(done.rev, revision.to_string(), "{event}");
    }
    assert_eq!(f.svc.db.rev(), revision);
    assert_eq!(f.svc.get("a").await.expect("still waiting"), before);
    // Publication happens before the reply, so an empty queue now stays empty.
    let quiet = std::time::Duration::from_millis(20);
    assert!(tokio::time::timeout(quiet, changed.recv()).await.is_err());
    assert!(tokio::time::timeout(quiet, attention.recv()).await.is_err());
    deliver(&f, &launch, call("PostToolUse", "Bash", &make)).await;
    assert_eq!(f.state().await, AgentState::Running);
    assert_eq!(f.svc.db.rev(), revision + 1);
}

#[tokio::test]
async fn a_restarted_daemon_keeps_the_approval_wait_until_the_turn_ends() {
    let mut f = Fixture::new().await;
    let launch = launched(&f).await;
    let (make, check) = (json!({"command":"make"}), json!({"command":"make check"}));
    deliver(&f, &launch, call("PermissionRequest", "Bash", &make)).await;
    let paths = f.svc.paths.clone();
    f.svc.shutdown().await;
    f.svc = Fixture::service(paths);
    let revision = f.svc.db.rev();
    // A request after the restart joins the wait without making it resumable.
    deliver(&f, &launch, call("PermissionRequest", "Bash", &check)).await;
    for input in [&make, &check] {
        deliver(&f, &launch, call("PostToolUse", "Bash", input)).await;
    }
    assert_eq!(f.state().await, AgentState::Waiting);
    assert_eq!(f.svc.db.rev(), revision);
    deliver(&f, &launch, json!({"hook_event_name":"Stop"})).await;
    assert_eq!(f.state().await, AgentState::Idle);
}

#[tokio::test]
async fn a_user_review_ends_the_wait_and_its_approvals() {
    let f = Fixture::new().await;
    let launch = launched(&f).await;
    let make = json!({"command":"make"});
    deliver(&f, &launch, call("PermissionRequest", "Bash", &make)).await;
    assert!(f.svc.approvals.has_pending("a"));
    f.svc
        .update("a", |t| apply_event(t, &AgentEvent::UserReview, now_ms()))
        .await
        .expect("review");
    assert_eq!(f.state().await, AgentState::Review);
    assert!(!f.svc.approvals.has_pending("a"));
}
