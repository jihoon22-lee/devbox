//! Whether each window is on screen, as the host tells that window's page.
//!
//! The page cannot tell by itself: a window closed to the tray or minimized may keep
//! `document.visibilityState` "visible" (inferred from the tray traffic of #678, not confirmed on
//! Windows). The page stops its display polls while the host says the window is not shown.
use std::collections::HashMap;

/// Sent to the window named in the payload. A page's plain listener also receives other windows'
/// events, so the label travels with the state.
pub const EVENT: &str = "devbox://window-shown";
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Shown<'a> {
    pub label: &'a str,
    pub shown: bool,
}

/// What the host just did to a window, or learned about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// `hide()` succeeded: closed to the tray, toggled away, or a dismissed capture window.
    Hidden,
    /// `show()` succeeded. It does not restore a minimized window.
    Revealed { minimized: bool },
    /// The window took focus, so it is on screen.
    Focused,
    /// The window's own state: after a size event (minimize and restore are size events) or
    /// when its page asks.
    Observed { visible: bool, minimized: bool },
}
impl Signal {
    pub fn shown(self) -> bool {
        match self {
            Self::Hidden => false,
            Self::Revealed { minimized } => !minimized,
            Self::Focused => true,
            Self::Observed { visible, minimized } => visible && !minimized,
        }
    }
}
/// A state the window could not report never hides it: the page then decides alone, as before.
pub fn observed(visible: Option<bool>, minimized: Option<bool>) -> Signal {
    Signal::Observed {
        visible: visible.unwrap_or(true),
        minimized: minimized.unwrap_or(false),
    }
}

/// The last state told to each window label.
#[derive(Default)]
pub struct Presence(HashMap<String, bool>);
impl Presence {
    /// The state to send to `label`'s page: only a change, and the first one known.
    pub fn report(&mut self, label: &str, signal: Signal) -> Option<bool> {
        let shown = signal.shown();
        (self.0.insert(label.to_owned(), shown) != Some(shown)).then_some(shown)
    }
    /// The answer to the page's own question. Later reports are compared with it.
    pub fn answer(&mut self, label: &str, signal: Signal) -> bool {
        let shown = signal.shown();
        self.0.insert(label.to_owned(), shown);
        shown
    }
    /// A destroyed window's label can be used again by a new window.
    pub fn forget(&mut self, label: &str) {
        self.0.remove(label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_to_the_tray_or_minimized_is_not_shown_and_show_or_restore_is() {
        for (signal, shown) in [
            (Signal::Hidden, false),
            (Signal::Revealed { minimized: false }, true),
            // show() from the tray leaves a window that was minimized before it was hidden.
            (Signal::Revealed { minimized: true }, false),
            (Signal::Focused, true),
            (observed(Some(true), Some(false)), true),
            (observed(Some(true), Some(true)), false),
            (observed(Some(false), Some(false)), false),
            (observed(Some(false), Some(true)), false),
        ] {
            assert_eq!(signal.shown(), shown, "{signal:?}");
        }
    }

    #[test]
    fn a_state_the_window_cannot_report_counts_as_shown() {
        assert!(observed(None, None).shown());
        assert!(observed(None, Some(false)).shown());
        assert!(observed(Some(true), None).shown());
        assert!(!observed(None, Some(true)).shown());
        assert!(!observed(Some(false), None).shown());
    }

    #[test]
    fn each_window_gets_only_its_own_changes() {
        let mut presence = Presence::default();
        // Tray close, tray open, minimize, restore: one report each, none for a repeat.
        assert_eq!(presence.report("main", Signal::Hidden), Some(false));
        assert_eq!(presence.report("main", Signal::Hidden), None);
        assert_eq!(
            presence.report("main", observed(Some(false), Some(false))),
            None
        );
        assert_eq!(presence.report("main", Signal::Focused), Some(true));
        assert_eq!(
            presence.report("main", observed(Some(true), Some(false))),
            None
        );
        assert_eq!(
            presence.report("main", observed(Some(true), Some(true))),
            Some(false)
        );
        assert_eq!(
            presence.report("main", observed(Some(true), Some(false))),
            Some(true)
        );
        // Another window is independent, and its first known state is always sent.
        assert_eq!(
            presence.report("popout-agent-a1", Signal::Focused),
            Some(true)
        );
        assert_eq!(
            presence.report("popout-agent-a1", Signal::Hidden),
            Some(false)
        );
        assert_eq!(presence.report("main", Signal::Focused), None);
    }

    #[test]
    fn an_answer_is_the_baseline_for_later_reports_and_a_destroyed_label_starts_over() {
        let mut presence = Presence::default();
        // A sign-in start goes straight to the tray: the page asks and learns it is hidden.
        assert!(!presence.answer("main", observed(Some(false), Some(false))));
        assert_eq!(presence.report("main", Signal::Hidden), None);
        assert_eq!(
            presence.report("main", Signal::Revealed { minimized: false }),
            Some(true)
        );
        assert!(presence.answer("main", observed(Some(true), Some(false))));
        presence.forget("main");
        assert_eq!(presence.report("main", Signal::Focused), Some(true));
    }

    #[test]
    fn the_page_reads_the_label_and_the_state_from_the_event() {
        assert_eq!(EVENT, "devbox://window-shown");
        assert_eq!(
            serde_json::to_string(&Shown {
                label: "main",
                shown: false
            })
            .unwrap(),
            r#"{"label":"main","shown":false}"#
        );
    }
}
