//! Session-local initialization/resize ordering and bounded peer cleanup.
use crate::{TerminalError, attach::pty_error, tmux::Tmux};
use devbox_protocol::Fail;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
type Result<T> = std::result::Result<T, Fail<TerminalError>>;
#[derive(Default)]
pub(crate) struct SessionGate {
    lock: Arc<AsyncMutex<()>>,
    pending: Mutex<Vec<u32>>,
}
struct Entry {
    weak: Weak<SessionGate>,
    unresolved: Option<Arc<SessionGate>>,
}
#[derive(Clone, Default)]
pub(crate) struct Gates(Arc<Mutex<HashMap<String, Entry>>>);
impl Gates {
    fn gate(&self, id: &str) -> Arc<SessionGate> {
        let mut gates = self.0.lock().expect("session gates");
        gates.retain(|_, entry| entry.unresolved.is_some() || entry.weak.strong_count() != 0);
        if let Some(gate) = gates.get(id).and_then(|entry| entry.weak.upgrade()) {
            return gate;
        }
        let gate = Arc::new(SessionGate::default());
        gates.insert(
            id.into(),
            Entry {
                weak: Arc::downgrade(&gate),
                unresolved: None,
            },
        );
        gate
    }
    fn pin(&self, id: &str, gate: &Arc<SessionGate>, pending: Vec<u32>) {
        *gate.pending.lock().expect("pending peers") = pending;
        self.0.lock().expect("session gates").insert(
            id.into(),
            Entry {
                weak: Arc::downgrade(gate),
                unresolved: Some(gate.clone()),
            },
        );
    }
    fn resolved(&self, id: &str, gate: &Arc<SessionGate>) {
        gate.pending.lock().expect("pending peers").clear();
        self.0.lock().expect("session gates").insert(
            id.into(),
            Entry {
                weak: Arc::downgrade(gate),
                unresolved: None,
            },
        );
    }
    pub async fn enter(&self, id: &str, tmux: &Tmux) -> Result<Lease> {
        self.enter_checked(id, |pid| async move {
            tmux.client_present(pid).await.map_err(pty_error)
        })
        .await
    }
    async fn enter_checked<F, Fut>(&self, id: &str, mut present: F) -> Result<Lease>
    where
        F: FnMut(u32) -> Fut + Send,
        Fut: Future<Output = Result<bool>> + Send,
    {
        let lease = self.cleanup_lease(id).await;
        let pending = lease.peers.lock().expect("pending peers").clone();
        for pid in pending {
            if present(pid).await? {
                return Err(pty_error("previous attachment cleanup is unresolved"));
            }
        }
        self.resolved(id, &lease.gate);
        lease.peers.lock().expect("owned peers").clear();
        Ok(lease)
    }
    // Closing an owned peer must remain possible when an earlier failed ACK
    // pinned unresolved state. Confirm every pending peer before resolving it.
    pub async fn cleanup_lease(&self, id: &str) -> Lease {
        let gate = self.gate(id);
        let held = gate.lock.clone().lock_owned().await;
        let peers = gate.pending.lock().expect("pending peers").clone();
        Lease {
            gates: self.clone(),
            id: id.into(),
            gate,
            _held: held,
            peers: Mutex::new(peers),
        }
    }
}
pub(crate) struct Lease {
    gates: Gates,
    id: String,
    gate: Arc<SessionGate>,
    _held: OwnedMutexGuard<()>,
    peers: Mutex<Vec<u32>>,
}
impl Lease {
    pub fn record(&self, pid: u32) {
        self.peers.lock().expect("owned peers").push(pid);
    }
    pub async fn cleanup(&self, tmux: &Tmux) -> Result<()> {
        self.cleanup_checked(|pid| async move { tmux.client_present(pid).await.map_err(pty_error) })
            .await
    }
    async fn cleanup_checked<F, Fut>(&self, mut present: F) -> Result<()>
    where
        F: FnMut(u32) -> Fut + Send,
        Fut: Future<Output = Result<bool>> + Send,
    {
        let peers = self.peers.lock().expect("owned peers").clone();
        let confirmed = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let mut any = false;
                for &pid in &peers {
                    any |= present(pid).await?;
                }
                if !any {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(pty_error)
        .and_then(|result| result);
        if confirmed.is_err() {
            self.gates.pin(&self.id, &self.gate, peers);
        } else {
            self.gates.resolved(&self.id, &self.gate);
        }
        confirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn expired_gates_are_pruned_without_replacing_live_gates() {
        let gates = Gates::default();
        let live = gates.gate("live");
        for n in 0..100 {
            drop(gates.gate(&format!("expired-{n}")));
        }
        assert!(Arc::ptr_eq(&live, &gates.gate("live")));
        assert_eq!(gates.0.lock().expect("gates").len(), 1);
    }
    #[tokio::test]
    async fn permanent_query_failure_finishes_cleanup_and_retains_unresolved_session() {
        let gates = Gates::default();
        let lease = gates
            .enter_checked("session", async |_| Ok(false))
            .await
            .expect("enter");
        lease.record(123);
        tokio::time::timeout(
            Duration::from_millis(100),
            lease.cleanup_checked(async |_| Err(pty_error("permanent fixture query failure"))),
        )
        .await
        .expect("bounded cleanup")
        .expect_err("query failure");
        let original = Arc::downgrade(&lease.gate);
        drop(lease);
        assert!(
            original.upgrade().is_some(),
            "unresolved state outlives all active operations"
        );
        gates
            .enter_checked("session", async |_| {
                Err(pty_error("permanent query failure"))
            })
            .await
            .err()
            .expect("fail explicitly until query recovers");
        gates
            .enter_checked("session", async |_| Ok(true))
            .await
            .err()
            .expect("live peer blocks initialization");
        let cleaning = gates.cleanup_lease("session").await;
        cleaning.record(456);
        cleaning
            .cleanup_checked(async |_| Err(pty_error("still unresolved")))
            .await
            .expect_err("cleanup remains bounded while queries fail");
        drop(cleaning);
        let checked = Arc::new(Mutex::new(Vec::new()));
        let observed = checked.clone();
        let recovered = gates
            .enter_checked("session", move |pid| {
                observed.lock().expect("checked peers").push(pid);
                async { Ok(false) }
            })
            .await
            .expect("confirmed peer removal");
        assert_eq!(*checked.lock().expect("checked peers"), vec![123, 456]);
        assert!(Arc::ptr_eq(
            &recovered.gate,
            &original.upgrade().expect("same unresolved gate")
        ));
        drop(recovered);
        drop(gates.gate("other"));
        assert!(
            original.upgrade().is_none(),
            "resolved gates resume weak cleanup"
        );
    }
}
