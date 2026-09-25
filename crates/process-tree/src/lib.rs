//! Bounded ownership for one native child process tree.
//!
//! Windows starts the root suspended, assigns it to a kill-on-close Job
//! Object, proves that the Job contains only that root, and resumes its sole
//! primary thread. Unix uses a private process group. The group cannot own a
//! malicious descendant which deliberately calls `setsid()`; that OS authority
//! limit is documented by the stdio contract rather than hidden.

// The public unit error preserves existing consumer error mappings.
#![allow(clippy::result_unit_err)]

use std::time::Duration;
#[cfg(feature = "tokio")]
use tokio::process::Child;
#[cfg(feature = "tokio")]
use tokio::time::Instant;

pub const CLEANUP_TIMEOUT: Duration = Duration::from_millis(750);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[cfg(windows)]
pub mod windows_job;

pub struct ProcessTree {
    #[cfg(target_os = "windows")]
    job: windows_job::WindowsJob,
    #[cfg(unix)]
    process_group: i32,
    terminal_empty: bool,
}

impl ProcessTree {
    /// Prepare a private group or a suspended, non-windowed Windows child.
    pub fn prepare_std(command: &mut std::process::Command) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000 | 0x0000_0004);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(not(any(unix, windows)))]
        let _ = command;
    }

    #[cfg(feature = "tokio")]
    pub fn prepare_tokio(command: &mut tokio::process::Command) {
        Self::prepare_std(command.as_std_mut());
    }

    /// Call only for a child prepared by `prepare_std`; on failure the caller
    /// must kill and reap the root. Windows admission resumes it exactly once.
    pub fn assign_std(child: &std::process::Child) -> Result<Self, ()> {
        #[cfg(windows)]
        {
            return windows_job::WindowsJob::assign_std(child)
                .map(|job| Self {
                    job,
                    terminal_empty: false,
                })
                .map_err(|_| ());
        }
        #[cfg(unix)]
        {
            Self::assign_group(child.id())
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {
                terminal_empty: false,
            })
        }
    }

    #[cfg(feature = "tokio")]
    pub fn assign(child: &Child) -> Result<Self, ()> {
        #[cfg(windows)]
        {
            return windows_job::WindowsJob::assign_to(child)
                .map(|job| Self {
                    job,
                    terminal_empty: false,
                })
                .map_err(|_| ());
        }
        #[cfg(unix)]
        {
            Self::assign_group(child.id().ok_or(())?)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {
                terminal_empty: false,
            })
        }
    }

    #[cfg(unix)]
    fn assign_group(pid: u32) -> Result<Self, ()> {
        let process_group = i32::try_from(pid).map_err(|_| ())?;
        if process_group <= 1 {
            return Err(());
        }
        Ok(Self {
            process_group,
            terminal_empty: false,
        })
    }

    pub fn is_empty(&self) -> Option<bool> {
        if self.terminal_empty {
            Some(true)
        } else {
            self.authority_is_empty()
        }
    }

    pub fn terminate_blocking(
        &mut self,
        child: &mut std::process::Child,
        deadline: std::time::Instant,
    ) -> bool {
        let wait_root = |child: &mut std::process::Child, until: std::time::Instant| loop {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) if std::time::Instant::now() < until => std::thread::sleep(POLL_INTERVAL),
                _ => return false,
            }
        };
        if !self.terminal_empty
            && !self.signal_termination(false)
            && self.authority_is_empty() != Some(true)
        {
            let _ = child.kill();
        }
        let now = std::time::Instant::now();
        let grace = now + deadline.saturating_duration_since(now) / 2;
        let mut root_gone = wait_root(child, grace);
        if !root_gone || self.authority_is_empty() != Some(true) {
            let _ = self.signal_termination(true);
            if !root_gone {
                let _ = child.kill();
                root_gone = wait_root(child, deadline);
            }
        }
        let tree_gone = self.wait_empty_blocking(deadline);
        tree_gone && root_gone
    }

    pub fn terminate_descendants(&mut self) -> bool {
        self.terminate_descendants_until(std::time::Instant::now() + CLEANUP_TIMEOUT)
    }

    /// Consumer-specific budgets remain owned by the consumer.
    pub fn terminate_descendants_until(&mut self, deadline: std::time::Instant) -> bool {
        if self.is_empty() == Some(true) {
            self.terminal_empty = true;
            return true;
        }
        let _ = self.signal_termination(true);
        self.wait_empty_blocking(deadline)
    }

    /// Signals alone never establish successful cleanup. This low-level path
    /// lets existing consumers preserve their staged grace/reap deadlines.
    pub fn signal(&self, force: bool) -> bool {
        self.terminal_empty || self.signal_termination(force)
    }

    /// Send the final signal while the caller still reserves the root PID,
    /// then release this owner without a second Unix signal during Drop.
    /// This reports signal delivery only, never confirmed tree cleanup.
    pub fn signal_and_release(mut self, force: bool) -> bool {
        let sent = self.signal(force);
        self.terminal_empty = true;
        sent
    }

    pub fn wait_empty_blocking(&mut self, deadline: std::time::Instant) -> bool {
        loop {
            if self.is_empty() == Some(true) {
                self.terminal_empty = true;
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    #[cfg(feature = "tokio")]
    pub async fn terminate(&mut self, child: &mut Child) -> bool {
        self.terminate_until(child, Instant::now() + CLEANUP_TIMEOUT)
            .await
    }

    #[cfg(feature = "tokio")]
    pub async fn terminate_until(&mut self, child: &mut Child, deadline: Instant) -> bool {
        if self.terminal_empty {
            return tokio::time::timeout_at(deadline, child.wait())
                .await
                .is_ok_and(|result| result.is_ok());
        }
        if !self.signal_termination(false) && self.authority_is_empty() != Some(true) {
            // start_kill only sends a signal; all waits share the original deadline.
            let _ = child.start_kill();
        }
        let now = Instant::now();
        let grace = now + deadline.saturating_duration_since(now) / 2;
        let mut root_gone = tokio::time::timeout_at(grace, child.wait())
            .await
            .is_ok_and(|result| result.is_ok());
        if !root_gone || self.authority_is_empty() != Some(true) {
            let _ = self.signal_termination(true);
            if !root_gone {
                let _ = child.start_kill();
                root_gone = tokio::time::timeout_at(deadline, child.wait())
                    .await
                    .is_ok_and(|result| result.is_ok());
            }
        }
        let tree_gone = wait_for_empty(deadline, {
            let tree = &mut *self;
            move || tree.authority_is_empty()
        })
        .await;
        if tree_gone {
            self.terminal_empty = true;
        }
        tree_gone && root_gone
    }

    #[cfg(target_os = "windows")]
    fn signal_termination(&self, _force: bool) -> bool {
        self.job.terminate().is_ok()
    }

    #[cfg(unix)]
    fn signal_termination(&self, force: bool) -> bool {
        signal_group(
            self.process_group,
            if force { libc::SIGKILL } else { libc::SIGTERM },
        )
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    fn signal_termination(&self, _force: bool) -> bool {
        true
    }

    #[cfg(target_os = "windows")]
    fn authority_is_empty(&self) -> Option<bool> {
        self.job.is_empty().ok()
    }

    #[cfg(unix)]
    fn authority_is_empty(&self) -> Option<bool> {
        let result = unsafe { libc::kill(-self.process_group, 0) };
        if result == 0 {
            return Some(false);
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Some(true),
            Some(libc::EPERM) => Some(false),
            _ => None,
        }
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    fn authority_is_empty(&self) -> Option<bool> {
        Some(true)
    }

    #[cfg(feature = "tokio")]
    pub async fn terminate_unassigned(child: &mut Child) -> bool {
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        let _ = child.start_kill();
        tokio::time::timeout_at(deadline, child.wait())
            .await
            .is_ok_and(|result| result.is_ok())
    }
}

#[cfg(unix)]
fn signal_group(process_group: i32, signal: i32) -> bool {
    process_group > 1 && unsafe { libc::kill(-process_group, signal) } == 0
}

#[cfg(feature = "tokio")]
async fn wait_for_empty(deadline: Instant, mut probe: impl FnMut() -> Option<bool>) -> bool {
    loop {
        if probe() == Some(true) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep_until((Instant::now() + POLL_INTERVAL).min(deadline)).await;
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        // Drop is a signal/kill-on-close fallback, never proof of cleanup.
        if !self.terminal_empty {
            let _ = self.signal_termination(true);
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

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn unconfirmed_authority_yields_and_obeys_one_deadline() {
        use super::*;
        for state in [None, Some(false)] {
            let started = Instant::now();
            let deadline = started + Duration::from_millis(100);
            let (empty, heartbeat) = tokio::join!(wait_for_empty(deadline, || state), async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                Instant::now()
            });
            assert!(!empty);
            assert!(heartbeat < deadline, "cleanup blocked the executor");
            assert!(started.elapsed() < Duration::from_millis(500));
        }
        assert!(wait_for_empty(Instant::now(), || Some(true)).await);
        assert!(!wait_for_empty(Instant::now(), || None).await);
    }

    #[cfg(unix)]
    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn force_escalation_keeps_time_to_reap_before_the_same_deadline() {
        use super::*;
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut child = tokio::process::Command::new("/bin/sh")
            .args(["-c", "trap '' TERM; printf 'ready\\n'; exec sleep 30"])
            .stdout(std::process::Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut tree = ProcessTree::assign(&child).unwrap();
        let mut ready = String::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            BufReader::new(child.stdout.take().unwrap()).read_line(&mut ready),
        )
        .await
        .unwrap()
        .unwrap();
        let started = Instant::now();
        assert!(tree.terminate(&mut child).await);
        assert!(started.elapsed() < CLEANUP_TIMEOUT + Duration::from_millis(500));
        assert!(child.try_wait().unwrap().is_some());
        assert!(tree.terminate(&mut child).await);
    }

    #[cfg(unix)]
    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn unassigned_cleanup_reaps_and_drop_does_not_poll() {
        use super::*;
        let mut child = tokio::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let tree = ProcessTree::assign(&child).unwrap();
        let started = Instant::now();
        drop(tree);
        assert!(started.elapsed() < Duration::from_millis(200));
        assert!(ProcessTree::terminate_unassigned(&mut child).await);
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn cleanup_window_is_finite() {
        assert!(super::CLEANUP_TIMEOUT.as_millis() > 0);
        assert!(super::CLEANUP_TIMEOUT.as_secs() < 2);
    }

    #[cfg(unix)]
    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn terminate_reaps_the_root_and_private_group_descendant() {
        use std::process::Stdio;
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;

        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 30 & printf '%s\\n' $!; wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = command.spawn().unwrap();
        let root_pid = child.id().unwrap() as i32;
        let mut tree = super::ProcessTree::assign(&child).unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            BufReader::new(stdout).read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let descendant_pid = line.trim().parse::<i32>().unwrap();

        assert!(tree.terminate(&mut child).await);
        assert_eq!(unsafe { libc::kill(root_pid, 0) }, -1);
        assert_eq!(unsafe { libc::kill(descendant_pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[cfg(unix)]
    #[test]
    fn blocking_termination_ends_the_whole_group() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "sleep 30 & wait"]);
        super::ProcessTree::prepare_std(&mut command);
        let mut child = command.spawn().unwrap();
        let mut tree = super::ProcessTree::assign_std(&child).unwrap();
        assert_eq!(tree.is_empty(), Some(false));
        assert!(tree.terminate_blocking(
            &mut child,
            std::time::Instant::now() + super::CLEANUP_TIMEOUT
        ));
        assert_eq!(tree.is_empty(), Some(true));
    }

    #[cfg(unix)]
    #[test]
    fn descendants_are_ended_after_the_root_exits() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "sleep 30 & exit 0"]);
        super::ProcessTree::prepare_std(&mut command);
        let mut child = command.spawn().unwrap();
        let mut tree = super::ProcessTree::assign_std(&child).unwrap();
        child.wait().unwrap();
        assert!(tree.terminate_descendants());
        assert_eq!(tree.is_empty(), Some(true));
    }

    #[cfg(windows)]
    #[test]
    fn windows_children_start_owned_and_end_with_their_descendants() {
        let mut command = std::process::Command::new("cmd");
        command.args([
            "/C",
            "start /B ping -n 30 127.0.0.1 >NUL & ping -n 30 127.0.0.1 >NUL",
        ]);
        super::ProcessTree::prepare_std(&mut command);
        let mut child = command.spawn().unwrap();
        let mut tree = super::ProcessTree::assign_std(&child).unwrap();
        assert_eq!(tree.is_empty(), Some(false));
        assert!(tree.terminate_blocking(
            &mut child,
            std::time::Instant::now() + super::CLEANUP_TIMEOUT
        ));
        assert_eq!(tree.is_empty(), Some(true));
    }

    #[cfg(windows)]
    #[test]
    fn a_child_that_was_not_started_suspended_is_refused() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 5 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        assert!(super::ProcessTree::assign_std(&child).is_err());
        let _ = child.kill();
        let _ = child.wait();
    }
}
