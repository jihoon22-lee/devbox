#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_exit_requests_wait_until_cleanup_has_completed() {
        let shutdown = Shutdown::default();
        assert_eq!(shutdown.request(), ExitAction::StartCleanup);
        assert_eq!(shutdown.request(), ExitAction::Wait);
        assert_eq!(shutdown.request(), ExitAction::Wait);
        shutdown.completed();
        assert_eq!(shutdown.request(), ExitAction::Allow);
    }
}
use std::sync::atomic::{AtomicU8, Ordering};
#[derive(Default)]
pub struct Shutdown(AtomicU8);
#[derive(Debug, PartialEq, Eq)]
pub enum ExitAction {
    StartCleanup,
    Wait,
    Allow,
}
impl Shutdown {
    pub fn request(&self) -> ExitAction {
        match self
            .0
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => ExitAction::StartCleanup,
            Err(1) => ExitAction::Wait,
            Err(_) => ExitAction::Allow,
        }
    }
    pub fn completed(&self) {
        self.0.store(2, Ordering::SeqCst);
    }
}
