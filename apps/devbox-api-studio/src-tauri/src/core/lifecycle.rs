//! Window-close policy is independent of listener ownership and route mounting.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ClosePolicy {
    #[default]
    StopOnClose,
    KeepListening,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAction {
    Quit,
    Hide,
}
pub fn close_action(
    policy: ClosePolicy,
    listener_running: bool,
    tray_available: bool,
) -> CloseAction {
    if policy == ClosePolicy::KeepListening && listener_running && tray_available {
        CloseAction::Hide
    } else {
        CloseAction::Quit
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_explicit_recoverable_background_listening_hides_the_window() {
        for running in [false, true] {
            for tray in [false, true] {
                assert_eq!(
                    close_action(ClosePolicy::StopOnClose, running, tray),
                    CloseAction::Quit
                );
            }
        }
        assert_eq!(
            close_action(ClosePolicy::KeepListening, true, true),
            CloseAction::Hide
        );
        assert_eq!(
            close_action(ClosePolicy::KeepListening, false, true),
            CloseAction::Quit
        );
        assert_eq!(
            close_action(ClosePolicy::KeepListening, true, false),
            CloseAction::Quit
        );
        assert!(serde_json::from_str::<ClosePolicy>("\"start-on-login\"").is_err());
    }
}
