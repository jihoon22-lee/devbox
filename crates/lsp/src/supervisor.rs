use std::collections::VecDeque;
pub const IDLE_MS: u64 = 15 * 60 * 1000;
pub const CRASH_WINDOW_MS: u64 = 5 * 60 * 1000;
#[derive(Default)]
pub struct Supervisor {
    clients: usize,
    idle_since: Option<u64>,
    restarts: VecDeque<u64>,
    pub generation: u64,
    pub failed: bool,
}
impl Supervisor {
    pub fn joined(&mut self, _now: u64) {
        self.clients += 1;
        self.idle_since = None;
    }
    pub fn left(&mut self, now: u64) {
        self.clients = self.clients.saturating_sub(1);
        if self.clients == 0 {
            self.idle_since = Some(now);
        }
    }
    pub fn idle_due(&self, now: u64) -> bool {
        self.clients == 0
            && self
                .idle_since
                .is_some_and(|start| now.saturating_sub(start) >= IDLE_MS)
    }
    pub fn crashed(&mut self, now: u64) -> Option<u64> {
        while self
            .restarts
            .front()
            .is_some_and(|t| now.saturating_sub(*t) > CRASH_WINDOW_MS)
        {
            self.restarts.pop_front();
        }
        if self.failed || self.restarts.len() >= 3 {
            self.failed = true;
            return None;
        }
        let delay = [1000, 5000, 30000][self.restarts.len()];
        self.restarts.push_back(now);
        Some(delay)
    }
    pub fn manual(&mut self) {
        self.restarts.clear();
        self.failed = false;
        self.idle_since = None;
    }
    pub fn ensure_idle(&mut self, now: u64) {
        if self.clients == 0 && self.idle_since.is_none() {
            self.idle_since = Some(now);
        }
    }
    pub fn started(&mut self) {
        self.generation += 1;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crash_loop_stops_after_three_restarts_in_five_minutes() {
        let mut s = Supervisor::default();
        assert_eq!(s.crashed(0), Some(1000));
        assert_eq!(s.crashed(1000), Some(5000));
        assert_eq!(s.crashed(6000), Some(30000));
        assert_eq!(s.crashed(36000), None);
        assert!(s.failed);
        s.manual();
        assert!(!s.failed);
        assert_eq!(s.crashed(40000), Some(1000));
        assert_eq!(s.crashed(40000 + CRASH_WINDOW_MS + 1), Some(1000));
    }
    #[test]
    fn idle_server_stops_after_last_client_leaves() {
        let mut s = Supervisor::default();
        s.joined(0);
        s.joined(0);
        s.left(10);
        assert!(!s.idle_due(IDLE_MS + 10));
        s.left(20);
        assert!(!s.idle_due(IDLE_MS + 19));
        s.joined(IDLE_MS + 19);
        assert!(!s.idle_due(IDLE_MS * 2));
        s.left(IDLE_MS * 2);
        assert!(s.idle_due(IDLE_MS * 3));
    }
}
