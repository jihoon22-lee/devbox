use crate::approvals::{Approval, key};
use crate::plan::sh_quote;
use crate::state::AgentEvent;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HookSource {
    Claude,
    Codex,
    Setup,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unsupported agent hook source")]
pub struct InvalidHookSource;
impl std::str::FromStr for HookSource {
    type Err = InvalidHookSource;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "setup" => Ok(Self::Setup),
            "exit" => Ok(Self::Exit),
            _ => Err(InvalidHookSource),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CodexCompletionCandidate {
    #[serde(rename = "thread-id")]
    pub thread_id: String,
    #[serde(rename = "turn-id")]
    pub turn_id: String,
    pub cwd: String,
    #[serde(rename = "last-assistant-message")]
    pub message: Option<String>,
}

pub fn parse_codex_candidate(payload: &str) -> Option<CodexCompletionCandidate> {
    #[derive(Deserialize)]
    struct Notify {
        #[serde(rename = "type")]
        kind: String,
        #[serde(flatten)]
        candidate: CodexCompletionCandidate,
    }
    let notify: Notify = serde_json::from_str(payload).ok()?;
    let c = notify.candidate;
    (notify.kind == "agent-turn-complete"
        && !c.thread_id.trim().is_empty()
        && !c.turn_id.trim().is_empty()
        && !c.cwd.trim().is_empty())
    .then_some(c)
}

pub fn claude_settings(devbox: &str) -> String {
    let command = format!("{} agent event --from claude", sh_quote(devbox));
    let hook = serde_json::json!([{ "matcher": "", "hooks": [{ "type": "command", "command": command }] }]);
    serde_json::json!({ "hooks": {
        "SessionStart": hook, "UserPromptSubmit": hook, "PermissionRequest": hook,
        "PostToolUse": hook, "PostToolUseFailure": hook, "PermissionDenied": hook,
        "Notification": hook, "Stop": hook,
    } })
    .to_string()
}
pub fn codex_notify_arg(devbox: &str) -> String {
    format!(
        "notify={}",
        serde_json::json!([devbox, "agent", "event", "--from", "codex"])
    )
}

/// Codex completion is only a candidate; the service must verify thread and turn metadata.
pub fn parse(source: HookSource, payload: &str, code: Option<i32>) -> Vec<AgentEvent> {
    match source {
        HookSource::Setup => vec![if code == Some(0) {
            AgentEvent::SetupOk
        } else {
            AgentEvent::SetupFailed {
                code: code.unwrap_or(-1),
            }
        }],
        HookSource::Exit => vec![AgentEvent::Exited {
            code: code.unwrap_or(-1),
        }],
        HookSource::Codex => vec![],
        HookSource::Claude => {
            #[derive(Deserialize)]
            struct Payload {
                hook_event_name: String,
                session_id: Option<String>,
                tool_name: Option<String>,
                notification_type: Option<String>,
                message: Option<String>,
                last_assistant_message: Option<String>,
            }
            let Ok(p) = serde_json::from_str::<Payload>(payload) else {
                return vec![];
            };
            match p.hook_event_name.as_str() {
                "SessionStart" => p
                    .session_id
                    .filter(|s| !s.trim().is_empty())
                    .map(|session_id| AgentEvent::SessionStarted { session_id })
                    .into_iter()
                    .collect(),
                "UserPromptSubmit" => vec![AgentEvent::PromptSubmitted],
                "PermissionRequest" => vec![AgentEvent::NeedsInput {
                    message: format!(
                        "{} 사용 승인이 필요합니다.",
                        p.tool_name.unwrap_or_else(|| "도구".into())
                    ),
                }],
                // The service applies this only when it ends the wait's last approval
                // (see `approval`). Tool input, output and errors are never read here.
                "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" => {
                    vec![AgentEvent::OutputResumed]
                }
                "Notification" => match p.notification_type.as_deref() {
                    Some("idle_prompt") => vec![AgentEvent::TurnComplete { message: None }],
                    Some("permission_prompt" | "agent_needs_input") => {
                        vec![AgentEvent::NeedsInput {
                            message: p.message.unwrap_or_default(),
                        }]
                    }
                    _ => vec![],
                },
                "Stop" => vec![AgentEvent::TurnComplete {
                    message: p.last_assistant_message,
                }],
                _ => vec![],
            }
        }
    }
}

/// Pairs Claude's approval request with the end of that tool call. Only a digest of
/// (`agent_id`, `tool_name`, `tool_input`) leaves here; `tool_response` is never read.
pub(crate) fn approval(payload: &str) -> Option<Approval> {
    #[derive(Deserialize)]
    struct Call {
        hook_event_name: String,
        agent_id: Option<String>,
        tool_name: Option<String>,
        #[serde(default)]
        tool_input: serde_json::Value,
    }
    let call: Call = serde_json::from_str(payload).ok()?;
    let key = || {
        key(
            call.agent_id.as_deref().unwrap_or_default(),
            call.tool_name.as_deref().unwrap_or_default(),
            &call.tool_input,
        )
    };
    match call.hook_event_name.as_str() {
        "PermissionRequest" => Some(Approval::Requested(key())),
        "PostToolUse" | "PostToolUseFailure" | "PermissionDenied" => {
            Some(Approval::Finished(key()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AgentEvent as E;
    #[test]
    fn parses_claude_and_lifecycle_events() {
        assert_eq!(
            parse(
                HookSource::Claude,
                r#"{"hook_event_name":"SessionStart","session_id":"abc"}"#,
                None
            ),
            vec![E::SessionStarted {
                session_id: "abc".into()
            }]
        );
        assert_eq!(
            parse(
                HookSource::Claude,
                r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash"}"#,
                None
            ),
            vec![E::NeedsInput {
                message: "Bash 사용 승인이 필요합니다.".into()
            }]
        );
        assert_eq!(
            parse(
                HookSource::Claude,
                r#"{"hook_event_name":"Stop","last_assistant_message":"done"}"#,
                None
            ),
            vec![E::TurnComplete {
                message: Some("done".into())
            }]
        );
        assert_eq!(
            parse(HookSource::Setup, "", Some(3)),
            vec![E::SetupFailed { code: 3 }]
        );
        assert_eq!(
            parse(HookSource::Exit, "", Some(0)),
            vec![E::Exited { code: 0 }]
        );
        assert!(parse(HookSource::Claude, "not json", None).is_empty());
    }
    #[test]
    fn notifications_and_prompt_submission_follow_claude_contract() {
        assert_eq!(
            parse(
                HookSource::Claude,
                r#"{"hook_event_name":"UserPromptSubmit"}"#,
                None
            ),
            vec![E::PromptSubmitted]
        );
        for kind in ["permission_prompt", "agent_needs_input"] {
            let payload = format!(
                r#"{{"hook_event_name":"Notification","notification_type":"{kind}","message":"approval"}}"#
            );
            assert_eq!(
                parse(HookSource::Claude, &payload, None),
                vec![E::NeedsInput {
                    message: "approval".into()
                }]
            );
        }
        assert_eq!(
            parse(
                HookSource::Claude,
                r#"{"hook_event_name":"Notification","notification_type":"idle_prompt"}"#,
                None
            ),
            vec![E::TurnComplete { message: None }]
        );
        assert!(
            parse(
                HookSource::Claude,
                r#"{"hook_event_name":"Notification","notification_type":"unknown"}"#,
                None
            )
            .is_empty()
        );
    }
    #[test]
    fn codex_requires_identity_and_never_directly_emits_completion() {
        let payload = r#"{"type":"agent-turn-complete","thread-id":"thread","turn-id":"turn","cwd":"/work","last-assistant-message":"done"}"#;
        let candidate = parse_codex_candidate(payload).expect("candidate");
        assert_eq!(candidate.thread_id, "thread");
        assert_eq!(candidate.turn_id, "turn");
        assert_eq!(candidate.cwd, "/work");
        assert!(parse(HookSource::Codex, payload, None).is_empty());
        for payload in [
            r#"{"type":"agent-turn-complete"}"#,
            r#"{"type":"other","thread-id":"t","turn-id":"u","cwd":"/w"}"#,
            r#"{"type":"agent-turn-complete","thread-id":"","turn-id":"u","cwd":"/w"}"#,
        ] {
            assert!(parse_codex_candidate(payload).is_none());
        }
    }
    #[test]
    fn settings_route_every_hook_and_notify_escapes_paths() {
        let settings: serde_json::Value =
            serde_json::from_str(&claude_settings("/b/a'b")).expect("owned fixture");
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "PermissionRequest",
            "PostToolUse",
            "PostToolUseFailure",
            "PermissionDenied",
            "Notification",
            "Stop",
        ] {
            assert_eq!(
                settings["hooks"][event][0]["hooks"][0]["command"],
                "'/b/a'\\''b' agent event --from claude"
            );
        }
        let notify = codex_notify_arg("/b/\"quoted\\path");
        let args: Vec<String> =
            serde_json::from_str(notify.strip_prefix("notify=").expect("owned fixture"))
                .expect("owned fixture");
        assert_eq!(args[0], "/b/\"quoted\\path");
    }
    #[test]
    fn claude_settings_include_post_tool_use() {
        let settings: serde_json::Value =
            serde_json::from_str(&claude_settings("/b/devbox")).expect("owned fixture");
        // Every way the approved tool call can end, for every tool like the other hooks.
        for event in ["PostToolUse", "PostToolUseFailure", "PermissionDenied"] {
            assert_eq!(
                settings["hooks"][event],
                serde_json::json!([{ "matcher": "", "hooks": [{ "type": "command", "command": "'/b/devbox' agent event --from claude" }] }]),
                "{event}"
            );
        }
    }
    #[test]
    fn post_tool_use_maps_to_output_resumed() {
        // Tool input, output and errors can hold file contents or command output. Only a
        // digest of the call leaves the parser, and the event itself carries nothing.
        let secret = "OWNED_SECRET_MARKER=sk-live-0123456789";
        let input = serde_json::json!({ "command": format!("echo {secret}") });
        let request = serde_json::json!({ "hook_event_name": "PermissionRequest", "agent_id": "s1", "tool_name": "Bash", "tool_input": input, "permission_suggestions": [] });
        let Some(Approval::Requested(requested)) = approval(&request.to_string()) else {
            panic!("approval request");
        };
        for event in ["PostToolUse", "PostToolUseFailure", "PermissionDenied"] {
            let payload = serde_json::json!({
                "hook_event_name": event, "session_id": "sess-1", "agent_id": "s1",
                "tool_name": "Bash", "tool_input": input, "tool_use_id": "toolu_1",
                "tool_response": { "stdout": secret, "stderr": "" },
                "error": secret, "reason": secret, "duration_ms": 1,
            })
            .to_string();
            let events = parse(HookSource::Claude, &payload, None);
            assert_eq!(events, vec![E::OutputResumed], "{event}");
            let ended = approval(&payload);
            assert_eq!(ended, Some(Approval::Finished(requested)), "{event}");
            let shown = format!("{events:?} {ended:?}");
            assert!(!shown.contains("OWNED_SECRET_MARKER") && !shown.contains("toolu_1"));
        }
    }
}
