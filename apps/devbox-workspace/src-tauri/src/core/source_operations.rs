//! Cancellation begins at native admission, before queueing or Git evidence IO.
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    task::Poll,
};
type Result<T> = std::result::Result<T, &'static str>;
type Key = (String, String);
#[derive(Default)]
struct Table {
    active: HashMap<Key, Weak<Signal>>,
    cancelled: HashMap<Key, std::time::Instant>,
}
impl Table {
    fn expire(&mut self) {
        self.cancelled
            .retain(|_, at| at.elapsed() < std::time::Duration::from_secs(60));
    }
}
type Entries = Arc<Mutex<Table>>;
#[derive(Default, Clone)]
pub(crate) struct Operations(Entries);
#[derive(Default)]
struct Signal {
    flag: Arc<AtomicBool>,
    notify: tokio::sync::Notify,
}
struct Inner {
    entries: Entries,
    key: Option<Key>,
    signal: Arc<Signal>,
}
#[derive(Clone)]
pub(crate) struct Request(Arc<Inner>);
pub(crate) struct CancelOnDrop(Request);
fn id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        Err("invalid_request")
    } else {
        Ok(())
    }
}
impl Operations {
    pub(crate) fn register(&self, context: &str, operation_id: Option<&str>) -> Result<Request> {
        let signal = Arc::new(Signal::default());
        let key = if let Some(operation_id) = operation_id {
            id(operation_id)?;
            let key = (context.to_owned(), operation_id.to_owned());
            let mut entries = self.0.lock().map_err(|_| "busy")?;
            entries.expire();
            if entries.cancelled.contains_key(&key) {
                return Err("source_cancelled");
            }
            if entries.active.get(&key).and_then(Weak::upgrade).is_some()
                || entries.active.len() >= 16
            {
                return Err("busy");
            }
            entries.active.insert(key.clone(), Arc::downgrade(&signal));
            Some(key)
        } else {
            None
        };
        Ok(Request(Arc::new(Inner {
            entries: self.0.clone(),
            key,
            signal,
        })))
    }
    pub(crate) fn cancel(&self, context: &str, operation_id: &str) -> Result<bool> {
        id(operation_id)?;
        let mut entries = self.0.lock().map_err(|_| "busy")?;
        entries.expire();
        let key = (context.to_owned(), operation_id.to_owned());
        if entries.cancelled.len() >= 64 && !entries.cancelled.contains_key(&key) {
            return Err("busy");
        }
        // A cancel can reach native dispatch before the original invocation's
        // first poll. Retain bounded intent for longer than the request budget.
        entries
            .cancelled
            .insert(key.clone(), std::time::Instant::now());
        if let Some(signal) = entries.active.get(&key).and_then(Weak::upgrade) {
            signal.flag.store(true, Ordering::Release);
            signal.notify.notify_one();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        if let (Some(key), Ok(mut entries)) = (&self.key, self.entries.lock()) {
            if entries
                .active
                .get(key)
                .and_then(Weak::upgrade)
                .is_some_and(|value| Arc::ptr_eq(&value, &self.signal))
            {
                entries.active.remove(key);
            }
        }
    }
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
impl Request {
    pub(crate) fn flag(&self) -> Arc<AtomicBool> {
        self.0.signal.flag.clone()
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.0.signal.flag.load(Ordering::Acquire) {
            Err("source_cancelled")
        } else {
            Ok(())
        }
    }
    pub(crate) fn cancel(&self) {
        self.0.signal.flag.store(true, Ordering::Release);
        self.0.signal.notify.notify_one();
    }
    pub(crate) fn cancel_on_drop(&self) -> CancelOnDrop {
        CancelOnDrop(self.clone())
    }
    pub(crate) async fn until_cancelled<F: Future>(&self, future: F) -> Result<F::Output> {
        let mut future = std::pin::pin!(future);
        let mut cancelled = std::pin::pin!(self.0.signal.notify.notified());
        std::future::poll_fn(|context| {
            if let Err(error) = self.check() {
                return Poll::Ready(Err(error));
            }
            let _ = cancelled.as_mut().poll(context);
            if let Err(error) = self.check() {
                return Poll::Ready(Err(error));
            }
            future.as_mut().poll(context).map(Ok)
        })
        .await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn context_scoping_and_retained_workers_cover_admission_before_git_exists() {
        let operations = Operations::default();
        let first = operations.register("first", Some("same-id")).unwrap();
        let second = operations.register("second", Some("same-id")).unwrap();
        assert!(operations.register("first", Some("same-id")).is_err());
        let worker = first.clone();
        let cancel = first.cancel_on_drop();
        drop(first);
        drop(cancel);
        assert!(worker.check().is_err());
        assert!(second.check().is_ok());
        assert!(operations.cancel("first", "same-id").unwrap());
        drop(worker);
        assert!(!operations.cancel("first", "same-id").unwrap());
        assert!(matches!(
            operations.register("first", Some("same-id")),
            Err("source_cancelled")
        ));
        assert!(!operations.cancel("first", "not-yet-dispatched").unwrap());
        assert!(matches!(
            operations.register("first", Some("not-yet-dispatched")),
            Err("source_cancelled")
        ));
    }
    #[test]
    fn queued_cancellation_wakes_without_polling_any_native_work() {
        use std::task::{Context, Wake, Waker};
        struct Counter(std::sync::atomic::AtomicUsize);
        impl Wake for Counter {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let operations = Operations::default();
        let request = operations.register("context", Some("queued")).unwrap();
        let counter = Arc::new(Counter(std::sync::atomic::AtomicUsize::new(0)));
        let waker = Waker::from(counter.clone());
        let mut context = Context::from_waker(&waker);
        let future = request.until_cancelled(std::future::pending::<()>());
        let mut future = std::pin::pin!(future);
        assert!(future.as_mut().poll(&mut context).is_pending());
        assert!(operations.cancel("context", "queued").unwrap());
        assert!(counter.0.load(Ordering::Relaxed) > 0);
        assert_eq!(
            future.as_mut().poll(&mut context),
            Poll::Ready(Err("source_cancelled"))
        );
    }
}
