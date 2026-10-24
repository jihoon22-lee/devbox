//! In-process committed-save history for index consumers, separate from renderer
//! external-change events. A save never awaits an indexing callback under locks.
use crate::{DocKey, DocumentsService};
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::watch;

pub const COMMIT_HISTORY: usize = 256;

/// `paths = None` means the bounded history no longer covers the cursor: rescan.
/// Apply only this snapshot's revision after processing it, never a later one.
pub struct CommittedChanges {
    pub revision: u64,
    pub paths: Option<Vec<DocKey>>,
}
#[derive(Default)]
struct State {
    revision: u64,
    history: VecDeque<(u64, DocKey)>,
}
pub(crate) struct Commits {
    state: Mutex<State>,
    notify: watch::Sender<u64>,
}
impl Default for Commits {
    fn default() -> Self {
        let (notify, _) = watch::channel(0);
        Self {
            state: Mutex::default(),
            notify,
        }
    }
}
impl Commits {
    pub fn record(&self, key: DocKey) {
        let mut state = self.state.lock().expect("document commits");
        state.revision += 1;
        let revision = state.revision;
        state.history.push_back((revision, key));
        while state.history.len() > COMMIT_HISTORY {
            state.history.pop_front();
        }
        // Coalescing wakes are safe: readers consult history/cursor, not the wake payload.
        self.notify.send_replace(revision);
    }
    fn since(&self, cursor: u64) -> CommittedChanges {
        let state = self.state.lock().expect("document commits");
        let missing = state
            .history
            .front()
            .is_some_and(|(first, _)| cursor.saturating_add(1) < *first);
        let paths = (!missing).then(|| {
            let mut paths: Vec<_> = state
                .history
                .iter()
                .filter(|(seq, _)| *seq > cursor)
                .map(|(_, key)| key.clone())
                .collect();
            paths.sort();
            paths.dedup();
            paths
        });
        CommittedChanges {
            revision: state.revision,
            paths,
        }
    }
}
impl DocumentsService {
    pub fn subscribe_commits(&self) -> watch::Receiver<u64> {
        self.s.commits.notify.subscribe()
    }
    pub fn commit_revision(&self) -> u64 {
        self.s
            .commits
            .state
            .lock()
            .expect("document commits")
            .revision
    }
    pub fn committed_since(&self, cursor: u64) -> CommittedChanges {
        self.s.commits.since(cursor)
    }
}
