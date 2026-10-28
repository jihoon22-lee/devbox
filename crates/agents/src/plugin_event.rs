//! Reports from the agent CLIs devbox extends with its own code: the opencode plugin
//! (`assets/opencode-plugin.ts`) and the pi extension. Each report is one JSON argument of
//! `devbox agent event --from <source>`.
use crate::{hooks::HookSource, state::AgentEvent};
use serde::Deserialize;

/// The opencode plugin a task's session loads through `OPENCODE_CONFIG_CONTENT`.
pub(crate) const OPENCODE_PLUGIN: &str = include_str!("../assets/opencode-plugin.ts");

/// One report: `{"v":1,"event":…,"sessionId"?:…,"message"?:…}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginEvent {
    pub v: u32,
    pub event: PluginEventKind,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginEventKind {
    Working,
    Idle,
    NeedsInput,
    InputResolved,
}

/// How much of a request's own text an attention message shows.
const MESSAGE_CHARS: usize = 200;

/// Task events for one report. A report naming its session starts with `SessionStarted`,
/// which changes nothing once the task has that session. Another version or a malformed
/// report means nothing, like a malformed Claude hook.
pub fn parse_plugin_event(source: HookSource, payload: &str) -> Vec<AgentEvent> {
    let tool = match source {
        HookSource::Opencode => "opencode",
        HookSource::Pi => "pi",
        HookSource::Claude | HookSource::Codex | HookSource::Setup | HookSource::Exit => {
            return vec![];
        }
    };
    let Ok(report) = serde_json::from_str::<PluginEvent>(payload) else {
        return vec![];
    };
    if report.v != 1 {
        return vec![];
    }
    let mut events: Vec<AgentEvent> = report
        .session_id
        .filter(|id| !id.is_empty())
        .map(|session_id| AgentEvent::SessionStarted { session_id })
        .into_iter()
        .collect();
    events.push(match report.event {
        PluginEventKind::Working => AgentEvent::PromptSubmitted,
        PluginEventKind::Idle => AgentEvent::TurnComplete {
            message: report.message,
        },
        PluginEventKind::NeedsInput => AgentEvent::NeedsInput {
            message: asking(tool, report.message.as_deref().unwrap_or_default()),
        },
        // The plugin pairs requests with replies and sends this only when none is open.
        PluginEventKind::InputResolved => AgentEvent::OutputResumed,
    });
    events
}

/// What waits for the user: an opencode permission by its name, or pi's folder trust question.
fn asking(tool: &str, message: &str) -> String {
    if tool == "pi" && message == "project_trust" {
        return "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다.".into();
    }
    let message: String = message.chars().take(MESSAGE_CHARS).collect();
    if message.is_empty() {
        format!("{tool} 승인 요청")
    } else {
        format!("{tool} 승인 요청: {message}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentEvent as E;
    fn opencode(payload: &str) -> Vec<AgentEvent> {
        parse_plugin_event(HookSource::Opencode, payload)
    }
    #[test]
    fn maps_working_idle_needs_input_and_resolved() {
        assert_eq!(
            opencode(r#"{"v":1,"event":"working"}"#),
            [E::PromptSubmitted]
        );
        assert_eq!(
            opencode(r#"{"v":1,"event":"idle"}"#),
            [E::TurnComplete { message: None }]
        );
        assert_eq!(
            opencode(r#"{"v":1,"event":"needsInput","message":"bash"}"#),
            [E::NeedsInput {
                message: "opencode 승인 요청: bash".into()
            }]
        );
        assert_eq!(
            opencode(r#"{"v":1,"event":"inputResolved"}"#),
            [E::OutputResumed]
        );
        // The message is cut at 200 characters, not bytes.
        let long = format!(
            r#"{{"v":1,"event":"needsInput","message":"{}"}}"#,
            "권".repeat(201)
        );
        assert_eq!(
            opencode(&long),
            [E::NeedsInput {
                message: format!("opencode 승인 요청: {}", "권".repeat(200))
            }]
        );
        assert_eq!(
            parse_plugin_event(
                HookSource::Pi,
                r#"{"v":1,"event":"needsInput","message":"project_trust"}"#
            ),
            [E::NeedsInput {
                message: "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다.".into()
            }]
        );
        assert_eq!(
            parse_plugin_event(HookSource::Pi, r#"{"v":1,"event":"idle"}"#),
            [E::TurnComplete { message: None }]
        );
        // Only the plugin sources report this way.
        for source in [
            HookSource::Claude,
            HookSource::Codex,
            HookSource::Setup,
            HookSource::Exit,
        ] {
            assert!(parse_plugin_event(source, r#"{"v":1,"event":"working"}"#).is_empty());
        }
    }
    #[test]
    fn first_session_id_emits_session_started() {
        assert_eq!(
            opencode(r#"{"v":1,"event":"working","sessionId":"ses_root"}"#),
            [
                E::SessionStarted {
                    session_id: "ses_root".into()
                },
                E::PromptSubmitted,
            ]
        );
        assert_eq!(
            opencode(r#"{"v":1,"event":"working","sessionId":""}"#),
            [E::PromptSubmitted]
        );
        // A repeated session changes nothing on a task that has it.
        let mut task: crate::model::AgentTask = serde_json::from_value(serde_json::json!({
            "id": "a", "projectId": "p", "title": "t", "profile": "opencode",
            "baseBranch": "main", "branch": null, "worktreePath": null, "workdir": "/w",
            "sessionId": null, "terminalId": null, "state": "running", "reason": null,
            "attention": null, "port": 1, "prompt": "", "followUps": [], "tokens": null,
            "prUrl": null, "closedHow": null, "createdMs": 1, "updatedMs": 1, "closedMs": null,
        }))
        .expect("owned task");
        let started = &opencode(r#"{"v":1,"event":"working","sessionId":"ses_root"}"#)[0];
        assert!(crate::events::apply_event(&mut task, started, 2));
        assert_eq!(task.session_id.as_deref(), Some("ses_root"));
        assert!(!crate::events::apply_event(&mut task, started, 3));
    }
    #[test]
    fn unknown_version_or_garbage_is_ignored() {
        for payload in [
            "",
            "not json",
            "{}",
            "[]",
            "null",
            r#"{"v":2,"event":"working"}"#,
            r#"{"v":0,"event":"idle"}"#,
            r#"{"v":"1","event":"working"}"#,
            r#"{"event":"working"}"#,
            r#"{"v":1}"#,
            r#"{"v":1,"event":"started"}"#,
            r#"{"v":1,"event":"Working"}"#,
            r#"{"v":1,"event":"working","extra":true}"#,
            r#"{"v":1,"event":"needsInput","message":7}"#,
            r#"{"v":1,"event":"working","sessionId":["ses"]}"#,
        ] {
            assert!(opencode(payload).is_empty(), "{payload}");
        }
    }
}
