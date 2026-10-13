use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppIntent {
    Palette,
    BottomTerminal { was_focused: bool },
    Quit,
}
#[derive(Default)]
pub struct PendingIntents {
    lease: u64,
    ready: bool,
    pending: VecDeque<AppIntent>,
}
impl PendingIntents {
    pub fn ready(&mut self) -> u64 {
        self.lease = self.lease.saturating_add(1);
        self.ready = true;
        self.lease
    }
    pub fn not_ready(&mut self, lease: u64) {
        if lease == self.lease {
            self.ready = false;
        }
    }
    pub fn disconnect(&mut self) {
        self.ready = false;
    }
    pub fn enqueue(&mut self, intent: AppIntent) {
        self.pending.push_back(intent);
    }
    pub fn front(&self) -> Option<&AppIntent> {
        if self.ready {
            self.pending.front()
        } else {
            None
        }
    }
    pub fn delivered(&mut self) {
        self.pending.pop_front();
    }
}
pub fn close_to_tray(label: &str, tray_available: bool) -> bool {
    label == "main" && tray_available
}
pub fn reveal_on_start(minimized: bool, deep_link: bool) -> bool {
    !minimized || deep_link
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cold_intents_wait_for_listeners_and_stale_cleanup_preserves_new_listener() {
        let mut intents = PendingIntents::default();
        intents.enqueue(AppIntent::Palette);
        assert!(intents.front().is_none());
        let old = intents.ready();
        let latest = intents.ready();
        intents.not_ready(old);
        assert_eq!(intents.front(), Some(&AppIntent::Palette));
        intents.delivered();
        assert!(intents.front().is_none());
        intents.not_ready(latest);
        intents.enqueue(AppIntent::Quit);
        assert!(intents.front().is_none());
        intents.ready();
        assert_eq!(intents.front(), Some(&AppIntent::Quit));
        intents.delivered();
        intents.enqueue(AppIntent::BottomTerminal { was_focused: true });
        assert_eq!(
            intents.front(),
            Some(&AppIntent::BottomTerminal { was_focused: true })
        );
        intents.disconnect();
        assert!(intents.front().is_none());
    }
    #[test]
    fn only_main_with_a_working_tray_hides_and_deep_links_override_minimized() {
        assert!(close_to_tray("main", true));
        assert!(!close_to_tray("main", false));
        assert!(!close_to_tray("popout-terminal-1", true));
        assert!(!reveal_on_start(true, false));
        assert!(reveal_on_start(true, true));
    }
}
