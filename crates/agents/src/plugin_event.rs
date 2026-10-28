//! Reports from the agent CLIs devbox extends with its own code: the opencode plugin
//! (`assets/opencode-plugin.ts`) and the pi extension. Each report is one JSON argument of
//! `devbox agent event --from <source>`.
use crate::{hooks::HookSource, state::AgentEvent};
use serde::Deserialize;

/// The opencode plugin a task's session loads through `OPENCODE_CONFIG_CONTENT`.
pub(crate) const OPENCODE_PLUGIN: &str = include_str!("../assets/opencode-plugin.ts");
/// The pi extension a task's session loads with `-e`, next to the user's own extensions.
pub(crate) const PI_EXTENSION: &str = include_str!("../assets/pi-extension.ts");

/// One report: `{"v":1,"event":…,"kind"?:…,"sessionId"?:…,"message"?:…}`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginEvent {
    pub v: u32,
    pub event: PluginEventKind,
    /// What a `needsInput` waits on. Plugins from before the field never send it.
    #[serde(default)]
    pub kind: Option<InputKind>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputKind {
    /// A tool call waiting for approval; the message is the permission's name.
    Permission,
    /// A question to the user; the message is the question.
    Question,
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
            message: asking(
                tool,
                report.kind,
                report.message.as_deref().unwrap_or_default(),
            ),
        },
        // The plugin pairs requests with replies and sends this only when none is open.
        PluginEventKind::InputResolved => AgentEvent::OutputResumed,
    });
    events
}

/// What waits for the user: a permission by its name, a question, or pi's folder trust. The
/// trust report is the one without a kind, so a dialog titled the same is still a dialog.
fn asking(tool: &str, kind: Option<InputKind>, message: &str) -> String {
    if tool == "pi" && kind.is_none() && message == "project_trust" {
        return "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다.".into();
    }
    let what = match kind {
        Some(InputKind::Question) => "질문",
        Some(InputKind::Permission) | None => "승인 요청",
    };
    let message: String = message.chars().take(MESSAGE_CHARS).collect();
    if message.is_empty() {
        format!("{tool} {what}")
    } else {
        format!("{tool} {what}: {message}")
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
    fn a_question_waits_with_its_own_copy() {
        let asking = |source, payload| match &parse_plugin_event(source, payload)[..] {
            [E::NeedsInput { message }] => message.clone(),
            other => panic!("{payload}: {other:?}"),
        };
        let opencode = |payload| asking(HookSource::Opencode, payload);
        assert_eq!(
            opencode(r#"{"v":1,"event":"needsInput","kind":"question","message":"Which option?"}"#),
            "opencode 질문: Which option?"
        );
        assert_eq!(
            opencode(r#"{"v":1,"event":"needsInput","kind":"permission","message":"bash"}"#),
            "opencode 승인 요청: bash"
        );
        assert_eq!(
            opencode(r#"{"v":1,"event":"needsInput","kind":"question"}"#),
            "opencode 질문"
        );
        // A plugin from before the field keeps today's copy.
        assert_eq!(
            opencode(r#"{"v":1,"event":"needsInput","message":"bash"}"#),
            "opencode 승인 요청: bash"
        );
        // pi reuses the field: its extension sends a confirm dialog as a permission and any
        // other dialog as a question.
        let pi = |payload| asking(HookSource::Pi, payload);
        assert_eq!(
            pi(r#"{"v":1,"event":"needsInput","kind":"question","message":"Proceed?"}"#),
            "pi 질문: Proceed?"
        );
        assert_eq!(
            pi(
                r#"{"v":1,"event":"needsInput","kind":"permission","message":"Allow the fixture command?"}"#
            ),
            "pi 승인 요청: Allow the fixture command?"
        );
        // Only the trust report, which has no kind, gets the trust copy: a dialog with the
        // same title is still that dialog.
        assert_eq!(
            pi(r#"{"v":1,"event":"needsInput","kind":"question","message":"project_trust"}"#),
            "pi 질문: project_trust"
        );
        assert_eq!(
            pi(r#"{"v":1,"event":"needsInput","message":"project_trust"}"#),
            "pi가 이 폴더의 프로젝트 설정을 신뢰할지 묻고 있습니다."
        );
        // A kind this version does not know is malformed, like an unknown field.
        assert!(
            parse_plugin_event(
                HookSource::Opencode,
                r#"{"v":1,"event":"needsInput","kind":"other","message":"x"}"#
            )
            .is_empty()
        );
    }
    #[test]
    fn a_message_cut_at_an_emoji_still_parses() {
        // What the plugin sends for a question whose 200th code point is an emoji;
        // opencodePlugin.test.ts checks that it sends exactly this file.
        let report = include_str!("../tests/fixtures/plugin-report-emoji-cut.json");
        let message = format!("{}\u{1F600}", "가".repeat(199));
        assert_eq!(
            opencode(report.trim_end()),
            [E::NeedsInput {
                message: format!("opencode 질문: {message}")
            }]
        );
        // A cut through the emoji's UTF-16 pair leaves a lone surrogate, which JSON can only
        // write as an escape that a report cannot hold: the wait would be lost.
        let lone = format!(
            r#"{{"v":1,"event":"needsInput","kind":"question","message":"{}\ud83d"}}"#,
            "가".repeat(199)
        );
        assert!(opencode(&lone).is_empty());
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
