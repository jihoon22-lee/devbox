//! Window-close policy is independent of listener ownership and route mounting.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[derive(ts_rs::TS)]
pub enum ClosePolicy {
    StopOnClose,
    #[default]
    KeepListening,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    KeepOwnerAndQuit,
    StopOwnerAndQuit,
}
pub fn close_action(policy: ClosePolicy, installed: bool) -> CloseAction {
    if installed && policy == ClosePolicy::KeepListening {
        CloseAction::KeepOwnerAndQuit
    } else {
        CloseAction::StopOwnerAndQuit
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closing_only_stops_local_or_explicitly_stopped_listeners() {
        assert_eq!(
            close_action(ClosePolicy::KeepListening, true),
            CloseAction::KeepOwnerAndQuit
        );
        assert_eq!(
            close_action(ClosePolicy::StopOnClose, true),
            CloseAction::StopOwnerAndQuit
        );
        assert_eq!(
            close_action(ClosePolicy::KeepListening, false),
            CloseAction::StopOwnerAndQuit
        );
        assert_eq!(ClosePolicy::default(), ClosePolicy::KeepListening);
        assert_eq!(
            serde_json::from_str::<ClosePolicy>("\"stop-on-close\"").unwrap(),
            ClosePolicy::StopOnClose
        );
    }
}
