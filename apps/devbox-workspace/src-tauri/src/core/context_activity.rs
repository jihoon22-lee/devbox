//! An owned permit spans async queues and blocking workers. Context mutation
//! cannot retire a project while a file operation still uses its authority.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
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
