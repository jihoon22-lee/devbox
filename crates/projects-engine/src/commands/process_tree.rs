//! Probe-specific cleanup policy over the shared OS process-tree owner.
//! Preserve the existing 500 ms phases and bounded cancellation Drop backstop.
use std::time::{Duration, Instant};
use tokio::process::Child;

const CLEANUP_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) struct ProcessTree(process_tree::ProcessTree);

impl ProcessTree {
    pub(crate) fn prepare_tokio(command: &mut tokio::process::Command) {
        process_tree::ProcessTree::prepare_tokio(command);
    }

    pub(crate) fn assign(child: &Child) -> Result<Self, ()> {
        process_tree::ProcessTree::assign(child).map(Self)
    }

    pub(crate) async fn terminate(&mut self, child: &mut Child) -> bool {
        let tree_gone = self.terminate_descendants();
        let root_gone = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait())
            .await
            .is_ok_and(|result| result.is_ok());
        tree_gone && root_gone
    }

    pub(crate) fn terminate_descendants(&mut self) -> bool {
        if self.0.is_empty() == Some(true) {
            return self.0.wait_empty_blocking(Instant::now());
        }
        if !self.0.signal(false) && self.0.is_empty() != Some(true) {
            return false;
        }
        if self
            .0
            .wait_empty_blocking(Instant::now() + CLEANUP_TIMEOUT / 2)
        {
            return true;
        }
        if !self.0.signal(true) && self.0.is_empty() != Some(true) {
            return false;
        }
        self.0.wait_empty_blocking(Instant::now() + CLEANUP_TIMEOUT)
    }

    pub(crate) async fn terminate_unassigned(child: &mut Child) {
        let _ = child.kill().await;
        let _ = tokio::time::timeout(CLEANUP_TIMEOUT, child.wait()).await;
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        if self.0.is_empty() == Some(true) {
            let _ = self.0.wait_empty_blocking(Instant::now());
        } else {
            let _ = self.0.signal(true);
            let _ = self.0.wait_empty_blocking(Instant::now() + CLEANUP_TIMEOUT);
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    #[test]
    fn process_tree_can_move_with_its_async_worker() {
        fn assert_send<T: Send>() {}
        assert_send::<super::ProcessTree>();
    }

    #[test]
    fn cleanup_window_is_finite() {
        assert!(super::CLEANUP_TIMEOUT.as_millis() > 0);
        assert!(super::CLEANUP_TIMEOUT.as_secs() < 1);
    }

    #[cfg(unix)]
    #[test]
    fn wait_for_group_empty_honors_an_expired_deadline() {
        let mut command = std::process::Command::new("sleep");
        command.arg("30");
        process_tree::ProcessTree::prepare_std(&mut command);
        let mut child = command.spawn().unwrap();
        let mut tree = process_tree::ProcessTree::assign_std(&child).unwrap();
        let started = std::time::Instant::now();
        assert!(!tree.wait_empty_blocking(started));
        assert!(started.elapsed() < std::time::Duration::from_millis(100));
        assert!(tree.terminate_blocking(&mut child, started + super::CLEANUP_TIMEOUT));
    }
}
