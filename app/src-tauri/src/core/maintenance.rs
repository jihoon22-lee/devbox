use std::{sync::Mutex, time::Duration};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
#[derive(Default)]
pub struct AttemptControl {
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    revision: u64,
    paused: bool,
    current: Option<Attempt>,
}
struct Attempt {
    cancel: CancellationToken,
    completed: watch::Receiver<bool>,
}
pub struct Completion(watch::Sender<bool>);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum PauseError {
    Timeout,
    Superseded,
    WorkerUnavailable,
}
impl AttemptControl {
    pub fn begin(&self, cancel: CancellationToken) -> Option<Completion> {
        let mut state = self.state.lock().expect("owned attempt control");
        if state.paused
            || state
                .current
                .as_ref()
                .is_some_and(|attempt| !*attempt.completed.borrow())
        {
            cancel.cancel();
            return None;
        }
        let (done, completed) = watch::channel(false);
        state.revision = state
            .revision
            .checked_add(1)
            .expect("attempt revision exhausted");
        state.current = Some(Attempt { cancel, completed });
        Some(Completion(done))
    }
    pub fn resume(&self) {
        let mut state = self.state.lock().expect("owned attempt control");
        state.paused = false;
        state.revision = state
            .revision
            .checked_add(1)
            .expect("attempt revision exhausted");
    }
    pub async fn pause(&self, deadline: Duration) -> Result<(), PauseError> {
        let (revision, completion) = {
            let mut state = self.state.lock().expect("owned attempt control");
            state.paused = true;
            state.revision = state
                .revision
                .checked_add(1)
                .expect("attempt revision exhausted");
            let completion = state.current.as_ref().map(|attempt| {
                attempt.cancel.cancel();
                attempt.completed.clone()
            });
            (state.revision, completion)
        };
        if let Some(mut completion) = completion {
            tokio::time::timeout(deadline, async {
                completion
                    .wait_for(|done| *done)
                    .await
                    .map_err(|_| PauseError::WorkerUnavailable)?;
                Ok::<(), PauseError>(())
            })
            .await
            .map_err(|_| PauseError::Timeout)??;
        }
        let state = self.state.lock().expect("owned attempt control");
        if !state.paused || state.revision != revision {
            return Err(PauseError::Superseded);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[tokio::test]
    async fn cleanup_cancels_and_awaits_owned_worker_before_allowing_uninstall() {
        let control = Arc::new(AttemptControl::default());
        let token = tokio_util::sync::CancellationToken::new();
        let completed = control.begin(token.clone()).unwrap();
        let waiter = control.clone();
        let wait = tokio::spawn(async move { waiter.pause(Duration::from_secs(1)).await });
        token.cancelled().await;
        assert!(!wait.is_finished());
        assert!(
            control
                .begin(tokio_util::sync::CancellationToken::new())
                .is_none()
        );
        drop(completed);
        assert_eq!(wait.await.unwrap(), Ok(()));
        assert!(
            control
                .begin(tokio_util::sync::CancellationToken::new())
                .is_none()
        );
        control.resume();
        assert!(
            control
                .begin(tokio_util::sync::CancellationToken::new())
                .is_some()
        );
    }
    #[tokio::test]
    async fn cleanup_timeout_retains_ownership_and_retry_supersedes_pending_cleanup() {
        let control = Arc::new(AttemptControl::default());
        let token = tokio_util::sync::CancellationToken::new();
        let completed = control.begin(token.clone()).unwrap();
        assert_eq!(
            control.pause(Duration::ZERO).await,
            Err(PauseError::Timeout)
        );
        assert!(token.is_cancelled());
        assert!(
            control
                .begin(tokio_util::sync::CancellationToken::new())
                .is_none()
        );
        let wait = control.pause(Duration::from_secs(1));
        tokio::pin!(wait);
        std::future::poll_fn(|context| {
            use std::future::Future;
            assert!(wait.as_mut().poll(context).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        control.resume();
        drop(completed);
        assert_eq!(wait.await, Err(PauseError::Superseded));
    }
}
