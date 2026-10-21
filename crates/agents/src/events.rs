use crate::{
    AgentsAttentionTopic, AgentsChangedTopic,
    model::{AgentTask, AgentsAttention, AgentsChanged, Attention},
    state::{self, AgentEvent, AgentState},
};
use devbox_core::Hub;

pub(crate) fn changed(hub: &Hub, task: &AgentTask, rev: u64, entered_attention: bool) {
    hub.publish::<AgentsChangedTopic>(
        rev,
        &AgentsChanged {
            rev: rev.to_string(),
            id: task.id.clone(),
        },
    );
    if entered_attention && state::is_attention(task.state) {
        hub.publish::<AgentsAttentionTopic>(
            rev,
            &AgentsAttention {
                id: task.id.clone(),
                state: task.state,
                message: task
                    .attention
                    .as_ref()
                    .map(|a| a.message.clone())
                    .unwrap_or_default(),
            },
        );
    }
}
pub(crate) fn apply_event(t: &mut AgentTask, event: &AgentEvent, now: i64) -> bool {
    if matches!(event, AgentEvent::OutputWaiting) && t.state != AgentState::Running {
        return false;
    }
    if matches!(event, AgentEvent::OutputResumed)
        && t.attention.as_ref().is_none_or(|a| a.kind != "output")
    {
        return false;
    }
    if t.state == AgentState::Closed {
        return false;
    }
    let ended = matches!(
        event,
        AgentEvent::Exited { .. } | AgentEvent::SetupFailed { .. } | AgentEvent::SessionLost
    );
    if t.tool_exited
        && !ended
        && !matches!(event, AgentEvent::UserReview | AgentEvent::Closed { .. })
    {
        return false;
    }
    let exit_changed = ended && !t.tool_exited;
    t.tool_exited |= ended;
    let mut updated = exit_changed
        || matches!(
            event,
            AgentEvent::PromptSubmitted | AgentEvent::UserInstructed
        ) && matches!(
            t.state,
            AgentState::Running | AgentState::Waiting | AgentState::Idle
        );
    let mut transitioned = false;
    if let AgentEvent::SessionStarted { session_id } = event
        && t.session_id.as_ref() != Some(session_id)
    {
        t.session_id = Some(session_id.clone());
        updated = true;
    }
    if let Some(next) = state::next(t.state, event) {
        transitioned = true;
        t.state = next;
        t.reason = state::fail_reason(event);
        updated = true;
    }
    let message = match event {
        AgentEvent::NeedsInput { message } if t.state == AgentState::Waiting => {
            Some(crate::limits::attention_message(message))
        }
        AgentEvent::TurnComplete { message } if t.state == AgentState::Idle => {
            Some(crate::limits::attention_message(
                message.as_deref().unwrap_or("다음 지시를 기다립니다"),
            ))
        }
        AgentEvent::OutputWaiting if t.state == AgentState::Waiting => {
            Some("입력을 기다립니다".into())
        }
        _ => None,
    };
    if state::is_attention(t.state) && (transitioned || message.is_some()) {
        let message = message.unwrap_or_else(|| {
            t.reason
                .clone()
                .unwrap_or_else(|| "다음 지시를 기다립니다".into())
        });
        let kind = match t.state {
            AgentState::Waiting if matches!(event, AgentEvent::OutputWaiting) => "output",
            AgentState::Waiting => "input",
            AgentState::Idle => "idle",
            _ => "failed",
        };
        let attention_changed = t
            .attention
            .as_ref()
            .is_none_or(|a| a.message != message || a.kind != kind);
        if attention_changed {
            updated = true;
        }
        if transitioned || attention_changed {
            t.attention = Some(Attention {
                kind: kind.into(),
                message,
                at_ms: now,
            });
        }
    } else if transitioned {
        t.attention = None;
    }
    if updated {
        t.activity_seq = t.activity_seq.wrapping_add(1);
        t.updated_ms = now;
    }
    updated
}
