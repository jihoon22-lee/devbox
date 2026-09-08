//! Bounded ownership with nonblocking retirement. Closing an offline filesystem
//! handle is itself an OS operation, so it must not run under an IPC state lock.
use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
};
struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Retired<T> {
    value: T,
    _permit: Permit,
}
pub struct Pool<T: Send + Sync + 'static> {
    sender: mpsc::Sender<Retired<T>>,
    used: Arc<AtomicUsize>,
    failed: AtomicBool,
    limit: usize,
}
struct Held<T: Send + Sync + 'static> {
    value: Option<Retired<T>>,
    pool: Arc<Pool<T>>,
}
impl<T: Send + Sync + 'static> Drop for Held<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            if let Err(error) = self.pool.sender.send(value) {
                // The reaper owns no fallible user work and catches drop panics.
                // If it is nevertheless lost, disable admission and retain only
                // the already bounded objects until process exit, never block IPC.
                self.pool.failed.store(true, Ordering::Release);
                std::mem::forget(error.0);
            }
        }
    }
}
pub struct Lease<T: Send + Sync + 'static>(Arc<Held<T>>);
impl<T: Send + Sync + 'static> Clone for Lease<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: Send + Sync + 'static> Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self
            .0
            .value
            .as_ref()
            .expect("live lease retains its value")
            .value
    }
}
impl<T: Send + Sync + 'static> Pool<T> {
    pub fn new(limit: usize) -> Result<Arc<Self>, String> {
        let (sender, receiver) = mpsc::channel::<Retired<T>>();
        std::thread::Builder::new()
            .name("knowledge-object-retirement".into())
            .spawn(move || {
                while let Ok(value) = receiver.recv() {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(value)));
                }
            })
            .map_err(|_| "store_busy")?;
        Ok(Arc::new(Self {
            sender,
            used: Arc::new(AtomicUsize::new(0)),
            failed: AtomicBool::new(false),
            limit,
        }))
    }
    pub fn available(&self) -> bool {
        !self.failed.load(Ordering::Acquire) && self.used.load(Ordering::Acquire) < self.limit
    }
    /// Only worker code calls this: a rejected newly-opened object is closed by
    /// that worker. The queue is bounded by permits, including the object being
    /// closed; a slow reaper cannot admit an unbounded backlog.
    pub fn hold(self: &Arc<Self>, value: T) -> Option<Lease<T>> {
        if self.failed.load(Ordering::Acquire) {
            return None;
        }
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                (used < self.limit).then_some(used + 1)
            })
            .ok()?;
        Some(Lease(Arc::new(Held {
            value: Some(Retired {
                value,
                _permit: Permit(self.used.clone()),
            }),
            pool: self.clone(),
        })))
    }
    pub fn usage(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Condvar, Mutex},
        time::Duration,
    };
    struct Slow(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Slow {
        fn drop(&mut self) {
            let (lock, wake) = &*self.0;
            let mut ready = lock.lock().unwrap();
            while !*ready {
                ready = wake.wait(ready).unwrap();
            }
        }
    }
    #[test]
    fn slow_close_does_not_block_caller_or_release_admission_early() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let pool = Pool::new(1).unwrap();
        let lease = pool.hold(Slow(gate.clone())).unwrap();
        let (done, completed) = mpsc::channel();
        let caller = std::thread::spawn(move || {
            drop(lease);
            done.send(()).unwrap();
        });
        let returned = completed.recv_timeout(Duration::from_secs(1));
        let blocked_admission = !pool.available();
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        caller.join().unwrap();
        assert!(returned.is_ok());
        assert!(blocked_admission);
        for _ in 0..100 {
            if pool.usage() == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(pool.usage(), 0);
        assert!(pool.available());
    }
}
