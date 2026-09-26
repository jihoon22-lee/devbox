//! Opaque output subscriptions bound to the native companion incarnation.
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::{mpsc, watch};
const MAX_STREAMS_PER_SESSION: usize = 4;
const MAX_STREAMS: usize = 1024;
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Owner {
    pub window: String,
    pub session: String,
}
struct Entry {
    owner: Owner,
    session: String,
    acks: mpsc::Sender<u64>,
    stop: watch::Sender<bool>,
}
#[derive(Default)]
pub(crate) struct Subscriptions(Mutex<HashMap<String, Entry>>);
impl Subscriptions {
    pub(crate) fn insert(
        &self,
        owner: Owner,
        session: String,
        acks: mpsc::Sender<u64>,
        stop: watch::Sender<bool>,
    ) -> Result<String, &'static str> {
        let mut entries = self.0.lock().map_err(|_| "terminal_stream_unavailable")?;
        if entries.len() >= MAX_STREAMS
            || entries.values().filter(|e| e.session == session).count() >= MAX_STREAMS_PER_SESSION
        {
            return Err("terminal_stream_limit");
        }
        let id = uuid::Uuid::new_v4().to_string();
        entries.insert(
            id.clone(),
            Entry {
                owner,
                session,
                acks,
                stop,
            },
        );
        Ok(id)
    }
    pub(crate) fn ack(&self, owner: &Owner, id: &str, cursor: u64) -> Result<(), &'static str> {
        let entries = self.0.lock().map_err(|_| "terminal_stream_unavailable")?;
        let entry = entries
            .get(id)
            .filter(|e| &e.owner == owner)
            .ok_or("terminal_stream_denied")?;
        entry
            .acks
            .try_send(cursor)
            .map_err(|_| "terminal_stream_unavailable")
    }
    pub(crate) fn remove(&self, owner: &Owner, id: &str) -> Result<bool, &'static str> {
        let mut entries = self.0.lock().map_err(|_| "terminal_stream_unavailable")?;
        if entries.get(id).is_some_and(|e| &e.owner != owner) {
            return Err("terminal_stream_denied");
        }
        if let Some(entry) = entries.remove(id) {
            entry.stop.send_replace(true);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub(crate) fn remove_window(&self, window: &str) {
        if let Ok(mut entries) = self.0.lock() {
            entries.retain(|_, entry| {
                if entry.owner.window != window {
                    return true;
                }
                entry.stop.send_replace(true);
                false
            });
        }
    }
    pub(crate) fn clear(&self) {
        if let Ok(mut entries) = self.0.lock() {
            for (_, entry) in entries.drain() {
                entry.stop.send_replace(true);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn owner(window: &str, session: &str) -> Owner {
        Owner {
            window: window.into(),
            session: session.into(),
        }
    }
    #[test]
    fn acknowledgements_and_removal_require_the_exact_native_owner() {
        let subscriptions = Subscriptions::default();
        let current = owner("terminal-a", "peer-a");
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let (stop, stop_rx) = tokio::sync::watch::channel(false);
        let id = subscriptions
            .insert(current.clone(), "pty-a".into(), tx, stop)
            .unwrap();
        assert!(subscriptions
            .ack(&owner("terminal-b", "peer-a"), &id, 7)
            .is_err());
        assert!(subscriptions
            .remove(&owner("terminal-a", "peer-b"), &id)
            .is_err());
        subscriptions.ack(&current, &id, 7).unwrap();
        assert_eq!(rx.try_recv().unwrap(), 7);
        assert!(subscriptions.remove(&current, &id).unwrap());
        assert!(*stop_rx.borrow());
        assert!(!subscriptions.remove(&current, &id).unwrap());
        assert!(subscriptions.ack(&current, &id, 8).is_err());
    }
    #[test]
    fn streams_and_ack_queue_are_bounded_and_reload_retires_idle_streams() {
        let subscriptions = Subscriptions::default();
        let current = owner("terminal-a", "peer-a");
        let mut stops = Vec::new();
        for _ in 0..MAX_STREAMS_PER_SESSION {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let (stop, rx) = tokio::sync::watch::channel(false);
            subscriptions
                .insert(current.clone(), "pty-a".into(), tx, stop)
                .unwrap();
            stops.push(rx);
        }
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let (stop, _) = tokio::sync::watch::channel(false);
        assert!(subscriptions
            .insert(current.clone(), "pty-a".into(), tx, stop)
            .is_err());
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let (stop, other) = tokio::sync::watch::channel(false);
        let other_owner = owner("terminal-b", "peer-b");
        let id = subscriptions
            .insert(other_owner.clone(), "pty-b".into(), tx, stop)
            .unwrap();
        subscriptions.ack(&other_owner, &id, 1).unwrap();
        assert!(subscriptions.ack(&other_owner, &id, 2).is_err());
        subscriptions.remove_window("terminal-a");
        assert!(stops.iter().all(|rx| *rx.borrow()));
        assert!(!*other.borrow());
        subscriptions.remove_window("terminal-b");
        assert!(*other.borrow());
    }
}
