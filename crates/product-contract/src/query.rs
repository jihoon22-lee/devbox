//! Cancellation is keyed by a request UUID, separate from each view's display
//! generation. A cancellation arriving before its query remains effective.
use crate::commands::opaque_id;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
const MAX_REQUESTS: usize = 1024;
const MAX_LIFETIME_MS: u64 = 30_000;
struct Entry {
    cancelled: Arc<AtomicBool>,
    started: bool,
    expires: u64,
}
#[derive(Default)]
pub struct Queries(Mutex<BTreeMap<String, Entry>>);
#[derive(Clone)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
impl Queries {
    pub fn begin(&self, id: &str, deadline: u64, now: u64) -> Result<Cancellation, &'static str> {
        if !opaque_id(id) || deadline <= now || deadline - now > MAX_LIFETIME_MS {
            return Err("query_request_invalid");
        }
        let mut state = self.0.lock().map_err(|_| "query_busy")?;
        state.retain(|_, entry| entry.expires > now);
        if state.contains_key(id) {
            return Err("query_cancelled_or_replayed");
        }
        if state.len() >= MAX_REQUESTS {
            return Err("query_limit");
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        state.insert(
            id.into(),
            Entry {
                cancelled: cancelled.clone(),
                started: true,
                expires: deadline,
            },
        );
        Ok(Cancellation(cancelled))
    }
    pub fn cancel(&self, id: &str, now: u64) -> Result<(), &'static str> {
        if !opaque_id(id) {
            return Err("query_request_invalid");
        }
        let mut state = self.0.lock().map_err(|_| "query_busy")?;
        state.retain(|_, entry| entry.expires > now);
        if let Some(entry) = state.get_mut(id) {
            entry.cancelled.store(true, Ordering::Release);
            return Ok(());
        }
        if state.len() >= MAX_REQUESTS {
            return Err("query_limit");
        }
        state.insert(
            id.into(),
            Entry {
                cancelled: Arc::new(AtomicBool::new(true)),
                started: false,
                expires: now.saturating_add(MAX_LIFETIME_MS),
            },
        );
        Ok(())
    }
    pub fn cancel_all(&self) {
        if let Ok(state) = self.0.lock() {
            for entry in state.values() {
                if entry.started {
                    entry.cancelled.store(true, Ordering::Release);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancel_before_arrival_and_other_views_cannot_revive_or_cancel_each_other() {
        let queries = Queries::default();
        queries.cancel("late", 100).unwrap();
        assert!(queries.begin("late", 1000, 101).is_err());
        let first = queries.begin("pane-one", 1000, 101).unwrap();
        let second = queries.begin("pane-two", 1000, 101).unwrap();
        queries.cancel("pane-one", 102).unwrap();
        assert!(first.requested());
        assert!(!second.requested());
        assert!(queries.begin("pane-one", 1000, 103).is_err());
        queries.cancel_all();
        assert!(second.requested());
    }
}
