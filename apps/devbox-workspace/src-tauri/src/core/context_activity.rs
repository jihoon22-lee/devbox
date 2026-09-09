//! An owned permit spans async queues and blocking workers. Context mutation
//! cannot retire a project while a file operation still uses its authority.
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone, Default)]
pub struct ContextActivity(Arc<AtomicUsize>);
struct Lease {
    state: Arc<AtomicUsize>,
    change: bool,
}
#[derive(Clone)]
pub struct ContextPermit {
    _lease: Arc<Lease>,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.change {
            self.state.store(0, Ordering::Release);
        } else {
            self.state.fetch_sub(1, Ordering::AcqRel);
        }
    }
}
impl ContextActivity {
    pub async fn enter_change(
        &self,
        deadline: std::time::Instant,
        cancelled: &AtomicBool,
    ) -> Result<ContextPermit, &'static str> {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("context_cancelled");
            }
            if std::time::Instant::now() >= deadline {
                return Err("context_expired");
            }
            if let Ok(permit) = self.enter(true) {
                return Ok(permit);
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }
    pub fn enter(&self, change: bool) -> Result<ContextPermit, &'static str> {
        if change {
            self.0
                .compare_exchange(0, usize::MAX, Ordering::AcqRel, Ordering::Acquire)
                .map_err(|_| "context_busy")?;
        } else {
            self.0
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                    (count < usize::MAX - 1).then(|| count + 1)
                })
                .map_err(|_| "context_busy")?;
        }
        Ok(ContextPermit {
            _lease: Arc::new(Lease {
                state: self.0.clone(),
                change,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn a_selection_waits_for_the_retained_worker_and_respects_expiry_and_shutdown() {
        use std::time::{Duration, Instant};
        let activity = ContextActivity::default();
        let caller = activity.enter(false).unwrap();
        let worker = caller.clone();
        drop(caller);
        let cancelled = AtomicBool::new(false);
        assert!(matches!(
            activity
                .enter_change(Instant::now() + Duration::from_millis(15), &cancelled)
                .await,
            Err("context_expired")
        ));
        assert!(activity.enter(true).is_err());
        let finishing = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            drop(worker);
        });
        let change = activity
            .enter_change(Instant::now() + Duration::from_secs(1), &cancelled)
            .await
            .unwrap();
        finishing.await.unwrap();
        assert!(activity.enter(false).is_err());
        drop(change);
        cancelled.store(true, Ordering::Release);
        assert!(matches!(
            activity
                .enter_change(Instant::now() + Duration::from_secs(1), &cancelled)
                .await,
            Err("context_cancelled")
        ));
        assert!(activity.enter(false).is_ok());
    }
    #[test]
    fn a_cancelled_caller_does_not_release_its_running_worker() {
        let activity = ContextActivity::default();
        let request = activity.enter(false).unwrap();
        let worker = request.clone();
        drop(request);
        assert!(activity.enter(true).is_err());
        let second_file = activity.enter(false).unwrap();
        drop(worker);
        assert!(activity.enter(true).is_err());
        drop(second_file);
        let change = activity.enter(true).unwrap();
        assert!(activity.enter(false).is_err());
        assert!(activity.enter(true).is_err());
        drop(change);
        assert!(activity.enter(false).is_ok());
    }
    #[test]
    fn a_change_worker_retains_exclusion_until_commit_or_retirement() {
        let activity = ContextActivity::default();
        let request = activity.enter(true).unwrap();
        let worker = request.clone();
        drop(request);
        assert!(activity.enter(false).is_err());
        drop(worker);
        assert!(activity.enter(true).is_ok());
    }
}
