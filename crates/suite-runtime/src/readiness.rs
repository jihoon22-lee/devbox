//! Startup waiting never grants authority or replays a business operation.
use tokio::{sync::watch, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Preparing,
    Connected,
    Off,
    Failed(&'static str),
}

pub(crate) async fn wait(
    state: &mut watch::Receiver<State>,
    deadline: Instant,
) -> Result<(), &'static str> {
    loop {
        let current = *state.borrow_and_update();
        match current {
            State::Connected => return Ok(()),
            State::Off => return Err("suite_connection_off"),
            State::Failed(issue) => return Err(issue),
            State::Preparing => {}
        }
        tokio::time::timeout_at(deadline, state.changed())
            .await
            .map_err(|_| "suite_connection_timeout")?
            .map_err(|_| "suite_connection_failed")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(1)
    }

    #[tokio::test]
    async fn preparing_waits_until_connected() {
        let (tx, mut rx) = watch::channel(State::Preparing);
        let mut waiting = Box::pin(wait(&mut rx, deadline()));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut waiting)
                .await
                .is_err()
        );
        tx.send_replace(State::Connected);
        assert_eq!(waiting.await, Ok(()));
    }

    #[tokio::test]
    async fn already_connected_notification_is_not_lost() {
        let (tx, mut rx) = watch::channel(State::Preparing);
        tx.send_replace(State::Connected);
        assert_eq!(wait(&mut rx, deadline()).await, Ok(()));
    }

    #[tokio::test]
    async fn off_and_failure_finish_without_waiting() {
        for (state, expected) in [
            (State::Off, "suite_connection_off"),
            (
                State::Failed("suite_connection_failed"),
                "suite_connection_failed",
            ),
            (
                State::Failed("suite_review_required"),
                "suite_review_required",
            ),
        ] {
            let (_tx, mut rx) = watch::channel(state);
            assert_eq!(
                tokio::time::timeout(Duration::from_millis(20), wait(&mut rx, deadline())).await,
                Ok(Err(expected))
            );
        }
    }

    #[tokio::test]
    async fn original_deadline_bounds_preparation() {
        let (_tx, mut rx) = watch::channel(State::Preparing);
        assert_eq!(
            wait(&mut rx, Instant::now() + Duration::from_millis(10)).await,
            Err("suite_connection_timeout")
        );
    }

    #[tokio::test]
    async fn closed_startup_does_not_wait_until_deadline() {
        let (tx, mut rx) = watch::channel(State::Preparing);
        drop(tx);
        assert_eq!(
            wait(&mut rx, deadline()).await,
            Err("suite_connection_failed")
        );
    }

    #[tokio::test]
    async fn manual_decision_wakes_pending_waiters() {
        let (tx, mut rx) = watch::channel(State::Preparing);
        let mut waiting = Box::pin(wait(&mut rx, deadline()));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut waiting)
                .await
                .is_err()
        );
        tx.send_replace(State::Off);
        assert_eq!(waiting.await, Err("suite_connection_off"));
    }

    #[tokio::test]
    async fn newer_manual_decision_supersedes_ready_notification() {
        let (tx, mut rx) = watch::channel(State::Preparing);
        tx.send_replace(State::Connected);
        tx.send_replace(State::Off);
        assert_eq!(wait(&mut rx, deadline()).await, Err("suite_connection_off"));
    }

    #[tokio::test]
    async fn dropping_wait_does_not_change_authority() {
        let (tx, mut rx) = watch::channel(State::Preparing);
        let mut waiting = Box::pin(wait(&mut rx, deadline()));
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut waiting)
                .await
                .is_err()
        );
        drop(waiting);
        assert_eq!(*tx.borrow(), State::Preparing);
        tx.send_replace(State::Connected);
        assert_eq!(wait(&mut rx, deadline()).await, Ok(()));
    }
}
