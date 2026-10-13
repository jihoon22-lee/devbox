use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AgentState {
    Preparing,
    Running,
    Waiting,
    Idle,
    Failed,
    Review,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CloseHow {
    Merged,
    PullRequest,
    Discarded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    SetupOk,
    SetupFailed { code: i32 },
    SessionStarted { session_id: String },
    PromptSubmitted,
    NeedsInput { message: String },
    TurnComplete { message: Option<String> },
    OutputWaiting,
    OutputResumed,
    Exited { code: i32 },
    SessionLost,
    UserInstructed,
    UserReview,
    Closed { how: CloseHow },
}

pub fn next(s: AgentState, e: &AgentEvent) -> Option<AgentState> {
    use AgentEvent as E;
    use AgentState as S;
    if s == S::Closed {
        return None;
    }
    let live = matches!(s, S::Running | S::Waiting | S::Idle);
    let to = match e {
        E::SetupOk if s == S::Preparing => S::Running,
        E::SetupFailed { .. } if s == S::Preparing => S::Failed,
        E::SessionStarted { .. } if s == S::Preparing => S::Running,
        E::PromptSubmitted if live => S::Running,
        E::NeedsInput { .. } if live => S::Waiting,
        E::TurnComplete { .. } if live => S::Idle,
        E::OutputWaiting if s == S::Running => S::Waiting,
        E::OutputResumed if s == S::Waiting => S::Running,
        E::Exited { code: 0 } if live || s == S::Preparing => S::Review,
        E::Exited { .. } if live || s == S::Preparing => S::Failed,
        E::SessionLost if live || s == S::Preparing => S::Failed,
        E::UserInstructed if matches!(s, S::Idle | S::Waiting | S::Failed | S::Review) => {
            S::Running
        }
        E::UserReview => S::Review,
        E::Closed { .. } => S::Closed,
        _ => return None,
    };
    (to != s).then_some(to)
}

pub fn is_attention(s: AgentState) -> bool {
    matches!(
        s,
        AgentState::Waiting | AgentState::Idle | AgentState::Failed
    )
}

/// `failed`로 갈 때 남길 이유. 셸의 `$?`는 신호로 끝나면 128+신호 번호다.
/// 외부 종료(이 PC의 wsl-resource-guard 화면의 세션 종료, 커널 OOM 등)를 일반 실패와 구분해 보여 주기 위해서다.
pub fn fail_reason(e: &AgentEvent) -> Option<String> {
    match e {
        AgentEvent::Exited { code } if (129..=192).contains(code) => Some(match code - 128 {
            9 => "killed:KILL".into(),
            15 => "killed:TERM".into(),
            n => format!("killed:{n}"),
        }),
        AgentEvent::Exited { code } if *code != 0 => Some(format!("exit:{code}")),
        AgentEvent::SetupFailed { code } => Some(format!("setup:{code}")),
        AgentEvent::SessionLost => Some("session_lost".into()),
        _ => None,
    }
}

#[cfg(test)]
mod fail_reason_tests {
    use super::*;
    #[test]
    fn signal_exits_are_reported_as_external_kills() {
        assert_eq!(
            fail_reason(&AgentEvent::Exited { code: 137 }).as_deref(),
            Some("killed:KILL")
        );
        assert_eq!(
            fail_reason(&AgentEvent::Exited { code: 143 }).as_deref(),
            Some("killed:TERM")
        );
        assert_eq!(
            fail_reason(&AgentEvent::Exited { code: 2 }).as_deref(),
            Some("exit:2")
        );
        assert_eq!(fail_reason(&AgentEvent::Exited { code: 0 }), None);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use AgentEvent as E;
    use AgentState as S;

    #[test]
    fn follows_the_design_table() {
        let cases = [
            (S::Preparing, E::SetupOk, Some(S::Running)),
            (S::Preparing, E::SetupFailed { code: 1 }, Some(S::Failed)),
            (
                S::Running,
                E::NeedsInput {
                    message: "Bash?".into(),
                },
                Some(S::Waiting),
            ),
            (S::Running, E::TurnComplete { message: None }, Some(S::Idle)),
            (S::Idle, E::PromptSubmitted, Some(S::Running)),
            (S::Waiting, E::PromptSubmitted, Some(S::Running)),
            (S::Running, E::OutputWaiting, Some(S::Waiting)),
            (S::Waiting, E::OutputResumed, Some(S::Running)),
            (S::Idle, E::OutputWaiting, None),
            (S::Running, E::Exited { code: 0 }, Some(S::Review)),
            (S::Waiting, E::Exited { code: 2 }, Some(S::Failed)),
            (S::Idle, E::SessionLost, Some(S::Failed)),
            (S::Failed, E::UserInstructed, Some(S::Running)),
            (S::Review, E::UserInstructed, Some(S::Running)),
            (S::Idle, E::UserReview, Some(S::Review)),
            (
                S::Review,
                E::Closed {
                    how: CloseHow::Merged,
                },
                Some(S::Closed),
            ),
            (S::Closed, E::PromptSubmitted, None),
            (S::Closed, E::SessionLost, None),
        ];
        for (from, ev, want) in cases {
            assert_eq!(next(from, &ev), want, "{from:?} + {ev:?}");
        }
    }

    #[test]
    fn attention_states_are_waiting_idle_and_failed() {
        assert!(is_attention(S::Waiting) && is_attention(S::Idle) && is_attention(S::Failed));
        assert!(!is_attention(S::Running) && !is_attention(S::Review) && !is_attention(S::Closed));
    }
}

#[cfg(test)]
mod transition_tests {
    use super::*;
    #[test]
    fn live_states_and_closed_terminal_behavior() {
        use AgentEvent as E;
        use AgentState as S;
        for state in [S::Running, S::Waiting, S::Idle] {
            for (event, target) in [
                (E::PromptSubmitted, S::Running),
                (
                    E::NeedsInput {
                        message: "question".into(),
                    },
                    S::Waiting,
                ),
                (
                    E::TurnComplete {
                        message: Some("done".into()),
                    },
                    S::Idle,
                ),
                (E::Exited { code: 0 }, S::Review),
                (E::Exited { code: 1 }, S::Failed),
                (E::SessionLost, S::Failed),
            ] {
                assert_eq!(next(state, &event), (state != target).then_some(target));
                assert_eq!(next(S::Closed, &event), None);
            }
        }
        for state in [
            S::Preparing,
            S::Running,
            S::Waiting,
            S::Idle,
            S::Failed,
            S::Review,
        ] {
            assert_eq!(
                next(
                    state,
                    &E::Closed {
                        how: CloseHow::Discarded
                    }
                ),
                Some(S::Closed)
            );
            assert_eq!(
                next(state, &E::UserReview),
                (state != S::Review).then_some(S::Review)
            );
            assert_eq!(
                is_attention(state),
                matches!(state, S::Waiting | S::Idle | S::Failed)
            );
        }
        let session = E::SessionStarted {
            session_id: "id".into(),
        };
        assert_eq!(next(S::Preparing, &session), Some(S::Running));
        assert_eq!(next(S::Running, &session), None);
        assert_eq!(next(S::Preparing, &E::SessionLost), Some(S::Failed));
        assert_eq!(
            fail_reason(&E::SetupFailed { code: 7 }).as_deref(),
            Some("setup:7")
        );
        assert_eq!(
            fail_reason(&E::SessionLost).as_deref(),
            Some("session_lost")
        );
        for (code, reason) in [
            (129, "killed:1"),
            (192, "killed:64"),
            (193, "exit:193"),
            (-1, "exit:-1"),
        ] {
            assert_eq!(fail_reason(&E::Exited { code }).as_deref(), Some(reason));
        }
        assert_eq!(fail_reason(&session), None);
    }
}
