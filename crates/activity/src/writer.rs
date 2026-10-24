//! Batched session writes (01 §4.4: every second or per 100 sessions). A failed
//! batch goes back to the front of an in-memory retry queue of at most 10,000
//! sessions; beyond that the oldest are dropped and counted. Status changes are
//! published as `activity.status_changed`; the error text goes to the log only.
use crate::store::{Session, SessionSink, SinkError};
use crate::*;
use devbox_core::Hub;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const MAX_PENDING: usize = 10_000;
pub const BATCH: usize = 100;
pub const INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Queue {
    items: VecDeque<Session>,
    /// Dropped by the cap plus the app's reported offline drops.
    dropped: u64,
}
impl Queue {
    fn trim(&mut self) {
        while self.items.len() > MAX_PENDING {
            self.items.pop_front();
            self.dropped += 1;
        }
    }
    fn push_back(&mut self, sessions: Vec<Session>) {
        self.items.extend(sessions);
        self.trim();
    }
    fn drain_upto(&mut self, n: usize) -> Vec<Session> {
        let n = n.min(self.items.len());
        self.items.drain(..n).collect()
    }
    /// The failed batch is older than anything queued after it.
    fn requeue_front(&mut self, batch: Vec<Session>) {
        for session in batch.into_iter().rev() {
            self.items.push_front(session);
        }
        self.trim();
    }
}

struct Health {
    failure: Option<StorageProblem>,
    /// Bumped on every published change; starts at the clock so a restarted
    /// daemon does not repeat an earlier rev.
    rev: u64,
    published: Option<(Option<StorageProblem>, u32, u64)>,
}

pub struct Writer {
    sink: Arc<dyn SessionSink>,
    hub: Arc<Hub>,
    queue: Mutex<Queue>,
    health: Mutex<Health>,
    flushing: tokio::sync::Mutex<()>,
    wake: tokio::sync::Notify,
}

fn clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Writer {
    pub fn new(sink: Arc<dyn SessionSink>, hub: Arc<Hub>) -> Self {
        Self {
            sink,
            hub,
            queue: Mutex::new(Queue::default()),
            health: Mutex::new(Health {
                failure: None,
                rev: clock_ms(),
                published: Some((None, 0, 0)),
            }),
            flushing: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
        }
    }

    pub fn push(&self, sessions: Vec<Session>) {
        if sessions.is_empty() {
            return;
        }
        let full = {
            let mut queue = self.queue.lock().expect("activity queue");
            queue.push_back(sessions);
            queue.items.len() >= BATCH
        };
        if full {
            self.wake.notify_one();
        }
        self.publish_if_changed();
    }

    pub fn add_dropped(&self, n: u64) {
        if n == 0 {
            return;
        }
        self.queue.lock().expect("activity queue").dropped += n;
        self.publish_if_changed();
    }

    /// Writes every queued session in batches of 100, oldest first, until the
    /// queue is empty or a write fails.
    pub async fn flush(&self) {
        let _one = self.flushing.lock().await;
        loop {
            let batch = self.queue.lock().expect("activity queue").drain_upto(BATCH);
            if batch.is_empty() {
                break;
            }
            match self.sink.insert(batch.clone()).await {
                Ok(rev) => {
                    if self
                        .health
                        .lock()
                        .expect("activity health")
                        .failure
                        .take()
                        .is_some()
                    {
                        tracing::info!("activity writes recovered");
                    }
                    self.hub.publish::<ActivityChangedTopic>(
                        rev,
                        &ActivityChanged {
                            rev: rev.to_string(),
                        },
                    );
                }
                Err(e) => {
                    let problem = match e {
                        SinkError::DiskFull => StorageProblem::DiskFull,
                        SinkError::Database(_) => StorageProblem::Database,
                    };
                    let pending = batch.len();
                    self.queue
                        .lock()
                        .expect("activity queue")
                        .requeue_front(batch);
                    let before = self
                        .health
                        .lock()
                        .expect("activity health")
                        .failure
                        .replace(problem);
                    // Once per change of state, not every retry second.
                    if before != Some(problem) {
                        tracing::warn!(error = %e, sessions = pending, "activity write failed; retrying");
                    } else {
                        tracing::debug!(sessions = pending, "activity write still failing");
                    }
                    break;
                }
            }
        }
        self.publish_if_changed();
    }

    pub fn status(&self) -> ActivityStatus {
        let (pending, dropped) = {
            let queue = self.queue.lock().expect("activity queue");
            (queue.items.len() as u32, queue.dropped)
        };
        let health = self.health.lock().expect("activity health");
        ActivityStatus {
            rev: health.rev.to_string(),
            state: match health.failure {
                None => StorageState::Ok,
                Some(_) => StorageState::StorageFailed,
            },
            pending,
            dropped,
            last_error: health.failure,
        }
    }

    /// Publishes when the state, the drop count or (while failing) the pending
    /// count changed since the last publication.
    fn publish_if_changed(&self) {
        let (pending, dropped) = {
            let queue = self.queue.lock().expect("activity queue");
            (queue.items.len() as u32, queue.dropped)
        };
        let status = {
            let mut health = self.health.lock().expect("activity health");
            let shown = if health.failure.is_some() { pending } else { 0 };
            let now = (health.failure, shown, dropped);
            if health.published == Some(now) {
                return;
            }
            health.published = Some(now);
            health.rev += 1;
            drop(health);
            self.status()
        };
        let rev = status.rev.parse().unwrap_or(0);
        self.hub.publish::<ActivityStatusChangedTopic>(rev, &status);
    }

    /// Flushes every second, or as soon as 100 sessions wait.
    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::select! {
                _ = self.wake.notified() => {}
                _ = tokio::time::sleep(INTERVAL) => {}
            }
            self.flush().await;
        }
    }
}
