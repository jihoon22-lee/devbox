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
    serde_json::json!({ "hooks": { "SessionStart": hook, "UserPromptSubmit": hook, "PermissionRequest": hook, "Notification": hook, "Stop": hook } }).to_string()
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
        let settings: serde_json::Value = serde_json::from_str(&claude_settings("/b/a'b")).unwrap();
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "PermissionRequest",
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
            serde_json::from_str(notify.strip_prefix("notify=").unwrap()).unwrap();
        assert_eq!(args[0], "/b/\"quoted\\path");
    }
}
