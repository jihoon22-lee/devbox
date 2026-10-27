use super::*;
use crate::hooks::{HookSource, approval, parse};
use serde_json::{Value, json};

/// One task's hooks through the parse and correlation the service uses.
struct Hooks {
    approvals: Approvals,
    task: AgentTask,
}
impl Hooks {
    fn new() -> Self {
        let task = serde_json::from_str(r#"{"id":"a","projectId":"p","title":"x","profile":"claude","baseBranch":"main","branch":null,"worktreePath":null,"workdir":"/owned","sessionId":null,"terminalId":"term","state":"running","reason":null,"attention":null,"port":20000,"prompt":"","followUps":[],"tokens":null,"prUrl":null,"closedHow":null,"createdMs":0,"updatedMs":0,"closedMs":null}"#).expect("owned fixture");
        Self {
            approvals: Approvals::default(),
            task,
        }
    }
    fn send(&mut self, payload: Value) -> bool {
        self.send_from("L", payload)
    }
    fn send_from(&mut self, launch: &str, payload: Value) -> bool {
        let payload = payload.to_string();
        let events = parse(HookSource::Claude, &payload, None);
        let approval = approval(&payload);
        self.approvals
            .apply(&mut self.task, launch, approval, events)
    }
    fn state(&self) -> AgentState {
        self.task.state
    }
}
fn call(event: &str, agent: Option<&str>, tool: &str, input: &Value) -> Value {
    let mut payload = json!({"hook_event_name":event,"tool_name":tool,"tool_input":input});
    if let Some(agent) = agent {
        payload["agent_id"] = agent.into();
    }
    payload
}
fn notification() -> Value {
    json!({"hook_event_name":"Notification","notification_type":"permission_prompt","message":"needs Bash"})
}
const ENDINGS: [&str; 3] = ["PostToolUse", "PostToolUseFailure", "PermissionDenied"];

#[test]
fn a_parallel_tool_ending_keeps_the_approval_wait() {
    let mut h = Hooks::new();
    let build = json!({"command":"rm -rf build"});
    assert!(h.send(call("PermissionRequest", None, "Bash", &build)));
    assert_eq!(h.state(), AgentState::Waiting);
    let before = serde_json::to_string(&h.task).expect("owned fixture");
    let other = json!({"file_path":"/owned/a"});
    for event in ENDINGS {
        assert!(!h.send(call(event, None, "Read", &other)), "{event}");
        assert_eq!(serde_json::to_string(&h.task).expect("owned task"), before);
    }
    assert!(h.send(call("PostToolUse", None, "Bash", &build)));
    assert_eq!(h.state(), AgentState::Running);
    assert_eq!(h.task.attention, None);
}

#[test]
fn every_pending_approval_must_end_before_the_wait_does() {
    let mut h = Hooks::new();
    let (make, file) = (json!({"command":"make"}), json!({"file_path":"/owned/b"}));
    h.send(call("PermissionRequest", Some("s1"), "Bash", &make));
    h.send(call("PermissionRequest", Some("s2"), "Write", &file));
    assert!(!h.send(call("PostToolUse", Some("s1"), "Bash", &make)));
    assert_eq!(h.state(), AgentState::Waiting);
    assert!(h.send(call("PermissionDenied", Some("s2"), "Write", &file)));
    assert_eq!(h.state(), AgentState::Running);
}

#[test]
fn the_same_tool_call_from_another_agent_does_not_match() {
    let mut h = Hooks::new();
    let make = json!({"command":"make"});
    h.send(call("PermissionRequest", Some("s1"), "Bash", &make));
    for agent in [None, Some("s2")] {
        assert!(
            !h.send(call("PostToolUse", agent, "Bash", &make)),
            "{agent:?}"
        );
        assert_eq!(h.state(), AgentState::Waiting);
    }
}

#[test]
fn identical_concurrent_requests_each_need_their_own_ending() {
    let mut h = Hooks::new();
    let hosts = json!({"file_path":"/etc/hosts"});
    h.send(call("PermissionRequest", None, "Read", &hosts));
    h.send(call("PermissionRequest", None, "Read", &hosts));
    assert!(!h.send(call("PostToolUse", None, "Read", &hosts)));
    assert_eq!(h.state(), AgentState::Waiting);
    assert!(h.send(call("PostToolUse", None, "Read", &hosts)));
    assert_eq!(h.state(), AgentState::Running);
}

#[test]
fn a_wait_without_an_approval_fails_closed_until_the_turn_ends() {
    let (make, check) = (json!({"command":"make"}), json!({"command":"make check"}));
    // A notification-only wait, and a wait whose approvals a daemon restart lost.
    let mut notified = Hooks::new();
    assert!(notified.send(notification()));
    let mut restarted = Hooks::new();
    assert!(restarted.send(call("PermissionRequest", None, "Bash", &make)));
    restarted.approvals = Approvals::default();
    for mut h in [notified, restarted] {
        // The wait may hold a dialog this process never saw: a later request joins it
        // without making it resumable.
        h.send(call("PermissionRequest", Some("s2"), "Bash", &check));
        for (agent, input) in [(None, &make), (Some("s2"), &check)] {
            for event in ENDINGS {
                assert!(!h.send(call(event, agent, "Bash", input)), "{event}");
                assert_eq!(h.state(), AgentState::Waiting);
            }
        }
        assert!(h.send(json!({"hook_event_name":"Stop","last_assistant_message":"done"})));
        assert_eq!(h.state(), AgentState::Idle);
    }
}

#[test]
fn leaving_the_wait_by_any_path_forgets_its_approvals() {
    let make = json!({"command":"make"});
    // Each path then waits again without an approval, or under another launch.
    for path in ["hook", "outside the hook path", "another launch"] {
        let mut h = Hooks::new();
        h.send(call("PermissionRequest", None, "Bash", &make));
        let mut launch = "L";
        match path {
            "hook" => {
                assert!(h.send(json!({"hook_event_name":"UserPromptSubmit"})));
                assert!(h.send(notification()));
            }
            "outside the hook path" => {
                assert!(apply_event(&mut h.task, &AgentEvent::PromptSubmitted, 3));
                let again = AgentEvent::NeedsInput {
                    message: "again".into(),
                };
                assert!(apply_event(&mut h.task, &again, 4));
            }
            _ => launch = "L2",
        }
        assert_eq!(h.state(), AgentState::Waiting, "{path}");
        let ended = call("PostToolUse", None, "Bash", &make);
        assert!(!h.send_from(launch, ended), "{path}");
        assert_eq!(h.state(), AgentState::Waiting, "{path}");
    }
}

#[test]
fn tool_input_matches_regardless_of_key_order_and_whitespace() {
    let requested = approval(
        r#"{"hook_event_name":"PermissionRequest","tool_name":"Edit","tool_input":{"file_path":"/owned/a","edits":[{"old":"x","new":"y"}],"n":1}}"#,
    );
    let ended = approval(
        "{ \"tool_input\" : {\n \"n\" : 1, \"edits\" : [ { \"new\" : \"y\" , \"old\" : \"x\" } ] ,\n \"file_path\":\"/owned/a\" },\n \"tool_name\":\"Edit\", \"hook_event_name\":\"PostToolUse\" }",
    );
    let (Some(Approval::Requested(requested)), Some(Approval::Finished(ended))) =
        (requested, ended)
    else {
        panic!("approval keys for both hooks");
    };
    assert_eq!(requested, ended);
    let differs = |input: Value| key("", "Edit", &input) != requested;
    assert!(differs(
        json!({"file_path":"/owned/a","edits":[{"old":"x","new":"z"}],"n":1})
    ));
    assert!(differs(
        json!({"file_path":"/owned/a","edits":[{"old":"x","new":"y"}],"n":2})
    ));
    assert!(differs(
        json!({"file_path":"/owned/a","edits":[{"old":"x","new":"y"}]})
    ));
    assert_ne!(
        key("s1", "Edit", &json!(null)),
        key("", "s1Edit", &json!(null))
    );
}
