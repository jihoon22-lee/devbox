//! In-memory edit leases and per-document async locks.
//! A lease lives as long as the channel that claimed it (its owner token) and,
//! as a fallback for half-open connections, until 60 s pass without renewal.
use crate::model::{DocKey, DocKind, Eol, LeaseLoss};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;

pub const LEASE_TTL_MS: u64 = 60_000;
const LOST_MEMORY: usize = 1024;

/// Monotonic milliseconds (injectable for tests).
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;
/// Called (under the lease lock) when a lease ends: `(key, lease id, notify the holder)`.
pub type OnDrop = Arc<dyn Fn(&DocKey, &str, bool) + Send + Sync>;

struct Lease {
    key: DocKey,
    window: String,
    kind: DocKind,
    eol: Eol,
    bom: bool,
    owner: CancellationToken,
    epoch: u64,
    renewed_ms: u64,
    saved_seq: u64,
    watch: CancellationToken,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseView {
    pub id: String,
    pub key: DocKey,
    pub kind: DocKind,
    pub eol: Eol,
    pub bom: bool,
    pub saved_seq: u64,
}
pub enum Acquired {
    Granted(LeaseView),
    Elsewhere,
    Cancelled,
}
/// Line-ending choice recorded at claim time and reused by saves.
#[derive(Debug, Clone, Copy)]
pub struct Format {
    pub kind: DocKind,
    pub eol: Eol,
    pub bom: bool,
}

#[derive(Default)]
struct State {
    by_id: HashMap<String, Lease>,
    by_key: HashMap<DocKey, String>,
    lost: VecDeque<(String, LeaseLoss)>,
    epoch: u64,
}

pub struct Leases {
    state: Mutex<State>,
    locks: Mutex<HashMap<DocKey, Weak<AsyncMutex<()>>>>,
    clock: Clock,
    on_drop: OnDrop,
}

fn view(id: &str, l: &Lease) -> LeaseView {
    LeaseView {
        id: id.into(),
        key: l.key.clone(),
        kind: l.kind,
        eol: l.eol,
        bom: l.bom,
        saved_seq: l.saved_seq,
    }
}

impl Leases {
    pub fn new(clock: Clock, on_drop: OnDrop) -> Self {
        Self {
            state: Mutex::default(),
            locks: Mutex::default(),
            clock,
            on_drop,
        }
    }
    fn dead(&self, lease: &Lease, now: u64) -> bool {
        lease.owner.is_cancelled() || now.saturating_sub(lease.renewed_ms) > LEASE_TTL_MS
    }
    fn evict(&self, st: &mut State, id: &str, loss: Option<LeaseLoss>, notify: bool) {
        let Some(lease) = st.by_id.remove(id) else {
            return;
        };
        st.by_key.remove(&lease.key);
        lease.watch.cancel();
        if let Some(loss) = loss {
            if st.lost.len() == LOST_MEMORY {
                st.lost.pop_front();
            }
            st.lost.push_back((id.into(), loss));
        }
        (self.on_drop)(&lease.key, id, notify);
    }
    fn expire(&self, st: &mut State, id: &str, now: u64) {
        if let Some(lease) = st.by_id.get(id)
            && self.dead(lease, now)
        {
            // A closed owner needs no notice; a silent half-open window does.
            let notify = !lease.owner.is_cancelled();
            self.evict(st, id, Some(LeaseLoss::Expired), notify);
        }
    }
    fn lost(st: &State, id: &str) -> LeaseLoss {
        st.lost
            .iter()
            .rev()
            .find(|(lost, _)| lost == id)
            .map_or(LeaseLoss::Expired, |(_, loss)| *loss)
    }
    /// A live lease; any use counts as renewal.
    pub fn get(&self, id: &str) -> Result<LeaseView, LeaseLoss> {
        let now = (self.clock)();
        let mut st = self.state.lock().expect("leases");
        self.expire(&mut st, id, now);
        if let Some(lease) = st.by_id.get_mut(id) {
            lease.renewed_ms = now;
            return Ok(view(id, lease));
        }
        Err(Self::lost(&st, id))
    }
    /// The single critical section of claim/take: cancellation check, insertion
    /// and owner-watcher spawn happen under one lock with no `await` between them.
    pub fn acquire(
        self: &Arc<Self>,
        key: &DocKey,
        window: &str,
        owner: &CancellationToken,
        format: Format,
        take: bool,
    ) -> Acquired {
        let now = (self.clock)();
        let mut st = self.state.lock().expect("leases");
        if owner.is_cancelled() {
            return Acquired::Cancelled;
        }
        st.epoch += 1;
        let epoch = st.epoch;
        if let Some(id) = st.by_key.get(key).cloned() {
            self.expire(&mut st, &id, now);
            if let Some(lease) = st.by_id.get_mut(&id) {
                if lease.window == window {
                    lease.watch.cancel();
                    lease.watch = CancellationToken::new();
                    lease.owner = owner.clone();
                    lease.epoch = epoch;
                    lease.renewed_ms = now;
                    (lease.kind, lease.eol, lease.bom) = (format.kind, format.eol, format.bom);
                    self.spawn_owner_watch(&id, epoch, owner.clone(), lease.watch.clone());
                    return Acquired::Granted(view(&id, lease));
                }
                if !take {
                    return Acquired::Elsewhere;
                }
                self.evict(&mut st, &id, Some(LeaseLoss::Taken), true);
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let lease = Lease {
            key: key.clone(),
            window: window.into(),
            kind: format.kind,
            eol: format.eol,
            bom: format.bom,
            owner: owner.clone(),
            epoch,
            renewed_ms: now,
            saved_seq: 0,
            watch: CancellationToken::new(),
        };
        self.spawn_owner_watch(&id, epoch, owner.clone(), lease.watch.clone());
        let granted = view(&id, &lease);
        st.by_key.insert(key.clone(), id.clone());
        st.by_id.insert(id, lease);
        Acquired::Granted(granted)
    }
    fn spawn_owner_watch(
        self: &Arc<Self>,
        id: &str,
        epoch: u64,
        owner: CancellationToken,
        done: CancellationToken,
    ) {
        let (leases, id) = (Arc::downgrade(self), id.to_owned());
        tokio::spawn(async move {
            tokio::select! {
                _ = owner.cancelled() => {
                    if let Some(leases) = leases.upgrade() {
                        leases.release_if(&id, epoch);
                    }
                }
                _ = done.cancelled() => {}
            }
        });
    }
    /// Releases only if the lease still belongs to the owner generation `epoch`.
    fn release_if(&self, id: &str, epoch: u64) {
        let mut st = self.state.lock().expect("leases");
        if st.by_id.get(id).is_some_and(|l| l.epoch == epoch) {
            self.evict(&mut st, id, Some(LeaseLoss::Expired), false);
        }
    }
    pub fn release(&self, id: &str) {
        let mut st = self.state.lock().expect("leases");
        self.evict(&mut st, id, Some(LeaseLoss::Expired), false);
    }
    pub fn renew(&self, ids: &[String]) -> Vec<String> {
        ids.iter()
            .filter(|id| self.get(id).is_err())
            .cloned()
            .collect()
    }
    pub fn saved(&self, id: &str, seq: u64) {
        if let Some(l) = self.state.lock().expect("leases").by_id.get_mut(id) {
            l.saved_seq = l.saved_seq.max(seq);
        }
    }
    /// Runs `f` under the lease lock if lease `id` still holds `key`. Eviction
    /// removes the file watch under the same lock, so `f` cannot re-add one late.
    pub fn while_live(&self, id: &str, key: &DocKey, f: impl FnOnce()) {
        let st = self.state.lock().expect("leases");
        if st
            .by_id
            .get(id)
            .is_some_and(|l| l.key == *key && !l.owner.is_cancelled())
        {
            f();
        }
    }
    /// The live lease holding exactly `key`, if any (an expired one is evicted first).
    pub fn holder(&self, key: &DocKey) -> Option<String> {
        let now = (self.clock)();
        let mut st = self.state.lock().expect("leases");
        let id = st.by_key.get(key).cloned()?;
        self.expire(&mut st, &id, now);
        st.by_id.contains_key(&id).then_some(id)
    }
    /// True when a live lease holds `key` or a document below it.
    pub fn occupied(&self, key: &DocKey) -> bool {
        let now = (self.clock)();
        let mut st = self.state.lock().expect("leases");
        let prefix = format!("{}/", key.path);
        let ids: Vec<String> = st
            .by_id
            .iter()
            .filter(|(_, l)| {
                l.key.root == key.root && (l.key == *key || l.key.path.starts_with(&prefix))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            self.expire(&mut st, id, now);
        }
        ids.iter().any(|id| st.by_id.contains_key(id))
    }
    /// Points leases at `from` (or below it, for folders) to the moved location.
    pub fn rekey(&self, from: &DocKey, to: &DocKey) -> Vec<(DocKey, DocKey)> {
        let mut st = self.state.lock().expect("leases");
        let prefix = format!("{}/", from.path);
        let moved: Vec<(String, DocKey, DocKey)> = st
            .by_id
            .iter()
            .filter(|(_, l)| {
                l.key.root == from.root && (l.key == *from || l.key.path.starts_with(&prefix))
            })
            .map(|(id, l)| {
                let path = format!("{}{}", to.path, &l.key.path[from.path.len()..]);
                (
                    id.clone(),
                    l.key.clone(),
                    DocKey {
                        root: to.root.clone(),
                        path,
                    },
                )
            })
            .collect();
        for (id, old, new) in &moved {
            st.by_key.remove(old);
            st.by_key.insert(new.clone(), id.clone());
            if let Some(l) = st.by_id.get_mut(id) {
                l.key = new.clone();
            }
        }
        moved.into_iter().map(|(_, old, new)| (old, new)).collect()
    }
    /// Serializes save, journal and rename for one document key.
    pub async fn lock(&self, key: &DocKey) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.locks.lock().expect("document locks");
            locks.retain(|_, weak| weak.strong_count() > 0);
            match locks.get(key).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(AsyncMutex::new(()));
                    locks.insert(key.clone(), Arc::downgrade(&lock));
                    lock
                }
            }
        };
        lock.lock_owned().await
    }
}
